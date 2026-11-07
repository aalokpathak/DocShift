//! Image engine: decoding (with EXIF orientation), JPEG / WebP / PNG encoding, size-targeted compression,
//! lossless "decompress", EXIF dates (used by the image sorter) and thumbnails.
use crate::core::*;
use anyhow::{anyhow, bail, Context, Result};
use image::{imageops::FilterType, DynamicImage, GenericImageView, ImageDecoder, ImageReader};
use std::{fs, io::BufReader, path::Path};

/// A decoded image plus the colour profile to carry over.
pub struct Loaded {
    pub img: DynamicImage,
    pub format: Option<image::ImageFormat>,
    pub icc: Option<Vec<u8>>,
}

/// Phone photos (HEIC/HEIF) and AVIF aren't handled by the image crate; the bundled ffmpeg decodes them.
pub fn needs_ffmpeg(path: &Path) -> bool {
    matches!(crate::kinds::ext_of(path).as_str(), "heic" | "heif" | "hif" | "avif")
}

fn load_with_ffmpeg(path: &Path) -> Result<Loaded> {
    let png = crate::video::decode_still(path).map_err(|e| anyhow!("This photo could not be read ({e}). It may be damaged."))?;
    let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png).map_err(|_| anyhow!("This photo could not be read. It may be damaged."))?;
    Ok(Loaded { img, format: None, icc: None })
}

pub fn load(path: &Path) -> Result<Loaded> {
    if needs_ffmpeg(path) {
        return load_with_ffmpeg(path);
    }
    let reader = ImageReader::open(path)
        .with_context(|| format!("Can't open {}", path.display()))?
        .with_guessed_format()
        .context("This file could not be read as an image.")?;
    let format = reader.format();
    let bad = |_| anyhow!("This file could not be opened as an image. It may be damaged.");
    let mut decoder = reader.into_decoder().map_err(bad)?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let icc = decoder.icc_profile().ok().flatten();
    let mut img = DynamicImage::from_decoder(decoder).map_err(bad)?;
    img.apply_orientation(orientation);
    Ok(Loaded { img, format, icc })
}

pub fn has_alpha(img: &DynamicImage) -> bool {
    img.color().has_alpha()
}

fn is_gray(img: &DynamicImage) -> bool {
    !img.color().has_color()
}

/// Alpha composited over white, as RGB.
pub fn flatten(img: &DynamicImage) -> image::RgbImage {
    if !has_alpha(img) {
        return img.to_rgb8();
    }
    let rgba = img.to_rgba8();
    let mut out = image::RgbImage::new(rgba.width(), rgba.height());
    for (o, p) in out.pixels_mut().zip(rgba.pixels()) {
        let a = p[3] as u32;
        let mix = |c: u8| ((c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
        *o = image::Rgb([mix(p[0]), mix(p[1]), mix(p[2])]);
    }
    out
}

pub fn scaled(img: &DynamicImage, scale: f64) -> DynamicImage {
    if scale >= 0.999 {
        return img.clone();
    }
    let (w, h) = img.dimensions();
    let nw = ((w as f64 * scale).round() as u32).max(1);
    let nh = ((h as f64 * scale).round() as u32).max(1);
    img.resize_exact(nw, nh, FilterType::CatmullRom)
}

// ----------------------------------------------------------------------------------------------- encoders

pub fn encode_jpeg(img: &DynamicImage, quality: u8, icc: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut enc = jpeg_encoder::Encoder::new(&mut buf, quality.clamp(1, 100));
    enc.set_progressive(true);
    enc.set_optimized_huffman_tables(true);
    if let Some(p) = icc {
        let _ = enc.add_icc_profile(p);
    }
    let (w, h) = (img.width().min(65535) as u16, img.height().min(65535) as u16);
    if is_gray(img) && !has_alpha(img) {
        enc.encode(img.to_luma8().as_raw(), w, h, jpeg_encoder::ColorType::Luma)?;
    } else {
        enc.encode(flatten(img).as_raw(), w, h, jpeg_encoder::ColorType::Rgb)?;
    }
    Ok(buf)
}

pub fn encode_webp(img: &DynamicImage, quality: u8) -> Result<Vec<u8>> {
    let (w, h) = img.dimensions();
    let mem = if has_alpha(img) {
        let rgba = img.to_rgba8();
        webp::Encoder::from_rgba(rgba.as_raw(), w, h).encode(quality as f32)
    } else {
        let rgb = img.to_rgb8();
        webp::Encoder::from_rgb(rgb.as_raw(), w, h).encode(quality as f32)
    };
    Ok(mem.to_vec())
}

/// PNG, optionally reduced to `colors` palette entries (lossy, but a big saving for graphics and screenshots).
pub fn encode_png(img: &DynamicImage, colors: Option<usize>, icc: Option<&[u8]>) -> Result<Vec<u8>> {
    let (w, h) = img.dimensions();
    let mut buf = Vec::new();
    let alpha = has_alpha(img);
    {
        let mut enc = png::Encoder::new(&mut buf, w, h);
        enc.set_compression(png::Compression::Best);
        enc.set_adaptive_filter(png::AdaptiveFilterType::Adaptive);
        enc.set_depth(png::BitDepth::Eight);
        let _ = icc; // profile is kept for JPEG / WebP; PNG palette output is sRGB-tagged implicitly
        if let Some(n) = colors {
            let rgba = img.to_rgba8();
            let nq = color_quant::NeuQuant::new(10, n.clamp(2, 256), rgba.as_raw());
            let palette = nq.color_map_rgba();
            let mut rgb = Vec::with_capacity(palette.len() / 4 * 3);
            let mut trns = Vec::with_capacity(palette.len() / 4);
            for c in palette.chunks_exact(4) {
                rgb.extend_from_slice(&c[..3]);
                trns.push(c[3]);
            }
            let indices: Vec<u8> = rgba.pixels().map(|p| nq.index_of(&p.0) as u8).collect();
            enc.set_color(png::ColorType::Indexed);
            enc.set_palette(rgb);
            if alpha {
                enc.set_trns(trns);
            }
            let mut writer = enc.write_header()?;
            writer.write_image_data(&indices)?;
            writer.finish()?;
        } else if is_gray(img) && !alpha {
            enc.set_color(png::ColorType::Grayscale);
            let mut writer = enc.write_header()?;
            writer.write_image_data(img.to_luma8().as_raw())?;
            writer.finish()?;
        } else if alpha {
            enc.set_color(png::ColorType::Rgba);
            let mut writer = enc.write_header()?;
            writer.write_image_data(img.to_rgba8().as_raw())?;
            writer.finish()?;
        } else {
            enc.set_color(png::ColorType::Rgb);
            let mut writer = enc.write_header()?;
            writer.write_image_data(img.to_rgb8().as_raw())?;
            writer.finish()?;
        }
    }
    Ok(buf)
}

// ------------------------------------------------------------------------------------------- quality search

/// Highest quality in `lo..=hi` whose output still fits `target` bytes.
/// Returns `(Some(q), data)` on success or `(None, data_at_lo)` when even `lo` is too big.
/// Size grows smoothly with quality, so interpolating on log(size) needs about half the encodes of a bisection.
pub fn best_quality(
    mut encode: impl FnMut(u8) -> Result<Vec<u8>>,
    target: u64,
    mut lo: u8,
    mut hi: u8,
    ctl: &Ctl,
    max_iter: usize,
) -> Result<(Option<u8>, Vec<u8>)> {
    let low = encode(lo)?;
    if low.len() as u64 > target {
        return Ok((None, low));
    }
    let top = encode(hi)?;
    if top.len() as u64 <= target {
        return Ok((Some(hi), top));
    }
    let (mut best_q, mut best) = (lo, low);
    let (mut s_lo, mut s_hi) = (best.len() as f64, top.len() as f64);
    for _ in 0..max_iter {
        if hi - lo <= 1 || best.len() as f64 >= 0.97 * target as f64 {
            break;
        }
        ctl.check()?;
        let frac = if s_hi > s_lo { ((target as f64).ln() - s_lo.ln()) / (s_hi.ln() - s_lo.ln()) } else { 0.5 };
        let q = ((lo as f64 + frac * (hi - lo) as f64).round() as i32).clamp(lo as i32 + 1, hi as i32 - 1) as u8;
        let data = encode(q)?;
        if data.len() as u64 <= target {
            lo = q;
            s_lo = data.len() as f64;
            best_q = q;
            best = data;
        } else {
            hi = q;
            s_hi = data.len() as f64;
        }
    }
    Ok((Some(best_q), best))
}

/// How much to scale each side so a file of `size` bytes lands just under `target` (size ~ pixel count).
pub fn shrink_factor(target: u64, size: usize) -> f64 {
    ((target as f64 / size as f64).sqrt() * 0.97).min(0.92)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fmt {
    Jpeg,
    Webp,
    Png,
}

impl Fmt {
    pub fn ext(self) -> &'static str {
        match self {
            Fmt::Jpeg => ".jpg",
            Fmt::Webp => ".webp",
            Fmt::Png => ".png",
        }
    }
}

fn scale_note(scale: f64, colors: Option<usize>) -> String {
    let mut bits = vec![];
    if scale < 0.999 {
        bits.push(format!("resized to {:.0}% to fit", scale * 100.0));
    }
    if let Some(c) = colors {
        bits.push(format!("reduced to {c} colours"));
    }
    if bits.is_empty() { String::new() } else { format!("Image was {}.", bits.join(" and ")) }
}

/// Keep as much resolution and quality as possible under `target` bytes. The picture is only shrunk when even a
/// decent minimum quality doesn't fit.
fn fit_image(l: &Loaded, fmt: Fmt, target: u64, src_size: u64, ctl: &Ctl) -> Result<(Vec<u8>, String)> {
    let icc = l.icc.as_deref();
    let mut scale = 1.0f64;
    let mut size = target as usize;
    for _ in 0..10 {
        ctl.check()?;
        let work = scaled(&l.img, scale);
        if work.width().min(work.height()) < 16 {
            break;
        }
        if fmt == Fmt::Png {
            let mut ladder: Vec<Option<usize>> = vec![];
            if target as f64 >= 0.6 * src_size as f64 {
                ladder.push(None);
            }
            ladder.extend([Some(256), Some(128), Some(64)]);
            for colors in ladder {
                ctl.check()?;
                let data = encode_png(&work, colors, icc)?;
                if data.len() as u64 <= target {
                    return Ok((data, scale_note(scale, colors)));
                }
                size = data.len();
            }
        } else {
            let enc = |q: u8| if fmt == Fmt::Jpeg { encode_jpeg(&work, q, icc) } else { encode_webp(&work, q) };
            let (q, data) = best_quality(enc, target, 35, 95, ctl, 7)?;
            if q.is_some() {
                return Ok((data, scale_note(scale, None)));
            }
            size = data.len();
        }
        scale *= shrink_factor(target, size);
    }
    bail!("Couldn't get this image under {} - the smallest result was {}. Try a larger limit.", fmt_size(target), fmt_size(size as u64))
}

fn level_params(level: &str) -> (u8, Option<u32>, Option<usize>) {
    // (jpeg/webp quality, max long side, png colours)
    match level {
        "light" => (88, None, None),
        "strong" => (55, Some(2000), Some(64)),
        _ => (75, Some(3000), Some(256)),
    }
}

/// Copy the source unchanged when it is already under the limit.
pub fn already_under(src: &Path, target: Option<u64>, out_dir: &Path, tag: &str) -> Result<Option<Outcome>> {
    let Some(t) = target else { return Ok(None) };
    let size = fs::metadata(src)?.len();
    if size > t {
        return Ok(None);
    }
    fs::create_dir_all(out_dir)?;
    let ext = src.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let out = target_path(out_dir, &format!("{}_{tag}", stem_of(src)), &ext)?;
    fs::copy(src, &out)?;
    Ok(Some(
        Outcome::one(&out)
            .with_note(format!("Already under {}, so it was left unchanged.", fmt_size(t)))
            .with_detail(size_detail(size, size)),
    ))
}

/// Finalise: never hand back something larger than the input. `produced` is a finished file in a temp location.
pub fn deliver_path(src: &Path, produced: &Path, out_dir: &Path, tag: &str, ext: &str, note: String, keep_if_larger: bool) -> Result<Outcome> {
    let before = fs::metadata(src)?.len();
    let mut after = fs::metadata(produced)?.len();
    let (mut note, mut ext) = (note, ext.to_string());
    if keep_if_larger && after >= before {
        fs::copy(src, produced)?;
        after = before;
        ext = src.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        note = format!("{}{}It was already well compressed, so an unchanged copy was saved.", note, if note.is_empty() { "" } else { " " });
    }
    let out = publish(produced, out_dir, &format!("{}_{tag}", stem_of(src)), &ext)?;
    Ok(Outcome::one(&out).with_note(note).with_detail(size_detail(before, after)))
}

/// Same, for results that are still in memory.
pub fn deliver(src: &Path, data: &[u8], out_dir: &Path, tag: &str, ext: &str, note: String, keep_if_larger: bool) -> Result<Outcome> {
    let tmp = TempDir::new()?;
    let produced = tmp.join("out");
    fs::write(&produced, data)?;
    deliver_path(src, &produced, out_dir, tag, ext, note, keep_if_larger)
}

pub fn compress_image(src: &Path, out_dir: &Path, opts: &Opts, ctl: &Ctl) -> Result<Outcome> {
    let target = parse_target(&opts.str("target_mb", ""))?;
    if let Some(done) = already_under(src, target, out_dir, "compressed")? {
        return Ok(done);
    }
    let mut l = load(src)?;
    let level = opts.str("level", "balanced");
    let fmt = match opts.str("format", "same").as_str() {
        "jpg" => Fmt::Jpeg,
        "webp" => Fmt::Webp,
        "png" => Fmt::Png,
        _ => match l.format {
            Some(image::ImageFormat::Png) => Fmt::Png,
            Some(image::ImageFormat::WebP) => Fmt::Webp,
            _ => Fmt::Jpeg,
        },
    };
    ctl.check()?;
    let (data, note) = match target {
        None => {
            let (q, cap, colors) = level_params(&level);
            if let Some(cap) = cap {
                let long = l.img.width().max(l.img.height());
                if long > cap {
                    l.img = scaled(&l.img, cap as f64 / long as f64);
                }
            }
            let icc = l.icc.as_deref();
            let data = match fmt {
                Fmt::Jpeg => encode_jpeg(&l.img, q, icc)?,
                Fmt::Webp => encode_webp(&l.img, q)?,
                Fmt::Png => encode_png(&l.img, colors, icc)?,
            };
            (data, String::new())
        }
        Some(t) => fit_image(&l, fmt, t, fs::metadata(src)?.len(), ctl)?,
    };
    // HEIC/AVIF sources: always hand back the JPG/WebP/PNG (the point is usually compatibility), even if larger
    deliver(src, &data, out_dir, "compressed", fmt.ext(), note, !needs_ffmpeg(src))
}

pub fn expand_image(src: &Path, out_dir: &Path, opts: &Opts) -> Result<Outcome> {
    let l = load(src)?;
    let (format, ext) = match opts.str("expand_format", "png").as_str() {
        "tiff" => (image::ImageFormat::Tiff, ".tiff"),
        "bmp" => (image::ImageFormat::Bmp, ".bmp"),
        _ => (image::ImageFormat::Png, ".png"),
    };
    let img = if format == image::ImageFormat::Bmp { DynamicImage::ImageRgb8(flatten(&l.img)) } else { l.img };
    let tmp = TempDir::new()?;
    let produced = tmp.join("out");
    img.save_with_format(&produced, format).context("Couldn't write the image")?;
    let data = fs::read(&produced)?;
    deliver(src, &data, out_dir, "expanded", ext, "Detail already lost to earlier compression can't be recovered.".into(), false)
}

// ---------------------------------------------------------------------------------------- metadata, thumbs

/// Capture date from EXIF (DateTimeOriginal), as ms since the Unix epoch (treated as UTC; only used for ordering).
pub fn exif_taken_ms(path: &Path) -> Option<i64> {
    let file = fs::File::open(path).ok()?;
    let exif = exif::Reader::new().read_from_container(&mut BufReader::new(file)).ok()?;
    let field = exif
        .get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)
        .or_else(|| exif.get_field(exif::Tag::DateTime, exif::In::PRIMARY))?;
    let s = field.display_value().to_string(); // "2024-03-18 14:05:09"
    let (d, t) = s.split_once(' ')?;
    let dp: Vec<i64> = d.split('-').filter_map(|x| x.trim().parse().ok()).collect();
    let tp: Vec<i64> = t.split(':').filter_map(|x| x.trim().parse().ok()).collect();
    if dp.len() != 3 || tp.len() != 3 {
        return None;
    }
    Some((days_from_civil(dp[0], dp[1], dp[2]) * 86_400 + tp[0] * 3600 + tp[1] * 60 + tp[2]) * 1000)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn dimensions(path: &Path) -> Option<(u32, u32)> {
    if needs_ffmpeg(path) {
        return crate::video::probe(path).ok().map(|p| (p.width, p.height));
    }
    ImageReader::open(path).ok()?.with_guessed_format().ok()?.into_dimensions().ok()
}

/// Small JPEG preview (as bytes) of an image file.
pub fn thumbnail_jpeg(path: &Path, max_side: u32) -> Result<Vec<u8>> {
    let l = load(path)?;
    jpeg_thumb(&l.img, max_side)
}

pub fn jpeg_thumb(img: &DynamicImage, max_side: u32) -> Result<Vec<u8>> {
    let (w, h) = img.dimensions();
    let long = w.max(h);
    let small = if long > max_side { img.resize(max_side, max_side, FilterType::Triangle) } else { img.clone() };
    encode_jpeg(&small, 80, None)
}
