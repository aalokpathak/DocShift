//! Shared plumbing: errors, cancellation, progress, temp files, output naming, size parsing.
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::{
    fmt, fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};

/// 1 MB = 1,000,000 bytes: the stricter convention, so a result that fits is accepted by sites using either.
pub const MB: u64 = 1_000_000;

#[derive(Debug)]
pub struct Cancelled;
impl fmt::Display for Cancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Cancelled")
    }
}
impl std::error::Error for Cancelled {}

pub fn is_cancelled(e: &anyhow::Error) -> bool {
    e.downcast_ref::<Cancelled>().is_some()
}

/// Message shown to the user. Our own `bail!` texts are already friendly; foreign errors get their top-level text.
pub fn friendly(e: &anyhow::Error) -> String {
    if is_cancelled(e) {
        return "Cancelled".into();
    }
    if let Some(s) = e.downcast_ref::<Skipped>() {
        return s.to_string();
    }
    let msg = e.to_string();
    if let Some(io) = e.downcast_ref::<std::io::Error>() {
        return match io.kind() {
            std::io::ErrorKind::PermissionDenied => {
                "Permission denied - the file or destination folder is locked or read-only.".into()
            }
            std::io::ErrorKind::NotFound => "The file could not be found. It may have been moved or deleted.".into(),
            _ => msg,
        };
    }
    msg
}

/// Progress reporting + cooperative cancellation, shared by every engine.
#[derive(Clone)]
pub struct Ctl {
    cancelled: Arc<AtomicBool>,
    progress: Arc<dyn Fn(f64) + Send + Sync>,
}

impl Ctl {
    pub fn new(cancelled: Arc<AtomicBool>, progress: impl Fn(f64) + Send + Sync + 'static) -> Self {
        Self { cancelled, progress: Arc::new(progress) }
    }
    pub fn none() -> Self {
        Self::new(Arc::new(AtomicBool::new(false)), |_| {})
    }
    pub fn report(&self, fraction: f64) {
        (self.progress)(fraction.clamp(0.0, 1.0));
    }
    pub fn check(&self) -> Result<()> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }
        Ok(())
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
    /// The same control, with progress mapped onto the `lo..hi` slice (for multi-step tasks).
    pub fn slice(&self, lo: f64, hi: f64) -> Ctl {
        let parent = self.progress.clone();
        Ctl { cancelled: self.cancelled.clone(), progress: Arc::new(move |f| parent(lo + (hi - lo) * f)) }
    }
}

/// What one finished task produced.
#[derive(Default, Clone, Serialize, Debug)]
pub struct Outcome {
    pub outputs: Vec<String>,
    pub note: String,
    /// e.g. "12.40 MB → 1.98 MB (-84%)", shown on the file row.
    pub detail: String,
}

impl Outcome {
    pub fn one(path: &Path) -> Self {
        Self { outputs: vec![path.to_string_lossy().into_owned()], ..Default::default() }
    }
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = note.into();
        self
    }
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }
}

pub fn fmt_size(n: u64) -> String {
    let mut v = n as f64;
    for unit in ["B", "KB", "MB", "GB"] {
        if v < 1000.0 || unit == "GB" {
            return match unit {
                "B" => format!("{v:.0} B"),
                "MB" => format!("{v:.2} MB"),
                _ => format!("{v:.1} {unit}"),
            };
        }
        v /= 1000.0;
    }
    format!("{n} B")
}

pub fn size_detail(before: u64, after: u64) -> String {
    let pct = if before > 0 { format!(" ({:+.0}%)", (after as f64 / before as f64 - 1.0) * 100.0) } else { String::new() };
    format!("{} \u{2192} {}{}", fmt_size(before), fmt_size(after), pct)
}

/// `dir/stem+suffix`, or `stem (1)+suffix`, `stem (2)+suffix` ... if that exists. Never overwrites.
pub fn unique_path(dir: &Path, stem: &str, suffix: &str) -> PathBuf {
    let mut p = dir.join(format!("{stem}{suffix}"));
    let mut n = 1;
    while p.exists() {
        p = dir.join(format!("{stem} ({n}){suffix}"));
        n += 1;
    }
    p
}

/// What to do when an output file name is already taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clash {
    KeepBoth,
    Replace,
    Skip,
}

/// The user chose not to overwrite an existing file.
#[derive(Debug)]
pub struct Skipped(pub String);
impl fmt::Display for Skipped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Skipped: {} already exists.", self.0)
    }
}
impl std::error::Error for Skipped {}

pub type ClashResolver = Arc<dyn Fn(&Path) -> Clash + Send + Sync>;

thread_local! {
    /// Set by the job runner for the task running on this thread. None = keep both (never overwrite).
    static CLASH: std::cell::RefCell<Option<ClashResolver>> = const { std::cell::RefCell::new(None) };
}

/// Runs `f` with a name-clash policy in effect for every file it publishes on this thread.
pub fn with_clash_policy<R>(policy: Option<ClashResolver>, f: impl FnOnce() -> R) -> R {
    let prev = CLASH.with(|c| c.replace(policy));
    let r = f();
    CLASH.with(|c| *c.borrow_mut() = prev);
    r
}

/// Where a new output file goes: the plain name if free, otherwise per the clash policy.
pub fn target_path(dir: &Path, stem: &str, suffix: &str) -> Result<PathBuf> {
    let plain = dir.join(format!("{stem}{suffix}"));
    if !plain.exists() || plain.is_dir() {
        return Ok(if plain.exists() { unique_path(dir, stem, suffix) } else { plain });
    }
    let resolver = CLASH.with(|c| c.borrow().clone());
    match resolver.map(|r| r(&plain)).unwrap_or(Clash::KeepBoth) {
        Clash::KeepBoth => Ok(unique_path(dir, stem, suffix)),
        Clash::Replace => Ok(plain),
        Clash::Skip => Err(Skipped(format!("{stem}{suffix}")).into()),
    }
}

/// Moves a finished temp file into the output folder (name per the clash policy).
pub fn publish(tmp_file: &Path, out_dir: &Path, stem: &str, suffix: &str) -> Result<PathBuf> {
    fs::create_dir_all(out_dir).with_context(|| format!("Can't create the folder {}", out_dir.display()))?;
    let dst = target_path(out_dir, stem, suffix)?;
    place(tmp_file, &dst)?;
    Ok(dst)
}

/// Moves `from` to `to`, replacing `to` if it exists (copying across drives).
pub fn place(from: &Path, to: &Path) -> Result<()> {
    if fs::rename(from, to).is_err() {
        // different drive, or the old file is in use
        fs::copy(from, to).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                anyhow::anyhow!("{} is open in another program, so it can't be replaced. Close it and try again.", to.file_name().unwrap_or_default().to_string_lossy())
            } else {
                e.into()
            }
        })?;
        let _ = fs::remove_file(from);
    }
    Ok(())
}

/// Removes characters Windows doesn't allow in file names.
pub fn safe_stem(s: &str) -> String {
    let cleaned: String = s.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { '_' } else { c }).collect();
    let t = cleaned.trim().trim_end_matches('.').to_string();
    if t.is_empty() { "output".into() } else { t }
}

pub fn stem_of(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "output".into())
}

/// Scratch directory that is removed when dropped.
pub struct TempDir(PathBuf);
static COUNTER: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    pub fn new() -> Result<Self> {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let p = std::env::temp_dir().join(format!("docshift_{}_{}_{}", std::process::id(), n, t % 1_000_000));
        fs::create_dir_all(&p)?;
        Ok(Self(p))
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// "2", "2.5" or "2,5" -> bytes. Blank -> no limit.
pub fn parse_target(text: &str) -> Result<Option<u64>> {
    let t = text.trim().replace(',', ".");
    if t.is_empty() {
        return Ok(None);
    }
    let Ok(v) = t.parse::<f64>() else { bail!("The size limit must be a number of MB, for example 2 or 0.5.") };
    if v <= 0.0 {
        bail!("The size limit must be greater than zero.");
    }
    if v < 0.005 {
        bail!("That size limit is too small to be useful.");
    }
    Ok(Some((v * MB as f64) as u64))
}

/// "1-3, 5" -> [0, 1, 2, 4] (zero based, sorted, unique). Blank or "all" -> every page.
pub fn parse_pages(spec: &str, count: usize) -> Result<Vec<usize>> {
    let s = spec.trim().to_lowercase();
    if s.is_empty() || s == "all" || s == "*" {
        return Ok((0..count).collect());
    }
    let mut out = std::collections::BTreeSet::new();
    for part in s.split(|c: char| c == ',' || c == ';' || c.is_whitespace()).filter(|p| !p.is_empty()) {
        let bad = || anyhow::anyhow!("'{part}' is not a valid page range. Use something like 1-3, 5.");
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim().parse::<usize>().map_err(|_| bad())?, b.trim().parse::<usize>().map_err(|_| bad())?),
            None => {
                let n = part.parse::<usize>().map_err(|_| bad())?;
                (n, n)
            }
        };
        let (a, b) = if a > b { (b, a) } else { (a, b) };
        if a < 1 || b > count {
            bail!("Page range {part} is outside this PDF, which has {count} page(s).");
        }
        out.extend(a - 1..b);
    }
    Ok(out.into_iter().collect())
}

/// Shorthand to read values out of the JSON options a job carries.
pub struct Opts<'a>(pub &'a serde_json::Value);

impl Opts<'_> {
    pub fn str(&self, key: &str, default: &str) -> String {
        self.0.get(key).and_then(|v| v.as_str()).unwrap_or(default).to_string()
    }
    pub fn u32(&self, key: &str, default: u32) -> u32 {
        self.0
            .get(key)
            .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .map(|n| n as u32)
            .unwrap_or(default)
    }
    pub fn bool(&self, key: &str, default: bool) -> bool {
        self.0.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
    }
}
