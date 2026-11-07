//! Job runner: dispatches a task to the right engine, streams progress/results to the UI as events, supports cancel,
//! asks the UI what to do when an output name is taken, and runs independent image batches in parallel.
use crate::{core::*, imgops, kinds, ocr, office, pdf2x, pdfa, pdfops, textout, video};
use anyhow::{bail, Context, Result};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TaskSpec {
    pub id: String,
    pub inputs: Vec<String>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct JobSpec {
    pub mode: String,
    pub tasks: Vec<TaskSpec>,
    pub out_dir: String,
    #[serde(default)]
    pub options: serde_json::Value,
    /// "keep" (default: add (1), (2)...), "replace" or "ask"
    #[serde(default)]
    pub on_exists: String,
}

#[derive(Serialize, Clone)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum JobEvent {
    TaskStarted { job_id: String, task_id: String },
    Progress { job_id: String, task_id: String, fraction: f64 },
    TaskDone { job_id: String, task_id: String, outcome: Outcome },
    TaskFailed { job_id: String, task_id: String, message: String, cancelled: bool, skipped: bool },
    /// An output name is taken and the user asked to be asked. Answered with the `answer_ask` command.
    Ask { job_id: String, task_id: String, ask_id: u64, name: String, folder: String },
    JobDone { job_id: String, cancelled: bool },
}

#[derive(Default)]
pub struct Registry {
    jobs: Mutex<HashMap<String, Arc<AtomicBool>>>,
    asks: Mutex<HashMap<u64, mpsc::Sender<(Clash, bool)>>>,
}

impl Registry {
    pub fn cancel(&self, id: &str) {
        if let Some(flag) = self.jobs.lock().unwrap().get(id) {
            flag.store(true, Ordering::Relaxed);
        }
    }
    pub fn answer(&self, ask_id: u64, choice: &str, all: bool) {
        let c = match choice {
            "replace" => Clash::Replace,
            "skip" => Clash::Skip,
            _ => Clash::KeepBoth,
        };
        if let Some(tx) = self.asks.lock().unwrap().remove(&ask_id) {
            let _ = tx.send((c, all));
        }
    }
}

const EVENT: &str = "docshift://job";
static ASK_ID: AtomicU64 = AtomicU64::new(1);

fn is_office_mode(mode: &str) -> bool {
    matches!(mode, "word2pdf" | "excel2pdf" | "ppt2pdf" | "pdf2word" | "combine_pdf")
}

/// "Read scanned pages": if asked and the PDF has pages without text, run OCR into a temp copy (same file name,
/// so outputs are still named after the original) and return that instead. Uses the first 60% of the progress bar.
fn ocr_stage(src: &Path, opts: &Opts, ctl: &Ctl) -> Result<(Option<TempDir>, PathBuf, String, Ctl)> {
    if !opts.bool("ocr", false) || ocr::pages_without_text(src)?.is_empty() {
        return Ok((None, src.to_path_buf(), String::new(), ctl.clone()));
    }
    let tmp = TempDir::new()?;
    let copy = tmp.join(&src.file_name().unwrap_or_default().to_string_lossy());
    let n = ocr::searchable_copy(src, &copy, &opts.str("ocr_lang", ""), ctl, 0.0, 0.6)?;
    let note = if n > 0 { format!("Text was recognised on {n} scanned page(s); check names and numbers.") } else { "No text could be recognised on the scanned pages.".into() };
    Ok((Some(tmp), copy, note, ctl.slice(0.6, 1.0)))
}

fn add_note(mut o: Outcome, note: &str) -> Outcome {
    if !note.is_empty() {
        o.note = if o.note.is_empty() { note.to_string() } else { format!("{note} {}", o.note) };
    }
    o
}

/// Word, Excel, PowerPoint, images and PDFs, in order, into one PDF with a bookmark per file.
fn combine_to_pdf(inputs: &[PathBuf], out_dir: &Path, name: &str, opts: &Opts, session: &mut office::Session, ctl: &Ctl) -> Result<Outcome> {
    if inputs.len() < 2 {
        bail!("Add at least two files to combine.");
    }
    // each file is converted without PDF/A; the finished document gets it once at the end
    let mut per_file = opts.0.clone();
    if let Some(o) = per_file.as_object_mut() {
        o.insert("pdfa".into(), serde_json::Value::Bool(false));
    }
    let per_file = Opts(&per_file);
    let tmp = TempDir::new()?;
    let mut parts = vec![];
    let mut titles = vec![];
    let mut skipped = vec![];
    for (i, src) in inputs.iter().enumerate() {
        ctl.check()?;
        ctl.report(i as f64 / inputs.len() as f64 * 0.9);
        let fname = src.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let part = (|| -> Result<lopdf::Document> {
            match kinds::kind_of(src) {
                Some("pdf") => pdfops::load(src),
                Some("img") => Ok(pdfops::images_doc(std::slice::from_ref(src), &per_file, &Ctl::none())?.0),
                Some(k @ ("word" | "excel" | "ppt")) => {
                    let app = match k {
                        "word" => office::App::Word,
                        "excel" => office::App::Excel,
                        _ => office::App::Ppt,
                    };
                    let (file, _) = office::office_pdf_file(app, session, src, &tmp, &per_file)?;
                    pdfops::load(&file)
                }
                _ => bail!("this kind of file can't go into a PDF"),
            }
        })();
        match part {
            Ok(doc) => {
                parts.push(doc);
                titles.push(stem_of(src));
            }
            Err(e) => skipped.push(format!("{fname}: {}", friendly(&e))),
        }
    }
    if parts.is_empty() {
        bail!("None of the files could be converted. {}", skipped.join("; "));
    }
    let (mut doc, pages) = pdfops::merge_docs(parts, &titles, opts.bool("bookmarks", true), false, ctl)?;
    if opts.bool("pdfa", false) {
        pdfa::apply(&mut doc, name)?;
    }
    let file = tmp.join("combined.pdf");
    pdfops::save_doc_to(&mut doc, &file)?;
    let size = std::fs::metadata(&file)?.len();
    let dst = publish(&file, out_dir, &safe_stem(name), ".pdf")?;
    let mut o = Outcome::one(&dst).with_detail(format!("{} files, {pages} pages \u{2192} {}", titles.len(), fmt_size(size)));
    if !skipped.is_empty() {
        o = o.with_note(format!("Skipped: {}", skipped.join("; ")));
    }
    Ok(o)
}

/// Runs one task with the engine that handles `mode`.
pub(crate) fn run_task(mode: &str, inputs: &[PathBuf], out_dir: &Path, opts: &Opts, session: &mut office::Session, ctl: &Ctl) -> Result<Outcome> {
    let first = inputs.first().context("No file was given.")?;
    for p in inputs {
        if !p.exists() {
            bail!("The file could not be found: {}. It may have been moved or deleted.", p.display());
        }
    }
    let expand = opts.str("action", "compress") == "expand";
    let output_name = |default: String| {
        let n = opts.str("output_name", &default);
        if n.trim().is_empty() { default } else { n }
    };
    match mode {
        "word2pdf" => office::office_to_pdf(office::App::Word, session, first, out_dir, opts),
        "excel2pdf" => office::office_to_pdf(office::App::Excel, session, first, out_dir, opts),
        "ppt2pdf" => office::office_to_pdf(office::App::Ppt, session, first, out_dir, opts),
        "combine_pdf" => combine_to_pdf(inputs, out_dir, &output_name(format!("{}_combined", stem_of(first))), opts, session, ctl),
        "pdf2word" | "pdf2excel" | "pdf2ppt" | "pdf2txt" => {
            let (_keep, src, note, ctl) = ocr_stage(first, opts, ctl)?;
            let flow = opts.str("layout", "keep") == "flow";
            let o = match mode {
                "pdf2word" if flow || !office::installed(office::App::Word) => textout::pdf_to_docx_flowing(&src, &src, out_dir, &ctl, String::new())?,
                "pdf2word" => office::pdf_to_word(session, &src, out_dir, !note.is_empty())?,
                "pdf2excel" => pdf2x::pdf_to_excel(&src, out_dir, opts, &ctl)?,
                "pdf2ppt" => pdf2x::pdf_to_pptx(&src, out_dir, opts, &ctl)?,
                _ => textout::pdf_to_text(&src, &src, out_dir, opts, &ctl, String::new())?,
            };
            Ok(add_note(o, &note))
        }
        "pdf_ocr" => ocr::make_searchable(first, out_dir, opts, ctl),
        "pdf2img" => pdf2x::pdf_to_images(first, out_dir, opts, ctl),
        "img2pdf" => {
            let default = if inputs.len() > 1 { format!("{}_merged", stem_of(first)) } else { stem_of(first) };
            pdfops::images_to_pdf(inputs, out_dir, &output_name(default), opts, ctl)
        }
        "pdf_merge" => pdfops::merge(inputs, out_dir, &output_name("merged".into()), opts, ctl),
        "pdf_pages" => {
            let groups: Vec<Vec<pdfops::PageSel>> = serde_json::from_value(opts.0.get("groups").cloned().unwrap_or_default())
                .context("The page selection was not understood.")?;
            pdfops::build_pages(first, &groups, out_dir, &safe_stem(&opts.str("tag", "pages")), ctl)
        }
        "pdf_compress" => {
            if expand { pdfops::expand_pdf(first, out_dir) } else { pdfops::compress_pdf(first, out_dir, opts, ctl) }
        }
        "img_compress" => {
            if expand { imgops::expand_image(first, out_dir, opts) } else { imgops::compress_image(first, out_dir, opts, ctl) }
        }
        "video_compress" => {
            if expand { video::expand_video(first, out_dir, ctl) } else { video::compress_video(first, out_dir, opts, ctl) }
        }
        other => bail!("Unknown tool: {other}"),
    }
}

pub fn start(app: AppHandle, registry: &Registry, spec: JobSpec) -> String {
    let id = format!("job{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_micros()).unwrap_or(0));
    let flag = Arc::new(AtomicBool::new(false));
    registry.jobs.lock().unwrap().insert(id.clone(), flag.clone());
    let job_id = id.clone();
    std::thread::spawn(move || {
        execute(&app, &job_id, &spec, flag.clone());
        let _ = app.emit(EVENT, JobEvent::JobDone { job_id: job_id.clone(), cancelled: flag.load(Ordering::Relaxed) });
        if let Some(reg) = app.try_state::<Registry>() {
            reg.jobs.lock().unwrap().remove(&job_id);
        }
    });
    id
}

/// The clash policy for one task. "ask" sends the question to the UI and waits; "for all" answers are remembered
/// for the rest of the job.
fn clash_policy(app: &AppHandle, spec: &JobSpec, job_id: &str, task_id: &str, cancelled: Arc<AtomicBool>, remembered: Arc<Mutex<Option<Clash>>>) -> Option<ClashResolver> {
    match spec.on_exists.as_str() {
        "replace" => Some(Arc::new(|_| Clash::Replace)),
        "ask" => {
            let (app, job_id, task_id) = (app.clone(), job_id.to_string(), task_id.to_string());
            Some(Arc::new(move |path: &Path| {
                if let Some(c) = *remembered.lock().unwrap() {
                    return c;
                }
                // one question at a time, even when tasks run in parallel
                static ONE: Mutex<()> = Mutex::new(());
                let _g = ONE.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(c) = *remembered.lock().unwrap() {
                    return c;
                }
                let Some(reg) = app.try_state::<Registry>() else { return Clash::KeepBoth };
                let ask_id = ASK_ID.fetch_add(1, Ordering::Relaxed);
                let (tx, rx) = mpsc::channel();
                reg.asks.lock().unwrap().insert(ask_id, tx);
                let _ = app.emit(
                    EVENT,
                    JobEvent::Ask {
                        job_id: job_id.clone(),
                        task_id: task_id.clone(),
                        ask_id,
                        name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                        folder: path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
                    },
                );
                loop {
                    match rx.recv_timeout(Duration::from_millis(200)) {
                        Ok((c, all)) => {
                            if all {
                                *remembered.lock().unwrap() = Some(c);
                            }
                            return c;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) if !cancelled.load(Ordering::Relaxed) => continue,
                        _ => {
                            reg.asks.lock().unwrap().remove(&ask_id);
                            return Clash::Skip;
                        }
                    }
                }
            }))
        }
        _ => None,
    }
}

fn execute(app: &AppHandle, job_id: &str, spec: &JobSpec, cancelled: Arc<AtomicBool>) {
    let out_dir = PathBuf::from(&spec.out_dir);
    let remembered: Arc<Mutex<Option<Clash>>> = Arc::default();
    let emit = |e: JobEvent| {
        let _ = app.emit(EVENT, e);
    };
    let run_one = |task: &TaskSpec, session: &mut office::Session| {
        if cancelled.load(Ordering::Relaxed) {
            emit(JobEvent::TaskFailed { job_id: job_id.into(), task_id: task.id.clone(), message: "Cancelled".into(), cancelled: true, skipped: false });
            return;
        }
        emit(JobEvent::TaskStarted { job_id: job_id.into(), task_id: task.id.clone() });
        let (jid, tid, app2) = (job_id.to_string(), task.id.clone(), app.clone());
        let ctl = Ctl::new(cancelled.clone(), move |f| {
            let _ = app2.emit(EVENT, JobEvent::Progress { job_id: jid.clone(), task_id: tid.clone(), fraction: f });
        });
        let inputs: Vec<PathBuf> = task.inputs.iter().map(PathBuf::from).collect();
        let opts = Opts(&spec.options);
        let policy = clash_policy(app, spec, job_id, &task.id, cancelled.clone(), remembered.clone());
        // An engine panicking on a malformed file must not take the whole job down.
        let result = with_clash_policy(policy, || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_task(&spec.mode, &inputs, &out_dir, &opts, session, &ctl)))
                .unwrap_or_else(|_| Err(anyhow::anyhow!("This file could not be processed (the engine stopped unexpectedly).")))
        });
        match result {
            Ok(outcome) => emit(JobEvent::TaskDone { job_id: job_id.into(), task_id: task.id.clone(), outcome }),
            Err(e) => {
                let skipped = e.downcast_ref::<Skipped>().is_some();
                emit(JobEvent::TaskFailed {
                    job_id: job_id.into(),
                    task_id: task.id.clone(),
                    message: friendly(&e),
                    cancelled: is_cancelled(&e) || (skipped && cancelled.load(Ordering::Relaxed)),
                    skipped,
                })
            }
        }
    };

    let parallel = spec.mode == "img_compress" && spec.tasks.len() > 1;
    if parallel {
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(2, 4);
        if let Ok(pool) = rayon::ThreadPoolBuilder::new().num_threads(threads).build() {
            pool.install(|| {
                spec.tasks.par_iter().for_each(|t| {
                    let mut none = office::Session::default();
                    run_one(t, &mut none);
                });
            });
            return;
        }
    }
    // Office needs COM on this thread, and one hidden instance per app reused for the whole batch.
    let _com = if is_office_mode(&spec.mode) { Some(office::ComGuard::init()) } else { None };
    let mut session = office::Session::default();
    for t in &spec.tasks {
        run_one(t, &mut session);
    }
    session.close();
}
