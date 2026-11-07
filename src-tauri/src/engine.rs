//! The native engines DocShift ships inside its own executable (so the portable build is a single file):
//! ffmpeg for video and pdfium for rendering PDFs. They are unpacked once into the user's local app data.
use anyhow::{Context, Result};
use std::{
    fs,
    path::PathBuf,
    sync::OnceLock,
};

static FFMPEG_ZST: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ffmpeg.exe.zst"));
static PDFIUM_ZST: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pdfium.dll.zst"));

fn engine_dir() -> PathBuf {
    // The compressed sizes act as a content version: a new engine build lands in a fresh folder.
    let base = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir);
    base.join("DocShift").join("engine").join(format!("{}-{}", FFMPEG_ZST.len(), PDFIUM_ZST.len()))
}

fn unpack(name: &str, zst: &[u8]) -> Result<PathBuf> {
    let dir = engine_dir();
    let target = dir.join(name);
    if target.metadata().map(|m| m.len() > 1_000_000).unwrap_or(false) {
        return Ok(target);
    }
    fs::create_dir_all(&dir).with_context(|| format!("Can't create {}", dir.display()))?;
    let part = dir.join(format!("{name}.{}.part", std::process::id()));
    {
        let mut out = fs::File::create(&part)?;
        zstd::stream::copy_decode(zst, &mut out).context("Couldn't unpack a bundled component")?;
    }
    if fs::rename(&part, &target).is_err() {
        // another DocShift instance won the race, which is fine
        let _ = fs::remove_file(&part);
    }
    Ok(target)
}

pub fn ffmpeg_path() -> Option<PathBuf> {
    static CELL: OnceLock<Option<PathBuf>> = OnceLock::new();
    CELL.get_or_init(|| unpack("ffmpeg.exe", FFMPEG_ZST).ok()).clone()
}

pub fn pdfium_path() -> Result<PathBuf> {
    static CELL: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    CELL.get_or_init(|| unpack("pdfium.dll", PDFIUM_ZST).map_err(|e| format!("{e:#}"))).clone().map_err(anyhow::Error::msg)
}

/// Unpack both engines on a background thread at startup so the first video / PDF preview doesn't wait.
pub fn warm_up() {
    std::thread::spawn(|| {
        let _ = pdfium_path();
        let _ = ffmpeg_path();
    });
}
