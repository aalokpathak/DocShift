//! PDF/A-2b, the archival flavour of PDF. On top of a normal PDF the standard wants: XMP metadata that matches the
//! document info, an output intent with an embedded ICC colour profile, a file ID, a transparency group on pages
//! that use soft masks, and no JavaScript or encryption. Fonts must be embedded: Office embeds them, image-only PDFs
//! have none, and DocShift's invisible OCR text is exempt (text drawn in rendering mode 3).
use crate::pdfops;
use anyhow::{anyhow, Result};
use lopdf::{dictionary, Dictionary, Document, Object, Stream};
use std::path::Path;

fn s15(v: f64) -> [u8; 4] {
    ((v * 65536.0).round() as i32).to_be_bytes()
}

/// A compact ICC v2 display profile for sRGB (D50-adapted primaries, sRGB tone curve as a 1024-entry table).
pub fn srgb_icc() -> Vec<u8> {
    fn xyz(x: f64, y: f64, z: f64) -> Vec<u8> {
        let mut v = b"XYZ \0\0\0\0".to_vec();
        for c in [x, y, z] {
            v.extend_from_slice(&s15(c));
        }
        v
    }
    let mut desc = b"desc\0\0\0\0".to_vec();
    let name = b"sRGB IEC61966-2.1\0";
    desc.extend_from_slice(&(name.len() as u32).to_be_bytes());
    desc.extend_from_slice(name);
    desc.extend_from_slice(&[0u8; 4 + 4 + 2 + 1 + 67]); // no Unicode / ScriptCode descriptions
    let mut cprt = b"text\0\0\0\0".to_vec();
    cprt.extend_from_slice(b"No copyright, use freely\0");
    let mut curv = b"curv\0\0\0\0".to_vec();
    curv.extend_from_slice(&1024u32.to_be_bytes());
    for i in 0..1024 {
        let v = i as f64 / 1023.0;
        let lin = if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        curv.extend_from_slice(&((lin * 65535.0).round() as u16).to_be_bytes());
    }
    let tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"desc", desc),
        (b"cprt", cprt),
        (b"wtpt", xyz(0.9642, 1.0, 0.8249)),
        (b"rXYZ", xyz(0.436_074_7, 0.222_504_5, 0.013_932_2)),
        (b"gXYZ", xyz(0.385_064_9, 0.716_878_6, 0.097_104_5)),
        (b"bXYZ", xyz(0.143_080_4, 0.060_616_9, 0.714_173_3)),
        (b"rTRC", curv),
    ];
    // gTRC and bTRC share the rTRC data
    let count = tags.len() + 2;
    let mut offset = 128 + 4 + 12 * count;
    let mut table = (count as u32).to_be_bytes().to_vec();
    let mut data = vec![];
    let mut trc = (0u32, 0u32);
    for (sig, body) in &tags {
        while offset % 4 != 0 {
            offset += 1;
            data.push(0);
        }
        table.extend_from_slice(*sig);
        table.extend_from_slice(&(offset as u32).to_be_bytes());
        table.extend_from_slice(&(body.len() as u32).to_be_bytes());
        if *sig == b"rTRC" {
            trc = (offset as u32, body.len() as u32);
        }
        data.extend_from_slice(body);
        offset += body.len();
    }
    for sig in [b"gTRC", b"bTRC"] {
        table.extend_from_slice(sig);
        table.extend_from_slice(&trc.0.to_be_bytes());
        table.extend_from_slice(&trc.1.to_be_bytes());
    }
    let size = 128 + table.len() + data.len();
    let mut h = vec![0u8; 128];
    h[0..4].copy_from_slice(&(size as u32).to_be_bytes());
    h[8..12].copy_from_slice(&[0x02, 0x10, 0, 0]);
    h[12..16].copy_from_slice(b"mntr");
    h[16..20].copy_from_slice(b"RGB ");
    h[20..24].copy_from_slice(b"XYZ ");
    for (i, v) in [2026u16, 1, 1, 0, 0, 0].iter().enumerate() {
        h[24 + i * 2..26 + i * 2].copy_from_slice(&v.to_be_bytes());
    }
    h[36..40].copy_from_slice(b"acsp");
    h[68..72].copy_from_slice(&s15(0.9642));
    h[72..76].copy_from_slice(&s15(1.0));
    h[76..80].copy_from_slice(&s15(0.8249));
    let mut out = h;
    out.extend(table);
    out.extend(data);
    out
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Current UTC time as (PDF date, XMP date).
fn now() -> (String, String) {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // civil-from-days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    let (hh, mm, ss) = (rem / 3600, rem % 3600 / 60, rem % 60);
    (format!("D:{y:04}{m:02}{d:02}{hh:02}{mm:02}{ss:02}Z"), format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z"))
}

fn page_uses_soft_masks(doc: &Document, page: &Dictionary) -> bool {
    let res = match page.get(b"Resources") {
        Ok(Object::Reference(r)) => doc.get_dictionary(*r).ok(),
        Ok(Object::Dictionary(d)) => Some(d),
        _ => None,
    };
    let Some(xo) = res.and_then(|r| r.get(b"XObject").ok()).and_then(|o| match o {
        Object::Reference(r) => doc.get_dictionary(*r).ok(),
        Object::Dictionary(d) => Some(d),
        _ => None,
    }) else {
        return false;
    };
    xo.iter().any(|(_, v)| v.as_reference().ok().and_then(|id| doc.get_object(id).ok()).and_then(|o| o.as_stream().ok()).map(|s| s.dict.has(b"SMask")).unwrap_or(false))
}

/// Turns `doc` into a PDF/A-2b candidate in place.
pub fn apply(doc: &mut Document, title: &str) -> Result<()> {
    doc.version = "1.7".into();
    doc.trailer.remove(b"Encrypt");
    let (pdf_date, xmp_date) = now();

    // pages with transparency need a blending colour space
    let pages: Vec<_> = doc.get_pages().into_values().collect();
    for id in pages {
        let needs = doc.get_dictionary(id).map(|d| !d.has(b"Group") && page_uses_soft_masks(doc, d)).unwrap_or(false);
        if needs {
            if let Ok(d) = doc.get_dictionary_mut(id) {
                d.set("Group", dictionary! { "Type" => "Group", "S" => "Transparency", "CS" => "DeviceRGB" });
            }
        }
    }

    let icc = {
        let mut s = Stream::new(dictionary! { "N" => 3 }, srgb_icc());
        let _ = s.compress();
        doc.add_object(s)
    };
    let intent = dictionary! {
        "Type" => "OutputIntent",
        "S" => "GTS_PDFA1",
        "OutputConditionIdentifier" => Object::string_literal("sRGB IEC61966-2.1"),
        "Info" => Object::string_literal("sRGB IEC61966-2.1"),
        "RegistryName" => Object::string_literal("http://www.color.org"),
        "DestOutputProfile" => icc,
    };
    let xmp = format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
<rdf:Description rdf:about=\"\" xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" xmlns:pdf=\"http://ns.adobe.com/pdf/1.3/\">\n\
<pdfaid:part>2</pdfaid:part>\n<pdfaid:conformance>B</pdfaid:conformance>\n\
<dc:format>application/pdf</dc:format>\n\
<dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">{title}</rdf:li></rdf:Alt></dc:title>\n\
<xmp:CreatorTool>DocShift</xmp:CreatorTool>\n<xmp:CreateDate>{d}</xmp:CreateDate>\n<xmp:ModifyDate>{d}</xmp:ModifyDate>\n\
<xmp:MetadataDate>{d}</xmp:MetadataDate>\n<pdf:Producer>DocShift</pdf:Producer>\n\
</rdf:Description>\n</rdf:RDF>\n</x:xmpmeta>\n<?xpacket end=\"w\"?>",
        title = xml_escape(title),
        d = xmp_date
    );
    // the metadata stream must stay uncompressed so archive tools can read it without a PDF parser
    let meta = doc.add_object(Stream::new(dictionary! { "Type" => "Metadata", "Subtype" => "XML" }, xmp.into_bytes()).with_compression(false));

    let info = doc.add_object(dictionary! {
        "Title" => pdfops::pdf_text(title),
        "Creator" => Object::string_literal("DocShift"),
        "Producer" => Object::string_literal("DocShift"),
        "CreationDate" => Object::string_literal(pdf_date.clone()),
        "ModDate" => Object::string_literal(pdf_date),
    });
    doc.trailer.set("Info", info);

    let cat = doc.catalog_mut().map_err(|_| anyhow!("This PDF has no catalog and can't be made archival."))?;
    cat.set("Metadata", meta);
    cat.set("OutputIntents", vec![Object::Dictionary(intent)]);
    cat.remove(b"AA");
    if let Ok(Object::Dictionary(names)) = cat.get_mut(b"Names") {
        names.remove(b"JavaScript");
        names.remove(b"EmbeddedFiles");
    }
    if let Ok(Object::Dictionary(oa)) = cat.get(b"OpenAction") {
        if oa.get(b"S").ok().and_then(|s| s.as_name().ok()) == Some(b"JavaScript") {
            cat.remove(b"OpenAction");
        }
    }

    // a file identifier is mandatory
    let mut id = [0u8; 16];
    let seed = format!("{xmp_date}{title}{}", doc.objects.len());
    for (i, b) in seed.bytes().enumerate() {
        id[i % 16] = id[i % 16].wrapping_mul(31).wrapping_add(b);
    }
    let id_obj = Object::String(id.to_vec(), lopdf::StringFormat::Hexadecimal);
    doc.trailer.set("ID", vec![id_obj.clone(), id_obj]);
    Ok(())
}

/// Loads a finished PDF, makes it PDF/A-2b and writes it back.
pub fn apply_file(path: &Path, title: &str) -> Result<()> {
    let mut doc = pdfops::load(path)?;
    apply(&mut doc, title)?;
    pdfops::save_doc_to(&mut doc, path)
}
