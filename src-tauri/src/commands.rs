//! Commands the UI can call.
use crate::{core::*, engine, imgops, jobs, kinds, ocr, office, pdf2x, render, video};
use base64::Engine as _;
use rayon::prelude::*;
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
use tauri::{AppHandle, State};

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub path: String,
    pub name: String,
    pub ext: String,
    pub kind: Option<String>,
    pub size: u64,
    pub created: Option<i64>,
    pub modified: Option<i64>,
    /// EXIF capture date for photos.
    pub taken: Option<i64>,
    pub pages: Option<u32>,
    /// PDF whose first pages have no text: probably scanned
    pub scanned: Option<bool>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration: Option<f64>,
    pub error: Option<String>,
}

fn ms(t: std::io::Result<std::time::SystemTime>) -> Option<i64> {
    t.ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64)
}

fn inspect_one(path: &Path) -> FileInfo {
    let mut info = FileInfo {
        path: path.to_string_lossy().into_owned(),
        name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        ext: kinds::ext_of(path),
        kind: kinds::kind_of(path).map(String::from),
        ..Default::default()
    };
    match fs::metadata(path) {
        Ok(m) => {
            info.size = m.len();
            info.created = ms(m.created());
            info.modified = ms(m.modified());
        }
        Err(_) => {
            info.error = Some("The file could not be found.".into());
            return info;
        }
    }
    match info.kind.as_deref() {
        Some("img") => {
            if let Some((w, h)) = imgops::dimensions(path) {
                info.width = Some(w);
                info.height = Some(h);
            }
            info.taken = imgops::exif_taken_ms(path);
        }
        Some("pdf") => match render::page_count_and_scanned(path) {
            Ok((n, scanned)) => {
                info.pages = Some(n as u32);
                info.scanned = Some(scanned);
            }
            Err(e) => info.error = Some(friendly(&e)),
        },
        Some("video") => {
            if let Ok(p) = video::probe(path) {
                info.duration = p.duration;
                info.width = Some(p.width);
                info.height = Some(p.height);
            }
        }
        _ => {}
    }
    info
}

/// Describes dropped / picked files (folders contribute the files directly inside them).
#[tauri::command(async)]
pub fn inspect_files(paths: Vec<String>) -> Vec<FileInfo> {
    let mut files: Vec<PathBuf> = vec![];
    for p in paths {
        let p = PathBuf::from(p);
        if p.is_dir() {
            if let Ok(rd) = fs::read_dir(&p) {
                let mut inner: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_file()).collect();
                inner.sort();
                files.extend(inner);
            }
        } else {
            files.push(p);
        }
    }
    // PDF page counts serialise on the PDF engine anyway; everything else is read in parallel.
    files.par_iter().map(|p| inspect_one(p)).collect()
}

fn data_url(bytes: &[u8]) -> String {
    format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// A small preview as a data URL. `page` is 0-based and only used for PDFs.
#[tauri::command(async)]
pub fn thumbnail(path: String, page: Option<usize>, size: u32) -> Result<String, String> {
    let p = PathBuf::from(&path);
    let size = size.clamp(48, 1200);
    let r = match kinds::kind_of(&p) {
        Some("pdf") => render::thumbnail(&p, page.unwrap_or(0), size),
        Some("img") => imgops::thumbnail_jpeg(&p, size),
        Some("video") => video::thumbnail(&p, size),
        _ => Err(anyhow::anyhow!("No preview")),
    };
    r.map(|b| data_url(&b)).map_err(|e| friendly(&e))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfInfo {
    pub pages: usize,
    pub sizes: Vec<(f32, f32)>,
    pub no_text: bool,
}

#[tauri::command(async)]
pub fn pdf_info(path: String) -> Result<PdfInfo, String> {
    let p = PathBuf::from(path);
    let sizes = render::page_sizes(&p).map_err(|e| friendly(&e))?;
    let no_text = pdf2x::has_no_text(&p).unwrap_or(false);
    Ok(PdfInfo { pages: sizes.len(), sizes, no_text })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub word: bool,
    pub excel: bool,
    pub powerpoint: bool,
    pub libreoffice: bool,
    pub video: bool,
    pub pdf: bool,
    /// Windows has at least one OCR language installed
    pub ocr: bool,
}

#[tauri::command(async)]
pub fn engine_status() -> EngineStatus {
    EngineStatus {
        word: office::installed(office::App::Word),
        excel: office::installed(office::App::Excel),
        powerpoint: office::installed(office::App::Ppt),
        libreoffice: office::libreoffice_path().is_some(),
        video: engine::ffmpeg_path().is_some(),
        pdf: engine::pdfium_path().is_ok(),
        ocr: !ocr::languages().is_empty(),
    }
}

/// Languages the built-in Windows OCR can read on this computer.
#[tauri::command(async)]
pub fn ocr_languages() -> Vec<ocr::OcrLanguage> {
    ocr::languages()
}

/// Opens a file (or folder) with its default app. Errors come back so the UI can say what went wrong.
#[tauri::command(async)]
pub fn open_path(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err("That file isn't there any more. It may have been moved or deleted.".into());
    }
    tauri_plugin_opener::open_path(&p, None::<&str>).map_err(|e| format!("Windows couldn't open it: {e}"))
}

#[tauri::command]
pub fn answer_ask(registry: State<jobs::Registry>, ask_id: u64, choice: String, all: bool) {
    registry.answer(ask_id, &choice, all);
}

#[tauri::command]
pub fn default_output_dir() -> String {
    dirs::download_dir().or_else(dirs::document_dir).unwrap_or_else(|| PathBuf::from(".")).to_string_lossy().into_owned()
}

/// Makes sure we can write into the chosen folder before starting, so the failure is immediate and clear.
#[tauri::command]
pub fn check_output_dir(dir: String) -> Result<(), String> {
    let d = PathBuf::from(&dir);
    fs::create_dir_all(&d).map_err(|_| format!("Can't create {dir}."))?;
    let probe = d.join(format!(".docshift_{}", std::process::id()));
    fs::write(&probe, b"").map_err(|_| format!("Can't save to {dir}. The folder is missing or read-only. Pick another one."))?;
    let _ = fs::remove_file(probe);
    Ok(())
}

#[tauri::command]
pub fn validate_target(text: String) -> Result<(), String> {
    parse_target(&text).map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn validate_pages(spec: String, count: usize) -> Result<Vec<usize>, String> {
    parse_pages(&spec, count).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn start_job(app: AppHandle, registry: State<jobs::Registry>, spec: jobs::JobSpec) -> String {
    jobs::start(app, &registry, spec)
}

#[tauri::command]
pub fn cancel_job(registry: State<jobs::Registry>, job_id: String) {
    registry.cancel(&job_id);
}
