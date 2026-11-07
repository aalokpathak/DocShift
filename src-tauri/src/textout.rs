//! PDF -> plain text, and PDF -> Word as flowing text. Both read the positioned characters PDFium gives us
//! (scanned pages first go through OCR, see jobs.rs) and either keep the page layout with spaces, or rebuild
//! paragraphs and headings so the text reflows in an editor.
use crate::{
    core::*,
    pdf2x::{lines_of, words_of, Line},
    render::{self, Glyph},
};
use anyhow::Result;
use std::{fs, io::Write, path::Path};

/// One block of reflowed text.
struct Para {
    text: String,
    /// typical text height, used to spot headings
    size: f32,
}

fn page_lines(page: &pdfium_render::prelude::PdfPage) -> Result<Vec<Line>> {
    let glyphs: Vec<Glyph> = render::glyphs(page)?.into_iter().filter(|g| g.angle.abs() < 1.0 || (g.angle - 360.0).abs() < 1.0).collect();
    Ok(lines_of(words_of(&glyphs)))
}

fn line_text(l: &Line) -> String {
    l.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
}

fn line_height(l: &Line) -> f32 {
    l.words.iter().map(|w| w.y1 - w.y0).fold(0.0, f32::max).max(1.0)
}

/// Layout-preserving text: words are placed in character columns, and big vertical gaps become blank lines.
fn layout_text(lines: &[Line]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut widths: Vec<f32> = lines.iter().flat_map(|l| l.words.iter()).filter(|w| w.text.chars().count() > 1).map(|w| (w.x1 - w.x0) / w.text.chars().count() as f32).collect();
    widths.sort_by(f32::total_cmp);
    let char_w = widths.get(widths.len() / 2).copied().unwrap_or(5.0).max(1.0);
    let left = lines.iter().flat_map(|l| l.words.first()).map(|w| w.x0).fold(f32::MAX, f32::min);
    let mut out = String::new();
    let mut prev: Option<(f32, f32)> = None; // (bottom, height)
    for l in lines {
        let top = l.words.iter().map(|w| w.y0).fold(f32::MAX, f32::min);
        let h = line_height(l);
        if let Some((bottom, ph)) = prev {
            let gap = top - bottom;
            if gap > 1.2 * ph.max(h) {
                out.push_str("\r\n");
            }
        }
        let mut row = String::new();
        for w in &l.words {
            let col = ((w.x0 - left) / char_w).round().max(0.0) as usize;
            let len = row.chars().count();
            if col > len {
                row.push_str(&" ".repeat(col - len));
            } else if len > 0 {
                row.push(' ');
            }
            row.push_str(&w.text);
        }
        out.push_str(row.trim_end());
        out.push_str("\r\n");
        prev = Some((l.words.iter().map(|w| w.y1).fold(f32::MIN, f32::max), h));
    }
    out
}

/// Rebuilds paragraphs: lines join until a clear vertical gap, a change of text size, or a short last line.
fn paragraphs(lines: &[Line]) -> Vec<Para> {
    let mut out: Vec<Para> = vec![];
    if lines.is_empty() {
        return out;
    }
    let right = lines.iter().flat_map(|l| l.words.last()).map(|w| w.x1).fold(f32::MIN, f32::max);
    let left = lines.iter().flat_map(|l| l.words.first()).map(|w| w.x0).fold(f32::MAX, f32::min);
    let width = (right - left).max(1.0);
    let mut prev: Option<(f32, f32, f32)> = None; // bottom, height, right edge
    for l in lines {
        let text = line_text(l);
        if text.trim().is_empty() {
            continue;
        }
        let top = l.words.iter().map(|w| w.y0).fold(f32::MAX, f32::min);
        let bottom = l.words.iter().map(|w| w.y1).fold(f32::MIN, f32::max);
        let h = line_height(l);
        let end = l.words.last().map(|w| w.x1).unwrap_or(right);
        let join = match (prev, out.last()) {
            (Some((pb, ph, pend)), Some(_)) => {
                let gap = top - pb;
                let same_size = (h - ph).abs() <= 0.2 * ph.max(h);
                let prev_full = pend - left >= 0.72 * width;
                gap <= 0.75 * ph.max(h) && gap > -0.5 * h && same_size && prev_full
            }
            _ => false,
        };
        if join {
            let p = out.last_mut().unwrap();
            // undo end-of-line hyphenation: "infor-" + "mation"
            if p.text.ends_with('-') && p.text.chars().rev().nth(1).map(char::is_alphabetic).unwrap_or(false) && text.chars().next().map(char::is_lowercase).unwrap_or(false) {
                p.text.pop();
            } else {
                p.text.push(' ');
            }
            p.text.push_str(&text);
            p.size = p.size.max(h);
        } else {
            out.push(Para { text, size: h });
        }
        prev = Some((bottom, h, end));
    }
    out
}

fn read_pages(src: &Path, ctl: &Ctl, mut each: impl FnMut(usize, &[Line])) -> Result<usize> {
    render::with_doc(src, |d| {
        let n = d.pages().len() as usize;
        for (i, page) in d.pages().iter().enumerate() {
            ctl.check()?;
            ctl.report(0.5 + 0.45 * i as f64 / n.max(1) as f64);
            let lines = page_lines(&page)?;
            each(i, &lines);
        }
        Ok(n)
    })
}

/// PDF -> .txt (UTF-8). `layout` = "flow" reflows paragraphs; anything else keeps the page layout.
pub fn pdf_to_text(src: &Path, name_src: &Path, out_dir: &Path, opts: &Opts, ctl: &Ctl, ocr_note: String) -> Result<Outcome> {
    let flow = opts.str("layout", "keep") == "flow";
    let mut text = String::new();
    let mut chars = 0usize;
    read_pages(src, ctl, |i, lines| {
        if i > 0 {
            text.push_str("\r\n\r\n");
        }
        let page = if flow {
            paragraphs(lines).into_iter().map(|p| p.text).collect::<Vec<_>>().join("\r\n\r\n") + "\r\n"
        } else {
            layout_text(lines)
        };
        chars += page.chars().filter(|c| !c.is_whitespace()).count();
        text.push_str(&page);
    })?;
    let mut note = ocr_note;
    if chars == 0 {
        note = if note.is_empty() {
            "No text was found. This PDF looks scanned: turn on \u{201c}Read scanned pages\u{201d} to recognise it.".into()
        } else {
            "No text could be recognised on these pages.".into()
        };
    }
    let tmp = TempDir::new()?;
    let file = tmp.join("out.txt");
    fs::write(&file, text.as_bytes())?;
    let out = publish(&file, out_dir, &stem_of(name_src), ".txt")?;
    Ok(Outcome::one(&out).with_note(note).with_detail(format!("{} characters", chars)))
}

fn xml(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>"#;
const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
const DOC_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;
const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="Calibri" w:cs="Calibri"/><w:sz w:val="22"/><w:szCs w:val="22"/><w:lang w:val="en-US"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="264" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="360" w:after="120"/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:sz w:val="32"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="240" w:after="80"/><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:sz w:val="26"/></w:rPr></w:style></w:styles>"#;

/// PDF -> .docx made of plain paragraphs and headings that reflow freely (no text boxes or fixed positions).
pub fn pdf_to_docx_flowing(src: &Path, name_src: &Path, out_dir: &Path, ctl: &Ctl, ocr_note: String) -> Result<Outcome> {
    let mut paras: Vec<Para> = vec![];
    read_pages(src, ctl, |_, lines| paras.extend(paragraphs(lines)))?;
    if paras.is_empty() {
        anyhow::bail!(
            "No text was found in this PDF. If it is scanned, turn on \u{201c}Read scanned pages\u{201d}, or use \u{201c}Keep layout\u{201d} to get the pages as pictures."
        );
    }
    // body text size = the most common one, weighted by length
    let mut sizes: Vec<(f32, usize)> = vec![];
    for p in &paras {
        match sizes.iter_mut().find(|(s, _)| (s - p.size).abs() <= 0.1 * s.max(p.size)) {
            Some(e) => e.1 += p.text.len(),
            None => sizes.push((p.size, p.text.len())),
        }
    }
    let body = sizes.iter().max_by_key(|(_, n)| *n).map(|(s, _)| *s).unwrap_or(10.0);
    let mut doc = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#,
    );
    for p in &paras {
        let short = p.text.chars().count() < 120;
        let style = if short && p.size > 1.6 * body {
            Some("Heading1")
        } else if short && p.size > 1.2 * body {
            Some("Heading2")
        } else {
            None
        };
        doc.push_str("<w:p>");
        if let Some(st) = style {
            doc.push_str(&format!(r#"<w:pPr><w:pStyle w:val="{st}"/></w:pPr>"#));
        }
        doc.push_str(&format!(r#"<w:r><w:t xml:space="preserve">{}</w:t></w:r></w:p>"#, xml(&p.text)));
    }
    doc.push_str(r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr></w:body></w:document>"#);

    let tmp = TempDir::new()?;
    let file = tmp.join("out.docx");
    {
        let f = fs::File::create(&file)?;
        let mut z = zip::ZipWriter::new(f);
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, body) in [
            ("[Content_Types].xml", CONTENT_TYPES),
            ("_rels/.rels", RELS),
            ("word/_rels/document.xml.rels", DOC_RELS),
            ("word/styles.xml", STYLES),
            ("word/document.xml", doc.as_str()),
        ] {
            z.start_file(name, o)?;
            z.write_all(body.as_bytes())?;
        }
        z.finish()?;
    }
    let out = publish(&file, out_dir, &stem_of(name_src), ".docx")?;
    Ok(Outcome::one(&out).with_note(ocr_note).with_detail(format!("{} paragraphs", paras.len())))
}
