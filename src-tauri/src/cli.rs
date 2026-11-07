//! Headless entry points used for testing and for verifying a packaged build:
//!   docshift --run <mode> <out_dir> <options-json> <input>...     run any tool, print the result as JSON
//!   docshift --selftest <dir>                                       one task per engine, writes selftest.log
use crate::{core::*, engine, jobs, office, pdfops, video};
use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Returns Some(exit code) when the command line asked for a headless action.
pub fn maybe_run() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--run") if args.len() >= 6 => Some(run_cmd(&args)),
        Some("--selftest") if args.len() >= 3 => Some(selftest(Path::new(&args[2]))),
        _ => None,
    }
}

fn run_cmd(args: &[String]) -> i32 {
    let mode = &args[2];
    let out_dir = PathBuf::from(&args[3]);
    let options: serde_json::Value = serde_json::from_str(&args[4]).unwrap_or_default();
    let inputs: Vec<PathBuf> = args[5..].iter().map(PathBuf::from).collect();
    let _com = office::ComGuard::init();
    let mut session = office::Session::default();
    let ctl = Ctl::new(Default::default(), |_| {});
    let r = jobs::run_task(mode, &inputs, &out_dir, &Opts(&options), &mut session, &ctl);
    session.close();
    match r {
        Ok(o) => {
            println!("{}", serde_json::to_string(&o).unwrap_or_default());
            0
        }
        Err(e) => {
            println!("ERROR: {}", friendly(&e));
            1
        }
    }
}

fn gradient(w: u32, h: u32, seed: u32) -> image::DynamicImage {
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        let n = ((x * 7 + y * 13 + seed * 31) % 29) as u8;
        image::Rgb([(x * 255 / w) as u8 ^ n, (y * 255 / h) as u8, ((x + y) * 255 / (w + h)) as u8 ^ (n * 3)])
    });
    image::DynamicImage::ImageRgb8(img)
}

pub fn selftest(dir: &Path) -> i32 {
    let _ = fs::create_dir_all(dir);
    let log_path = dir.join("selftest.log");
    let mut log: Vec<String> = vec![];
    let mut all_ok = true;
    let input = dir.join("input");
    let output = dir.join("output");
    let _ = fs::create_dir_all(&input);
    let _ = fs::create_dir_all(&output);

    let mut step = |name: &str, r: Result<Outcome>| match r {
        Ok(o) => log.push(format!("OK    {name:<22} -> {}  {}", o.outputs.first().map(|p| Path::new(p).file_name().unwrap_or_default().to_string_lossy().into_owned()).unwrap_or_default(), o.detail)),
        Err(e) => {
            all_ok = false;
            log.push(format!("FAIL  {name:<22} -> {}", friendly(&e)));
        }
    };

    let ctl = Ctl::none();
    let _com = office::ComGuard::init();
    let mut session = office::Session::default();
    let none = serde_json::json!({});

    // inputs
    let png = input.join("sample.png");
    let jpg = input.join("sample.jpg");
    let big = input.join("big.jpg");
    let _ = gradient(640, 480, 1).save(&png);
    let _ = gradient(1000, 700, 2).save(&jpg);
    let _ = crate::imgops::encode_jpeg(&gradient(3000, 2000, 3), 95, None).map(|b| fs::write(&big, b));
    let _ = fs::write(input.join("sample.rtf"), r"{\rtf1\ansi \b DocShift\b0  self-test paragraph.\par Second line.}");
    let _ = fs::write(input.join("sample.csv"), "a,b,c\n1,2,3\n4,5,6\n7,8,9\n");

    // images -> pdf (merged, three pages), then the PDF tools on that file
    let imgs = vec![png.clone(), jpg.clone(), big.clone()];
    let o = serde_json::json!({"page": "a4", "output_name": "three_pages"});
    let made = pdfops::images_to_pdf(&imgs, &output, "three_pages", &Opts(&o), &ctl);
    let pdf = made.as_ref().ok().and_then(|o| o.outputs.first().cloned()).map(PathBuf::from);
    step("images -> pdf", made);
    if let Some(pdf) = pdf {
        let one = |mode: &str, opts: serde_json::Value, inputs: &[PathBuf], session: &mut office::Session| jobs::run_task(mode, inputs, &output, &Opts(&opts), session, &ctl);
        let p = [pdf.clone()];
        step("pdf merge", one("pdf_merge", serde_json::json!({"output_name": "merged"}), &[pdf.clone(), pdf.clone()], &mut session));
        step("pdf extract page 2", one("pdf_pages", serde_json::json!({"tag": "extract", "groups": [[{"index": 1}]]}), &p, &mut session));
        step("pdf reorder+rotate", one("pdf_pages", serde_json::json!({"tag": "organized", "groups": [[{"index": 2, "rotate": 90}, {"index": 0}]]}), &p, &mut session));
        step("pdf split", one("pdf_pages", serde_json::json!({"tag": "part", "groups": [[{"index": 0}], [{"index": 1}, {"index": 2}]]}), &p, &mut session));
        step("pdf -> images", one("pdf2img", serde_json::json!({"format": "webp", "dpi": 72, "pages": "1-2"}), &p, &mut session));
        step("pdf -> pptx editable", one("pdf2ppt", serde_json::json!({"style": "editable", "dpi": 100}), &p, &mut session));
        step("pdf compress (target)", one("pdf_compress", serde_json::json!({"action": "compress", "target_mb": "0.05"}), &p, &mut session));
        step("pdf compress (level)", one("pdf_compress", serde_json::json!({"action": "compress", "level": "strong"}), &p, &mut session));
        step("pdf decompress", one("pdf_compress", serde_json::json!({"action": "expand"}), &p, &mut session));
        let word_pdf = one("word2pdf", none.clone(), &[input.join("sample.rtf")], &mut session);
        let wp = word_pdf.as_ref().ok().and_then(|o| o.outputs.first().cloned()).map(PathBuf::from);
        step("word -> pdf", word_pdf);
        step("excel -> pdf", one("excel2pdf", none.clone(), &[input.join("sample.csv")], &mut session));
        if let Some(wp) = wp {
            step("pdf -> excel", one("pdf2excel", none.clone(), &[wp.clone()], &mut session));
            step("pdf -> word", one("pdf2word", none.clone(), &[wp.clone()], &mut session));
            step("pdf -> word (flowing)", one("pdf2word", serde_json::json!({"layout": "flow"}), &[wp.clone()], &mut session));
            step("pdf -> text", one("pdf2txt", none.clone(), &[wp.clone()], &mut session));
        }
        step("merge + bookmarks", one("pdf_merge", serde_json::json!({"output_name": "merged_bm", "bookmarks": true, "blank": true}), &[pdf.clone(), pdf.clone()], &mut session));
        step("combine office+img", one("combine_pdf", serde_json::json!({"output_name": "combined", "pdfa": true}), &[input.join("sample.rtf"), png.clone(), input.join("sample.csv")], &mut session));
        // a "scanned" page: a picture of text, then OCR it
        if !crate::ocr::languages().is_empty() {
            let scan = output.join("scan_src.pdf");
            let made = jobs::run_task("pdf2img", &[output.join("combined.pdf")], &output, &Opts(&serde_json::json!({"format": "png", "dpi": 150, "pages": "1"})), &mut session, &ctl)
                .and_then(|o| pdfops::images_to_pdf(&[PathBuf::from(&o.outputs[0])], &output, "scan_src", &Opts(&serde_json::json!({"quality": "print"})), &ctl));
            if made.is_ok() && scan.exists() {
                step("ocr searchable pdf", one("pdf_ocr", none.clone(), &[scan.clone()], &mut session));
                step("ocr -> text", one("pdf2txt", serde_json::json!({"ocr": true}), &[scan], &mut session));
            } else {
                step("ocr (prepare scan)", made);
            }
        } else {
            step("ocr (skipped)", Ok(Outcome::default().with_detail("no Windows OCR language installed")));
        }
    }
    let one = |mode: &str, opts: serde_json::Value, inputs: &[PathBuf]| -> Result<Outcome> {
        let mut s = office::Session::default();
        jobs::run_task(mode, inputs, &output, &Opts(&opts), &mut s, &ctl)
    };
    step("image compress (target)", one("img_compress", serde_json::json!({"action": "compress", "target_mb": "0.2"}), &[big.clone()]));
    step("image -> webp", one("img_compress", serde_json::json!({"action": "compress", "format": "webp", "level": "balanced"}), &[big.clone()]));
    step("png compress", one("img_compress", serde_json::json!({"action": "compress", "level": "strong"}), &[png.clone()]));
    step("image decompress", one("img_compress", serde_json::json!({"action": "expand", "expand_format": "tiff"}), &[jpg.clone()]));
    // an image whose extension lies (PNG bytes in a .jpg) must still make a proper page
    let liar = input.join("really_png.jpg");
    let _ = fs::copy(&png, &liar);
    step("mislabeled image->pdf", one("img2pdf", serde_json::json!({"quality": "standard", "pdfa": true}), &[liar]));

    // video: generate a clip with the bundled ffmpeg, then shrink and expand it
    if let Some(ff) = engine::ffmpeg_path() {
        let clip = input.join("clip.mp4");
        let made = Command::new(&ff)
            .args(["-y", "-loglevel", "error", "-f", "lavfi", "-i", "testsrc2=size=640x360:rate=24", "-f", "lavfi", "-i", "sine=frequency=440", "-t", "2", "-pix_fmt", "yuv420p"])
            .arg(&clip)
            .status()
            .context("ffmpeg");
        if made.map(|s| s.success()).unwrap_or(false) {
            step("video compress (target)", one("video_compress", serde_json::json!({"action": "compress", "target_mb": "0.1"}), &[clip.clone()]));
            step("video compress 240p", one("video_compress", serde_json::json!({"action": "compress", "resolution": 240}), &[clip.clone()]));
            step("video decompress", one("video_compress", serde_json::json!({"action": "expand"}), &[clip.clone()]));
            let _ = video::probe(&clip).map(|p| log.push(format!("      probe: {}x{} {:.1}s audio={}", p.width, p.height, p.duration.unwrap_or(0.0), p.has_audio)));
        } else {
            all_ok = false;
            log.push("FAIL  couldn't generate the test video".into());
        }
    } else {
        all_ok = false;
        log.push("FAIL  video engine missing".into());
    }
    session.close();
    log.push(if all_ok { "ALL OK".into() } else { "FAILED".into() });
    let _ = fs::write(&log_path, log.join("\n") + "\n");
    if all_ok { 0 } else { 1 }
}
