//! Microsoft Office automation over COM (late-bound IDispatch), written directly against the Win32 API.
//! Word / Excel / PowerPoint do the real export, so layout, fonts and formatting match the original exactly.
//! LibreOffice is used as a fallback when Office isn't installed.
use crate::core::*;
use anyhow::{anyhow, bail, Context, Result};
use std::{
    collections::{HashMap, HashSet},
    fs,
    mem::ManuallyDrop,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use windows::{
    core::{GUID, HSTRING, PCWSTR},
    Win32::{
        Foundation::{CloseHandle, DISP_E_EXCEPTION, WAIT_OBJECT_0},
        System::{
            Com::{
                CLSIDFromProgID, CoCreateInstance, CoInitializeEx, CoUninitialize, IDispatch, CLSCTX_LOCAL_SERVER,
                COINIT_APARTMENTTHREADED, DISPATCH_FLAGS, DISPATCH_METHOD, DISPATCH_PROPERTYGET, DISPATCH_PROPERTYPUT,
                DISPPARAMS, EXCEPINFO,
            },
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
            },
            Threading::{OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE},
            Variant::{
                VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_BOOL, VT_BSTR, VT_DISPATCH, VT_ERROR, VT_I2, VT_I4, VT_R4,
                VT_R8,
            },
        },
    },
};

const LOCALE_USER_DEFAULT: u32 = 0x0400;
const DISPID_PROPERTYPUT: i32 = -3;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// COM must be initialised once per thread that talks to Office.
pub struct ComGuard;
impl ComGuard {
    pub fn init() -> Self {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        ComGuard
    }
}
impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() }
    }
}

// ------------------------------------------------------------------------------------------- variants

#[derive(Clone)]
pub enum V<'a> {
    Missing,
    Bool(bool),
    Int(i32),
    Str(&'a str),
}

fn to_variant(v: &V) -> VARIANT {
    match v {
        V::Missing => VARIANT {
            Anonymous: VARIANT_0 {
                Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                    vt: VT_ERROR,
                    wReserved1: 0,
                    wReserved2: 0,
                    wReserved3: 0,
                    Anonymous: VARIANT_0_0_0 { scode: 0x8002_0004u32 as i32 }, // DISP_E_PARAMNOTFOUND
                }),
            },
        },
        V::Bool(b) => VARIANT::from(*b),
        V::Int(i) => VARIANT::from(*i),
        V::Str(s) => VARIANT::from(*s),
    }
}

#[allow(dead_code)] // Bool/Str are read back from Office only for diagnostics
pub enum Val {
    Empty,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Obj(Disp),
}

#[allow(dead_code)]
impl Val {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Val::Float(f) => Some(*f),
            Val::Int(i) => Some(*i as f64),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Val::Int(i) => Some(*i),
            Val::Float(f) => Some(*f as i64),
            Val::Bool(b) => Some(*b as i64),
            _ => None,
        }
    }
}

fn from_variant(v: &VARIANT) -> Val {
    unsafe {
        let inner = &v.Anonymous.Anonymous;
        let vt = inner.vt;
        let data = &inner.Anonymous;
        if vt == VT_BSTR {
            Val::Str(data.bstrVal.to_string())
        } else if vt == VT_I4 {
            Val::Int(data.lVal as i64)
        } else if vt == VT_I2 {
            Val::Int(data.iVal as i64)
        } else if vt == VT_BOOL {
            Val::Bool(data.boolVal.as_bool())
        } else if vt == VT_R8 {
            Val::Float(data.dblVal)
        } else if vt == VT_R4 {
            Val::Float(data.fltVal as f64)
        } else if vt == VT_DISPATCH {
            match &*data.pdispVal {
                Some(d) => Val::Obj(Disp(d.clone())),
                None => Val::Empty,
            }
        } else {
            Val::Empty
        }
    }
}

// ----------------------------------------------------------------------------------------------- dispatch

#[derive(Clone)]
pub struct Disp(IDispatch);

impl Disp {
    /// Always starts a fresh instance (like Python's DispatchEx), never attaching to one the user has open.
    pub fn create(progid: &str) -> Result<Disp> {
        unsafe {
            let clsid = CLSIDFromProgID(&HSTRING::from(progid)).map_err(|e| anyhow!("{progid} is not installed ({e})"))?;
            let d: IDispatch = CoCreateInstance(&clsid, None, CLSCTX_LOCAL_SERVER).map_err(|e| anyhow!("{e}"))?;
            Ok(Disp(d))
        }
    }

    fn dispid(&self, name: &str) -> Result<i32> {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let names = [PCWSTR(wide.as_ptr())];
        let mut id = 0i32;
        unsafe {
            self.0
                .GetIDsOfNames(&GUID::zeroed(), names.as_ptr(), 1, LOCALE_USER_DEFAULT, &mut id)
                .map_err(|e| anyhow!("Office has no member '{name}' ({e})"))?;
        }
        Ok(id)
    }

    fn invoke(&self, name: &str, flags: DISPATCH_FLAGS, args: &[V]) -> Result<Val> {
        let id = self.dispid(name)?;
        let mut rev: Vec<VARIANT> = args.iter().rev().map(to_variant).collect();
        let mut named = DISPID_PROPERTYPUT;
        let put = flags == DISPATCH_PROPERTYPUT;
        let params = DISPPARAMS {
            rgvarg: if rev.is_empty() { std::ptr::null_mut() } else { rev.as_mut_ptr() },
            rgdispidNamedArgs: if put { &mut named } else { std::ptr::null_mut() },
            cArgs: rev.len() as u32,
            cNamedArgs: if put { 1 } else { 0 },
        };
        let mut result = VARIANT::default();
        let mut excep = EXCEPINFO::default();
        let mut argerr = 0u32;
        let r = unsafe {
            self.0.Invoke(id, &GUID::zeroed(), LOCALE_USER_DEFAULT, flags, &params, Some(&mut result), Some(&mut excep), Some(&mut argerr))
        };
        match r {
            Ok(()) => Ok(from_variant(&result)),
            Err(e) => {
                let desc = excep.bstrDescription.to_string();
                if e.code() == DISP_E_EXCEPTION && !desc.is_empty() {
                    bail!("{}", desc.trim());
                }
                bail!("Office call '{name}' failed ({e})")
            }
        }
    }

    pub fn get(&self, name: &str) -> Result<Val> {
        self.invoke(name, DISPATCH_PROPERTYGET, &[])
    }
    pub fn obj(&self, name: &str) -> Result<Disp> {
        match self.get(name)? {
            Val::Obj(d) => Ok(d),
            _ => bail!("Office returned no object for '{name}'"),
        }
    }
    pub fn set(&self, name: &str, v: V) -> Result<()> {
        self.invoke(name, DISPATCH_PROPERTYPUT, &[v]).map(|_| ())
    }
    /// Calls a method, or reads an indexed property such as `Worksheets.Item(1)`. Like VBA, ask for both at once:
    /// COM decides which one the member actually is.
    pub fn call(&self, name: &str, args: &[V]) -> Result<Val> {
        self.invoke(name, DISPATCH_FLAGS(DISPATCH_METHOD.0 | DISPATCH_PROPERTYGET.0), args)
    }
    pub fn call_obj(&self, name: &str, args: &[V]) -> Result<Disp> {
        match self.call(name, args)? {
            Val::Obj(d) => Ok(d),
            _ => bail!("Office returned no object for '{name}'"),
        }
    }
    /// Best-effort property set (some properties don't exist in every Office build).
    fn try_set(&self, name: &str, v: V) {
        let _ = self.set(name, v);
    }
}

// ---------------------------------------------------------------------------------------------- processes

fn pids_of(image: &str) -> HashSet<u32> {
    let mut out = HashSet::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            if String::from_utf16_lossy(&e.szExeFile[..len]).eq_ignore_ascii_case(image) {
                out.insert(e.th32ProcessID);
            }
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// Give processes `ms` milliseconds to exit on their own, then terminate any that are still running.
fn wait_or_kill(pids: &HashSet<u32>, ms: u32) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(ms as u64);
    for &pid in pids {
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, false, pid) else { continue };
            let left = deadline.saturating_duration_since(std::time::Instant::now()).as_millis() as u32;
            if WaitForSingleObject(h, left) != WAIT_OBJECT_0 {
                let _ = TerminateProcess(h, 1);
            }
            let _ = CloseHandle(h);
        }
    }
}

// ------------------------------------------------------------------------------------------------- session

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum App {
    Word,
    Excel,
    Ppt,
}

impl App {
    fn progid(self) -> &'static str {
        match self {
            App::Word => "Word.Application",
            App::Excel => "Excel.Application",
            App::Ppt => "PowerPoint.Application",
        }
    }
    fn image(self) -> &'static str {
        match self {
            App::Word => "WINWORD.EXE",
            App::Excel => "EXCEL.EXE",
            App::Ppt => "POWERPNT.EXE",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            App::Word => "Word",
            App::Excel => "Excel",
            App::Ppt => "PowerPoint",
        }
    }
}

pub fn installed(app: App) -> bool {
    let _com = ComGuard::init();
    unsafe { CLSIDFromProgID(&HSTRING::from(app.progid())).is_ok() }
}

pub fn libreoffice_path() -> Option<PathBuf> {
    let mut cands: Vec<PathBuf> = vec![];
    for var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Ok(base) = std::env::var(var) {
            cands.push(PathBuf::from(base).join("LibreOffice").join("program").join("soffice.exe"));
        }
    }
    cands.into_iter().find(|p| p.is_file())
}

/// One hidden Office instance per application, reused for a whole batch and always cleaned up afterwards.
#[derive(Default)]
pub struct Session {
    apps: HashMap<App, Disp>,
    pids: HashMap<App, HashSet<u32>>,
}

impl Session {
    pub fn get(&mut self, app: App) -> Result<Disp> {
        if let Some(d) = self.apps.get(&app) {
            return Ok(d.clone());
        }
        let before = pids_of(app.image());
        let d = Disp::create(app.progid()).map_err(|e| anyhow!("Could not start Microsoft {}: {e}", app.name()))?;
        let mine: HashSet<u32> = pids_of(app.image()).difference(&before).copied().collect();
        d.try_set("AutomationSecurity", V::Int(3)); // never run macros in opened files
        match app {
            App::Word => {
                d.try_set("Visible", V::Bool(false));
                d.try_set("DisplayAlerts", V::Int(0));
            }
            App::Excel => {
                d.try_set("Visible", V::Bool(false));
                d.try_set("DisplayAlerts", V::Bool(false));
                d.try_set("EnableEvents", V::Bool(false));
                d.try_set("AskToUpdateLinks", V::Bool(false));
            }
            App::Ppt => {
                d.try_set("DisplayAlerts", V::Int(1));
                d.try_set("WindowState", V::Int(2));
            }
        }
        self.apps.insert(app, d.clone());
        self.pids.insert(app, mine);
        Ok(d)
    }

    /// A crashed instance can't be reused for the next file: drop it so it restarts cleanly.
    pub fn reset(&mut self, app: App) {
        if let Some(d) = self.apps.remove(&app) {
            let _ = if app == App::Word { d.call("Quit", &[V::Int(0)]) } else { d.call("Quit", &[]) };
        }
        if let Some(p) = self.pids.remove(&app) {
            wait_or_kill(&p, 1500);
        }
    }

    pub fn close(&mut self) {
        let apps: Vec<(App, Disp)> = self.apps.drain().collect();
        for (app, d) in apps {
            let _ = if app == App::Word { d.call("Quit", &[V::Int(0)]) } else { d.call("Quit", &[]) };
        }
        // Office on some builds returns from Quit() but lingers: end only the instances DocShift itself started.
        for (_, p) in self.pids.drain() {
            wait_or_kill(&p, 2000);
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}

pub fn friendly_office(e: anyhow::Error, app: App) -> anyhow::Error {
    let m = e.to_string();
    let l = m.to_lowercase();
    if l.contains("password") {
        anyhow!("This file is password-protected. Remove the password and try again.")
    } else if l.contains("corrupt") || l.contains("unreadable") || l.contains("cannot open") || l.contains("can't open") || l.contains("not a valid") {
        anyhow!("The file appears to be corrupt or is not a valid document.")
    } else if l.contains("server execution failed") || l.contains("rpc server") || l.contains("disconnected") {
        anyhow!("Microsoft {} stopped responding. Close any open dialog in it and try again.", app.name())
    } else {
        anyhow!("Microsoft {} reported an error: {}", app.name(), m)
    }
}

// ----------------------------------------------------------------------------------------- conversions

fn path_str(p: &Path) -> String {
    let s = p.canonicalize().unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().into_owned();
    s.strip_prefix(r"\\?\").map(|x| x.to_string()).unwrap_or(s)
}

const OLE_MAGIC: [u8; 4] = [0xD0, 0xCF, 0x11, 0xE0];

/// Instant, clear errors for files Office would take many seconds to reject.
pub fn precheck(src: &Path) -> Result<()> {
    let meta = fs::metadata(src).with_context(|| format!("Can't read {}", src.display()))?;
    if meta.len() == 0 {
        bail!("This file is empty.");
    }
    let ext = crate::kinds::ext_of(src);
    if ["docx", "docm", "dotx", "xlsx", "xlsm", "pptx", "pptm", "ppsx"].contains(&ext.as_str()) {
        let mut head = [0u8; 4];
        use std::io::Read;
        fs::File::open(src)?.read_exact(&mut head).ok();
        if head == OLE_MAGIC {
            bail!("This file is password-protected (or its extension doesn't match its contents). Remove the password and try again.");
        }
    }
    Ok(())
}

fn word_open(app: &Disp, src: &Path) -> Result<Disp> {
    let docs = app.obj("Documents")?;
    let m = V::Missing;
    // The dummy passwords make protected files fail with an error instead of an invisible prompt.
    docs.call_obj(
        "Open",
        &[
            V::Str(&path_str(src)), V::Bool(false), V::Bool(true), V::Bool(false), V::Str("x"), V::Str("x"), V::Bool(false),
            V::Str("x"), V::Str("x"), m.clone(), m.clone(), V::Bool(false), V::Bool(false), m, V::Bool(true),
        ],
    )
}

fn word_to_pdf(app: &Disp, src: &Path, dst: &Path, opts: &Opts) -> Result<()> {
    let doc = word_open(app, src)?;
    let _ = doc.call("Repaginate", &[]);
    let optimize = if screen_quality(opts) { 1 } else { 0 };
    let bookmarks = if opts.bool("bookmarks", true) { 1 } else { 0 };
    let pdfa = opts.bool("pdfa", false);
    // ExportAsFixedFormat(Output, Format=pdf, OpenAfter, OptimizeFor, Range, From, To, Item, DocProps, KeepIRM,
    //                     Bookmarks, StructureTags, BitmapMissingFonts, ISO19005)
    let r = doc.call(
        "ExportAsFixedFormat",
        &[
            V::Str(&path_str(dst)), V::Int(17), V::Bool(false), V::Int(optimize), V::Int(0), V::Int(1), V::Int(1), V::Int(0),
            V::Bool(true), V::Bool(true), V::Int(bookmarks), V::Bool(true), V::Bool(true), V::Bool(pdfa),
        ],
    );
    let _ = doc.call("Close", &[V::Int(0)]);
    r.map(|_| ())
}

fn excel_fit_width(app: &Disp, wb: &Disp) {
    app.try_set("PrintCommunication", V::Bool(false)); // much faster PageSetup changes
    let run = || -> Result<()> {
        let sheets = wb.obj("Worksheets")?;
        let n = sheets.get("Count")?.as_i64().unwrap_or(0) as i32;
        for i in 1..=n {
            let ws = match sheets.call_obj("Item", &[V::Int(i)]) {
                Ok(ws) => ws,
                Err(e) => {
                    eprintln!("excel fit-width: can't open sheet {i}: {e:#}");
                    continue;
                }
            };
            let result = (|| -> Result<()> {
                if ws.get("Visible")?.as_i64() != Some(-1) {
                    return Ok(());
                }
                let ps = ws.obj("PageSetup")?;
                if let Val::Bool(false) = ps.get("Zoom")? {
                    return Ok(()); // the author already chose "fit to N pages": respect it
                }
                let used = ws.obj("UsedRange")?;
                let wide = used.get("Width")?.as_f64().unwrap_or(0.0) > used.get("Height")?.as_f64().unwrap_or(0.0);
                ps.set("Zoom", V::Bool(false))?;
                ps.set("FitToPagesWide", V::Int(1))?;
                ps.set("FitToPagesTall", V::Bool(false))?;
                if wide && ps.get("Orientation")?.as_i64() == Some(1) {
                    ps.set("Orientation", V::Int(2))?; // landscape for wide sheets
                }
                Ok(())
            })();
            if let Err(e) = &result {
                eprintln!("excel fit-width: sheet {i}: {e:#}");
            }
        }
        Ok(())
    };
    if let Err(e) = run() {
        eprintln!("excel fit-width: {e:#}");
    }
    app.try_set("PrintCommunication", V::Bool(true));
}

fn excel_to_pdf(app: &Disp, src: &Path, dst: &Path, opts: &Opts) -> Result<()> {
    let books = app.obj("Workbooks")?;
    let m = V::Missing;
    let wb = books.call_obj(
        "Open",
        &[
            V::Str(&path_str(src)), V::Int(0), V::Bool(true), m.clone(), V::Str("x"), V::Str("x"), V::Bool(true), m.clone(),
            m.clone(), V::Bool(false), V::Bool(false), m, V::Bool(false),
        ],
    )?;
    if opts.bool("fit_width", true) {
        excel_fit_width(app, &wb);
    }
    let quality = if screen_quality(opts) { 1 } else { 0 };
    let m = V::Missing;
    let r = wb.call(
        "ExportAsFixedFormat",
        &[V::Int(0), V::Str(&path_str(dst)), V::Int(quality), V::Bool(true), V::Bool(false), m.clone(), m, V::Bool(false)],
    );
    let _ = wb.call("Close", &[V::Bool(false)]);
    r.map(|_| ())
}

fn ppt_to_pdf(app: &Disp, src: &Path, dst: &Path, opts: &Opts) -> Result<()> {
    let pres_list = app.obj("Presentations")?;
    let pres = pres_list.call_obj("Open", &[V::Str(&path_str(src)), V::Int(-1), V::Int(0), V::Int(0)])?;
    let intent = if screen_quality(opts) { 1 } else { 2 };
    let pdfa = opts.bool("pdfa", false);
    let out = path_str(dst);
    let r = pres
        .call(
            "ExportAsFixedFormat",
            &[
                V::Str(&out), V::Int(2), V::Int(intent), V::Int(0), V::Int(1), V::Int(1), V::Int(0), V::Missing, V::Int(1),
                V::Str(""), V::Bool(true), V::Bool(true), V::Bool(true), V::Bool(true), V::Bool(pdfa),
            ],
        )
        .map(|_| ())
        .or_else(|_| {
            let _ = fs::remove_file(dst);
            pres.call("SaveAs", &[V::Str(&out), V::Int(32)]).map(|_| ()) // ppSaveAsPDF
        });
    let _ = pres.call("Close", &[]);
    r
}

fn libreoffice_to_pdf(soffice: &Path, src: &Path, tmp: &Path) -> Result<PathBuf> {
    let profile = tmp.join("lo_profile");
    let uri = format!("-env:UserInstallation=file:///{}", profile.to_string_lossy().replace('\\', "/"));
    let status = Command::new(soffice)
        .args([&uri, "--headless", "--norestore", "--convert-to", "pdf", "--outdir"])
        .arg(tmp)
        .arg(path_str(src))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .context("Couldn't start LibreOffice")?;
    let out = tmp.join(format!("{}.pdf", stem_of(src)));
    if !status.success() || !out.exists() {
        bail!("LibreOffice did not produce a PDF for this file.");
    }
    Ok(out)
}

/// "small" asks Office for its minimum-size export; "standard" and "print" use the print-quality export.
fn screen_quality(opts: &Opts) -> bool {
    matches!(opts.str("quality", "standard").as_str(), "small" | "screen")
}

/// Word / Excel / PowerPoint document -> PDF.
pub fn office_to_pdf(app: App, session: &mut Session, src: &Path, out_dir: &Path, opts: &Opts) -> Result<Outcome> {
    let tmp = TempDir::new()?;
    let (produced, note) = office_pdf_file(app, session, src, &tmp, opts)?;
    let before = fs::metadata(src)?.len();
    let after = fs::metadata(&produced)?.len();
    let out = publish(&produced, out_dir, &stem_of(src), ".pdf")?;
    Ok(Outcome::one(&out).with_note(note).with_detail(size_detail(before, after)))
}

/// Exports an Office document to a PDF inside `tmp`, applying the quality and PDF/A options. Returns the file and
/// a note for the user (empty if nothing to say).
pub fn office_pdf_file(app: App, session: &mut Session, src: &Path, tmp: &TempDir, opts: &Opts) -> Result<(PathBuf, String)> {
    precheck(src)?;
    let target = tmp.join(&format!("{}.pdf", tmp_name(src)));
    let mut note = String::new();
    let produced = if installed(app) {
        let disp = session.get(app)?;
        let r = match app {
            App::Word => word_to_pdf(&disp, src, &target, opts),
            App::Excel => excel_to_pdf(&disp, src, &target, opts),
            App::Ppt => ppt_to_pdf(&disp, src, &target, opts),
        };
        if let Err(e) = r {
            session.reset(app); // the instance may be wedged
            return Err(friendly_office(e, app));
        }
        target
    } else if let Some(lo) = libreoffice_path() {
        note = "Converted with LibreOffice (Microsoft Office not found), so layout may differ slightly.".into();
        libreoffice_to_pdf(&lo, src, tmp.path())?
    } else {
        bail!("Microsoft {} (or LibreOffice) is required for this conversion but neither was found.", app.name());
    };
    if fs::metadata(&produced).map(|m| m.len()).unwrap_or(0) == 0 {
        if app == App::Excel {
            bail!("Nothing to export - the workbook appears to be empty or has nothing to print.");
        }
        bail!("The PDF was not created.");
    }
    match opts.str("quality", "standard").as_str() {
        // keep text and vector art as they are; only images beyond what the setting needs are re-encoded
        "small" | "screen" => crate::pdfops::shrink_in_place(&produced, 150, 70)?,
        "standard" => crate::pdfops::shrink_in_place(&produced, 220, 85)?,
        _ => {}
    }
    // Word and PowerPoint write PDF/A themselves; Excel (and LibreOffice here) can't, so DocShift adds it
    if opts.bool("pdfa", false) && (app == App::Excel || !installed(app)) {
        crate::pdfa::apply_file(&produced, &stem_of(src))?;
    }
    Ok((produced, note))
}

/// A short ASCII name for temp files (Office is fussy about unusual characters in paths).
fn tmp_name(src: &Path) -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let stem: String = stem_of(src).chars().filter(|c| c.is_ascii_alphanumeric()).take(24).collect();
    format!("{}_{}", if stem.is_empty() { "doc" } else { &stem }, N.fetch_add(1, Ordering::Relaxed))
}

/// PDF -> editable Word document, using Word's own PDF reflow.
pub fn pdf_to_word(session: &mut Session, src: &Path, out_dir: &Path, ocr_used: bool) -> Result<Outcome> {
    if !installed(App::Word) {
        bail!("Microsoft Word is required to convert a PDF to Word, and it wasn't found on this computer.");
    }
    let scanned = crate::pdf2x::has_no_text(src).unwrap_or(false);
    let tmp = TempDir::new()?;
    let copy = tmp.join("in.pdf"); // Word's PDF importer is fussy with unusual paths
    fs::copy(src, &copy)?;
    let target = tmp.join("out.docx");
    let word = session.get(App::Word)?;
    let r = (|| -> Result<()> {
        let doc = word_open(&word, &copy)?;
        let r = doc.call("SaveAs2", &[V::Str(&path_str(&target)), V::Int(16)]).map(|_| ());
        let _ = doc.call("Close", &[V::Int(0)]);
        r
    })();
    if let Err(e) = r {
        session.reset(App::Word);
        return Err(friendly_office(e, App::Word));
    }
    if !target.exists() {
        bail!("Word could not convert this PDF.");
    }
    let note = if scanned && !ocr_used {
        "This PDF looks scanned, so the Word file contains page images. Turn on \u{201c}Read scanned pages\u{201d} to get editable text."
    } else {
        ""
    };
    let before = fs::metadata(src)?.len();
    let after = fs::metadata(&target)?.len();
    let out = publish(&target, out_dir, &stem_of(src), ".docx")?;
    Ok(Outcome::one(&out).with_note(note).with_detail(size_detail(before, after)))
}
