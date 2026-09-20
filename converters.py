"""DocShift conversion engines. No GUI code lives here.

Office documents -> PDF is done by driving the real Microsoft Office apps through COM
(the same engine as File > Save As > PDF), which is what preserves layout, fonts and
formatting exactly. LibreOffice is used as a fallback when Office is not installed.
Everything that starts from a PDF (or an image) is done with PyMuPDF / Pillow.
"""
from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

try:  # Windows-only; the app degrades gracefully without it
    import pythoncom
    import pywintypes
    import win32com.client
    import win32com.client.dynamic
    HAVE_COM = True
except Exception:  # pragma: no cover
    HAVE_COM = False

# --------------------------------------------------------------------------- #
# File kinds and conversion modes
# --------------------------------------------------------------------------- #
KIND_EXTS = {
    "word": (".doc", ".docx", ".docm", ".dot", ".dotx", ".rtf", ".odt", ".txt"),
    "excel": (".xls", ".xlsx", ".xlsm", ".xlsb", ".csv", ".ods"),
    "ppt": (".ppt", ".pptx", ".pptm", ".pps", ".ppsx", ".odp"),
    "img": (".jpg", ".jpeg", ".jfif", ".png", ".webp", ".bmp", ".gif", ".tif", ".tiff", ".ico"),
    "pdf": (".pdf",),
}


def kind_of(path) -> str | None:
    ext = Path(path).suffix.lower()
    for kind, exts in KIND_EXTS.items():
        if ext in exts:
            return kind
    return None


@dataclass(frozen=True)
class Mode:
    key: str
    title: str          # "Word to PDF"
    src: str            # kind of input files
    dst: str            # kind of output ("pdf", "word", ...)
    sub: str            # short hint under the title
    blurb: str          # longer description shown in the header


MODES = [
    Mode("word2pdf", "Word to PDF", "word", "pdf", "DOC, DOCX, RTF, TXT",
         "Exported by Microsoft Word itself, so fonts, layout, tables, headers and images match the original."),
    Mode("excel2pdf", "Excel to PDF", "excel", "pdf", "XLS, XLSX, CSV",
         "Exported by Microsoft Excel, with formatting, charts and borders intact. Wide sheets can be fitted to the page width."),
    Mode("ppt2pdf", "PowerPoint to PDF", "ppt", "pdf", "PPT, PPTX, ODP",
         "Exported by Microsoft PowerPoint, so every slide, font, shape and image matches."),
    Mode("img2pdf", "Images to PDF", "img", "pdf", "JPG, PNG, WEBP & more",
         "Turn one or many images into a PDF - one image per page - optionally merged into a single file."),
    Mode("pdf2word", "PDF to Word", "pdf", "word", "Editable .docx",
         "Converts the PDF into an editable Word document using Word's own PDF reflow engine."),
    Mode("pdf2excel", "PDF to Excel", "pdf", "excel", "Tables to sheets",
         "Detects tables in the PDF and places them in an Excel workbook, keeping other text alongside."),
    Mode("pdf2ppt", "PDF to PowerPoint", "pdf", "ppt", "One slide per page",
         "Each PDF page becomes a slide - either pixel-exact, or with editable text on a clean background."),
    Mode("pdf2img", "PDF to Images", "pdf", "img", "PNG, JPG, WEBP, TIFF",
         "Renders pages to high-resolution images. Choose the format, resolution and which pages."),
]
MODE_BY_KEY = {m.key: m for m in MODES}


class ConversionError(Exception):
    """An error whose message is safe and helpful to show to the user."""


@dataclass
class Result:
    outputs: list = field(default_factory=list)
    note: str = ""

    @property
    def reveal(self) -> Path | None:
        return self.outputs[0] if self.outputs else None


# --------------------------------------------------------------------------- #
# Small helpers
# --------------------------------------------------------------------------- #
def unique_path(directory: Path, stem: str, suffix: str) -> Path:
    """directory/stem+suffix, or 'stem (1)+suffix' etc. if it already exists."""
    p = directory / f"{stem}{suffix}"
    n = 1
    while p.exists():
        p = directory / f"{stem} ({n}){suffix}"
        n += 1
    return p


def _publish(tmp_file: Path, out_dir: Path, stem: str, suffix: str) -> Path:
    out_dir.mkdir(parents=True, exist_ok=True)
    dst = unique_path(out_dir, stem, suffix)
    shutil.move(str(tmp_file), str(dst))
    return dst


def _abs(p) -> str:
    return str(Path(p).resolve())


def parse_pages(spec: str, count: int) -> list[int]:
    """'1-3, 5' -> [0, 1, 2, 4] (zero based). Blank / 'all' -> every page."""
    spec = (spec or "").strip().lower()
    if spec in ("", "all", "*"):
        return list(range(count))
    pages: list[int] = []
    for part in re.split(r"[,\s;]+", spec):
        if not part:
            continue
        m = re.fullmatch(r"(\d+)(?:\s*-\s*(\d+))?", part)
        if not m:
            raise ConversionError(f"'{part}' is not a valid page range. Use something like 1-3, 5.")
        a = int(m.group(1))
        b = int(m.group(2) or a)
        if a > b:
            a, b = b, a
        if a < 1 or b > count:
            raise ConversionError(f"Page range {part} is outside this PDF, which has {count} page(s).")
        pages.extend(range(a - 1, b))
    return sorted(set(pages))


# --------------------------------------------------------------------------- #
# COM plumbing
# --------------------------------------------------------------------------- #
_PROGIDS = {"word": "Word.Application", "excel": "Excel.Application", "ppt": "PowerPoint.Application"}
_APP_NAMES = {"word": "Word", "excel": "Excel", "ppt": "PowerPoint"}


def com_init():
    if HAVE_COM:
        pythoncom.CoInitialize()


def com_uninit():
    if HAVE_COM:
        pythoncom.CoUninitialize()


def office_installed(kind: str) -> bool:
    if not HAVE_COM:
        return False
    try:
        pywintypes.IID(_PROGIDS[kind])  # raises com_error "Invalid class string" if the ProgID isn't registered
        return True
    except Exception:
        return False


def libreoffice_path() -> str | None:
    cands = [shutil.which("soffice")]
    for var in ("ProgramFiles", "ProgramFiles(x86)"):
        base = os.environ.get(var)
        if base:
            cands.append(os.path.join(base, "LibreOffice", "program", "soffice.exe"))
    return next((c for c in cands if c and os.path.isfile(c)), None)


def engine_status() -> dict:
    """What can we convert with on this machine? Used for the status line in the UI."""
    have = {k: office_installed(k) for k in _PROGIDS}
    lo = libreoffice_path()
    return {"office": have, "libreoffice": lo}


MISSING = pythoncom.Missing if HAVE_COM else None


class OfficeSession:
    """Starts one hidden Office instance per app on demand and reuses it for a whole batch."""

    def __init__(self):
        self._apps: dict[str, object] = {}

    def get(self, kind: str):
        app = self._apps.get(kind)
        if app is None:
            app = self._apps[kind] = self._launch(kind)
        return app

    @staticmethod
    def _launch(kind: str):
        name = _APP_NAMES[kind]
        if not HAVE_COM or not office_installed(kind):
            raise ConversionError(f"Microsoft {name} is not installed, so this conversion is not available.")
        try:
            raw = win32com.client.DispatchEx(_PROGIDS[kind])
            app = win32com.client.dynamic.Dispatch(raw._oleobj_)  # late-bound: no generated wrappers needed
        except Exception as e:
            raise ConversionError(f"Could not start Microsoft {name}: {friendly_error(e)}")
        try:
            app.AutomationSecurity = 3  # msoAutomationSecurityForceDisable: never run macros in opened files
        except Exception:
            pass
        try:
            if kind == "word":
                app.Visible = False
                app.DisplayAlerts = 0
            elif kind == "excel":
                app.Visible = False
                app.DisplayAlerts = False
                app.EnableEvents = False
                app.AskToUpdateLinks = False
            else:  # PowerPoint refuses Visible = False; documents are opened without a window instead
                app.DisplayAlerts = 1
                try:
                    app.WindowState = 2  # minimised
                except Exception:
                    pass
        except Exception:
            pass
        return app

    def close(self):
        for kind, app in list(self._apps.items()):
            try:
                app.Quit(0) if kind == "word" else app.Quit()
            except Exception:
                pass
        self._apps.clear()


def friendly_error(exc: BaseException) -> str:
    if isinstance(exc, ConversionError):
        return str(exc)
    if HAVE_COM and isinstance(exc, pywintypes.com_error):
        detail = ""
        try:
            info = exc.excepinfo
            if info and len(info) > 2 and info[2]:
                detail = str(info[2]).strip()
        except Exception:
            pass
        detail = detail or (exc.strerror or "").strip()
        low = detail.lower()
        hr = (exc.hresult or 0) & 0xFFFFFFFF
        if "password" in low:
            return "This file is password-protected. Remove the password and try again."
        if hr in (0x800706BA, 0x800706BE, 0x80010108, 0x800706BF):
            return "Microsoft Office was closed while converting. Please try again."
        if hr == 0x80010001 or "busy" in low:
            return "Microsoft Office is busy (a dialog may be open). Close it and try again."
        if "corrupt" in low or "unreadable" in low or "cannot open" in low or "can't open" in low:
            return "The file appears to be corrupt or is not a valid document."
        return f"Microsoft Office reported an error: {detail or hex(hr)}"
    if isinstance(exc, PermissionError):
        return "Permission denied - the file or destination folder is locked or read-only."
    if isinstance(exc, FileNotFoundError):
        return "The file could not be found. It may have been moved or deleted."
    if isinstance(exc, MemoryError):
        return "Not enough memory to convert this file at this resolution. Try a lower DPI."
    return f"{type(exc).__name__}: {exc}"


# --------------------------------------------------------------------------- #
# Office -> PDF
# --------------------------------------------------------------------------- #
def _word_to_pdf(app, src: Path, dst: Path, opts: dict):
    # Open(FileName, ConfirmConversions, ReadOnly, AddToRecentFiles, PasswordDocument, PasswordTemplate,
    #      Revert, WritePasswordDocument, WritePasswordTemplate, Format, Encoding, Visible, OpenAndRepair,
    #      DocumentDirection, NoEncodingDialog)
    # The dummy passwords make protected files raise an error instead of showing an invisible prompt.
    doc = app.Documents.Open(_abs(src), False, True, False, "x", "x", False, "x", "x",
                             MISSING, MISSING, False, False, MISSING, True)
    try:
        try:
            doc.Repaginate()
        except Exception:
            pass
        optimize = 0 if opts.get("quality", "print") == "print" else 1
        bookmarks = 1 if opts.get("bookmarks", True) else 0
        # ExportAsFixedFormat(OutputFileName, ExportFormat, OpenAfterExport, OptimizeFor, Range, From, To, Item,
        #                     IncludeDocProps, KeepIRM, CreateBookmarks, DocStructureTags, BitmapMissingFonts,
        #                     UseISO19005_1)
        doc.ExportAsFixedFormat(_abs(dst), 17, False, optimize, 0, 1, 1, 0, True, True, bookmarks, True, True, False)
    finally:
        try:
            doc.Close(0)
        except Exception:
            pass


def _excel_fit_width(app, wb):
    """Make each sheet one page wide (unless the author already set fit-to-pages) so columns are never split."""
    try:
        app.PrintCommunication = False  # much faster PageSetup changes
    except Exception:
        pass
    try:
        for ws in wb.Worksheets:
            try:
                if ws.Visible != -1:
                    continue
                ps = ws.PageSetup
                if ps.Zoom is False:  # author already chose "fit to N pages"; respect it
                    continue
                used = ws.UsedRange
                wide = used.Width > used.Height
                ps.Zoom = False
                ps.FitToPagesWide = 1
                ps.FitToPagesTall = False
                if wide and ps.Orientation == 1:
                    ps.Orientation = 2  # landscape for wide sheets
            except Exception:
                continue
    finally:
        try:
            app.PrintCommunication = True
        except Exception:
            pass


def _excel_to_pdf(app, src: Path, dst: Path, opts: dict):
    # Open(Filename, UpdateLinks, ReadOnly, Format, Password, WriteResPassword, IgnoreReadOnlyRecommended,
    #      Origin, Delimiter, Editable, Notify, Converter, AddToMru)
    wb = app.Workbooks.Open(_abs(src), 0, True, MISSING, "x", "x", True, MISSING, MISSING, False, False, MISSING, False)
    try:
        if opts.get("fit_width", True):
            _excel_fit_width(app, wb)
        quality = 0 if opts.get("quality", "print") == "print" else 1
        # ExportAsFixedFormat(Type, Filename, Quality, IncludeDocProperties, IgnorePrintAreas, From, To, OpenAfterPublish)
        wb.ExportAsFixedFormat(0, _abs(dst), quality, True, False, MISSING, MISSING, False)
    finally:
        try:
            wb.Close(False)
        except Exception:
            pass


def _ppt_to_pdf(app, src: Path, dst: Path, opts: dict):
    # Open(FileName, ReadOnly, Untitled, WithWindow)   (msoTrue = -1, msoFalse = 0)
    pres = app.Presentations.Open(_abs(src), -1, 0, 0)
    try:
        intent = 2 if opts.get("quality", "print") == "print" else 1  # ppFixedFormatIntentPrint / Screen
        try:
            # ExportAsFixedFormat(Path, FixedFormatType, Intent, FrameSlides, HandoutOrder, OutputType,
            #                     PrintHiddenSlides, PrintRange, RangeType, SlideShowName, IncludeDocProperties,
            #                     KeepIRM, DocStructureTags, BitmapMissingFonts, UseISO19005_1)
            pres.ExportAsFixedFormat(_abs(dst), 2, intent, 0, 1, 1, 0, MISSING, 1, "", True, True, True, True, False)
        except Exception:
            if dst.exists():
                dst.unlink()
            pres.SaveAs(_abs(dst), 32)  # ppSaveAsPDF
    finally:
        try:
            pres.Close()
        except Exception:
            pass


def _libreoffice_to_pdf(soffice: str, src: Path, tmp: Path) -> Path:
    profile = tmp / "lo_profile"
    cmd = [soffice, f"-env:UserInstallation={profile.as_uri()}", "--headless", "--norestore",
           "--convert-to", "pdf", "--outdir", str(tmp), _abs(src)]
    flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    try:
        subprocess.run(cmd, check=True, capture_output=True, timeout=300, creationflags=flags)
    except subprocess.TimeoutExpired:
        raise ConversionError("LibreOffice took too long to convert this file.")
    except subprocess.CalledProcessError as e:
        raise ConversionError(f"LibreOffice failed: {(e.stderr or b'').decode(errors='ignore').strip() or e}")
    out = tmp / (src.stem + ".pdf")
    if not out.exists():
        raise ConversionError("LibreOffice did not produce a PDF for this file.")
    return out


_OFFICE_TO_PDF = {"word2pdf": ("word", _word_to_pdf), "excel2pdf": ("excel", _excel_to_pdf),
                  "ppt2pdf": ("ppt", _ppt_to_pdf)}


def office_to_pdf(mode: str, src: Path, out_dir: Path, opts: dict, office: OfficeSession) -> Result:
    kind, fn = _OFFICE_TO_PDF[mode]
    tmp = Path(tempfile.mkdtemp(prefix="docshift_"))
    try:
        target = tmp / "out.pdf"
        note = ""
        if office_installed(kind):
            fn(office.get(kind), src, target, opts)
        elif libreoffice_path():
            target = _libreoffice_to_pdf(libreoffice_path(), src, tmp)
            note = "Converted with LibreOffice (Microsoft Office not found), so layout may differ slightly."
        else:
            raise ConversionError(f"Microsoft {_APP_NAMES[kind]} (or LibreOffice) is required for this conversion "
                                  f"but neither was found.")
        if not target.exists() or target.stat().st_size == 0:
            if kind == "excel":
                raise ConversionError("Nothing to export - the workbook appears to be empty or has nothing to print.")
            raise ConversionError("The PDF was not created.")
        return Result([_publish(target, out_dir, src.stem, ".pdf")], note)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


# --------------------------------------------------------------------------- #
# Images -> PDF
# --------------------------------------------------------------------------- #
_PAGE_PT = {"a4": (595.276, 841.89), "letter": (612.0, 792.0)}
_MARGIN_PT = {"none": 0, "small": 18, "medium": 36}


def _image_frames(src: Path):
    """Yield (encoded_bytes, width_px, height_px, dpi) for each page an image should produce."""
    from PIL import Image, ImageOps

    with Image.open(src) as im:
        fmt = im.format
        frames = getattr(im, "n_frames", 1) if fmt == "TIFF" else 1
        dpi_info = im.info.get("dpi")
        dpi = None
        if dpi_info:
            try:
                dpi = float(dpi_info[0])
            except Exception:
                dpi = None
        if not dpi or not (36 <= dpi <= 1200):
            dpi = 96.0
        for i in range(frames):
            im.seek(i)
            orientation = 1
            try:
                orientation = im.getexif().get(0x0112, 1)
            except Exception:
                pass
            if fmt in ("JPEG", "PNG") and frames == 1 and orientation in (None, 1) \
                    and im.mode in ("RGB", "L", "RGBA", "LA", "P") and src.stat().st_size > 0:
                yield src.read_bytes(), im.width, im.height, dpi
                continue
            frame = ImageOps.exif_transpose(im) if i == 0 else im.copy()
            has_alpha = frame.mode in ("RGBA", "LA") or (frame.mode == "P" and "transparency" in frame.info)
            import io
            buf = io.BytesIO()
            if has_alpha:
                frame.convert("RGBA").save(buf, "PNG")
            else:
                frame.convert("RGB").save(buf, "JPEG", quality=95, subsampling=0)
            yield buf.getvalue(), frame.width, frame.height, dpi


def images_to_pdf(sources: list[Path], out_dir: Path, opts: dict) -> Result:
    import pymupdf

    page_mode = opts.get("page", "fit")
    margin = _MARGIN_PT.get(opts.get("margin", "small"), 18)
    doc = pymupdf.open()
    skipped = []
    try:
        for src in sources:
            try:
                frames = list(_image_frames(src))
            except Exception as e:
                skipped.append(f"{src.name}: {friendly_error(e)}")
                continue
            for data, w, h, dpi in frames:
                if page_mode == "fit":
                    pw, ph = w * 72.0 / dpi, h * 72.0 / dpi
                    longest = max(pw, ph)
                    if longest > 1500:  # keep pages a sensible size for huge photos
                        pw, ph = pw * 1500 / longest, ph * 1500 / longest
                    page = doc.new_page(width=pw, height=ph)
                    page.insert_image(page.rect, stream=data)
                else:
                    pw, ph = _PAGE_PT[page_mode]
                    if w > h:
                        pw, ph = ph, pw  # landscape image -> landscape page
                    page = doc.new_page(width=pw, height=ph)
                    rect = pymupdf.Rect(margin, margin, pw - margin, ph - margin)
                    page.insert_image(rect, stream=data, keep_proportion=True)
        if doc.page_count == 0:
            raise ConversionError("None of the images could be read. " + "; ".join(skipped))
        tmp = Path(tempfile.mkdtemp(prefix="docshift_"))
        try:
            target = tmp / "out.pdf"
            doc.save(str(target), garbage=3, deflate=True)
            stem = sources[0].stem if len(sources) == 1 else f"{sources[0].stem}_merged"
            out = _publish(target, out_dir, stem, ".pdf")
        finally:
            shutil.rmtree(tmp, ignore_errors=True)
    finally:
        doc.close()
    return Result([out], ("Skipped: " + "; ".join(skipped)) if skipped else "")


# --------------------------------------------------------------------------- #
# PDF -> Word / Excel / PowerPoint / Images
# --------------------------------------------------------------------------- #
def _open_pdf(src: Path):
    import pymupdf

    try:
        doc = pymupdf.open(str(src))
    except Exception:
        raise ConversionError("This file could not be opened as a PDF. It may be damaged.")
    if doc.needs_pass:
        doc.close()
        raise ConversionError("This PDF is password-protected. Remove the password and try again.")
    if doc.page_count == 0:
        doc.close()
        raise ConversionError("This PDF has no pages.")
    return doc


def _has_text(doc) -> bool:
    return any(page.get_text("text").strip() for page in doc)


def pdf_to_word(src: Path, out_dir: Path, opts: dict, office: OfficeSession) -> Result:
    doc = _open_pdf(src)
    scanned = not _has_text(doc)
    doc.close()
    tmp = Path(tempfile.mkdtemp(prefix="docshift_"))
    try:
        # Copy to a plain ASCII temp name: Word's PDF importer is fussy with unusual paths.
        pdf_copy = tmp / "in.pdf"
        shutil.copyfile(src, pdf_copy)
        target = tmp / "out.docx"
        app = office.get("word")
        d = app.Documents.Open(_abs(pdf_copy), False, True, False, "x", "x", False, "x", "x",
                               MISSING, MISSING, False, False, MISSING, True)
        try:
            d.SaveAs2(_abs(target), 16)  # wdFormatXMLDocument
        finally:
            try:
                d.Close(0)
            except Exception:
                pass
        if not target.exists():
            raise ConversionError("Word could not convert this PDF.")
        note = ("This PDF looks scanned (no selectable text), so the Word file contains page images. "
                "OCR is not included.") if scanned else ""
        return Result([_publish(target, out_dir, src.stem, ".docx")], note)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


_NUM = re.compile(r"^-?(?:\d{1,3}(?:,\d{3})+|\d+)(?:\.\d+)?$")


def _coerce(cell):
    """'1,234.50' -> 1234.5 ; leave IDs with leading zeros, phone numbers etc. as text."""
    if cell is None:
        return ""
    s = str(cell).strip()
    if _NUM.match(s):
        digits = s.replace(",", "")
        body = digits.lstrip("-")
        if len(body) > 1 and body.startswith("0") and not body.startswith("0."):
            return s
        try:
            return float(digits) if "." in digits else int(digits)
        except ValueError:
            return s
    return s


def _text_grid_tables(page) -> list:
    """Find tables that have no ruling lines (e.g. a spreadsheet printout) purely from word positions.

    Words are grouped into rows; a row is split into cells wherever the horizontal gap is clearly wider than a
    normal word space; overlapping cell x-ranges across rows are merged into column bands. Ordinary paragraphs
    have one cell per row, so they never qualify. Returns [(bbox, rows)]."""
    words = page.get_text("words")
    if not words:
        return []
    words.sort(key=lambda w: ((w[1] + w[3]) / 2, w[0]))
    lines: list[list] = []
    for w in words:
        yc, h = (w[1] + w[3]) / 2, w[3] - w[1]
        if lines and abs(yc - lines[-1][0]) <= 0.5 * max(h, 1):
            lines[-1][1].append(w)
        else:
            lines.append([yc, [w]])

    def cells_of(ws):
        ws = sorted(ws, key=lambda w: w[0])
        avg_h = sum(w[3] - w[1] for w in ws) / len(ws)
        cells, cur = [], [ws[0]]
        for w in ws[1:]:
            if w[0] - cur[-1][2] > 0.32 * avg_h:  # word space ~0.19, tight column gap ~0.4+
                cells.append(cur)
                cur = [w]
            else:
                cur.append(w)
        cells.append(cur)
        return [(min(w[0] for w in c), max(w[2] for w in c), " ".join(w[4] for w in c)) for c in cells]

    parsed = [(ln[0], cells_of(ln[1]), ln[1]) for ln in lines]
    multi = [len(p[1]) >= 2 for p in parsed]
    # runs of rows that have >= 2 cells (a single sparse row between two table rows stays inside the table)
    runs, i = [], 0
    while i < len(parsed):
        if not multi[i]:
            i += 1
            continue
        j = i
        while j + 1 < len(parsed) and (multi[j + 1] or (j + 2 < len(parsed) and multi[j + 2])):
            j += 1
        runs.append((i, j))
        i = j + 1

    tables = []
    for a, b in runs:
        rows = parsed[a:b + 1]
        if len(rows) < 3:
            continue
        spans = sorted((c[0], c[1]) for _, cells, _ in rows for c in cells)
        bands = [list(spans[0])]
        for x0, x1 in spans[1:]:
            if x0 <= bands[-1][1]:
                bands[-1][1] = max(bands[-1][1], x1)
            else:
                bands.append([x0, x1])
        if len(bands) < 2:
            continue
        data = []
        for _, cells, _ws in rows:
            row = [""] * len(bands)
            for x0, x1, text in cells:
                xc = (x0 + x1) / 2
                k = next((n for n, (bx0, bx1) in enumerate(bands) if bx0 <= xc <= bx1), None)
                if k is not None:
                    row[k] = (row[k] + " " + text).strip()
            data.append(row)
        allw = [w for _, _c, ws in rows for w in ws]
        bbox = (min(w[0] for w in allw), min(w[1] for w in allw), max(w[2] for w in allw), max(w[3] for w in allw))
        tables.append((bbox, data))
    return tables


def _compact_columns(data: list) -> list:
    """Ruled tables from Word often come back as several staggered sub-columns per real column. Merge adjacent
    columns whose cells are never filled in the same row (and drop columns that are always empty)."""
    ncols = max((len(r) for r in data), default=0)
    rows = [list(r) + [None] * (ncols - len(r)) for r in data]
    filled = lambda v: v is not None and str(v).strip() != ""
    groups: list[list[int]] = []
    for c in range(ncols):
        if groups and all(sum(filled(r[k]) for k in groups[-1] + [c]) <= 1 for r in rows):
            groups[-1].append(c)
        else:
            groups.append([c])
    out = []
    for r in rows:
        new = []
        for g in groups:
            vals = [str(r[k]).strip() for k in g if filled(r[k])]
            new.append(vals[0] if vals else "")
        out.append(new)
    return out


def _find_tables(page) -> list:
    """[(bbox, rows)] - ruled tables first; otherwise tables recognised from text alignment."""
    found = []
    try:
        for t in page.find_tables().tables:
            try:
                data = t.extract()
            except Exception:
                continue
            if data and any(any((c or "").strip() for c in r) for r in data):
                found.append((tuple(t.bbox), _compact_columns(data)))
    except Exception:
        pass
    return found or _text_grid_tables(page)


def pdf_to_excel(src: Path, out_dir: Path, opts: dict) -> Result:
    import pymupdf
    from openpyxl import Workbook
    from openpyxl.styles import Alignment, Border, Font, PatternFill, Side
    from openpyxl.utils import get_column_letter

    doc = _open_pdf(src)
    single = opts.get("layout", "pages") == "single"
    wb = Workbook()
    wb.remove(wb.active)
    thin = Side(style="thin", color="BFBFBF")
    border = Border(left=thin, right=thin, top=thin, bottom=thin)
    head_fill = PatternFill("solid", fgColor="EDEFF7")
    tables_found = 0
    try:
        ws = None
        row = 1
        widths: dict[int, int] = {}

        def finish_sheet():
            if ws is None:
                return
            for col, w in widths.items():
                ws.column_dimensions[get_column_letter(col)].width = min(max(w + 2, 8), 60)

        for pno, page in enumerate(doc):
            items = [(bbox[1], "table", data, bbox) for bbox, data in _find_tables(page)]  # (y, kind, payload, bbox)
            table_rects = [pymupdf.Rect(i[3]) for i in items]
            for b in page.get_text("blocks"):
                if b[6] != 0 or not b[4].strip():
                    continue
                r = pymupdf.Rect(b[:4])
                if any((r & tr).get_area() > 0.5 * max(r.get_area(), 1) for tr in table_rects):
                    continue
                items.append((b[1], "text", b[4].strip("\n"), None))
            if not items:
                continue
            items.sort(key=lambda i: (round(i[0], 1)))
            if ws is None or not single:
                finish_sheet()
                ws = wb.create_sheet(f"Page {pno + 1}" if not single else "Data")
                row, widths = 1, {}
            elif row > 1:
                row += 1
            for _, kind, payload, _bbox in items:
                if kind == "table":
                    tables_found += 1
                    for ri, r in enumerate(payload):
                        for ci, cell in enumerate(r, start=1):
                            val = _coerce(cell)
                            c = ws.cell(row=row, column=ci, value=val)
                            c.border = border
                            c.alignment = Alignment(wrap_text=True, vertical="top")
                            if ri == 0:
                                c.font = Font(bold=True)
                                c.fill = head_fill
                            widths[ci] = max(widths.get(ci, 0), min(len(str(val)), 60))
                        row += 1
                    row += 1
                else:
                    for line in payload.split("\n"):
                        if line.strip():
                            ws.cell(row=row, column=1, value=line.strip())
                            widths[1] = max(widths.get(1, 0), min(len(line), 60))
                            row += 1
                    row += 1
        finish_sheet()
        if not wb.sheetnames:
            raise ConversionError("No text or tables were found. If this PDF is a scan, it needs OCR first.")
        tmp = Path(tempfile.mkdtemp(prefix="docshift_"))
        try:
            target = tmp / "out.xlsx"
            wb.save(target)
            out = _publish(target, out_dir, src.stem, ".xlsx")
        finally:
            shutil.rmtree(tmp, ignore_errors=True)
    finally:
        doc.close()
    note = "" if tables_found else "No table structure was detected, so the page text was placed line by line."
    return Result([out], note)


def _clean_font(name: str, text: str = "") -> str:
    name = re.sub(r"^[A-Z]{6}\+", "", name or "")
    base = re.split(r"[-,]", name)[0]
    base = re.sub(r"(PSMT|PS|MT)$", "", base)
    base = re.sub(r"(?<=[a-z])(?=[A-Z])", " ", base).strip()
    if base.lower() in ("symbol", "wingdings", "zapfdingbats", "webdings") \
            and not any(0xF000 <= ord(c) <= 0xF0FF for c in text):
        return "Arial"  # e.g. a normal U+2022 bullet tagged with Symbol would render as an empty box
    mapping = {"Helvetica": "Arial", "Times": "Times New Roman", "Times Roman": "Times New Roman",
               "Courier": "Courier New"}
    return mapping.get(base, base) or "Arial"


def pdf_to_pptx(src: Path, out_dir: Path, opts: dict) -> Result:
    import pymupdf
    from pptx import Presentation
    from pptx.dml.color import RGBColor
    from pptx.enum.text import MSO_ANCHOR, MSO_AUTO_SIZE
    from pptx.util import Emu, Pt

    editable = opts.get("style", "exact") == "editable"
    dpi = int(opts.get("dpi", 150))
    doc = _open_pdf(src)
    emu = 12700  # EMU per PDF point
    tmp = Path(tempfile.mkdtemp(prefix="docshift_"))
    try:
        prs = Presentation()
        first = doc[0].rect
        prs.slide_width, prs.slide_height = Emu(int(first.width * emu)), Emu(int(first.height * emu))
        blank = prs.slide_layouts[6]
        zoom = dpi / 72.0
        fallback_pages = 0
        for pno, page in enumerate(doc):
            slide = prs.slides.add_slide(blank)
            pw, ph = page.rect.width, page.rect.height
            scale = min(first.width / pw, first.height / ph)
            ox, oy = (first.width - pw * scale) / 2, (first.height - ph * scale) / 2

            spans_ok = editable and page.rotation == 0
            lines = []
            if spans_ok:
                for block in page.get_text("dict")["blocks"]:
                    if block.get("type") != 0:
                        continue
                    for ln in block["lines"]:
                        dx, dy = ln["dir"]
                        if abs(dx - 1) > 0.01 or abs(dy) > 0.01 or not any(s["text"].strip() for s in ln["spans"]):
                            continue  # rotated text is left in the background image
                        lines.append(ln)
            if editable and page.rotation != 0:
                fallback_pages += 1

            if lines:
                # Remove exactly the text we are about to re-create, keep images and vector art.
                for ln in lines:
                    page.add_redact_annot(pymupdf.Rect(ln["bbox"]), fill=False)
                page.apply_redactions(images=pymupdf.PDF_REDACT_IMAGE_NONE,
                                      graphics=pymupdf.PDF_REDACT_LINE_ART_NONE,
                                      text=pymupdf.PDF_REDACT_TEXT_REMOVE)
            pix = page.get_pixmap(matrix=pymupdf.Matrix(zoom, zoom), alpha=False)
            img_path = tmp / f"p{pno}.png"
            pix.save(str(img_path))
            slide.shapes.add_picture(str(img_path), Emu(int(ox * emu)), Emu(int(oy * emu)),
                                     Emu(int(pw * scale * emu)), Emu(int(ph * scale * emu)))

            for ln in lines:
                x0, y0, x1, y1 = ln["bbox"]
                box = slide.shapes.add_textbox(Emu(int((ox + x0 * scale) * emu)), Emu(int((oy + y0 * scale) * emu)),
                                               Emu(int(((x1 - x0) * scale + 6) * emu)), Emu(int((y1 - y0) * scale * emu)))
                tf = box.text_frame
                tf.word_wrap = False
                tf.auto_size = MSO_AUTO_SIZE.NONE
                tf.margin_left = tf.margin_right = tf.margin_top = tf.margin_bottom = 0
                tf.vertical_anchor = MSO_ANCHOR.TOP
                para = tf.paragraphs[0]
                for s in ln["spans"]:
                    if not s["text"]:
                        continue
                    run = para.add_run()
                    run.text = s["text"]
                    f = run.font
                    f.name = _clean_font(s["font"], s["text"])
                    f.size = Pt(max(1.0, round(s["size"] * scale, 1)))
                    f.bold = bool(s["flags"] & 16) or "bold" in s["font"].lower()
                    f.italic = bool(s["flags"] & 2) or "italic" in s["font"].lower() or "oblique" in s["font"].lower()
                    col = s["color"]
                    f.color.rgb = RGBColor((col >> 16) & 255, (col >> 8) & 255, col & 255)

            # Text for search / screen readers even in exact mode.
            txt = doc.load_page(pno).get_text("text").strip() if not lines else ""
            if txt:
                slide.notes_slide.notes_text_frame.text = txt
        target = tmp / "out.pptx"
        prs.save(str(target))
        out = _publish(target, out_dir, src.stem, ".pptx")
    finally:
        doc.close()
        shutil.rmtree(tmp, ignore_errors=True)
    note = ""
    if editable and fallback_pages:
        note = f"{fallback_pages} rotated page(s) were kept as images."
    return Result([out], note)


def pdf_to_images(src: Path, out_dir: Path, opts: dict) -> Result:
    import pymupdf
    from PIL import Image

    fmt = opts.get("format", "png").lower()
    dpi = int(opts.get("dpi", 200))
    ext = {"jpg": ".jpg", "png": ".png", "webp": ".webp", "tiff": ".tiff", "bmp": ".bmp"}[fmt]
    doc = _open_pdf(src)
    tmp = Path(tempfile.mkdtemp(prefix="docshift_"))
    try:
        pages = parse_pages(opts.get("pages", ""), doc.page_count)
        zoom = dpi / 72.0
        mat = pymupdf.Matrix(zoom, zoom)
        made = []
        for i in pages:
            pix = doc[i].get_pixmap(matrix=mat, alpha=False)
            path = tmp / f"{src.stem}_page_{i + 1:03d}{ext}"
            if fmt == "png":
                pix.save(str(path))
            else:
                im = Image.frombytes("RGB", (pix.width, pix.height), pix.samples)
                kwargs = {"dpi": (dpi, dpi)}
                if fmt == "jpg":
                    kwargs.update(quality=95, subsampling=0, optimize=True)
                elif fmt == "webp":
                    kwargs.update(quality=92, method=4)
                elif fmt == "tiff":
                    kwargs.update(compression="tiff_lzw")
                im.save(path, **kwargs)
            made.append(path)
        out_dir.mkdir(parents=True, exist_ok=True)
        if len(made) == 1:
            dst = unique_path(out_dir, made[0].stem, ext)
            shutil.move(str(made[0]), str(dst))
            return Result([dst])
        folder = unique_path(out_dir, f"{src.stem}_images", "")
        folder.mkdir(parents=True)
        for p in made:
            shutil.move(str(p), str(folder / p.name))
        return Result([folder], f"{len(made)} images saved in a folder.")
    finally:
        doc.close()
        shutil.rmtree(tmp, ignore_errors=True)


# --------------------------------------------------------------------------- #
# Dispatcher
# --------------------------------------------------------------------------- #
OLE_MAGIC = bytes([0xD0, 0xCF, 0x11, 0xE0])  # header of an OLE compound file
_OOXML = {".docx", ".docm", ".dotx", ".xlsx", ".xlsm", ".pptx", ".pptm", ".ppsx"}


def _precheck(src: Path):
    """Cheap sanity checks that give instant, clear errors (Office can take many seconds to fail on these)."""
    if not src.exists():
        raise FileNotFoundError(src)
    if src.stat().st_size == 0:
        raise ConversionError("This file is empty.")
    if src.suffix.lower() in _OOXML:
        with open(src, "rb") as fh:
            if fh.read(4) == OLE_MAGIC:  # OLE container, not a ZIP: encrypted OOXML (or a misnamed old file)
                raise ConversionError("This file is password-protected (or its extension doesn't match its "
                                      "contents). Remove the password and try again.")


def run(mode: str, sources: list[Path], out_dir: Path, opts: dict, office: OfficeSession) -> Result:
    """Convert `sources` (one file, or many for merged image->PDF) and return where the output went."""
    src = sources[0]
    if mode != "img2pdf":  # images are validated by Pillow, which reports per-file problems itself
        _precheck(src)
    elif not src.exists():
        raise FileNotFoundError(src)
    try:
        if mode in _OFFICE_TO_PDF:
            return office_to_pdf(mode, src, out_dir, opts, office)
        if mode == "img2pdf":
            return images_to_pdf(sources, out_dir, opts)
        if mode == "pdf2word":
            return pdf_to_word(src, out_dir, opts, office)
        if mode == "pdf2excel":
            return pdf_to_excel(src, out_dir, opts)
        if mode == "pdf2ppt":
            return pdf_to_pptx(src, out_dir, opts)
        if mode == "pdf2img":
            return pdf_to_images(src, out_dir, opts)
    except Exception:
        if HAVE_COM and mode in ("word2pdf", "excel2pdf", "ppt2pdf", "pdf2word"):
            # A crashed Office instance can't be reused for the next file; drop it so it restarts cleanly.
            office.close()
        raise
    raise ConversionError(f"Unknown conversion: {mode}")
