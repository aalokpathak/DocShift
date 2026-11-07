//! File kinds. The frontend uses these to detect what was dropped and offer the right tools.
use std::path::Path;

const WORD: &[&str] = &["doc", "docx", "docm", "dot", "dotx", "rtf", "odt", "txt"];
const EXCEL: &[&str] = &["xls", "xlsx", "xlsm", "xlsb", "csv", "ods"];
const PPT: &[&str] = &["ppt", "pptx", "pptm", "pps", "ppsx", "odp"];
const IMG: &[&str] = &["jpg", "jpeg", "jfif", "png", "webp", "bmp", "gif", "tif", "tiff", "ico", "heic", "heif", "hif", "avif"];
const VIDEO: &[&str] = &["mp4", "mov", "mkv", "avi", "webm", "wmv", "flv", "m4v", "mpg", "mpeg", "3gp"];

pub fn ext_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn kind_of(path: &Path) -> Option<&'static str> {
    let e = ext_of(path);
    let e = e.as_str();
    if e == "pdf" {
        Some("pdf")
    } else if WORD.contains(&e) {
        Some("word")
    } else if EXCEL.contains(&e) {
        Some("excel")
    } else if PPT.contains(&e) {
        Some("ppt")
    } else if IMG.contains(&e) {
        Some("img")
    } else if VIDEO.contains(&e) {
        Some("video")
    } else {
        None
    }
}
