//! Video compression / decompression with the bundled ffmpeg. With a size limit the bitrate is computed from the
//! video's length, a sensible resolution is chosen and a two-pass encode is run, tightened if it overshoots.
use crate::{core::*, engine, imgops};
use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Default, Clone, Debug)]
pub struct Probe {
    pub duration: Option<f64>,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub has_audio: bool,
}

pub fn ffmpeg() -> Result<PathBuf> {
    engine::ffmpeg_path().ok_or_else(|| anyhow!("The video engine (ffmpeg) is missing, so videos can't be processed."))
}

fn command(ff: &Path) -> Command {
    let mut c = Command::new(ff);
    c.creation_flags(CREATE_NO_WINDOW).stdin(Stdio::null());
    c
}

pub fn probe(src: &Path) -> Result<Probe> {
    let ff = ffmpeg()?;
    let out = command(&ff).args(["-hide_banner", "-i"]).arg(src).stdout(Stdio::null()).stderr(Stdio::piped()).output()?;
    let text = String::from_utf8_lossy(&out.stderr).into_owned();
    let mut p = Probe { fps: 30.0, ..Default::default() };
    if let Some(m) = Regex::new(r"Duration:\s*(\d+):(\d+):(\d+(?:\.\d+)?)")?.captures(&text) {
        p.duration = Some(m[1].parse::<f64>()? * 3600.0 + m[2].parse::<f64>()? * 60.0 + m[3].parse::<f64>()?);
    }
    let line = text
        .lines()
        .find(|l| l.contains("Video:") && !l.contains("attached pic"))
        .ok_or_else(|| anyhow!("No video track was found in this file."))?;
    if let Some(m) = Regex::new(r"[\s,](\d{2,5})x(\d{2,5})(?:[\s,\[])")?.captures(line) {
        p.width = m[1].parse()?;
        p.height = m[2].parse()?;
    }
    if let Some(m) = Regex::new(r"(\d+(?:\.\d+)?)\s*fps")?.captures(line) {
        p.fps = m[1].parse::<f64>()?.clamp(1.0, 120.0);
    }
    if let Some(m) = Regex::new(r"rotation of (-?\d+(?:\.\d+)?)")?.captures(&text) {
        let rot = m[1].parse::<f64>()?.round() as i64;
        if rot.abs() % 180 == 90 {
            std::mem::swap(&mut p.width, &mut p.height); // phone video stored sideways
        }
    }
    p.has_audio = text.contains("Audio:");
    if p.width == 0 || p.height == 0 {
        bail!("Couldn't read this video's size. The file may be damaged.");
    }
    Ok(p)
}

/// A JPEG still from near the start of the video.
pub fn thumbnail(src: &Path, width: u32) -> Result<Vec<u8>> {
    let ff = ffmpeg()?;
    let seek = match probe(src).ok().and_then(|p| p.duration) {
        Some(d) if d > 4.0 => "1.5",
        _ => "0",
    };
    let mut child = command(&ff)
        .args(["-hide_banner", "-loglevel", "error", "-ss", seek, "-i"])
        .arg(src)
        .args(["-frames:v", "1", "-vf", &format!("scale={width}:-2"), "-f", "image2pipe", "-vcodec", "mjpeg", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut buf = Vec::new();
    child.stdout.take().context("ffmpeg gave no output")?.read_to_end(&mut buf)?;
    let _ = child.wait();
    if buf.is_empty() {
        bail!("No preview available");
    }
    Ok(buf)
}

/// Decodes a still image ffmpeg understands but the image crate doesn't (HEIC/HEIF phone photos, AVIF) to PNG bytes.
pub fn decode_still(src: &Path) -> Result<Vec<u8>> {
    let ff = ffmpeg()?;
    let out = command(&ff)
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(path_arg(src))
        .args(["-frames:v", "1", "-f", "image2pipe", "-vcodec", "png", "-pix_fmt", "rgb24", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    if out.stdout.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("{}", err.lines().last().unwrap_or("no picture found").trim());
    }
    Ok(out.stdout)
}

/// Runs one ffmpeg pass; its progress is mapped onto the `lo..hi` slice of the overall progress bar.
fn run_ffmpeg(args: &[String], tmp: &Path, duration: Option<f64>, ctl: &Ctl, lo: f64, hi: f64) -> Result<()> {
    let ff = ffmpeg()?;
    let log_path = tmp.join("ffmpeg.log");
    let log = fs::File::create(&log_path)?;
    let mut child = command(&ff)
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-progress", "pipe:1", "-nostats"])
        .args(args)
        .current_dir(tmp)
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log))
        .spawn()
        .context("Couldn't start the video engine")?;
    let reader = BufReader::new(child.stdout.take().context("ffmpeg gave no output")?);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if ctl.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Cancelled.into());
        }
        if let (Some(d), Some(v)) = (duration, line.strip_prefix("out_time_us=")) {
            if let Ok(us) = v.trim().parse::<f64>() {
                ctl.report(lo + (hi - lo) * (us / 1e6 / d).min(1.0));
            }
        }
    }
    let status = child.wait()?;
    if !status.success() {
        let text = fs::read_to_string(&log_path).unwrap_or_default();
        let tail: Vec<&str> = text.lines().rev().filter(|l| !l.trim().is_empty()).take(2).collect();
        let msg = tail.into_iter().rev().collect::<Vec<_>>().join(" ");
        bail!("The video could not be processed: {}", if msg.is_empty() { format!("ffmpeg exit code {:?}", status.code()) } else { msg });
    }
    ctl.report(hi);
    Ok(())
}

const HEIGHTS: [u32; 8] = [2160, 1440, 1080, 720, 540, 480, 360, 240];

/// Largest standard short side that still gets about 0.05 bits per pixel; `None` keeps the original size.
fn pick_height(p: &Probe, video_bps: f64) -> Option<u32> {
    let (long, short) = (p.width.max(p.height), p.width.min(p.height));
    let enough = |h: u32| {
        let w = long as f64 * h as f64 / short as f64;
        video_bps / (w * h as f64 * p.fps) >= 0.05
    };
    if enough(short) {
        return None;
    }
    HEIGHTS.iter().copied().find(|&h| h < short && enough(h)).or(Some(240.min(short)))
}

/// Scale filter for a target "p" size (the short side: 720 means 1280x720, or 720x1280 for a phone video).
fn vf_for(p: &Probe, short: Option<u32>) -> String {
    // H.264 needs even dimensions; setsar keeps pixels square after resizing
    match short {
        Some(h) if p.width < p.height => format!("scale={h}:-2,setsar=1"),
        Some(h) => format!("scale=-2:{h},setsar=1"),
        None => "scale=trunc(iw/2)*2:trunc(ih/2)*2,setsar=1".into(),
    }
}

/// The resolution the user picked ("1080", "720", "480"), never upscaling. None = keep / decide automatically.
fn chosen_res(p: &Probe, opts: &Opts) -> Option<u32> {
    let short = p.width.min(p.height);
    match opts.u32("resolution", 0) {
        0 => None,
        r if r < short => Some(r),
        _ => None,
    }
}

fn s(x: impl ToString) -> String {
    x.to_string()
}

fn deliver_path(src: &Path, produced: &Path, out_dir: &Path, tag: &str, ext: &str, note: String, keep_if_larger: bool) -> Result<Outcome> {
    imgops::deliver_path(src, produced, out_dir, tag, ext, note, keep_if_larger)
}

pub fn compress_video(src: &Path, out_dir: &Path, opts: &Opts, ctl: &Ctl) -> Result<Outcome> {
    let target = parse_target(&opts.str("target_mb", ""))?;
    if let Some(done) = imgops::already_under(src, target, out_dir, "compressed")? {
        return Ok(done);
    }
    let info = probe(src)?;
    let level = opts.str("level", "balanced");
    let tmp = TempDir::new()?;
    let out = tmp.join("out.mp4");
    let src_arg = path_arg(src);
    let mux = ["-pix_fmt", "yuv420p", "-movflags", "+faststart"].map(s);
    let note = match target {
        None => {
            let (crf, cap) = match level.as_str() {
                "light" => (23, None),
                "strong" => (31, Some(720)),
                _ => (27, Some(1080)),
            };
            let short = info.width.min(info.height);
            let height = chosen_res(&info, opts).or(cap.filter(|c| short > *c));
            let mut args = vec![s("-i"), src_arg, s("-map"), s("0:v:0"), s("-map"), s("0:a:0?"), s("-vf"), vf_for(&info, height)];
            args.extend(["-c:v", "libx264", "-preset", "fast", "-crf"].map(s));
            args.push(s(crf));
            args.extend(mux);
            args.extend(["-c:a", "aac", "-b:a"].map(s));
            args.push(s(if level == "strong" { "96k" } else { "128k" }));
            args.push(path_arg(&out));
            run_ffmpeg(&args, tmp.path(), info.duration, ctl, 0.0, 1.0)?;
            String::new()
        }
        Some(t) => two_pass(&src_arg, &out, tmp.path(), &info, chosen_res(&info, opts), t, ctl, &mux)?,
    };
    deliver_path(src, &out, out_dir, "compressed", ".mp4", note, true)
}

fn path_arg(p: &Path) -> String {
    let s = p.canonicalize().unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().into_owned();
    s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s)
}

fn two_pass(src_arg: &str, out: &Path, tmp: &Path, info: &Probe, res: Option<u32>, target: u64, ctl: &Ctl, mux: &[String; 4]) -> Result<String> {
    let duration = info.duration.ok_or_else(|| anyhow!("Couldn't read this video's length, which is needed to hit a size limit."))?;
    let mut audio_bps: f64 = if info.has_audio { 128_000.0 } else { 0.0 };
    let total_bps = target as f64 * 8.0 * 0.94 / duration; // keep ~6% in hand for container overhead
    if info.has_audio && total_bps < 300_000.0 {
        audio_bps = if total_bps >= 120_000.0 { 64_000.0 } else { 32_000.0 };
    }
    let mut video_bps = total_bps - audio_bps;
    if video_bps < 40_000.0 {
        let need = (40_000.0 + audio_bps) * duration / 8.0 / 0.94 / MB as f64;
        let length = if duration < 90.0 { format!("{duration:.0}-second") } else { format!("{:.1}-minute", duration / 60.0) };
        bail!("A {length} video can't fit in {}. It needs at least about {:.2} MB.", fmt_size(target), need.max(0.05));
    }
    // the smaller of what the bitrate can carry and what the user asked for
    let height = match (pick_height(info, video_bps), res) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    let vf_arg = vf_for(info, height);
    let enc: Vec<String> = ["-c:v", "libx264", "-preset", "fast", "-passlogfile", "ffpass"].map(s).to_vec();
    let audio: Vec<String> = if info.has_audio {
        let mut a = vec![s("-c:a"), s("aac"), s("-b:a"), s(audio_bps as u64)];
        if audio_bps <= 32_000.0 {
            a.extend([s("-ac"), s("1")]);
        }
        a
    } else {
        vec![s("-an")]
    };
    let mut pass1 = vec![s("-i"), s(src_arg), s("-map"), s("0:v:0"), s("-vf"), vf_arg.clone()];
    pass1.extend(enc.clone());
    pass1.extend([s("-b:v"), s(video_bps as u64), s("-pass"), s("1"), s("-an"), s("-f"), s("null"), s("-")]);
    run_ffmpeg(&pass1, tmp, Some(duration), ctl, 0.0, 0.4)?;
    let mut size = 0u64;
    for attempt in 0..3 {
        let lo = 0.4 + 0.2 * attempt as f64;
        let mut pass2 = vec![s("-i"), s(src_arg), s("-map"), s("0:v:0"), s("-map"), s("0:a:0?"), s("-vf"), vf_arg.clone()];
        pass2.extend(enc.clone());
        pass2.extend([s("-b:v"), s(video_bps as u64), s("-pass"), s("2")]);
        pass2.extend(mux.iter().cloned());
        pass2.extend(audio.clone());
        pass2.push(path_arg(out));
        run_ffmpeg(&pass2, tmp, Some(duration), ctl, lo, lo + 0.2)?;
        size = fs::metadata(out)?.len();
        if size <= target {
            ctl.report(1.0);
            return Ok(height.map(|h| format!("Resized to {h}p to fit.")).unwrap_or_default());
        }
        video_bps = video_bps * target as f64 / size as f64 * 0.96; // rate control overshot: tighten and redo pass 2
        if video_bps < 20_000.0 {
            break;
        }
    }
    bail!("Couldn't get this video under {} - the smallest result was {}. Try a larger limit.", fmt_size(target), fmt_size(size))
}

pub fn expand_video(src: &Path, out_dir: &Path, ctl: &Ctl) -> Result<Outcome> {
    let info = probe(src)?;
    let tmp = TempDir::new()?;
    let out = tmp.join("out.mkv");
    let args: Vec<String> = ["-i"].map(s).into_iter().chain([path_arg(src)]).chain(
        ["-map", "0:v:0", "-map", "0:a:0?", "-c:v", "ffv1", "-level", "3", "-c:a", "pcm_s16le"].map(s),
    ).chain([path_arg(&out)]).collect();
    run_ffmpeg(&args, tmp.path(), info.duration, ctl, 0.0, 1.0)?;
    deliver_path(
        src,
        &out,
        out_dir,
        "expanded",
        ".mkv",
        "Saved losslessly (FFV1 in MKV). It is much larger and the quality of the original is unchanged, not improved.".into(),
        false,
    )
}
