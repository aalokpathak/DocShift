//! PDF -> images, Excel (tables) and PowerPoint (editable text over a clean background, or exact page images).
use crate::{core::*, imgops, render};
use anyhow::{anyhow, bail, Context, Result};
use image::DynamicImage;
use regex::Regex;
use render::Glyph;
use std::{
    fs,
    io::{Cursor, Read, Write},
    path::Path,
};

// -------------------------------------------------------------------------------------------------- images

pub fn has_no_text(src: &Path) -> Result<bool> {
    render::with_doc(src, |d| {
        for page in d.pages().iter() {
            if let Ok(t) = page.text() {
                if !t.all().trim().is_empty() {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    })
}

fn encode_as(img: &DynamicImage, fmt: &str, dpi: u32) -> Result<Vec<u8>> {
    let _ = dpi;
    Ok(match fmt {
        "jpg" => imgops::encode_jpeg(img, 95, None)?,
        "webp" => imgops::encode_webp(img, 92)?,
        "tiff" | "bmp" => {
            let f = if fmt == "tiff" { image::ImageFormat::Tiff } else { image::ImageFormat::Bmp };
            let mut buf = Cursor::new(Vec::new());
            DynamicImage::ImageRgb8(img.to_rgb8()).write_to(&mut buf, f)?;
            buf.into_inner()
        }
        _ => imgops::encode_png(&DynamicImage::ImageRgb8(img.to_rgb8()), None, None)?,
    })
}

pub fn pdf_to_images(src: &Path, out_dir: &Path, opts: &Opts, ctl: &Ctl) -> Result<Outcome> {
    let fmt = opts.str("format", "png");
    let dpi = opts.u32("dpi", 200).clamp(36, 1200);
    let ext = match fmt.as_str() {
        "jpg" => ".jpg",
        "webp" => ".webp",
        "tiff" => ".tiff",
        "bmp" => ".bmp",
        _ => ".png",
    };
    let stem = stem_of(src);
    let tmp = TempDir::new()?;
    let mut made: Vec<(String, std::path::PathBuf)> = vec![];
    render::with_doc(src, |d| {
        let count = d.pages().len() as usize;
        let pages = parse_pages(&opts.str("pages", ""), count)?;
        for (n, &i) in pages.iter().enumerate() {
            ctl.check()?;
            ctl.report(n as f64 / pages.len() as f64);
            let page = d.pages().get(i as u16).map_err(|e| anyhow!("Couldn't read page {}: {e}", i + 1))?;
            let img = render::render(&page, dpi as f32 / 72.0).map_err(|e| anyhow!("Page {} is too large at {dpi} dpi, try a lower resolution ({e})", i + 1))?;
            let name = format!("{stem}_page_{:03}{ext}", i + 1);
            let path = tmp.join(&name);
            fs::write(&path, encode_as(&img, &fmt, dpi)?)?;
            made.push((name, path));
        }
        Ok(())
    })?;
    fs::create_dir_all(out_dir)?;
    if made.len() == 1 {
        let (_, path) = &made[0];
        let out = publish(path, out_dir, &format!("{stem}_page_{:03}", first_page_number(&opts.str("pages", ""))), ext)?;
        return Ok(Outcome::one(&out));
    }
    let folder = unique_path(out_dir, &format!("{stem}_images"), "");
    fs::create_dir_all(&folder)?;
    for (name, path) in &made {
        let dst = folder.join(name);
        if fs::rename(path, &dst).is_err() {
            fs::copy(path, &dst)?;
        }
    }
    Ok(Outcome::one(&folder).with_note(format!("{} images saved in a folder.", made.len())))
}

fn first_page_number(spec: &str) -> usize {
    parse_pages(spec, usize::MAX / 2).ok().and_then(|p| p.first().map(|i| i + 1)).unwrap_or(1)
}

// --------------------------------------------------------------------------------------------------- Excel

#[derive(Clone, Debug)]
pub(crate) struct Word {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub text: String,
}

pub(crate) fn words_of(glyphs: &[Glyph]) -> Vec<Word> {
    let mut words: Vec<Word> = vec![];
    let mut cur: Option<(Word, f32)> = None; // word + font size
    for g in glyphs {
        if g.ch.is_whitespace() || g.ch.is_control() {
            if let Some((w, _)) = cur.take() {
                words.push(w);
            }
            continue;
        }
        match &mut cur {
            // glyph boxes may overlap (wide letters, recognised text), so only a clear gap or a jump back starts a new word
            Some((w, size)) if g.x0 - w.x1 <= 0.35 * *size && g.x0 - w.x1 >= -1.0 * *size && g.x0 >= w.x0 && ((g.y0 + g.y1) - (w.y0 + w.y1)).abs() <= 1.2 * *size => {
                w.x1 = w.x1.max(g.x1);
                w.y0 = w.y0.min(g.y0);
                w.y1 = w.y1.max(g.y1);
                w.text.push(g.ch);
            }
            _ => {
                if let Some((w, _)) = cur.take() {
                    words.push(w);
                }
                cur = Some((Word { x0: g.x0, y0: g.y0, x1: g.x1, y1: g.y1, text: g.ch.to_string() }, g.size.max(1.0)));
            }
        }
    }
    if let Some((w, _)) = cur {
        words.push(w);
    }
    words
}

pub(crate) struct Line {
    pub yc: f32,
    pub words: Vec<Word>,
}

pub(crate) fn lines_of(mut words: Vec<Word>) -> Vec<Line> {
    words.sort_by(|a, b| ((a.y0 + a.y1) / 2.0).total_cmp(&((b.y0 + b.y1) / 2.0)).then(a.x0.total_cmp(&b.x0)));
    let mut lines: Vec<Line> = vec![];
    for w in words {
        let yc = (w.y0 + w.y1) / 2.0;
        let h = (w.y1 - w.y0).max(1.0);
        match lines.last_mut() {
            Some(l) if (yc - l.yc).abs() <= 0.5 * h => l.words.push(w),
            _ => lines.push(Line { yc, words: vec![w] }),
        }
    }
    for l in &mut lines {
        l.words.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    }
    lines
}

/// Splits a line into cells wherever the gap is clearly wider than a normal word space.
fn cells_of(l: &Line) -> Vec<(f32, f32, String)> {
    let avg_h = l.words.iter().map(|w| w.y1 - w.y0).sum::<f32>() / l.words.len() as f32;
    let mut cells: Vec<(f32, f32, String)> = vec![];
    for w in &l.words {
        match cells.last_mut() {
            Some(c) if w.x0 - c.1 <= 0.32 * avg_h => {
                c.1 = c.1.max(w.x1);
                c.2.push(' ');
                c.2.push_str(&w.text);
            }
            _ => cells.push((w.x0, w.x1, w.text.clone())),
        }
    }
    cells
}

struct Table {
    y0: f32,
    rows: Vec<Vec<String>>,
}

/// Finds tables from word positions alone (works for ruled and un-ruled tables). Ordinary paragraphs have one cell
/// per row, so they never qualify.
fn find_tables(lines: &[Line]) -> Vec<(Table, std::ops::Range<usize>)> {
    let parsed: Vec<Vec<(f32, f32, String)>> = lines.iter().map(cells_of).collect();
    let multi: Vec<bool> = parsed.iter().map(|c| c.len() >= 2).collect();
    let mut runs = vec![];
    let mut i = 0;
    while i < parsed.len() {
        if !multi[i] {
            i += 1;
            continue;
        }
        let mut j = i;
        // a single sparse row between two table rows stays inside the table
        while j + 1 < parsed.len() && (multi[j + 1] || (j + 2 < parsed.len() && multi[j + 2])) {
            j += 1;
        }
        runs.push((i, j));
        i = j + 1;
    }
    let mut out = vec![];
    for (a, b) in runs {
        if b + 1 - a < 3 {
            continue;
        }
        let mut spans: Vec<(f32, f32)> = parsed[a..=b].iter().flat_map(|r| r.iter().map(|c| (c.0, c.1))).collect();
        spans.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut bands: Vec<(f32, f32)> = vec![spans[0]];
        for (x0, x1) in spans.into_iter().skip(1) {
            let last = bands.last_mut().unwrap();
            if x0 <= last.1 {
                last.1 = last.1.max(x1);
            } else {
                bands.push((x0, x1));
            }
        }
        if bands.len() < 2 {
            continue;
        }
        let rows: Vec<Vec<String>> = parsed[a..=b]
            .iter()
            .map(|cells| {
                let mut row = vec![String::new(); bands.len()];
                for (x0, x1, text) in cells {
                    let xc = (x0 + x1) / 2.0;
                    if let Some(k) = bands.iter().position(|(b0, b1)| *b0 <= xc && xc <= *b1) {
                        if !row[k].is_empty() {
                            row[k].push(' ');
                        }
                        row[k].push_str(text);
                    }
                }
                row
            })
            .collect();
        let y0 = lines[a].words.iter().map(|w| w.y0).fold(f32::MAX, f32::min);
        out.push((Table { y0, rows }, a..b + 1));
    }
    out
}

/// '1,234.50' -> number; keep IDs with leading zeros, phone numbers etc. as text.
fn coerce(re: &Regex, s: &str) -> Option<f64> {
    let t = s.trim();
    if !re.is_match(t) {
        return None;
    }
    let digits = t.replace(',', "");
    let body = digits.trim_start_matches('-');
    if body.len() > 1 && body.starts_with('0') && !body.starts_with("0.") {
        return None;
    }
    digits.parse().ok()
}

pub fn pdf_to_excel(src: &Path, out_dir: &Path, opts: &Opts, ctl: &Ctl) -> Result<Outcome> {
    use rust_xlsxwriter::{Color, Format, FormatAlign, FormatBorder, Workbook};
    let single = opts.str("layout", "pages") == "single";
    let num_re = Regex::new(r"^-?(?:\d{1,3}(?:,\d{3})+|\d+)(?:\.\d+)?$")?;
    let head = Format::new().set_bold().set_background_color(Color::RGB(0xEDEFF7)).set_border(FormatBorder::Thin).set_align(FormatAlign::Top).set_text_wrap();
    let cell = Format::new().set_border(FormatBorder::Thin).set_align(FormatAlign::Top).set_text_wrap();
    let mut wb = Workbook::new();
    let mut tables_found = 0usize;
    // One entry per sheet: (name, items)
    enum Item {
        Table(Vec<Vec<String>>),
        Text(String),
    }
    let mut sheets: Vec<(String, Vec<Item>)> = vec![];
    render::with_doc(src, |d| {
        let n = d.pages().len() as usize;
        for (pno, page) in d.pages().iter().enumerate() {
            ctl.check()?;
            ctl.report(pno as f64 / n as f64 * 0.9);
            let glyphs: Vec<Glyph> = render::glyphs(&page)?.into_iter().filter(|g| g.angle.abs() < 1.0 || (g.angle - 360.0).abs() < 1.0).collect();
            let lines = lines_of(words_of(&glyphs));
            let tables = find_tables(&lines);
            let mut items: Vec<(f32, Item)> = vec![];
            let mut used = vec![false; lines.len()];
            for (t, range) in tables {
                for k in range {
                    used[k] = true;
                }
                tables_found += 1;
                items.push((t.y0, Item::Table(t.rows)));
            }
            for (k, l) in lines.iter().enumerate() {
                if !used[k] {
                    let text = l.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
                    items.push((l.yc, Item::Text(text)));
                }
            }
            if items.is_empty() {
                continue;
            }
            items.sort_by(|a, b| a.0.total_cmp(&b.0));
            let name = if single { "Data".to_string() } else { format!("Page {}", pno + 1) };
            let items: Vec<Item> = items.into_iter().map(|(_, i)| i).collect();
            match sheets.last_mut() {
                Some((n, existing)) if single && *n == name => {
                    existing.push(Item::Text(String::new()));
                    existing.extend(items);
                }
                _ => sheets.push((name, items)),
            }
        }
        Ok(())
    })?;
    if sheets.is_empty() {
        bail!("No text or tables were found. If this PDF is a scan, it needs OCR first.");
    }
    for (name, items) in &sheets {
        let ws = wb.add_worksheet();
        ws.set_name(name)?;
        let mut row = 0u32;
        let mut widths: Vec<usize> = vec![];
        let bump = |c: usize, len: usize, widths: &mut Vec<usize>| {
            if widths.len() <= c {
                widths.resize(c + 1, 0);
            }
            widths[c] = widths[c].max(len.min(60));
        };
        for item in items {
            match item {
                Item::Table(rows) => {
                    for (ri, r) in rows.iter().enumerate() {
                        for (ci, text) in r.iter().enumerate() {
                            let fmt = if ri == 0 { &head } else { &cell };
                            match coerce(&num_re, text) {
                                Some(v) if ri > 0 || true => ws.write_number_with_format(row, ci as u16, v, fmt)?,
                                _ => ws.write_string_with_format(row, ci as u16, text, fmt)?,
                            };
                            bump(ci, text.chars().count(), &mut widths);
                        }
                        row += 1;
                    }
                    row += 1;
                }
                Item::Text(t) => {
                    if !t.is_empty() {
                        ws.write_string(row, 0, t)?;
                        bump(0, t.chars().count(), &mut widths);
                    }
                    row += 1;
                }
            }
        }
        for (c, w) in widths.iter().enumerate() {
            ws.set_column_width(c as u16, ((*w + 2) as f64).clamp(8.0, 60.0))?;
        }
    }
    let tmp = TempDir::new()?;
    let file = tmp.join("out.xlsx");
    wb.save(&file).map_err(|e| anyhow!("Couldn't write the workbook: {e}"))?;
    let before = fs::metadata(src)?.len();
    let after = fs::metadata(&file)?.len();
    let out = publish(&file, out_dir, &stem_of(src), ".xlsx")?;
    let note = if tables_found == 0 { "No table structure was detected, so the page text was placed line by line." } else { "" };
    Ok(Outcome::one(&out).with_note(note).with_detail(size_detail(before, after)))
}

// ------------------------------------------------------------------------------------------------ PowerPoint

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn clean_font(name: &str, text: &str) -> String {
    let re_prefix = Regex::new(r"^[A-Z]{6}\+").unwrap();
    let n = re_prefix.replace(name, "").to_string();
    let base = n.split(['-', ',']).next().unwrap_or("").to_string();
    let re_suffix = Regex::new(r"(PSMT|PS|MT)$").unwrap();
    let base = re_suffix.replace(&base, "").to_string();
    let re_camel = Regex::new(r"([a-z])([A-Z])").unwrap();
    let base = re_camel.replace_all(&base, "$1 $2").trim().to_string();
    let lower = base.to_lowercase();
    if ["symbol", "wingdings", "zapfdingbats", "webdings"].contains(&lower.as_str()) && !text.chars().any(|c| ('\u{F000}'..='\u{F0FF}').contains(&c)) {
        return "Arial".into(); // a normal bullet tagged with Symbol would render as an empty box
    }
    match base.as_str() {
        "Helvetica" => "Arial".into(),
        "Times" | "Times Roman" => "Times New Roman".into(),
        "Courier" => "Courier New".into(),
        "" => "Arial".into(),
        other => other.to_string(),
    }
}

struct TextLine {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    runs: Vec<(String, Glyph)>, // text, style-defining glyph
}

fn text_lines(glyphs: &[Glyph]) -> Vec<TextLine> {
    let mut lines: Vec<TextLine> = vec![];
    for g in glyphs {
        if g.angle.abs() > 1.0 && (g.angle - 360.0).abs() > 1.0 {
            continue; // rotated text stays in the background picture
        }
        let yc = (g.y0 + g.y1) / 2.0;
        let start_new = match lines.last() {
            Some(l) => {
                let lyc = (l.y0 + l.y1) / 2.0;
                (yc - lyc).abs() > 0.5 * g.size.max(1.0) || g.x0 < l.x1 - 2.0 * g.size.max(1.0)
            }
            None => true,
        };
        if start_new {
            lines.push(TextLine { x0: g.x0, y0: g.y0, x1: g.x1, y1: g.y1, runs: vec![] });
        }
        let l = lines.last_mut().unwrap();
        l.x0 = l.x0.min(g.x0);
        l.y0 = l.y0.min(g.y0);
        l.x1 = l.x1.max(g.x1);
        l.y1 = l.y1.max(g.y1);
        let same = l.runs.last().map(|(_, s)| s.font == g.font && (s.size - g.size).abs() < 0.2 && s.bold == g.bold && s.italic == g.italic && s.color == g.color).unwrap_or(false);
        if same {
            l.runs.last_mut().unwrap().0.push(g.ch);
        } else {
            l.runs.push((g.ch.to_string(), g.clone()));
        }
    }
    lines.retain(|l| l.runs.iter().any(|(t, _)| !t.trim().is_empty()));
    lines
}

const EMU: f32 = 12700.0;

fn slide_xml(pic_rid: &str, place: (f32, f32, f32, f32), lines: &[TextLine], ox: f32, oy: f32, scale: f32) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#,
    );
    let (x, y, w, h) = place;
    s.push_str(&format!(
        r#"<p:pic><p:nvPicPr><p:cNvPr id="2" name="Page"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="{pic_rid}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>"#,
        (x * EMU) as i64, (y * EMU) as i64, (w * EMU) as i64, (h * EMU) as i64
    ));
    for (n, l) in lines.iter().enumerate() {
        let (lx, ly) = (ox + l.x0 * scale, oy + l.y0 * scale);
        let (lw, lh) = ((l.x1 - l.x0) * scale + 6.0, (l.y1 - l.y0) * scale);
        s.push_str(&format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{}" name="Text {}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{}" y="{}"/><a:ext cx="{}" cy="{}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap="none" lIns="0" tIns="0" rIns="0" bIns="0" anchor="t"><a:noAutofit/></a:bodyPr><a:lstStyle/><a:p>"#,
            n + 3, n + 1, (lx * EMU) as i64, (ly * EMU) as i64, (lw * EMU) as i64, (lh * EMU) as i64
        ));
        for (text, g) in &l.runs {
            let (r, gr, b) = g.color;
            s.push_str(&format!(
                r#"<a:r><a:rPr lang="en-US" sz="{}" b="{}" i="{}" dirty="0"><a:solidFill><a:srgbClr val="{:02X}{:02X}{:02X}"/></a:solidFill><a:latin typeface="{}"/></a:rPr><a:t>{}</a:t></a:r>"#,
                ((g.size * scale).max(1.0) * 100.0).round() as i64,
                g.bold as u8,
                g.italic as u8,
                r, gr, b,
                esc(&clean_font(&g.font, text)),
                esc(text)
            ));
        }
        s.push_str("</a:p></p:txBody></p:sp>");
    }
    s.push_str(r#"</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#);
    s
}

/// Paints over a text line with the colour of the page around it, so the text can be re-created as a real text box.
/// Returns false (and leaves the pixels alone) when the surroundings aren't a flat colour, e.g. text over a photo.
fn erase_line(img: &mut image::RgbImage, l: &TextLine, scale: f32) -> bool {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let pad = 2.0;
    let x0 = ((l.x0 * scale) as i32 - pad as i32).clamp(0, w - 1);
    let y0 = ((l.y0 * scale) as i32 - pad as i32).clamp(0, h - 1);
    let x1 = ((l.x1 * scale).ceil() as i32 + pad as i32).clamp(x0 + 1, w);
    let y1 = ((l.y1 * scale).ceil() as i32 + pad as i32).clamp(y0 + 1, h);
    // ring of pixels just outside the box
    let ring = 3;
    let mut samples: Vec<[u8; 3]> = vec![];
    for y in (y0 - ring).max(0)..(y1 + ring).min(h) {
        for x in (x0 - ring).max(0)..(x1 + ring).min(w) {
            if x >= x0 && x < x1 && y >= y0 && y < y1 {
                continue;
            }
            samples.push(img.get_pixel(x as u32, y as u32).0);
        }
    }
    if samples.is_empty() {
        return false;
    }
    let mut chan = [vec![], vec![], vec![]];
    for p in &samples {
        for c in 0..3 {
            chan[c].push(p[c]);
        }
    }
    let mut med = [0u8; 3];
    let mut spread = 0u32;
    for c in 0..3 {
        chan[c].sort_unstable();
        med[c] = chan[c][chan[c].len() / 2];
        let lo = chan[c][chan[c].len() / 10] as i32;
        let hi = chan[c][chan[c].len() * 9 / 10] as i32;
        spread = spread.max((hi - lo) as u32);
    }
    if spread > 24 {
        return false; // busy background: leave this line as part of the picture
    }
    for y in y0..y1 {
        for x in x0..x1 {
            img.put_pixel(x as u32, y as u32, image::Rgb(med));
        }
    }
    true
}

pub fn pdf_to_pptx(src: &Path, out_dir: &Path, opts: &Opts, ctl: &Ctl) -> Result<Outcome> {
    let editable = opts.str("style", "exact") == "editable";
    let dpi = opts.u32("dpi", 150).clamp(72, 300);
    struct PageOut {
        jpeg: Vec<u8>,
        w: f32,
        h: f32,
        lines: Vec<TextLine>,
    }
    let mut pages: Vec<PageOut> = vec![];
    let mut kept_as_image = 0usize;
    render::with_doc(src, |d| {
        let n = d.pages().len() as usize;
        for i in 0..n {
            ctl.check()?;
            ctl.report(i as f64 / n as f64 * 0.9);
            let page = d.pages().get(i as u16).map_err(|e| anyhow!("Couldn't read page {}: {e}", i + 1))?;
            let (w, h) = (page.width().value, page.height().value);
            let mut lines = vec![];
            let mut to_erase = vec![];
            if editable {
                let glyphs = render::glyphs(&page)?;
                let rotated = glyphs.iter().any(|g| g.angle.abs() > 1.0 && (g.angle - 360.0).abs() > 1.0);
                if rotated {
                    kept_as_image += 1; // can't safely separate rotated text from the picture
                } else {
                    to_erase = text_lines(&glyphs);
                }
            }
            let scale = dpi as f32 / 72.0;
            let mut rgb = render::render(&page, scale)?.to_rgb8();
            for l in to_erase {
                if erase_line(&mut rgb, &l, scale) {
                    lines.push(l);
                }
            }
            let jpeg = imgops::encode_jpeg(&DynamicImage::ImageRgb8(rgb), 90, None)?;
            pages.push(PageOut { jpeg, w, h, lines });
        }
        Ok(())
    })?;
    if pages.is_empty() {
        bail!("This PDF has no pages.");
    }
    let (sw, sh) = (pages[0].w, pages[0].h);

    // Build the package from a known-good blank deck: copy every part, patch three, add the slides.
    let template: &[u8] = include_bytes!("../assets/blank.pptx");
    let mut zin = zip::ZipArchive::new(Cursor::new(template)).context("Internal template is damaged")?;
    let tmp = TempDir::new()?;
    let file = tmp.join("out.pptx");
    let mut zout = zip::ZipWriter::new(fs::File::create(&file)?);
    let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut sld_ids = String::new();
    let mut pres_rels = String::new();
    let mut ctypes = String::from(r#"<Default Extension="jpg" ContentType="image/jpeg"/>"#);
    for (i, p) in pages.iter().enumerate() {
        let n = i + 1;
        let scale = (sw / p.w).min(sh / p.h);
        let (ox, oy) = ((sw - p.w * scale) / 2.0, (sh - p.h * scale) / 2.0);
        let xml = slide_xml("rId2", (ox, oy, p.w * scale, p.h * scale), &p.lines, ox, oy, scale);
        zout.start_file(format!("ppt/slides/slide{n}.xml"), opt)?;
        zout.write_all(xml.as_bytes())?;
        zout.start_file(format!("ppt/slides/_rels/slide{n}.xml.rels"), opt)?;
        zout.write_all(format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout7.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/page{n}.jpg"/></Relationships>"#
        ).as_bytes())?;
        zout.start_file(format!("ppt/media/page{n}.jpg"), opt)?;
        zout.write_all(&p.jpeg)?;
        sld_ids.push_str(&format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 255 + n, 100 + n));
        pres_rels.push_str(&format!(r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{n}.xml"/>"#, 100 + n));
        ctypes.push_str(&format!(r#"<Override PartName="/ppt/slides/slide{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#));
    }
    let size_re = Regex::new(r#"<p:sldSz[^>]*/>"#)?;
    for i in 0..zin.len() {
        let mut f = zin.by_index(i)?;
        let name = f.name().to_string();
        if name == "docProps/thumbnail.jpeg" {
            continue; // would show a blank slide in Explorer
        }
        let mut data = Vec::new();
        f.read_to_end(&mut data)?;
        let text = || String::from_utf8_lossy(&data).into_owned();
        let patched: Option<String> = match name.as_str() {
            "ppt/presentation.xml" => {
                let x = text().replace("</p:sldMasterIdLst>", &format!("</p:sldMasterIdLst><p:sldIdLst>{sld_ids}</p:sldIdLst>"));
                let size = format!(r#"<p:sldSz cx="{}" cy="{}"/>"#, (sw * EMU) as i64, (sh * EMU) as i64);
                Some(size_re.replace(&x, size.as_str()).into_owned())
            }
            "ppt/_rels/presentation.xml.rels" => Some(text().replace("</Relationships>", &format!("{pres_rels}</Relationships>"))),
            "[Content_Types].xml" => Some(text().replace("</Types>", &format!("{ctypes}</Types>"))),
            "_rels/.rels" => Some(Regex::new(r#"<Relationship [^>]*thumbnail[^>]*/>"#)?.replace_all(&text(), "").into_owned()),
            _ => None,
        };
        zout.start_file(&name, opt)?;
        match patched {
            Some(p) => zout.write_all(p.as_bytes())?,
            None => zout.write_all(&data)?,
        }
    }
    zout.finish()?;
    let before = fs::metadata(src)?.len();
    let after = fs::metadata(&file)?.len();
    let out = publish(&file, out_dir, &stem_of(src), ".pptx")?;
    let note = if editable && kept_as_image > 0 { format!("{kept_as_image} page(s) with rotated text were kept as images.") } else { String::new() };
    Ok(Outcome::one(&out).with_note(note).with_detail(size_detail(before, after)))
}
