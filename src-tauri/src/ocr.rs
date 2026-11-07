//! Text recognition for scanned pages, using the OCR engine built into Windows 10/11 (offline, no download).
//! Recognised words are written into the PDF as an invisible text layer (rendering mode 3) exactly over the
//! picture of each word, which makes scanned pages searchable and lets every other tool (Word, Excel, PowerPoint,
//! plain text) read them like a normal PDF.
use crate::{core::*, pdfops, render};
use anyhow::{anyhow, bail, Context, Result};
use image::DynamicImage;
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use serde::Serialize;
use std::{fs, path::Path};
use windows::{
    core::HSTRING,
    Globalization::Language,
    Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap},
    Media::Ocr::OcrEngine,
    Storage::Streams::DataWriter,
};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OcrLanguage {
    pub tag: String,
    pub name: String,
}

/// Languages Windows can recognise on this computer (one per installed language pack with OCR support).
pub fn languages() -> Vec<OcrLanguage> {
    let Ok(list) = OcrEngine::AvailableRecognizerLanguages() else { return vec![] };
    list.into_iter()
        .filter_map(|l| Some(OcrLanguage { tag: l.LanguageTag().ok()?.to_string(), name: l.DisplayName().ok()?.to_string() }))
        .collect()
}

fn engine(tag: &str) -> Result<OcrEngine> {
    if !tag.is_empty() {
        if let Ok(lang) = Language::CreateLanguage(&HSTRING::from(tag)) {
            if OcrEngine::IsLanguageSupported(&lang).unwrap_or(false) {
                if let Ok(e) = OcrEngine::TryCreateFromLanguage(&lang) {
                    return Ok(e);
                }
            }
        }
    }
    OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| {
        anyhow!("Text recognition isn't available: Windows has no OCR language installed. Add a language in Windows Settings > Time & language > Language & region.")
    })
}

/// One recognised word, in pixels of the image it was read from.
#[derive(Clone, Debug)]
pub struct OcrWord {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Words grouped into lines, in reading order.
pub fn recognize(img: &DynamicImage, tag: &str) -> Result<Vec<Vec<OcrWord>>> {
    let eng = engine(tag)?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut bgra = rgba.into_raw();
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let writer = DataWriter::new()?;
    writer.WriteBytes(&bgra)?;
    let buffer = writer.DetachBuffer()?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, w as i32, h as i32)?;
    let result = eng.RecognizeAsync(&bitmap)?.get().context("Text recognition failed")?;
    let mut lines = vec![];
    for line in result.Lines()? {
        let mut words = vec![];
        for word in line.Words()? {
            let r = word.BoundingRect()?;
            let text = word.Text()?.to_string();
            if !text.trim().is_empty() {
                words.push(OcrWord { text, x: r.X, y: r.Y, w: r.Width, h: r.Height });
            }
        }
        if !words.is_empty() {
            lines.push(words);
        }
    }
    Ok(lines)
}

// ------------------------------------------------------------------------------------------ invisible text layer

/// A font for invisible text: Identity-H with CID = Unicode code unit, plus a ToUnicode map so copy, search and
/// text extraction give the real characters. It is never drawn, so it needs no glyphs.
fn add_font(doc: &mut Document) -> ObjectId {
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let highs: Vec<u32> = (0..=0xFFu32).filter(|h| !(0xD8..=0xDF).contains(h)).collect();
    for chunk in highs.chunks(100) {
        cmap.push_str(&format!("{} beginbfrange\n", chunk.len()));
        for h in chunk {
            cmap.push_str(&format!("<{h:02X}00> <{h:02X}FF> <{h:02X}00>\n"));
        }
        cmap.push_str("endbfrange\n");
    }
    cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    let mut tu = Stream::new(dictionary! {}, cmap.into_bytes());
    let _ = tu.compress();
    let tu = doc.add_object(tu);
    let fd = doc.add_object(dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => "GlyphLessFont",
        "Flags" => 5,
        "FontBBox" => vec![0.into(), (-200).into(), 500.into(), 800.into()],
        "ItalicAngle" => 0,
        "Ascent" => 800,
        "Descent" => -200,
        "CapHeight" => 700,
        "StemV" => 80,
    });
    let cid = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => "GlyphLessFont",
        "CIDSystemInfo" => dictionary! { "Registry" => Object::string_literal("Adobe"), "Ordering" => Object::string_literal("Identity"), "Supplement" => 0 },
        "FontDescriptor" => fd,
        "DW" => 500,
        "CIDToGIDMap" => "Identity",
    });
    doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "GlyphLessFont",
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![Object::Reference(cid)],
        "ToUnicode" => tu,
    })
}

fn num(o: &Object) -> Option<f32> {
    o.as_float().ok().or_else(|| o.as_i64().ok().map(|v| v as f32))
}

/// The visible box (CropBox, else MediaBox) as (x0, y0, width, height), and /Rotate.
fn page_geometry(doc: &Document, id: ObjectId) -> ((f32, f32, f32, f32), i64) {
    let get = |key: &[u8]| -> Option<Object> {
        let mut cur = id;
        for _ in 0..32 {
            let d = doc.get_dictionary(cur).ok()?;
            if let Ok(v) = d.get(key) {
                return Some(match v {
                    Object::Reference(r) => doc.get_object(*r).ok()?.clone(),
                    o => o.clone(),
                });
            }
            cur = d.get(b"Parent").ok()?.as_reference().ok()?;
        }
        None
    };
    let rect = |o: Option<Object>| -> Option<(f32, f32, f32, f32)> {
        let a = o?.as_array().ok()?.clone();
        let v: Vec<f32> = a.iter().filter_map(num).collect();
        (v.len() == 4).then(|| (v[0].min(v[2]), v[1].min(v[3]), (v[2] - v[0]).abs(), (v[3] - v[1]).abs()))
    };
    let b = rect(get(b"CropBox")).or_else(|| rect(get(b"MediaBox"))).unwrap_or((0.0, 0.0, 612.0, 792.0));
    let rot = get(b"Rotate").and_then(|o| o.as_i64().ok()).unwrap_or(0).rem_euclid(360);
    (b, rot)
}

/// Content stream drawing the words invisibly. `scale` = image pixels per point of the rendered (rotated) page.
fn text_layer(lines: &[Vec<OcrWord>], scale: f32, b: (f32, f32, f32, f32), rot: i64) -> Vec<u8> {
    let (x0, y0, w, h) = b;
    // view (rotated, y down, points) -> unrotated user space: u = A v + t
    let (a, t): ([f32; 4], (f32, f32)) = match rot {
        90 => ([0.0, 1.0, 1.0, 0.0], (x0, y0)),
        180 => ([-1.0, 0.0, 0.0, 1.0], (x0 + w, y0)),
        270 => ([0.0, -1.0, -1.0, 0.0], (x0 + w, y0 + h)),
        _ => ([1.0, 0.0, 0.0, -1.0], (x0, y0 + h)),
    };
    let [a11, a12, a21, a22] = a;
    let mut s = String::from("BT\n3 Tr\n/FOCR 1 Tf\n");
    for line in lines {
        // Typeset like real text: one size and baseline per line (the line's full ink height = 1 em, with the
        // font's 0.2 em descent below the baseline), and each word stretched to leave a normal 0.25 em space before
        // the next one. Exact-ink boxes would leave wide gaps that read as column breaks.
        let f = line.iter().map(|w| w.h).fold(0.0f32, f32::max).max(1.0) / scale;
        let bottom = line.iter().map(|w| w.y + w.h).fold(0.0f32, f32::max) / scale;
        let oy = bottom - 0.2 * f; // baseline
        for (i, word) in line.iter().enumerate() {
            let chars: Vec<u16> = word.text.encode_utf16().filter(|u| !(0xD800..=0xDFFF).contains(u)).collect();
            if chars.is_empty() {
                continue;
            }
            let (vx, mut ww) = (word.x / scale, word.w / scale);
            if let Some(next) = line.get(i + 1) {
                let gap = next.x / scale - (vx + ww);
                if gap > 0.25 * f && gap < 1.2 * f {
                    ww += gap - 0.25 * f;
                }
            }
            let k = ww.max(0.5) / (chars.len() as f32 * 0.5 * f);
            let ox = vx;
            let m = [a11 * k * f, a21 * k * f, -a12 * f, -a22 * f, a11 * ox + a12 * oy + t.0, a21 * ox + a22 * oy + t.1];
            let mut hex = String::with_capacity(chars.len() * 4 + 4);
            for u in &chars {
                hex.push_str(&format!("{u:04X}"));
            }
            if i + 1 < line.len() {
                hex.push_str("0020"); // real spaces between words, so extracted text reads naturally
            }
            s.push_str(&format!("{:.4} {:.4} {:.4} {:.4} {:.3} {:.3} Tm <{hex}> Tj\n", m[0], m[1], m[2], m[3], m[4], m[5]));
        }
    }
    s.push_str("ET\n");
    s.into_bytes()
}

/// Resolves a dictionary that may be inline or referenced, as an owned copy.
fn owned_dict(doc: &Document, o: Option<&Object>) -> Dictionary {
    match o {
        Some(Object::Dictionary(d)) => d.clone(),
        Some(Object::Reference(r)) => doc.get_dictionary(*r).cloned().unwrap_or_default(),
        _ => Dictionary::new(),
    }
}

fn add_layer_to_page(doc: &mut Document, page: ObjectId, font: ObjectId, layer: Vec<u8>) -> Result<()> {
    // Wrap the existing content in q/Q so whatever state it leaves behind can't move our text.
    let open = doc.add_object(Stream::new(dictionary! {}, b"q\n".to_vec()));
    let mut close_and_text = b"Q\n".to_vec();
    close_and_text.extend(layer);
    let mut text = Stream::new(dictionary! {}, close_and_text);
    let _ = text.compress();
    let text = doc.add_object(text);
    let (contents, resources) = {
        let d = doc.get_dictionary(page)?;
        (d.get(b"Contents").ok().cloned(), d.get(b"Resources").ok().cloned())
    };
    let mut list: Vec<Object> = vec![Object::Reference(open)];
    match contents {
        Some(Object::Array(a)) => list.extend(a),
        Some(o @ Object::Reference(_)) => {
            // a reference may point at an array of streams
            let inner = o.as_reference().ok().and_then(|r| doc.get_object(r).ok()).and_then(|x| x.as_array().ok().cloned());
            match inner {
                Some(a) => list.extend(a),
                None => list.push(o),
            }
        }
        _ => {}
    }
    list.push(Object::Reference(text));
    // Resources are often shared between pages: give this page its own copy with the font added.
    let mut res = owned_dict(doc, resources.as_ref());
    let mut fonts = owned_dict(doc, res.get(b"Font").ok());
    fonts.set("FOCR", font);
    res.set("Font", fonts);
    let d = doc.get_dictionary_mut(page)?;
    d.set("Contents", list);
    d.set("Resources", res);
    Ok(())
}

/// Pages (0-based) that have no selectable text.
pub fn pages_without_text(src: &Path) -> Result<Vec<usize>> {
    render::with_doc(src, |d| {
        Ok(d.pages().iter().enumerate().filter(|(_, p)| p.text().map(|t| t.all().trim().chars().count() < 3).unwrap_or(true)).map(|(i, _)| i).collect())
    })
}

/// Writes a copy of `src` with recognised text added to every page that has none. Returns the number of pages read.
/// `lo..hi` is the slice of the progress bar this step may use.
pub fn searchable_copy(src: &Path, dst: &Path, tag: &str, ctl: &Ctl, lo: f64, hi: f64) -> Result<usize> {
    let todo = pages_without_text(src)?;
    let mut doc = pdfops::load(src)?;
    if todo.is_empty() {
        fs::copy(src, dst)?;
        return Ok(0);
    }
    engine(tag)?; // fail early and clearly if Windows has no OCR language
    let max_dim = OcrEngine::MaxImageDimension().unwrap_or(10_000).max(1000) as f32;
    let page_ids: Vec<ObjectId> = doc.get_pages().into_values().collect();
    let font = add_font(&mut doc);
    let mut done = 0;
    for (n, &i) in todo.iter().enumerate() {
        ctl.check()?;
        ctl.report(lo + (hi - lo) * n as f64 / todo.len() as f64);
        // ~300 dpi reads small print well; stay inside what the engine accepts
        let (img, scale) = render::with_doc(src, |d| {
            let page = d.pages().get(i as u16).map_err(|e| anyhow!("Couldn't read page {}: {e}", i + 1))?;
            let (pw, ph) = (page.width().value, page.height().value);
            let scale = (300.0 / 72.0f32).min(max_dim / pw.max(ph));
            Ok((render::render(&page, scale)?, scale))
        })?;
        let lines = recognize(&img, tag)?;
        if lines.is_empty() {
            continue;
        }
        let Some(&pid) = page_ids.get(i) else { continue };
        let (b, rot) = page_geometry(&doc, pid);
        add_layer_to_page(&mut doc, pid, font, text_layer(&lines, scale, b, rot))?;
        done += 1;
    }
    pdfops::save_doc_to(&mut doc, dst)?;
    ctl.report(hi);
    Ok(done)
}

/// "Searchable PDF": the same pages with invisible recognised text.
pub fn make_searchable(src: &Path, out_dir: &Path, opts: &Opts, ctl: &Ctl) -> Result<Outcome> {
    let tmp = TempDir::new()?;
    let file = tmp.join("out.pdf");
    let n = searchable_copy(src, &file, &opts.str("ocr_lang", ""), ctl, 0.0, 0.95)?;
    if n == 0 && pages_without_text(src)?.is_empty() {
        bail!("This PDF already has selectable text on every page, so there's nothing to recognise.");
    }
    if opts.bool("pdfa", false) {
        crate::pdfa::apply_file(&file, &stem_of(src))?;
    }
    let out = publish(&file, out_dir, &format!("{}_searchable", stem_of(src)), ".pdf")?;
    let note = if n == 0 { "No text was found on the scanned pages.".to_string() } else { format!("Text recognised on {n} page(s). You can now search, select and copy it.") };
    Ok(Outcome::one(&out).with_note(note))
}
