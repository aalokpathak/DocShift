//! PDFium wrapper: page counts, rendering, thumbnails and glyph extraction.
//! PDFium itself is not re-entrant, so every use goes through one lock.
use anyhow::{anyhow, bail, Result};
use image::DynamicImage;
use pdfium_render::prelude::*;
use std::{
    path::Path,
    sync::{Mutex, MutexGuard, OnceLock},
};

/// PDFium's bindings aren't marked Send/Sync, but every access in this module goes through `LOCK`.
struct Shared(Pdfium);
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

static PDFIUM: OnceLock<Result<Shared, String>> = OnceLock::new();
static LOCK: Mutex<()> = Mutex::new(());

fn pdfium() -> Result<&'static Pdfium> {
    let cell = PDFIUM.get_or_init(|| {
        let path = crate::engine::pdfium_path().map_err(|e| format!("{e:#}"))?;
        let bindings = Pdfium::bind_to_library(&path).map_err(|e| format!("Couldn't load the PDF engine: {e}"))?;
        Ok(Shared(Pdfium::new(bindings)))
    });
    cell.as_ref().map(|s| &s.0).map_err(|e| anyhow!(e.clone()))
}

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

fn open_err(e: PdfiumError) -> anyhow::Error {
    match e {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
            anyhow!("This PDF is password-protected. Remove the password and try again.")
        }
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::FormatError) => {
            anyhow!("This file could not be opened as a PDF. It may be damaged.")
        }
        other => anyhow!("This file could not be opened as a PDF ({other})."),
    }
}

/// Run `f` with a loaded document, holding the PDFium lock.
pub fn with_doc<R>(path: &Path, f: impl FnOnce(&PdfDocument) -> Result<R>) -> Result<R> {
    let _g = lock();
    let pdfium = pdfium()?;
    let doc = pdfium.load_pdf_from_file(path, None).map_err(open_err)?;
    f(&doc)
}

/// Page count, and whether the first few pages have no selectable text (a scan).
pub fn page_count_and_scanned(path: &Path) -> Result<(usize, bool)> {
    with_doc(path, |d| {
        let n = d.pages().len() as usize;
        let scanned = d.pages().iter().take(3).all(|p| p.text().map(|t| t.all().trim().chars().count() < 3).unwrap_or(true));
        Ok((n, scanned))
    })
}

/// (width, height) in points for each page, after rotation.
pub fn page_sizes(path: &Path) -> Result<Vec<(f32, f32)>> {
    with_doc(path, |d| Ok(d.pages().iter().map(|p| (p.width().value, p.height().value)).collect()))
}

pub fn render(page: &PdfPage, scale: f32) -> Result<DynamicImage> {
    let cfg = PdfRenderConfig::new().scale_page_by_factor(scale).render_form_data(true).render_annotations(true);
    let bitmap = page.render_with_config(&cfg).map_err(|e| anyhow!("Couldn't render a page: {e}"))?;
    Ok(bitmap.as_image())
}

pub fn render_to_width(page: &PdfPage, width_px: u32) -> Result<DynamicImage> {
    let cfg = PdfRenderConfig::new().set_target_width(width_px as i32).render_form_data(true);
    let bitmap = page.render_with_config(&cfg).map_err(|e| anyhow!("Couldn't render a page: {e}"))?;
    Ok(bitmap.as_image())
}

/// JPEG thumbnail of page `index` (0-based) about `width_px` wide.
pub fn thumbnail(path: &Path, index: usize, width_px: u32) -> Result<Vec<u8>> {
    with_doc(path, |d| {
        let pages = d.pages();
        if index >= pages.len() as usize {
            bail!("Page {} doesn't exist.", index + 1);
        }
        let page = pages.get(index as u16).map_err(|e| anyhow!("Couldn't read page {}: {e}", index + 1))?;
        let img = render_to_width(&page, width_px)?;
        crate::imgops::encode_jpeg(&DynamicImage::ImageRgb8(img.to_rgb8()), 78, None)
    })
}

/// One positioned character. Coordinates are in points with the origin at the page's top-left.
#[derive(Clone, Debug)]
pub struct Glyph {
    pub ch: char,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub size: f32,
    pub font: String,
    pub bold: bool,
    pub italic: bool,
    pub color: (u8, u8, u8),
    pub angle: f32,
}

pub fn glyphs(page: &PdfPage) -> Result<Vec<Glyph>> {
    let text = page.text().map_err(|e| anyhow!("Couldn't read the page text: {e}"))?;
    let page_h = page.height().value;
    let mut out = Vec::new();
    for c in text.chars().iter() {
        let Some(ch) = c.unicode_char() else { continue };
        if ch == '\u{fffe}' || ch == '\u{ffff}' {
            continue;
        }
        let Ok(b) = c.loose_bounds() else { continue };
        let font = c.font_name();
        let lower = font.to_lowercase();
        let color = c.fill_color().map(|col| (col.red(), col.green(), col.blue())).unwrap_or((0, 0, 0));
        let weight_bold = matches!(c.font_weight(), Some(PdfFontWeight::Weight700Bold) | Some(PdfFontWeight::Weight800) | Some(PdfFontWeight::Weight900));
        out.push(Glyph {
            ch,
            x0: b.left().value,
            y0: page_h - b.top().value,
            x1: b.right().value,
            y1: page_h - b.bottom().value,
            size: c.scaled_font_size().value,
            bold: weight_bold || lower.contains("bold") || lower.contains("black"),
            italic: c.font_is_italic() || lower.contains("italic") || lower.contains("oblique"),
            font,
            color,
            angle: c.angle_degrees().unwrap_or(0.0),
        });
    }
    Ok(out)
}
