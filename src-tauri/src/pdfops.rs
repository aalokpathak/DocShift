//! PDF engine built on lopdf: merge, pick/reorder/rotate/delete pages, split, compress (image re-encoding with a
//! size-targeted search), decompress, and building PDFs from images.
use crate::{
    core::*,
    imgops::{self, best_quality},
    render,
};
use anyhow::{anyhow, bail, Result};
use image::{DynamicImage, GenericImageView, ImageDecoder};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use rayon::prelude::*;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

// ------------------------------------------------------------------------------------------------ loading

pub fn load(path: &Path) -> Result<Document> {
    let mut doc = Document::load(path).map_err(|e| {
        let m = e.to_string().to_lowercase();
        if m.contains("password") || m.contains("encrypt") {
            anyhow!("This PDF is password-protected. Remove the password and try again.")
        } else {
            anyhow!("This file could not be opened as a PDF. It may be damaged.")
        }
    })?;
    if doc.is_encrypted() {
        // Many "protected" PDFs only restrict printing/copying and open with an empty password.
        if doc.decrypt("").is_err() {
            bail!("This PDF is password-protected. Remove the password and try again.");
        }
    }
    if doc.get_pages().is_empty() {
        bail!("This PDF has no pages.");
    }
    Ok(doc)
}

fn save_doc(doc: &mut Document, path: &Path) -> Result<()> {
    doc.save(path).map_err(|e| anyhow!("Couldn't write the PDF: {e}"))?;
    Ok(())
}

/// Look up an inheritable page attribute (Resources, MediaBox, Rotate ...).
fn inherited(doc: &Document, page: ObjectId, key: &[u8]) -> Option<Object> {
    let mut id = page;
    for _ in 0..32 {
        let d = doc.get_dictionary(id).ok()?;
        if let Ok(v) = d.get(key) {
            return Some(v.clone());
        }
        id = d.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}

/// Copies inherited attributes onto each page so pages can be moved between page trees safely.
fn flatten_inherited(doc: &mut Document) {
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    for id in pages {
        let mut add: Vec<(&[u8], Object)> = vec![];
        for key in [&b"Resources"[..], b"MediaBox", b"CropBox", b"Rotate"] {
            let has = doc.get_dictionary(id).map(|d| d.has(key)).unwrap_or(true);
            if !has {
                if let Some(v) = inherited(doc, id, key) {
                    add.push((key, v));
                }
            }
        }
        if let Ok(d) = doc.get_dictionary_mut(id) {
            for (k, v) in add {
                d.set(k, v);
            }
        }
    }
}

fn num(o: &Object) -> Option<f32> {
    o.as_float().ok().or_else(|| o.as_i64().ok().map(|v| v as f32))
}

/// (width, height) in points of a page's MediaBox, honouring /Rotate.
fn page_size(doc: &Document, id: ObjectId) -> (f32, f32) {
    let mb = inherited(doc, id, b"MediaBox").and_then(|o| match o {
        Object::Array(a) => Some(a),
        Object::Reference(r) => doc.get_object(r).ok().and_then(|o| o.as_array().ok().cloned()),
        _ => None,
    });
    let (mut w, mut h) = match mb {
        Some(a) if a.len() == 4 => {
            let v: Vec<f32> = a.iter().filter_map(num).collect();
            if v.len() == 4 { ((v[2] - v[0]).abs(), (v[3] - v[1]).abs()) } else { (595.0, 842.0) }
        }
        _ => (595.0, 842.0),
    };
    let rot = inherited(doc, id, b"Rotate").and_then(|o| o.as_i64().ok()).unwrap_or(0).rem_euclid(360);
    if rot == 90 || rot == 270 {
        std::mem::swap(&mut w, &mut h);
    }
    (w, h)
}

// ------------------------------------------------------------------------------------------------- merge

/// A PDF text string: plain bytes for ASCII, UTF-16BE with a BOM otherwise.
pub fn pdf_text(s: &str) -> Object {
    if s.is_ascii() {
        Object::String(s.as_bytes().to_vec(), lopdf::StringFormat::Literal)
    } else {
        let mut b = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            b.extend_from_slice(&u.to_be_bytes());
        }
        Object::String(b, lopdf::StringFormat::Hexadecimal)
    }
}

/// Joins documents in order. `titles` (one per part) become top-level bookmarks when `bookmarks` is set;
/// `blank` puts an empty page (the size of the previous page) between parts.
pub fn merge_docs(parts: Vec<Document>, titles: &[String], bookmarks: bool, blank: bool, ctl: &Ctl) -> Result<(Document, usize)> {
    let mut objects: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut page_order: Vec<ObjectId> = vec![];
    let mut starts: Vec<ObjectId> = vec![];
    let mut blanks: Vec<(usize, (f32, f32))> = vec![];
    let mut max_id = 1u32;
    let n = parts.len();
    for (i, mut doc) in parts.into_iter().enumerate() {
        ctl.check()?;
        flatten_inherited(&mut doc);
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;
        let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
        if blank && i > 0 {
            let size = page_order.last().map(|id| page_size_in(&objects, *id)).unwrap_or((595.0, 842.0));
            blanks.push((page_order.len(), size));
            page_order.push((0, 0)); // placeholder, filled in below
        }
        starts.push(pages[0]);
        page_order.extend(pages);
        objects.extend(doc.objects);
    }
    let mut out = Document::with_version("1.7");
    for (id, obj) in objects {
        match obj.type_name().unwrap_or(b"") {
            b"Catalog" | b"Pages" | b"Outlines" | b"Outline" => {}
            _ => {
                out.objects.insert(id, obj);
            }
        }
    }
    out.max_id = max_id;
    let pages_id = out.new_object_id();
    for (pos, (w, h)) in blanks {
        let id = out.add_object(dictionary! {
            "Type" => "Page",
            "MediaBox" => vec![0.into(), 0.into(), w.into(), h.into()],
            "Resources" => dictionary! {},
        });
        page_order[pos] = id;
    }
    for id in &page_order {
        if let Ok(Object::Dictionary(d)) = out.get_object_mut(*id) {
            d.set("Parent", pages_id);
        }
    }
    out.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => page_order.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>(),
            "Count" => page_order.len() as i64,
        }),
    );
    let mut catalog = dictionary! { "Type" => "Catalog", "Pages" => pages_id };
    if bookmarks && n > 0 {
        let outlines_id = out.new_object_id();
        let ids: Vec<ObjectId> = (0..n).map(|_| out.new_object_id()).collect();
        for (i, id) in ids.iter().enumerate() {
            let mut item = dictionary! {
                "Title" => pdf_text(titles.get(i).map(String::as_str).unwrap_or("Document")),
                "Parent" => outlines_id,
                "Dest" => vec![Object::Reference(starts[i]), "Fit".into()],
            };
            if i > 0 {
                item.set("Prev", ids[i - 1]);
            }
            if i + 1 < n {
                item.set("Next", ids[i + 1]);
            }
            out.objects.insert(*id, Object::Dictionary(item));
        }
        out.objects.insert(
            outlines_id,
            Object::Dictionary(dictionary! { "Type" => "Outlines", "First" => ids[0], "Last" => ids[n - 1], "Count" => n as i64 }),
        );
        catalog.set("Outlines", outlines_id);
        catalog.set("PageMode", "UseOutlines");
    }
    let catalog_id = out.add_object(catalog);
    out.trailer.set("Root", catalog_id);
    out.prune_objects();
    let count = page_order.len();
    Ok((out, count))
}

fn page_size_in(objects: &BTreeMap<ObjectId, Object>, id: ObjectId) -> (f32, f32) {
    let mb = objects.get(&id).and_then(|o| o.as_dict().ok()).and_then(|d| d.get(b"MediaBox").ok()).and_then(|o| match o {
        Object::Array(a) => Some(a.clone()),
        Object::Reference(r) => objects.get(r).and_then(|o| o.as_array().ok().cloned()),
        _ => None,
    });
    match mb {
        Some(a) if a.len() == 4 => {
            let v: Vec<f32> = a.iter().filter_map(num).collect();
            if v.len() == 4 { ((v[2] - v[0]).abs(), (v[3] - v[1]).abs()) } else { (595.0, 842.0) }
        }
        _ => (595.0, 842.0),
    }
}

pub fn merge(inputs: &[PathBuf], out_dir: &Path, name: &str, opts: &Opts, ctl: &Ctl) -> Result<Outcome> {
    if inputs.len() < 2 {
        bail!("Add at least two PDFs to merge.");
    }
    let mut total_in = 0u64;
    let mut parts = vec![];
    for (i, p) in inputs.iter().enumerate() {
        ctl.check()?;
        ctl.report(i as f64 / inputs.len() as f64 * 0.7);
        total_in += fs::metadata(p)?.len();
        parts.push(load(p).map_err(|e| anyhow!("{}: {}", p.file_name().unwrap_or_default().to_string_lossy(), e))?);
    }
    let titles: Vec<String> = inputs.iter().map(|p| stem_of(p)).collect();
    let blank = opts.bool("blank", false);
    let (mut out, pages) = merge_docs(parts, &titles, opts.bool("bookmarks", true), blank, ctl)?;
    ctl.report(0.9);
    let tmp = TempDir::new()?;
    let file = tmp.join("out.pdf");
    save_doc(&mut out, &file)?;
    let size = fs::metadata(&file)?.len();
    let dst = publish(&file, out_dir, &safe_stem(name), ".pdf")?;
    Ok(Outcome::one(&dst)
        .with_note(format!("{} PDFs, {} pages{}.", inputs.len(), pages, if blank { " including blank separator pages" } else { "" }))
        .with_detail(format!("{} files ({}) \u{2192} {}, {} pages", inputs.len(), fmt_size(total_in), fmt_size(size), pages)))
}

// ------------------------------------------------------------------------------------------ page building

#[derive(Deserialize, Clone, Debug)]
pub struct PageSel {
    pub index: usize,
    #[serde(default)]
    pub rotate: i32,
}

fn assemble(mut doc: Document, sel: &[PageSel]) -> Result<Document> {
    if sel.is_empty() {
        bail!("No pages were selected.");
    }
    flatten_inherited(&mut doc);
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    let root = doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"Pages").ok())
        .and_then(|p| p.as_reference().ok())
        .ok_or_else(|| anyhow!("This PDF has an unusual structure and can't be edited."))?;
    let mut kids: Vec<Object> = vec![];
    let mut seen: HashSet<ObjectId> = HashSet::new();
    for s in sel {
        let id = *pages.get(s.index).ok_or_else(|| anyhow!("Page {} doesn't exist in this PDF.", s.index + 1))?;
        let mut pid = id;
        if !seen.insert(id) {
            let copy = doc.get_object(id)?.clone();
            pid = doc.add_object(copy); // the same page used twice needs its own object
        }
        if let Ok(Object::Dictionary(d)) = doc.get_object_mut(pid) {
            d.set("Parent", root);
            if s.rotate != 0 {
                let cur = d.get(b"Rotate").ok().and_then(|o| o.as_i64().ok()).unwrap_or(0);
                d.set("Rotate", Object::Integer((cur + s.rotate as i64).rem_euclid(360)));
            }
        }
        kids.push(Object::Reference(pid));
    }
    if let Ok(Object::Dictionary(d)) = doc.get_object_mut(root) {
        d.set("Count", kids.len() as i64);
        d.set("Kids", kids);
    }
    if let Ok(cat) = doc.catalog_mut() {
        for k in [&b"Outlines"[..], b"OpenAction", b"Names", b"Dests", b"PageLabels", b"StructTreeRoot"] {
            cat.remove(k);
        }
    }
    doc.prune_objects();
    Ok(doc)
}

/// Writes one PDF per group of pages. Extract / delete / reorder / rotate use a single group; split uses several.
pub fn build_pages(src: &Path, groups: &[Vec<PageSel>], out_dir: &Path, tag: &str, ctl: &Ctl) -> Result<Outcome> {
    let original = load(src)?;
    let before = fs::metadata(src)?.len();
    let stem = stem_of(src);
    let tmp = TempDir::new()?;
    let mut outputs = vec![];
    let mut total_pages = 0usize;
    let digits = groups.len().to_string().len().max(2);
    for (i, g) in groups.iter().enumerate() {
        ctl.check()?;
        ctl.report(i as f64 / groups.len() as f64);
        let mut doc = assemble(original.clone(), g)?;
        let file = tmp.join(&format!("part{i}.pdf"));
        save_doc(&mut doc, &file)?;
        total_pages += g.len();
        let name = if groups.len() == 1 { format!("{stem}_{tag}") } else { format!("{stem}_{tag}{:0width$}", i + 1, width = digits) };
        outputs.push(publish(&file, out_dir, &name, ".pdf")?.to_string_lossy().into_owned());
    }
    let after: u64 = outputs.iter().filter_map(|p| fs::metadata(p).ok()).map(|m| m.len()).sum();
    let detail = if groups.len() == 1 {
        format!("{total_pages} page(s) \u{2192} {}", fmt_size(after))
    } else {
        format!("{} files, {total_pages} pages in total", groups.len())
    };
    let _ = before;
    Ok(Outcome { outputs, note: String::new(), detail })
}

// --------------------------------------------------------------------------------------------- compress

/// An image inside a PDF that we are willing to re-encode.
struct Item {
    id: ObjectId,
    img: DynamicImage,
    /// Longest side, in inches, of the largest place it appears on a page (an upper bound for its useful pixels).
    max_inches: f32,
    orig_len: usize,
}

fn dict_of<'a>(doc: &'a Document, o: &'a Object) -> Option<&'a Dictionary> {
    match o {
        Object::Dictionary(d) => Some(d),
        Object::Reference(r) => doc.get_object(*r).ok().and_then(|o| o.as_dict().ok()),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

fn colorspace_components(doc: &Document, cs: &Object) -> Option<u8> {
    match cs {
        Object::Name(n) => match n.as_slice() {
            b"DeviceGray" | b"CalGray" => Some(1),
            b"DeviceRGB" | b"CalRGB" => Some(3),
            _ => None,
        },
        Object::Reference(r) => colorspace_components(doc, doc.get_object(*r).ok()?),
        Object::Array(a) => {
            let first = a.first()?.as_name().ok()?;
            if first == b"ICCBased" {
                let prof = match a.get(1)? {
                    Object::Reference(r) => doc.get_object(*r).ok()?,
                    o => o,
                };
                let n = prof.as_stream().ok()?.dict.get(b"N").ok()?.as_i64().ok()?;
                if n == 1 || n == 3 { Some(n as u8) } else { None }
            } else {
                colorspace_components(doc, &Object::Name(first.to_vec()))
            }
        }
        _ => None,
    }
}

fn decode_pdf_image(doc: &Document, stream: &Stream) -> Option<(DynamicImage, bool)> {
    let d = &stream.dict;
    if d.get(b"ImageMask").ok().and_then(|o| o.as_bool().ok()).unwrap_or(false) || d.has(b"Decode") {
        return None;
    }
    if d.get(b"BitsPerComponent").ok()?.as_i64().ok()? != 8 {
        return None;
    }
    let (w, h) = (d.get(b"Width").ok()?.as_i64().ok()? as u32, d.get(b"Height").ok()?.as_i64().ok()? as u32);
    let comps = colorspace_components(doc, d.get(b"ColorSpace").ok()?)?;
    let filters = stream.filters().ok()?;
    if filters.len() != 1 {
        return None;
    }
    let img = match filters[0] {
        b"DCTDecode" => {
            let img = image::load_from_memory_with_format(&stream.content, image::ImageFormat::Jpeg).ok()?;
            if img.width() != w || img.height() != h {
                return None;
            }
            if comps == 1 { DynamicImage::ImageLuma8(img.to_luma8()) } else { DynamicImage::ImageRgb8(img.to_rgb8()) }
        }
        b"FlateDecode" => {
            if let Ok(p) = d.get(b"DecodeParms") {
                if !matches!(p, Object::Dictionary(_)) {
                    return None;
                }
            }
            let raw = stream.decompressed_content().ok()?;
            if raw.len() != (w as usize) * (h as usize) * comps as usize {
                return None;
            }
            if comps == 1 {
                DynamicImage::ImageLuma8(image::GrayImage::from_raw(w, h, raw)?)
            } else {
                DynamicImage::ImageRgb8(image::RgbImage::from_raw(w, h, raw)?)
            }
        }
        _ => return None,
    };
    Some((img, comps == 1))
}

fn collect_images(doc: &Document) -> Vec<Item> {
    let mut best: HashMap<ObjectId, f32> = HashMap::new();
    fn walk(doc: &Document, resources: Option<&Dictionary>, page_inches: f32, depth: u8, best: &mut HashMap<ObjectId, f32>) {
        let Some(res) = resources else { return };
        let Some(xo) = res.get(b"XObject").ok().and_then(|o| dict_of(doc, o)) else { return };
        for (_, v) in xo.iter() {
            let Ok(id) = v.as_reference() else { continue };
            let Ok(Object::Stream(s)) = doc.get_object(id) else { continue };
            match s.dict.get(b"Subtype").ok().and_then(|o| o.as_name().ok()) {
                Some(b"Image") => {
                    let e = best.entry(id).or_insert(0.0);
                    *e = e.max(page_inches);
                }
                Some(b"Form") if depth < 3 => {
                    let r = s.dict.get(b"Resources").ok().and_then(|o| dict_of(doc, o));
                    walk(doc, r, page_inches, depth + 1, best);
                }
                _ => {}
            }
        }
    }
    for (_, pid) in doc.get_pages() {
        let (w, h) = page_size(doc, pid);
        let res = inherited(doc, pid, b"Resources");
        let res_dict = res.as_ref().and_then(|o| dict_of(doc, o));
        walk(doc, res_dict, w.max(h) / 72.0, 0, &mut best);
    }
    let mut items = vec![];
    for (id, inches) in best {
        let Ok(Object::Stream(s)) = doc.get_object(id) else { continue };
        if s.content.len() < 4096 {
            continue; // tiny images (icons, rules) aren't worth touching
        }
        if let Some((img, _gray)) = decode_pdf_image(doc, s) {
            items.push(Item { id, img, max_inches: inches.max(1.0), orig_len: s.content.len() });
        }
    }
    items.sort_by_key(|i| i.id);
    items
}

/// Re-encode every item at `dpi` / `quality` into a copy of the document and return the resulting file bytes.
fn recompress(base: &Document, items: &[Item], dpi: u32, quality: u8) -> Result<Vec<u8>> {
    let encoded: Vec<Option<(ObjectId, Vec<u8>, u32, u32)>> = items
        .par_iter()
        .map(|it| {
            let max_px = (it.max_inches * dpi as f32).round().max(16.0) as u32;
            let long = it.img.width().max(it.img.height());
            let work = if long as f32 > max_px as f32 * 1.05 { imgops::scaled(&it.img, max_px as f64 / long as f64) } else { it.img.clone() };
            let data = imgops::encode_jpeg(&work, quality, None).ok()?;
            if (data.len() as f64) < it.orig_len as f64 * 0.95 {
                Some((it.id, data, work.width(), work.height()))
            } else {
                None
            }
        })
        .collect();
    let mut doc = base.clone();
    for (id, data, w, h) in encoded.into_iter().flatten() {
        if let Ok(Object::Stream(s)) = doc.get_object_mut(id) {
            s.dict.set("Filter", "DCTDecode");
            s.dict.remove(b"DecodeParms");
            s.dict.set("Width", w as i64);
            s.dict.set("Height", h as i64);
            s.set_content(data);
        }
    }
    doc.prune_objects();
    doc.delete_zero_length_streams();
    doc.compress();
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| anyhow!("Couldn't write the PDF: {e}"))?;
    Ok(out)
}

const DPI_TIERS: [u32; 7] = [250, 200, 150, 110, 80, 60, 45];

pub fn compress_pdf(src: &Path, out_dir: &Path, opts: &Opts, ctl: &Ctl) -> Result<Outcome> {
    let target = parse_target(&opts.str("target_mb", ""))?;
    if let Some(done) = imgops::already_under(src, target, out_dir, "compressed")? {
        return Ok(done);
    }
    let base = load(src)?;
    let items = collect_images(&base);
    ctl.check()?;
    let mut note = String::new();
    let data = match target {
        None => {
            let (dpi, q) = match opts.str("level", "balanced").as_str() {
                "light" => (200, 85),
                "strong" => (110, 50),
                _ => (150, 70),
            };
            recompress(&base, &items, dpi, q)?
        }
        Some(t) => {
            let mut found: Option<Vec<u8>> = None;
            let mut smallest = usize::MAX;
            for (n, dpi) in DPI_TIERS.iter().enumerate() {
                ctl.check()?;
                ctl.report(n as f64 / (DPI_TIERS.len() + 1) as f64);
                let (q, data) = best_quality(|q| recompress(&base, &items, *dpi, q), t, 35, 90, ctl, 5)?;
                smallest = smallest.min(data.len());
                if q.is_some() {
                    found = Some(data);
                    break;
                }
            }
            if found.is_none() {
                for (dpi, q) in [(130u32, 60u8), (100, 50), (80, 45)] {
                    ctl.check()?;
                    let data = flattened(src, dpi, q, ctl)?;
                    smallest = smallest.min(data.len());
                    if data.len() as u64 <= t {
                        note = "Pages were converted to images to fit, so the text is no longer selectable.".into();
                        found = Some(data);
                        break;
                    }
                }
            }
            found.ok_or_else(|| {
                anyhow!("Couldn't get this PDF under {} - the smallest result was {}. Try a larger limit.", fmt_size(t), fmt_size(smallest as u64))
            })?
        }
    };
    imgops::deliver(src, &data, out_dir, "compressed", ".pdf", note, true)
}

/// Last resort for size limits: every page becomes a JPEG.
fn flattened(src: &Path, dpi: u32, quality: u8, ctl: &Ctl) -> Result<Vec<u8>> {
    let sizes = render::page_sizes(src)?;
    let mut b = Builder::new();
    render::with_doc(src, |d| {
        for (i, page) in d.pages().iter().enumerate() {
            ctl.check()?;
            let img = render::render(&page, dpi as f32 / 72.0)?;
            let rgb = DynamicImage::ImageRgb8(img.to_rgb8());
            let jpg = imgops::encode_jpeg(&rgb, quality, None)?;
            let (pw, ph) = sizes[i];
            b.add_page(pw, ph, &PageImg { data: jpg, w: rgb.width(), h: rgb.height(), gray: false, dct: true, smask: None }, (0.0, 0.0, pw, ph));
        }
        Ok(())
    })?;
    let mut doc = b.finish();
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| anyhow!("Couldn't write the PDF: {e}"))?;
    Ok(out)
}

pub fn expand_pdf(src: &Path, out_dir: &Path) -> Result<Outcome> {
    let mut doc = load(src)?;
    doc.decompress();
    let tmp = TempDir::new()?;
    let file = tmp.join("out.pdf");
    save_doc(&mut doc, &file)?;
    let data = fs::read(&file)?;
    imgops::deliver(
        src,
        &data,
        out_dir,
        "expanded",
        ".pdf",
        "All streams are stored uncompressed, which makes the file larger but easy to inspect or edit.".into(),
        false,
    )
}

// ------------------------------------------------------------------------------------- building from images

pub struct PageImg {
    pub data: Vec<u8>,
    pub w: u32,
    pub h: u32,
    pub gray: bool,
    /// `data` is already a JPEG (DCT) stream; otherwise it is raw 8-bit samples that get Flate-compressed.
    pub dct: bool,
    pub smask: Option<Vec<u8>>,
}

pub struct Builder {
    doc: Document,
    pages_id: ObjectId,
    kids: Vec<Object>,
}

impl Builder {
    pub fn new() -> Self {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        Self { doc, pages_id, kids: vec![] }
    }

    /// Adds a page of `pw` x `ph` points with the image drawn into `place` = (x, y, w, h).
    pub fn add_page(&mut self, pw: f32, ph: f32, img: &PageImg, place: (f32, f32, f32, f32)) {
        let mut dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => img.w as i64,
            "Height" => img.h as i64,
            "ColorSpace" => if img.gray { "DeviceGray" } else { "DeviceRGB" },
            "BitsPerComponent" => 8,
        };
        if let Some(mask) = &img.smask {
            let mdict = dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => img.w as i64, "Height" => img.h as i64,
                "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
            };
            let mut ms = Stream::new(mdict, mask.clone());
            let _ = ms.compress();
            let mid = self.doc.add_object(ms);
            dict.set("SMask", mid);
        }
        let stream = if img.dct {
            dict.set("Filter", "DCTDecode");
            Stream::new(dict, img.data.clone()).with_compression(false)
        } else {
            let mut s = Stream::new(dict, img.data.clone());
            let _ = s.compress();
            s
        };
        let img_id = self.doc.add_object(stream);
        let (x, y, w, h) = place;
        let content = format!("q {w:.3} 0 0 {h:.3} {x:.3} {y:.3} cm /Im0 Do Q");
        let content_id = self.doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        let page_id = self.doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => self.pages_id,
            "MediaBox" => vec![0.into(), 0.into(), pw.into(), ph.into()],
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => img_id } },
            "Contents" => content_id,
        });
        self.kids.push(page_id.into());
    }

    pub fn finish(mut self) -> Document {
        let count = self.kids.len() as i64;
        self.doc.objects.insert(
            self.pages_id,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => self.kids, "Count" => count }),
        );
        let catalog = self.doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => self.pages_id });
        self.doc.trailer.set("Root", catalog);
        self.doc
    }
}

/// How hard images are squeezed when building a PDF: "print" keeps them as they are, "standard" caps their
/// resolution, "small" also re-encodes photos as JPEG.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ImgQuality {
    Print,
    Standard,
    Small,
}

impl ImgQuality {
    pub fn from_opts(opts: &Opts) -> Self {
        match opts.str("quality", "standard").as_str() {
            "print" => Self::Print,
            "small" | "screen" => Self::Small,
            _ => Self::Standard,
        }
    }
    fn cap(self) -> Option<u32> {
        match self {
            Self::Print => None,
            Self::Standard => Some(2400),
            Self::Small => Some(1600),
        }
    }
}

/// Number of colour components of a real JPEG stream, or None if `data` isn't one.
fn jpeg_components(data: &[u8]) -> Option<u8> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return None;
    }
    let mut i = 2;
    while i + 9 < data.len() {
        if data[i] != 0xFF {
            return None;
        }
        let m = data[i + 1];
        if m == 0xFF {
            i += 1; // fill byte
            continue;
        }
        if (0xC0..=0xCF).contains(&m) && !matches!(m, 0xC4 | 0xC8 | 0xCC) {
            return Some(data[i + 9]);
        }
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        i += 2 + len;
    }
    None
}

fn page_img_from(img: &DynamicImage, q: ImgQuality) -> Result<PageImg> {
    let mut img = std::borrow::Cow::Borrowed(img);
    if let Some(cap) = q.cap() {
        let long = img.width().max(img.height());
        if long > cap {
            img = std::borrow::Cow::Owned(imgops::scaled(&img, cap as f64 / long as f64));
        }
    }
    let (w, h) = img.dimensions();
    if imgops::has_alpha(&img) {
        let rgba = img.to_rgba8();
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        let mut a = Vec::with_capacity((w * h) as usize);
        for p in rgba.pixels() {
            rgb.extend_from_slice(&p.0[..3]);
            a.push(p.0[3]);
        }
        // fully opaque alpha (common in PNG screenshots) needs no mask
        let smask = if a.iter().all(|&v| v == 255) { None } else { Some(a) };
        return Ok(PageImg { data: rgb, w, h, gray: false, dct: false, smask });
    }
    let gray = !img.color().has_color();
    if q == ImgQuality::Small {
        // photos and scans: JPEG is far smaller than lossless at this setting
        return Ok(PageImg { data: imgops::encode_jpeg(&img, 72, None)?, w, h, gray, dct: true, smask: None });
    }
    if q == ImgQuality::Standard && looks_like_photo(&img) {
        // a photo saved as PNG/BMP/TIFF: high-quality JPEG is visually the same and a fraction of the size;
        // screenshots and graphics (few colours, sharp edges) stay lossless
        return Ok(PageImg { data: imgops::encode_jpeg(&img, 88, None)?, w, h, gray, dct: true, smask: None });
    }
    if gray {
        return Ok(PageImg { data: img.to_luma8().into_raw(), w, h, gray: true, dct: false, smask: None });
    }
    Ok(PageImg { data: img.to_rgb8().into_raw(), w, h, gray: false, dct: false, smask: None })
}

/// Many distinct colours on a sample grid = continuous-tone photo rather than a screenshot or drawing.
fn looks_like_photo(img: &DynamicImage) -> bool {
    let (w, h) = img.dimensions();
    if (w as u64) * (h as u64) < 250_000 {
        return false; // small images: lossless costs little
    }
    let rgb = img.to_rgb8();
    let step = (((w as u64 * h as u64) / 40_000) as f64).sqrt().max(1.0) as u32;
    let mut seen = HashSet::new();
    for y in (0..h).step_by(step as usize) {
        for x in (0..w).step_by(step as usize) {
            let p = rgb.get_pixel(x, y).0;
            seen.insert(((p[0] as u32 >> 2) << 12) | ((p[1] as u32 >> 2) << 6) | (p[2] as u32 >> 2));
        }
    }
    seen.len() > 3000
}

/// Pages 2.. of a multi-page TIFF (scans, faxes). The first page comes from the regular decoder.
fn extra_tiff_pages(path: &Path) -> Vec<Result<DynamicImage>> {
    use tiff::{
        decoder::{Decoder, DecodingResult},
        ColorType,
    };
    let Ok(file) = fs::File::open(path) else { return vec![] };
    let Ok(mut dec) = Decoder::new(std::io::BufReader::new(file)) else { return vec![] };
    let mut out = vec![];
    while dec.more_images() {
        if dec.next_image().is_err() {
            break;
        }
        let page = (|| -> Result<DynamicImage> {
            let (w, h) = dec.dimensions()?;
            let ct = dec.colortype()?;
            let data = dec.read_image()?;
            let bad = || anyhow!("this page uses a TIFF format that isn't supported");
            Ok(match (ct, data) {
                (ColorType::Gray(8), DecodingResult::U8(v)) => DynamicImage::ImageLuma8(image::GrayImage::from_raw(w, h, v).ok_or_else(bad)?),
                (ColorType::GrayA(8), DecodingResult::U8(v)) => DynamicImage::ImageLumaA8(image::GrayAlphaImage::from_raw(w, h, v).ok_or_else(bad)?),
                (ColorType::RGB(8), DecodingResult::U8(v)) => DynamicImage::ImageRgb8(image::RgbImage::from_raw(w, h, v).ok_or_else(bad)?),
                (ColorType::RGBA(8), DecodingResult::U8(v)) => DynamicImage::ImageRgba8(image::RgbaImage::from_raw(w, h, v).ok_or_else(bad)?),
                (ColorType::Gray(16), DecodingResult::U16(v)) => DynamicImage::ImageLuma16(image::ImageBuffer::from_raw(w, h, v).ok_or_else(bad)?),
                (ColorType::RGB(16), DecodingResult::U16(v)) => DynamicImage::ImageRgb16(image::ImageBuffer::from_raw(w, h, v).ok_or_else(bad)?),
                (ColorType::Gray(1), DecodingResult::U8(v)) => {
                    // packed bilevel rows (black-and-white scans)
                    let row = (w as usize).div_ceil(8);
                    if v.len() < row * h as usize {
                        return Err(bad());
                    }
                    let px = image::GrayImage::from_fn(w, h, |x, y| {
                        let byte = v[y as usize * row + x as usize / 8];
                        image::Luma([if byte & (0x80 >> (x % 8)) != 0 { 255 } else { 0 }])
                    });
                    DynamicImage::ImageLuma8(px)
                }
                (ColorType::CMYK(8), DecodingResult::U8(v)) => {
                    let mut rgb = Vec::with_capacity(v.len() / 4 * 3);
                    for c in v.chunks_exact(4) {
                        let k = 255 - c[3] as u32;
                        for ch in &c[..3] {
                            rgb.push(((255 - *ch as u32) * k / 255) as u8);
                        }
                    }
                    DynamicImage::ImageRgb8(image::RgbImage::from_raw(w, h, rgb).ok_or_else(bad)?)
                }
                _ => return Err(bad()),
            })
        })();
        out.push(page);
    }
    out
}

/// Decide how an image file is stored in the PDF. Real JPEGs (1 or 3 colour channels, upright) are embedded untouched:
/// no quality loss and a small file. Everything else is decoded first, which also catches files whose extension lies
/// (a PNG or WebP saved as .jpg) and CMYK JPEGs. Multi-page TIFFs give one image per page.
fn prepare_page_images(path: &Path, q: ImgQuality) -> Result<Vec<Result<PageImg>>> {
    let bytes = fs::read(path)?;
    if let Some(comps) = jpeg_components(&bytes).filter(|c| *c == 1 || *c == 3) {
        if let Ok(mut dec) = image::codecs::jpeg::JpegDecoder::new(std::io::Cursor::new(&bytes)) {
            let upright = matches!(dec.orientation(), Ok(image::metadata::Orientation::NoTransforms));
            let (w, h) = dec.dimensions();
            let fits = q.cap().map(|c| w.max(h) <= c).unwrap_or(true);
            // at "small", a heavy JPEG (more than ~2 bits per pixel) is worth re-encoding
            let light = q != ImgQuality::Small || (bytes.len() as f64 * 8.0 / (w as f64 * h as f64)) <= 2.0;
            if upright && fits && light {
                return Ok(vec![Ok(PageImg { data: bytes, w, h, gray: comps == 1, dct: true, smask: None })]);
            }
        }
    }
    let first = imgops::load(path)?;
    let mut pages = vec![page_img_from(&first.img, q)];
    let ext = crate::kinds::ext_of(path);
    if ext == "tif" || ext == "tiff" {
        for p in extra_tiff_pages(path) {
            pages.push(p.and_then(|img| page_img_from(&img, q)));
        }
    }
    Ok(pages)
}

/// Lays images out as PDF pages. Returns the document, the number of pages and what was skipped.
pub fn images_doc(sources: &[PathBuf], opts: &Opts, ctl: &Ctl) -> Result<(Document, usize, Vec<String>)> {
    let page = opts.str("page", "fit");
    let q = ImgQuality::from_opts(opts);
    let margin = match opts.str("margin", "small").as_str() {
        "none" => 0.0,
        "medium" => 36.0,
        _ => 18.0,
    };
    let mut b = Builder::new();
    let mut skipped: Vec<String> = vec![];
    let mut pages = 0;
    for (i, src) in sources.iter().enumerate() {
        ctl.check()?;
        ctl.report(i as f64 / sources.len() as f64);
        let fname = src.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let imgs = match prepare_page_images(src, q) {
            Ok(v) => v,
            Err(e) => {
                skipped.push(format!("{fname}: {}", friendly(&e)));
                continue;
            }
        };
        for (n, img) in imgs.into_iter().enumerate() {
            let img = match img {
                Ok(img) => img,
                Err(e) => {
                    skipped.push(format!("{fname} page {}: {e}", n + 1));
                    continue;
                }
            };
            add_image_page(&mut b, &img, &page, margin);
            pages += 1;
        }
    }
    if pages == 0 {
        bail!("None of the images could be read. {}", skipped.join("; "));
    }
    Ok((b.finish(), pages, skipped))
}

fn add_image_page(b: &mut Builder, img: &PageImg, page: &str, margin: f32) {
    let (iw, ih) = (img.w as f32, img.h as f32);
    if page == "fit" {
        // 96 dpi screen pixels -> points, capped so huge photos don't make absurd pages
        let (mut pw, mut ph) = (iw * 0.75, ih * 0.75);
        let long = pw.max(ph);
        if long > 1500.0 {
            pw *= 1500.0 / long;
            ph *= 1500.0 / long;
        }
        b.add_page(pw, ph, img, (0.0, 0.0, pw, ph));
    } else {
        let (mut pw, mut ph) = if page == "letter" { (612.0f32, 792.0f32) } else { (595.276, 841.89) };
        if iw > ih {
            std::mem::swap(&mut pw, &mut ph); // landscape picture -> landscape page
        }
        let (aw, ah) = (pw - 2.0 * margin, ph - 2.0 * margin);
        let s = (aw / iw).min(ah / ih);
        let (w, h) = (iw * s, ih * s);
        b.add_page(pw, ph, img, (margin + (aw - w) / 2.0, margin + (ah - h) / 2.0, w, h));
    }
}

pub fn images_to_pdf(sources: &[PathBuf], out_dir: &Path, name: &str, opts: &Opts, ctl: &Ctl) -> Result<Outcome> {
    let (mut doc, pages, skipped) = images_doc(sources, opts, ctl)?;
    if opts.bool("pdfa", false) {
        crate::pdfa::apply(&mut doc, name)?;
    }
    let tmp = TempDir::new()?;
    let file = tmp.join("out.pdf");
    save_doc(&mut doc, &file)?;
    let size = fs::metadata(&file)?.len();
    let dst = publish(&file, out_dir, &safe_stem(name), ".pdf")?;
    let mut o = Outcome::one(&dst).with_detail(format!("{pages} page(s) \u{2192} {}", fmt_size(size)));
    if !skipped.is_empty() {
        o = o.with_note(format!("Skipped: {}", skipped.join("; ")));
    }
    Ok(o)
}

/// Re-encodes the images of a finished PDF in place (used for the "Small" quality setting), if that helps.
pub fn shrink_in_place(path: &Path, dpi: u32, quality: u8) -> Result<()> {
    let base = load(path)?;
    let items = collect_images(&base);
    if items.is_empty() {
        return Ok(());
    }
    let data = recompress(&base, &items, dpi, quality)?;
    if (data.len() as u64) < fs::metadata(path)?.len() {
        fs::write(path, data)?;
    }
    Ok(())
}

pub fn save_doc_to(doc: &mut Document, path: &Path) -> Result<()> {
    save_doc(doc, path)
}
