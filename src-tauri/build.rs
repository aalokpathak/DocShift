//! Compresses the bundled engines (ffmpeg, pdfium) with zstd so they can be embedded in the executable and
//! unpacked on first use. That keeps the portable build a single file.
use std::{env, fs, io, path::PathBuf};

const ENGINES: &[&str] = &["ffmpeg.exe", "pdfium.dll"];

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    for name in ENGINES {
        let src = manifest.join("binaries").join(name);
        println!("cargo:rerun-if-changed={}", src.display());
        let dst = out.join(format!("{name}.zst"));
        if !src.exists() {
            panic!(
                "missing engine binary {}.\nRun scripts/fetch-engines.ps1 once to download ffmpeg and pdfium.",
                src.display()
            );
        }
        let fresh = match (fs::metadata(&src).and_then(|m| m.modified()), fs::metadata(&dst).and_then(|m| m.modified())) {
            (Ok(s), Ok(d)) => d >= s,
            _ => false,
        };
        if fresh {
            continue;
        }
        let mut input = io::BufReader::new(fs::File::open(&src).unwrap());
        let output = io::BufWriter::new(fs::File::create(&dst).unwrap());
        let mut enc = zstd::stream::Encoder::new(output, 15).unwrap();
        io::copy(&mut input, &mut enc).unwrap();
        enc.finish().unwrap();
    }
    tauri_build::build();
}
