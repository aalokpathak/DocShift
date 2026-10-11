"""DocShift - convert Word, Excel, PowerPoint and images to PDF, and back again."""
from __future__ import annotations

import ctypes
import os
import subprocess
import sys
import tempfile
import uuid
from ctypes import wintypes
from dataclasses import dataclass, field
from pathlib import Path
from string import Template

from PyQt5.QtCore import QPointF, QRectF, QRegularExpression, QSettings, Qt, QThread, QTimer, pyqtSignal
from PyQt5.QtGui import QColor, QFont, QIcon, QPainter, QPainterPath, QPen, QPixmap, QRegularExpressionValidator
from PyQt5.QtSvg import QSvgRenderer
from PyQt5.QtWidgets import (QApplication, QCheckBox, QComboBox, QFileDialog, QFrame, QGridLayout, QHBoxLayout,
                             QLabel, QLineEdit, QMessageBox, QProgressBar, QPushButton, QScrollArea,
                             QSizePolicy, QStackedWidget, QVBoxLayout, QWidget)

import compressors
import converters as cv

APP_NAME = "DocShift"
APP_ID = "DocShift.Converter.1"


def resource(rel: str) -> Path:
    base = Path(getattr(sys, "_MEIPASS", Path(__file__).resolve().parent))
    return base / rel


# --------------------------------------------------------------------------- #
# Small platform helpers
# --------------------------------------------------------------------------- #
def downloads_dir() -> Path:
    """The user's real Downloads folder (it can be relocated, so ask Windows rather than guessing)."""
    try:
        class GUID(ctypes.Structure):
            _fields_ = [("Data1", wintypes.DWORD), ("Data2", wintypes.WORD), ("Data3", wintypes.WORD),
                        ("Data4", ctypes.c_ubyte * 8)]

        guid = GUID.from_buffer_copy(uuid.UUID("374DE290-123F-4565-9164-39C4925E467B").bytes_le)
        fn = ctypes.windll.shell32.SHGetKnownFolderPath
        fn.argtypes = [ctypes.POINTER(GUID), wintypes.DWORD, wintypes.HANDLE, ctypes.POINTER(ctypes.c_wchar_p)]
        buf = ctypes.c_wchar_p()
        if fn(ctypes.byref(guid), 0, None, ctypes.byref(buf)) == 0 and buf.value:
            path = Path(buf.value)
            ctypes.windll.ole32.CoTaskMemFree(buf)
            return path
    except Exception:
        pass
    return Path.home() / "Downloads"


def reveal(path: Path):
    """Open Explorer at a file (selected) or folder."""
    try:
        if path.is_dir():
            os.startfile(str(path))
        else:
            subprocess.Popen(f'explorer /select,"{path}"')
    except Exception:
        pass


def set_dark_titlebar(widget: QWidget, dark: bool):
    try:
        hwnd = int(widget.winId())
        val = ctypes.c_int(1 if dark else 0)
        for attr in (20, 19):  # DWMWA_USE_IMMERSIVE_DARK_MODE (Win10 20H1+ / older builds)
            if ctypes.windll.dwmapi.DwmSetWindowAttribute(hwnd, attr, ctypes.byref(val), 4) == 0:
                break
    except Exception:
        pass


def human_size(n: int) -> str:
    size = float(n)
    for unit in ("B", "KB", "MB", "GB"):
        if size < 1024 or unit == "GB":
            return f"{size:.0f} {unit}" if unit == "B" else f"{size:.1f} {unit}"
        size /= 1024
    return f"{n} B"


# --------------------------------------------------------------------------- #
# Theme
# --------------------------------------------------------------------------- #
DARK = dict(bg="#11131B", sidebar="#161925", card="#1B1F2E", hover="#232838", border="#2A3046",
            border_strong="#3A425E", text="#E9ECF6", muted="#98A0BC", accent="#6366F1", accent_hover="#7578F5",
            accent_press="#5153D9", accent_soft="rgba(99,102,241,0.16)", accent_line="rgba(129,140,248,0.55)",
            success="#34D399", danger="#F87171", warning="#FBBF24", on_accent="#FFFFFF",
            field="#141724", disabled="#2A3046", disabled_text="#6B7391")
LIGHT = dict(bg="#F4F5FA", sidebar="#FFFFFF", card="#FFFFFF", hover="#F0F1F9", border="#E3E6F1",
             border_strong="#C9CEE0", text="#1A1E33", muted="#6A7191", accent="#4F46E5", accent_hover="#6058F0",
             accent_press="#4038C9", accent_soft="rgba(79,70,229,0.10)", accent_line="rgba(79,70,229,0.45)",
             success="#059669", danger="#DC2626", warning="#B45309", on_accent="#FFFFFF",
             field="#FFFFFF", disabled="#E3E6F1", disabled_text="#9AA0BC")

QSS = Template(r"""
* { font-family: "Segoe UI", "Segoe UI Variable", sans-serif; font-size: 10pt; color: $text; }
QWidget#Root, QWidget#Content { background: $bg; }
QToolTip { background: $card; color: $text; border: 1px solid $border_strong; padding: 5px 8px; }

QFrame#Sidebar { background: $sidebar; border-right: 1px solid $border; }
QLabel#AppName { font-size: 17pt; font-weight: 700; }
QLabel#Tagline { color: $muted; font-size: 8.5pt; }
QLabel#SectionLabel { color: $muted; font-size: 8pt; font-weight: 700; letter-spacing: 1px; padding-left: 6px; }

QFrame#ModeItem { background: transparent; border: 1px solid transparent; border-radius: 12px; }
QFrame#ModeItem:hover { background: $hover; }
QFrame#ModeItem[active="true"] { background: $accent_soft; border: 1px solid $accent_line; }
QFrame#ModeItem:focus { border: 1px solid $accent_line; }
QFrame#ModeItem QLabel { background: transparent; }
QLabel#ModeTitle { font-weight: 600; }
QLabel#ModeSub { color: $muted; font-size: 8pt; }

QLabel#EngineText { color: $muted; font-size: 8.5pt; }
QLabel#Title { font-size: 20pt; font-weight: 700; }
QLabel#Blurb { color: $muted; }
QLabel#Banner { background: rgba(251,191,36,0.14); color: $warning; border: 1px solid rgba(251,191,36,0.45);
                border-radius: 10px; padding: 8px 12px; }

QFrame#DropZone { background: $card; border: 2px dashed $border_strong; border-radius: 16px; }
QFrame#DropZone[drag="true"] { background: $accent_soft; border: 2px dashed $accent; }
QFrame#DropZone QLabel { background: transparent; }
QLabel#DropTitle { font-size: 12pt; font-weight: 600; }
QLabel#DropSub { color: $muted; }

QLabel#ListHeader { font-weight: 600; }
QLabel#EmptyState { color: $muted; }
QScrollArea, QScrollArea > QWidget > QWidget { background: transparent; border: none; }
QWidget#SideList { background: transparent; }

QFrame#FileRow { background: $card; border: 1px solid $border; border-radius: 12px; }
QFrame#FileRow QLabel { background: transparent; }
QLabel#FileName { font-weight: 600; }
QLabel#FileMeta { color: $muted; font-size: 8.5pt; }
QLabel#Status { color: $muted; font-size: 9pt; }
QLabel#Status[state="running"] { color: $accent; font-weight: 600; }
QLabel#Status[state="done"] { color: $success; font-weight: 600; }
QLabel#Status[state="error"] { color: $danger; }

QFrame#Card { background: $card; border: 1px solid $border; border-radius: 14px; }
QFrame#Card QLabel { background: transparent; }
QLabel#CardTitle { font-weight: 600; }
QLabel#Hint { color: $muted; font-size: 9pt; }
QLabel#FieldLabel { color: $muted; }

QLineEdit { background: $field; border: 1px solid $border_strong; border-radius: 8px; padding: 7px 10px;
            selection-background-color: $accent; }
QLineEdit:focus { border: 1px solid $accent; }
QLineEdit[readOnly="true"] { color: $text; }

QComboBox { background: $field; border: 1px solid $border_strong; border-radius: 8px; padding: 6px 10px; min-height: 20px; }
QComboBox:hover { border-color: $accent; }
QComboBox:disabled { color: $disabled_text; border-color: $border; }
QComboBox::drop-down { border: none; width: 26px; }
QComboBox::down-arrow { image: url($chevron); width: 12px; height: 12px; }
QComboBox QAbstractItemView { background: $card; border: 1px solid $border_strong; outline: 0; padding: 4px;
                              selection-background-color: $accent; selection-color: $on_accent; }

QCheckBox { spacing: 9px; background: transparent; }
QCheckBox::indicator { width: 18px; height: 18px; border-radius: 5px; border: 1px solid $border_strong; background: $field; }
QCheckBox::indicator:hover { border-color: $accent; }
QCheckBox::indicator:checked { background: $accent; border-color: $accent; image: url($check); }
QCheckBox:disabled { color: $disabled_text; }

QPushButton { background: transparent; border: 1px solid $border_strong; border-radius: 9px; padding: 8px 16px; font-weight: 600; }
QPushButton:hover { background: $hover; border-color: $accent; }
QPushButton:pressed { background: $accent_soft; }
QPushButton:disabled { color: $disabled_text; border-color: $border; }
QPushButton#Primary { background: $accent; color: $on_accent; border: none; border-radius: 11px; padding: 12px 30px; font-size: 11pt; }
QPushButton#Primary:hover { background: $accent_hover; }
QPushButton#Primary:pressed { background: $accent_press; }
QPushButton#Primary:disabled { background: $disabled; color: $disabled_text; }
QPushButton#Browse { background: $accent; color: $on_accent; border: none; padding: 10px 22px; }
QPushButton#Browse:hover { background: $accent_hover; }
QPushButton#Browse:pressed { background: $accent_press; }
QPushButton#Link { border: none; padding: 4px 8px; color: $accent; background: transparent; }
QPushButton#Link:hover { background: $accent_soft; }
QPushButton#Link:disabled { color: $disabled_text; }
QPushButton#Icon { border: none; padding: 0; border-radius: 8px; color: $muted; font-size: 11pt; min-width: 28px; max-width: 28px; min-height: 28px; max-height: 28px; }
QPushButton#Icon:hover { background: $hover; color: $text; }
QPushButton#Theme { border: 1px solid $border; border-radius: 9px; padding: 6px 12px; color: $muted; font-weight: 500; }

QProgressBar { background: $border; border: none; border-radius: 4px; max-height: 8px; min-height: 8px; text-align: center; }
QProgressBar::chunk { background: $accent; border-radius: 4px; }
QLabel#StatusLine { color: $muted; }
QLabel#StatusLine[state="ok"] { color: $success; font-weight: 600; }
QLabel#StatusLine[state="warn"] { color: $warning; font-weight: 600; }
QLabel#StatusLine[state="err"] { color: $danger; font-weight: 600; }

QScrollBar:vertical { background: transparent; width: 10px; margin: 2px; }
QScrollBar::handle:vertical { background: $border_strong; border-radius: 4px; min-height: 30px; }
QScrollBar::handle:vertical:hover { background: $muted; }
QScrollBar::add-line:vertical, QScrollBar::sub-line:vertical { height: 0; }
QScrollBar::add-page:vertical, QScrollBar::sub-page:vertical { background: transparent; }
""")


def make_ui_icons(p: dict, tag: str) -> dict:
    """Small PNGs the stylesheet needs (checkbox tick, combo chevron), drawn to match the theme."""
    d = Path(tempfile.gettempdir()) / f"docshift_ui_{tag}"
    d.mkdir(parents=True, exist_ok=True)
    check = QPixmap(36, 36)
    check.fill(Qt.transparent)
    q = QPainter(check)
    q.setRenderHint(QPainter.Antialiasing)
    pen = QPen(QColor("#FFFFFF"), 4.5, Qt.SolidLine, Qt.RoundCap, Qt.RoundJoin)
    q.setPen(pen)
    q.drawPolyline(QPointF(9, 19), QPointF(15, 25), QPointF(27, 11))
    q.end()
    chev = QPixmap(36, 36)
    chev.fill(Qt.transparent)
    q = QPainter(chev)
    q.setRenderHint(QPainter.Antialiasing)
    q.setPen(QPen(QColor(p["muted"]), 4, Qt.SolidLine, Qt.RoundCap, Qt.RoundJoin))
    q.drawPolyline(QPointF(8, 13), QPointF(18, 23), QPointF(28, 13))
    q.end()
    check.save(str(d / "check.png"))
    chev.save(str(d / "chevron.png"))
    return {"check": (d / "check.png").as_posix(), "chevron": (d / "chevron.png").as_posix()}


def build_qss(p: dict, tag: str) -> str:
    icons = make_ui_icons(p, tag)
    return QSS.substitute(**p, **icons)


# --------------------------------------------------------------------------- #
# Painted bits: file-type badges, drop-zone icon
# --------------------------------------------------------------------------- #
KIND_STYLE = {
    "word": ("W", "#2B579A"), "excel": ("X", "#1D7A46"), "ppt": ("P", "#C8431F"),
    "img": ("IMG", "#8B5CF6"), "pdf": ("PDF", "#E5484D"), "video": ("VID", "#0891B2"),
}
KIND_NOUN = {"word": "Word documents", "excel": "Excel workbooks", "ppt": "PowerPoint presentations",
             "img": "images", "pdf": "PDF files", "video": "videos"}


def badge_pixmap(kind: str, size: int = 32) -> QPixmap:
    text, color = KIND_STYLE.get(kind, ("?", "#64748B"))
    dpr = 2
    pm = QPixmap(size * dpr, size * dpr)
    pm.setDevicePixelRatio(dpr)
    pm.fill(Qt.transparent)
    p = QPainter(pm)
    p.setRenderHint(QPainter.Antialiasing)
    p.setPen(Qt.NoPen)
    p.setBrush(QColor(color))
    p.drawRoundedRect(0, 0, size, size, size * 0.28, size * 0.28)
    f = QFont("Segoe UI", 1)
    f.setBold(True)
    f.setPixelSize(int(size * (0.48 if len(text) == 1 else 0.30)))
    p.setFont(f)
    p.setPen(QColor("#FFFFFF"))
    p.drawText(QRectF(0, 0, size, size), Qt.AlignCenter, text)
    p.end()
    return pm


def upload_pixmap(color: str, size: int = 44) -> QPixmap:
    dpr = 2
    pm = QPixmap(size * dpr, size * dpr)
    pm.setDevicePixelRatio(dpr)
    pm.fill(Qt.transparent)
    p = QPainter(pm)
    p.setRenderHint(QPainter.Antialiasing)
    c = QColor(color)
    p.setPen(Qt.NoPen)
    bg = QColor(c)
    bg.setAlpha(38)
    p.setBrush(bg)
    p.drawRoundedRect(0, 0, size, size, size * 0.3, size * 0.3)
    pen = QPen(c, size * 0.075, Qt.SolidLine, Qt.RoundCap, Qt.RoundJoin)
    p.setPen(pen)
    p.setBrush(Qt.NoBrush)
    cx = size / 2
    p.drawLine(QPointF(cx, size * 0.30), QPointF(cx, size * 0.62))
    p.drawPolyline(QPointF(cx - size * 0.13, size * 0.43), QPointF(cx, size * 0.30), QPointF(cx + size * 0.13, size * 0.43))
    path = QPainterPath()
    path.moveTo(size * 0.26, size * 0.62)
    path.lineTo(size * 0.26, size * 0.72)
    path.lineTo(size * 0.74, size * 0.72)
    path.lineTo(size * 0.74, size * 0.62)
    p.drawPath(path)
    p.end()
    return pm


def svg_icon(path: Path) -> QIcon:
    icon = QIcon()
    r = QSvgRenderer(str(path))
    for s in (16, 20, 24, 32, 40, 48, 64, 128, 256):
        pm = QPixmap(s, s)
        pm.fill(Qt.transparent)
        p = QPainter(pm)
        p.setRenderHint(QPainter.Antialiasing)
        r.render(p)
        p.end()
        icon.addPixmap(pm)
    return icon


def svg_pixmap(path: Path, size: int) -> QPixmap:
    dpr = 2
    pm = QPixmap(size * dpr, size * dpr)
    pm.setDevicePixelRatio(dpr)
    pm.fill(Qt.transparent)
    p = QPainter(pm)
    p.setRenderHint(QPainter.Antialiasing)
    QSvgRenderer(str(path)).render(p, QRectF(0, 0, size, size))
    p.end()
    return pm


def repolish(w: QWidget):
    w.style().unpolish(w)
    w.style().polish(w)
    w.update()


# --------------------------------------------------------------------------- #
# Widgets
# --------------------------------------------------------------------------- #
class ElidedLabel(QLabel):
    """A label that shortens long text with an ellipsis instead of forcing the window wider.

    Normally it elides to whatever width the layout gives it. With `max_px` it keeps a normal size policy and
    elides at that fixed width instead - use that for labels that share a row with buttons."""

    def __init__(self, text="", parent=None, mode=Qt.ElideMiddle, max_px: int | None = None):
        super().__init__(parent)
        self._full = ""
        self._mode = mode
        self._max_px = max_px
        if max_px:
            self.setMaximumWidth(max_px)
        else:
            self.setSizePolicy(QSizePolicy.Ignored, QSizePolicy.Preferred)
            self.setMinimumWidth(30)
        self.setText(text)

    def setText(self, text):
        self._full = text
        self._apply()

    def fullText(self):
        return self._full

    def _apply(self):
        w = self._max_px or max(self.width() - 2, 10)
        super().setText(self.fontMetrics().elidedText(self._full, self._mode, w))

    def resizeEvent(self, e):
        super().resizeEvent(e)
        if not self._max_px:
            self._apply()


class ModeItem(QFrame):
    clicked = pyqtSignal(str)

    def __init__(self, mode: cv.Mode):
        super().__init__()
        self.mode = mode
        self.setObjectName("ModeItem")
        self.setCursor(Qt.PointingHandCursor)
        self.setFocusPolicy(Qt.TabFocus)  # ring only when reached by keyboard, never after a mouse click
        self.setProperty("active", False)
        row = QHBoxLayout(self)
        row.setContentsMargins(10, 6, 10, 6)
        row.setSpacing(10)
        badges = QHBoxLayout()
        badges.setSpacing(3)
        for kind in ((mode.src, None, mode.dst) if mode.dst else (mode.src,)):
            lbl = QLabel()
            if kind:
                lbl.setPixmap(badge_pixmap(kind, 24))
                lbl.setFixedSize(24, 24)
            else:
                lbl.setText("›")
                lbl.setObjectName("ModeSub")
                lbl.setFixedWidth(8)
                lbl.setAlignment(Qt.AlignCenter)
            badges.addWidget(lbl)
        row.addLayout(badges)
        col = QVBoxLayout()
        col.setSpacing(0)
        title = QLabel(mode.title)
        title.setObjectName("ModeTitle")
        sub = ElidedLabel(mode.sub, mode=Qt.ElideRight)
        sub.setObjectName("ModeSub")
        col.addWidget(title)
        col.addWidget(sub)
        row.addLayout(col, 1)

    def set_active(self, on: bool):
        self.setProperty("active", on)
        repolish(self)

    def mousePressEvent(self, e):
        if e.button() == Qt.LeftButton:
            self.clicked.emit(self.mode.key)

    def keyPressEvent(self, e):
        if e.key() in (Qt.Key_Return, Qt.Key_Enter, Qt.Key_Space):
            self.clicked.emit(self.mode.key)
        else:
            super().keyPressEvent(e)


class FileRow(QFrame):
    removeRequested = pyqtSignal(object)
    moveRequested = pyqtSignal(object, int)

    def __init__(self, path: Path, kind: str, uid: int):
        super().__init__()
        self.path, self.kind, self.uid = path, kind, uid
        self.state = "queued"
        self.reveal_path: Path | None = None
        self.setObjectName("FileRow")
        row = QHBoxLayout(self)
        row.setContentsMargins(12, 9, 8, 9)
        row.setSpacing(12)
        badge = QLabel()
        badge.setPixmap(badge_pixmap(kind, 34))
        badge.setFixedSize(34, 34)
        row.addWidget(badge)
        col = QVBoxLayout()
        col.setSpacing(1)
        self.name = ElidedLabel(path.name)
        self.name.setObjectName("FileName")
        self.name.setToolTip(str(path))
        try:
            size = human_size(path.stat().st_size)
        except OSError:
            size = "?"
        meta = QHBoxLayout()
        meta.setSpacing(6)
        self.size_lbl = QLabel(f"{size}  ·")
        self.size_lbl.setObjectName("FileMeta")
        self.meta = ElidedLabel(str(path.parent), mode=Qt.ElideLeft)
        self.meta.setObjectName("FileMeta")
        meta.addWidget(self.size_lbl)
        meta.addWidget(self.meta, 1)
        col.addWidget(self.name)
        col.addLayout(meta)
        row.addLayout(col, 1)
        self.status = ElidedLabel("Ready", mode=Qt.ElideRight, max_px=250)
        self.status.setObjectName("Status")
        self.status.setProperty("state", "queued")
        self.status.setAlignment(Qt.AlignRight | Qt.AlignVCenter)
        row.addWidget(self.status)
        self.show_btn = QPushButton("Show")
        self.show_btn.setObjectName("Link")
        self.show_btn.setCursor(Qt.PointingHandCursor)
        self.show_btn.hide()
        self.show_btn.clicked.connect(lambda: self.reveal_path and reveal(self.reveal_path))
        row.addWidget(self.show_btn)
        self.up = self._icon_button("▲", "Move up")
        self.down = self._icon_button("▼", "Move down")
        self.up.clicked.connect(lambda: self.moveRequested.emit(self, -1))
        self.down.clicked.connect(lambda: self.moveRequested.emit(self, 1))
        self.up.hide()
        self.down.hide()
        row.addWidget(self.up)
        row.addWidget(self.down)
        self.rm = self._icon_button("✕", "Remove from list")
        self.rm.clicked.connect(lambda: self.removeRequested.emit(self))
        row.addWidget(self.rm)

    @staticmethod
    def _icon_button(text, tip) -> QPushButton:
        b = QPushButton(text)
        b.setObjectName("Icon")
        b.setToolTip(tip)
        b.setCursor(Qt.PointingHandCursor)
        b.setFocusPolicy(Qt.NoFocus)
        return b

    def set_state(self, state: str, text: str = "", tip: str = "", reveal_path: Path | None = None):
        self.state = state
        self.status.setText(text or {"queued": "Ready", "running": "Converting…"}.get(state, ""))
        self.status.setToolTip(tip or text)
        self.status.setProperty("state", state)
        repolish(self.status)
        self.reveal_path = reveal_path
        self.show_btn.setVisible(state == "done" and reveal_path is not None)
        self.rm.setEnabled(state != "running")


# --------------------------------------------------------------------------- #
# Options (per conversion mode)
# --------------------------------------------------------------------------- #
@dataclass
class Opt:
    kind: str                 # combo | check | text
    key: str
    label: str
    choices: list = field(default_factory=list)   # [(label, value)]
    default: object = None
    placeholder: str = ""
    when: tuple | None = None         # (other_key, value): only shown while that option has that value
    numeric: bool = False             # text box accepts a number like 2 or 0.5
    off_if_filled: str = ""           # disabled while the text box with this key has something typed in it


QUALITY = [("High quality (print)", "print"), ("Smaller file (screen)", "screen")]
LEVELS = [("Light (best quality)", "light"), ("Balanced", "balanced"), ("Strong (smallest)", "strong")]
SQUEEZE_COMMON = [
    Opt("combo", "action", "Action", [("Compress", "compress"), ("Decompress", "expand")]),
    Opt("combo", "level", "Level", LEVELS, default="balanced", when=("action", "compress"), off_if_filled="target_mb"),
]
TARGET = Opt("text", "target_mb", "Target size in MB (optional)", placeholder="No limit", numeric=True,
             when=("action", "compress"))
TARGET_HINT = ("Leave the size empty to use the level, or type a limit (for example 2) and DocShift keeps the best "
               "quality that fits under it. 1 MB = 1,000,000 bytes, so the result passes upload checks that count "
               "either way.")
OPTIONS: dict[str, tuple[list[Opt], object]] = {
    "word2pdf": ([Opt("combo", "quality", "Quality", QUALITY),
                  Opt("check", "bookmarks", "Create bookmarks from headings", default=True)],
                 "Uses Word's own PDF export, so pagination, fonts, headers, footers and images match."),
    "excel2pdf": ([Opt("combo", "quality", "Quality", QUALITY),
                   Opt("check", "fit_width", "Fit each sheet to one page wide", default=True)],
                  "Stops wide sheets from being split across pages. Sheets that already have a fit-to-page "
                  "setting keep it."),
    "ppt2pdf": ([Opt("combo", "quality", "Quality", QUALITY)],
                "Uses PowerPoint's own PDF export. Hidden slides are skipped."),
    "img2pdf": ([Opt("combo", "page", "Page size", [("Fit to image", "fit"), ("A4", "a4"), ("US Letter", "letter")]),
                 Opt("combo", "margin", "Margin", [("Small", "small"), ("None", "none"), ("Medium", "medium")]),
                 Opt("check", "merge", "Merge all images into one PDF", default=True)],
                "Images keep their original quality. When merging, use ▲ ▼ on each file to set the page order."),
    "pdf2word": ([], "Uses Word's PDF reflow, so text, tables and images stay editable. Scanned PDFs are "
                     "inserted as page images (no OCR)."),
    "pdf2excel": ([Opt("combo", "layout", "Layout", [("One sheet per page", "pages"),
                                                     ("Everything on one sheet", "single")])],
                  "Tables become real cells (numbers stay numbers). Text outside tables is kept above and below."),
    "pdf2ppt": ([Opt("combo", "style", "Slide style", [("Editable text on clean background", "editable"),
                                                       ("Exact look (page images)", "exact")]),
                 Opt("combo", "dpi", "Image quality", [("Standard (150 dpi)", 150), ("High (220 dpi)", 220)])],
                "Editable mode rebuilds the text as real text boxes; graphics and photos stay as a background image."),
    "pdf2img": ([Opt("combo", "format", "Format", [("PNG", "png"), ("JPG", "jpg"), ("WEBP", "webp"),
                                                   ("TIFF", "tiff"), ("BMP", "bmp")]),
                 Opt("combo", "dpi", "Resolution", [("72 dpi (screen)", 72), ("150 dpi", 150), ("200 dpi", 200),
                                                    ("300 dpi (print)", 300), ("600 dpi", 600)], default=200),
                 Opt("text", "pages", "Pages", placeholder="All pages - or e.g. 1-3, 5")],
                "Multi-page PDFs are saved into a new folder, one image per page."),
    "compress_pdf": ([*SQUEEZE_COMMON, TARGET],
                     {"compress": TARGET_HINT + " If images alone can't get there, pages are flattened to pictures.",
                      "expand": "Rewrites the PDF with every stream uncompressed. Bigger, but easy to inspect or edit. "
                                "It can't restore quality lost in an earlier compression."}),
    "compress_img": ([*SQUEEZE_COMMON,
                      Opt("combo", "format", "Save as", [("Same format", "same"), ("JPG", "jpg"), ("WEBP", "webp"),
                                                         ("PNG", "png")], when=("action", "compress")),
                      Opt("combo", "expand_format", "Save as", [("PNG (lossless)", "png"), ("TIFF (uncompressed)", "tiff"),
                                                                ("BMP (uncompressed)", "bmp")],
                          when=("action", "expand")),
                      TARGET],
                     {"compress": TARGET_HINT + " The picture is only shrunk if a good quality can't fit.",
                      "expand": "Saves the image in a lossless or uncompressed format. Detail lost to earlier "
                                "compression can't be recovered."}),
    "compress_video": ([*SQUEEZE_COMMON, TARGET],
                       {"compress": TARGET_HINT + " Longer videos or smaller limits lower the resolution. The result "
                                                  "is an MP4 (H.264).",
                        "expand": "Saves the video losslessly (FFV1 in an MKV). Expect a file many times larger; "
                                  "quality is not improved."}),
}


class OptionsPanel(QWidget):
    """Builds the controls for one mode and reads their values back."""
    changed = pyqtSignal()

    def __init__(self, mode_key: str):
        super().__init__()
        specs, self._hints = OPTIONS[mode_key]
        self.widgets: dict[str, QWidget] = {}
        self.boxes: dict[str, QWidget] = {}
        self.specs = {o.key: o for o in specs}
        lay = QVBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(10)
        row = QHBoxLayout()
        row.setSpacing(16)
        for o in specs:
            box = QWidget()
            bl = QVBoxLayout(box)
            bl.setContentsMargins(0, 0, 0, 0)
            bl.setSpacing(4)
            if o.kind == "check":
                w = QCheckBox(o.label)
                w.setChecked(bool(o.default))
                w.setCursor(Qt.PointingHandCursor)
                w.toggled.connect(self._on_change)
                bl.addStretch(1)
                bl.addWidget(w)
            else:
                lbl = QLabel(o.label)
                lbl.setObjectName("FieldLabel")
                if o.kind == "combo":
                    w = QComboBox()
                    for text, value in o.choices:
                        w.addItem(text, value)
                    if o.default is not None:
                        w.setCurrentIndex(max(0, w.findData(o.default)))
                    w.setMinimumWidth(150)
                    w.currentIndexChanged.connect(self._on_change)
                else:
                    w = QLineEdit()
                    w.setPlaceholderText(o.placeholder)
                    w.setMinimumWidth(150 if o.numeric else 220)
                    if o.numeric:
                        w.setValidator(QRegularExpressionValidator(QRegularExpression(r"\d{0,5}([.,]\d{0,3})?"), w))
                    w.textChanged.connect(self._on_change)
                bl.addWidget(lbl)
                bl.addWidget(w)
            self.widgets[o.key] = w
            self.boxes[o.key] = box
            row.addWidget(box)
        row.addStretch(1)
        if specs:
            lay.addLayout(row)
        self.hint = QLabel()
        self.hint.setObjectName("Hint")
        self.hint.setWordWrap(True)
        lay.addWidget(self.hint)
        self._sync()
        if mode_key == "img2pdf":  # margin only matters for fixed page sizes
            page, margin = self.widgets["page"], self.widgets["margin"]
            sync = lambda: margin.setEnabled(page.currentData() != "fit")
            page.currentIndexChanged.connect(sync)
            sync()

    def _on_change(self, *_):
        self._sync()
        self.changed.emit()

    def _sync(self):
        """Show only the options that apply to the chosen action; grey out Level while a size limit is typed."""
        vals = self.values()
        for key, o in self.specs.items():
            if o.when:
                self.boxes[key].setVisible(vals.get(o.when[0]) == o.when[1])
            if o.off_if_filled:
                filled = bool(vals.get(o.off_if_filled))
                self.widgets[key].setEnabled(not filled)
                self.widgets[key].setToolTip("Not used while a target size is set." if filled else "")
        hint = self._hints
        if isinstance(hint, dict):
            hint = hint.get(vals.get("action"), "")
        self.hint.setText(hint)

    def values(self) -> dict:
        out = {}
        for key, w in self.widgets.items():
            if isinstance(w, QComboBox):
                out[key] = w.currentData()
            elif isinstance(w, QCheckBox):
                out[key] = w.isChecked()
            else:
                out[key] = w.text().strip()
        return out


# --------------------------------------------------------------------------- #
# Worker
# --------------------------------------------------------------------------- #
class Worker(QThread):
    task_started = pyqtSignal(list)
    task_done = pyqtSignal(list, object)
    task_failed = pyqtSignal(list, str)
    task_progress = pyqtSignal(float)
    all_done = pyqtSignal()

    def __init__(self, mode: str, tasks: list, out_dir: Path, opts: dict):
        super().__init__()
        self.mode, self.tasks, self.out_dir, self.opts = mode, tasks, out_dir, opts
        self.cancelled = False

    def run(self):
        cv.com_init()
        office = cv.OfficeSession()
        try:
            for ids, paths in self.tasks:
                if self.cancelled:
                    break
                self.task_started.emit(ids)
                try:
                    ctl = cv.Control(self.task_progress.emit, lambda: self.cancelled)
                    res = cv.run(self.mode, paths, self.out_dir, self.opts, office, ctl)
                except Exception as e:  # noqa: BLE001 - every failure is reported per file
                    self.task_failed.emit(ids, cv.friendly_error(e))
                else:
                    self.task_done.emit(ids, res)
        finally:
            office.close()
            cv.com_uninit()
            self.all_done.emit()


# --------------------------------------------------------------------------- #
# Main window
# --------------------------------------------------------------------------- #
class MainWindow(QWidget):
    def __init__(self):
        super().__init__()
        self.setObjectName("Root")
        self.setWindowTitle(APP_NAME)
        self.resize(1120, 820)
        self.setMinimumSize(980, 740)
        self.setAcceptDrops(True)
        self.settings = QSettings("DocShift", "DocShift")
        self.dark = self.settings.value("dark", True, type=bool)
        self.mode_key = "word2pdf"
        self.rows: list[FileRow] = []
        self._uid = 0
        self.worker: Worker | None = None
        self._task_frac = 0.0
        self._tasks_done = 0
        self._tasks_total = 0
        self._running_tasks = 0
        self._results: list[tuple[str, object]] = []
        self._focused_once = False
        self._real_progress = False
        self.engines = cv.engine_status()
        self.last_dir = str(Path.home() / "Documents")
        self._build()
        self.apply_theme()
        self.set_mode("word2pdf")
        self.tick = QTimer(self)
        self.tick.timeout.connect(self._on_tick)

    # ---- layout ----------------------------------------------------------- #
    def _build(self):
        root = QHBoxLayout(self)
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(0)
        root.addWidget(self._build_sidebar())
        root.addWidget(self._build_content(), 1)

    def _build_sidebar(self) -> QWidget:
        side = QFrame()
        side.setObjectName("Sidebar")
        side.setFixedWidth(272)
        self.sidebar = side
        lay = QVBoxLayout(side)
        lay.setContentsMargins(16, 22, 16, 16)
        lay.setSpacing(4)
        brand = QHBoxLayout()
        brand.setSpacing(12)
        logo = QLabel()
        logo.setPixmap(svg_pixmap(resource("assets/icon.svg"), 48))
        logo.setFixedSize(48, 48)
        names = QVBoxLayout()
        names.setSpacing(0)
        name = QLabel(APP_NAME)
        name.setObjectName("AppName")
        tag = QLabel("Documents ⇄ PDF, done right")
        tag.setObjectName("Tagline")
        names.addWidget(name)
        names.addWidget(tag)
        brand.addWidget(logo)
        brand.addLayout(names, 1)
        lay.addLayout(brand)
        lay.addSpacing(8)

        self.mode_items: dict[str, ModeItem] = {}
        scroll = QScrollArea()  # only scrolls on short screens; otherwise every mode is visible at once
        scroll.setWidgetResizable(True)
        scroll.setFrameShape(QFrame.NoFrame)
        scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        holder = QWidget()
        holder.setObjectName("SideList")
        hl = QVBoxLayout(holder)
        hl.setContentsMargins(0, 0, 6, 0)
        hl.setSpacing(2)
        for title, key in (("CONVERT TO PDF", "to"), ("CONVERT FROM PDF", "from"), ("COMPRESS & DECOMPRESS", "squeeze")):
            lbl = QLabel(title)
            lbl.setObjectName("SectionLabel")
            hl.addSpacing(8)
            hl.addWidget(lbl)
            hl.addSpacing(2)
            for m in (m for m in cv.MODES if m.group == key):
                item = ModeItem(m)
                item.clicked.connect(self.set_mode)
                self.mode_items[m.key] = item
                hl.addWidget(item)
        hl.addStretch(1)
        scroll.setWidget(holder)
        lay.addWidget(scroll, 1)

        st = self.engines
        if all(st["office"].values()):
            engine, ok = "Microsoft Office detected", True
        elif any(st["office"].values()):
            engine, ok = "Some Office apps missing", False
        elif st["libreoffice"]:
            engine, ok = "LibreOffice detected", True
        else:
            engine, ok = "No Office engine found", False
        foot = QHBoxLayout()
        dot = QLabel("●")
        dot.setStyleSheet(f"color: {'#34D399' if ok else '#FBBF24'}; background: transparent;")
        dot.setFixedWidth(14)
        et = QLabel(engine)
        et.setObjectName("EngineText")
        foot.addWidget(dot)
        foot.addWidget(et, 1)
        self.theme_btn = QPushButton()
        self.theme_btn.setObjectName("Theme")
        self.theme_btn.setCursor(Qt.PointingHandCursor)
        self.theme_btn.clicked.connect(self.toggle_theme)
        lay.addLayout(foot)
        lay.addSpacing(8)
        lay.addWidget(self.theme_btn)
        return side

    def _build_content(self) -> QWidget:
        content = QWidget()
        content.setObjectName("Content")
        lay = QVBoxLayout(content)
        lay.setContentsMargins(34, 28, 34, 24)
        lay.setSpacing(14)

        self.title = QLabel()
        self.title.setObjectName("Title")
        self.blurb = QLabel()
        self.blurb.setObjectName("Blurb")
        self.blurb.setWordWrap(True)
        self.banner = QLabel()
        self.banner.setObjectName("Banner")
        self.banner.setWordWrap(True)
        self.banner.hide()
        lay.addWidget(self.title)
        lay.addWidget(self.blurb)
        lay.addWidget(self.banner)

        # drop zone
        self.drop = QFrame()
        self.drop.setObjectName("DropZone")
        self.drop.setProperty("drag", False)
        self.drop.setFixedHeight(104)
        dz = QHBoxLayout(self.drop)
        dz.setContentsMargins(22, 0, 22, 0)
        dz.setSpacing(16)
        self.drop_icon = QLabel()
        self.drop_icon.setFixedSize(44, 44)
        dz.addWidget(self.drop_icon)
        txt = QVBoxLayout()
        txt.setSpacing(2)
        txt.addStretch(1)
        self.drop_title = QLabel()
        self.drop_title.setObjectName("DropTitle")
        self.drop_sub = QLabel()
        self.drop_sub.setObjectName("DropSub")
        txt.addWidget(self.drop_title)
        txt.addWidget(self.drop_sub)
        txt.addStretch(1)
        dz.addLayout(txt, 1)
        self.browse_btn = QPushButton("Browse files…")
        self.browse_btn.setObjectName("Browse")
        self.browse_btn.setCursor(Qt.PointingHandCursor)
        self.browse_btn.clicked.connect(self.browse)
        dz.addWidget(self.browse_btn)
        lay.addWidget(self.drop)

        # list
        head = QHBoxLayout()
        self.list_header = QLabel("Files")
        self.list_header.setObjectName("ListHeader")
        self.clear_btn = QPushButton("Clear all")
        self.clear_btn.setObjectName("Link")
        self.clear_btn.setCursor(Qt.PointingHandCursor)
        self.clear_btn.clicked.connect(self.clear_all)
        head.addWidget(self.list_header)
        head.addStretch(1)
        head.addWidget(self.clear_btn)
        lay.addLayout(head)

        self.scroll = QScrollArea()
        self.scroll.setWidgetResizable(True)
        self.scroll.setFrameShape(QFrame.NoFrame)
        holder = QWidget()
        self.list_lay = QVBoxLayout(holder)
        self.list_lay.setContentsMargins(0, 0, 4, 0)
        self.list_lay.setSpacing(8)
        self.empty = QLabel("No files yet - drop some above or use Browse.")
        self.empty.setObjectName("EmptyState")
        self.empty.setAlignment(Qt.AlignCenter)
        self.list_lay.addWidget(self.empty)
        self.list_lay.addStretch(1)
        self.scroll.setWidget(holder)
        lay.addWidget(self.scroll, 1)

        # options
        self.opt_card = QFrame()
        self.opt_card.setObjectName("Card")
        oc = QVBoxLayout(self.opt_card)
        oc.setContentsMargins(18, 14, 18, 14)
        oc.setSpacing(8)
        t = QLabel("Options")
        t.setObjectName("CardTitle")
        oc.addWidget(t)
        self.opt_stack = QStackedWidget()
        self.panels: dict[str, OptionsPanel] = {}
        for m in cv.MODES:
            panel = OptionsPanel(m.key)
            panel.changed.connect(self._options_changed)
            self.panels[m.key] = panel
            self.opt_stack.addWidget(panel)
        oc.addWidget(self.opt_stack)
        lay.addWidget(self.opt_card)

        # save location
        save = QFrame()
        save.setObjectName("Card")
        sl = QHBoxLayout(save)
        sl.setContentsMargins(18, 12, 18, 12)
        sl.setSpacing(12)
        sl.addWidget(self._label("Save to"))
        self.out_edit = QLineEdit()
        self.set_out_dir(downloads_dir())
        self.out_edit.setReadOnly(True)
        self.out_edit.setToolTip("Converted files are saved here. Click Change… to pick another folder.")
        sl.addWidget(self.out_edit, 1)
        self.change_btn = QPushButton("Change…")
        self.change_btn.setCursor(Qt.PointingHandCursor)
        self.change_btn.clicked.connect(self.choose_output)
        sl.addWidget(self.change_btn)
        self.open_after = QCheckBox("Open folder when done")
        self.open_after.setChecked(self.settings.value("open_after", True, type=bool))
        self.open_after.setCursor(Qt.PointingHandCursor)
        self.open_after.toggled.connect(lambda v: self.settings.setValue("open_after", v))
        sl.addWidget(self.open_after)
        lay.addWidget(save)

        # action row
        act = QHBoxLayout()
        act.setSpacing(12)
        stat = QVBoxLayout()
        stat.setSpacing(6)
        self.status_line = ElidedLabel("Add files to get started.")
        self.status_line.setObjectName("StatusLine")
        self.status_line.setProperty("state", "")
        self.progress = QProgressBar()
        self.progress.setRange(0, 1000)
        self.progress.setTextVisible(False)
        self.progress.hide()
        stat.addWidget(self.status_line)
        stat.addWidget(self.progress)
        act.addLayout(stat, 1)
        self.cancel_btn = QPushButton("Cancel")
        self.cancel_btn.setCursor(Qt.PointingHandCursor)
        self.cancel_btn.clicked.connect(self.cancel)
        self.cancel_btn.hide()
        act.addWidget(self.cancel_btn)
        self.go_btn = QPushButton("Convert && Save")
        self.go_btn.setObjectName("Primary")
        self.go_btn.setCursor(Qt.PointingHandCursor)
        self.go_btn.setDefault(True)
        self.go_btn.clicked.connect(self.start)
        act.addWidget(self.go_btn)
        lay.addLayout(act)
        return content

    @staticmethod
    def _label(text: str) -> QLabel:
        l = QLabel(text)
        l.setObjectName("CardTitle")
        return l

    # ---- theme ------------------------------------------------------------ #
    def apply_theme(self):
        p = DARK if self.dark else LIGHT
        QApplication.instance().setStyleSheet(build_qss(p, "dark" if self.dark else "light"))
        self.theme_btn.setText("☀  Switch to light" if self.dark else "☾  Switch to dark")
        self.drop_icon.setPixmap(upload_pixmap(p["accent"]))
        set_dark_titlebar(self, self.dark)

    def toggle_theme(self):
        self.dark = not self.dark
        self.settings.setValue("dark", self.dark)
        self.apply_theme()

    def showEvent(self, e):
        super().showEvent(e)
        set_dark_titlebar(self, self.dark)
        if not self._focused_once:
            self._focused_once = True
            self.browse_btn.setFocus()

    # ---- modes ------------------------------------------------------------ #
    @property
    def mode(self) -> cv.Mode:
        return cv.MODE_BY_KEY[self.mode_key]

    def set_mode(self, key: str):
        if self.worker:
            return
        self.mode_key = key
        m = self.mode
        for k, item in self.mode_items.items():
            item.set_active(k == key)
        self.blurb.setText(m.blurb)
        self.opt_stack.setCurrentWidget(self.panels[key])
        self._update_title()
        noun = KIND_NOUN[m.src]
        self.drop_title.setText(f"Drop {noun} here")
        shown = [e.lstrip(".").upper() for e in cv.KIND_EXTS[m.src] if e not in (".jfif", ".ico", ".dot", ".dotx")]
        exts = ", ".join(shown[:7]) + (" & more" if len(shown) > 7 else "")
        self.drop_sub.setText(f"or click Browse to choose them  ·  {exts}")
        stale = [r for r in self.rows if r.kind != m.src]
        for r in stale:
            self._remove_row(r, refresh=False)
        self._refresh_list()
        problem = self._engine_problem(key)
        self.banner.setVisible(bool(problem))
        self.banner.setText(problem or "")
        if stale:
            self.set_status(f"Removed {len(stale)} file(s) that don't fit {m.title}.", "warn")
        else:
            self.set_status("Add files to get started." if not self.rows else f"{len(self.rows)} file(s) ready.")

    def _action(self) -> str:
        return self.panels[self.mode_key].values().get("action", "convert")

    def _update_title(self):
        title = self.mode.title
        if self._action() == "expand":
            title = title.replace("Compress", "Decompress")
        self.title.setText(title)
        pending = sum(1 for r in self.rows if r.state in ("queued", "error"))
        verb = {"compress": "Compress", "expand": "Decompress"}.get(self._action(), "Convert")
        self.go_btn.setText(f"{verb} && Save {pending} files" if pending > 1 else f"{verb} && Save")

    def _options_changed(self):
        """New settings mean the earlier results no longer apply - let finished files be run again."""
        if self.worker:
            return
        for r in self.rows:
            if r.state == "done":
                r.set_state("queued")
        self._refresh_list()
        self._update_title()

    def _engine_problem(self, key: str) -> str | None:
        st = self.engines
        if key == "compress_video" and not st.get("ffmpeg"):
            return "The video engine (ffmpeg) wasn't found, so video compression isn't available."
        kind = {"word2pdf": "word", "excel2pdf": "excel", "ppt2pdf": "ppt", "pdf2word": "word"}.get(key)
        if kind is None or st["office"][kind]:
            return None
        name = {"word": "Word", "excel": "Excel", "ppt": "PowerPoint"}[kind]
        if key != "pdf2word" and st["libreoffice"]:
            return f"Microsoft {name} wasn't found - LibreOffice will be used instead, so layout may differ slightly."
        return f"Microsoft {name} wasn't found on this computer, so this conversion isn't available."

    # ---- files ------------------------------------------------------------ #
    def dragEnterEvent(self, e):
        if e.mimeData().hasUrls():
            e.acceptProposedAction()
            self.drop.setProperty("drag", True)
            repolish(self.drop)
            self.drop_title.setText("Release to add files")

    def dragLeaveEvent(self, e):
        self._end_drag()

    def dropEvent(self, e):
        self._end_drag()
        paths = [Path(u.toLocalFile()) for u in e.mimeData().urls() if u.isLocalFile()]
        self.add_paths(paths)

    def _end_drag(self):
        self.drop.setProperty("drag", False)
        repolish(self.drop)
        self.drop_title.setText(f"Drop {KIND_NOUN[self.mode.src]} here")

    def browse(self):
        m = self.mode
        exts = " ".join(f"*{e}" for e in cv.KIND_EXTS[m.src])
        files, _ = QFileDialog.getOpenFileNames(self, f"Select {KIND_NOUN[m.src]}", self.last_dir,
                                                f"{KIND_NOUN[m.src].capitalize()} ({exts});;All files (*.*)")
        if files:
            self.last_dir = str(Path(files[0]).parent)
            self.add_paths([Path(f) for f in files])

    def add_paths(self, paths: list[Path]):
        if self.worker:
            return
        files: list[Path] = []
        for p in paths:
            if p.is_dir():
                files.extend(sorted(c for c in p.iterdir() if c.is_file()))
            elif p.is_file():
                files.append(p)
        files = [f for f in files if cv.kind_of(f)]
        if not files:
            self.set_status("Those files aren't a supported type.", "warn")
            return
        kinds = [cv.kind_of(f) for f in files]
        want = self.mode.src
        if want not in kinds:
            # Nothing matches the current tab: jump to the tab for what was dropped, if the list is empty.
            first = kinds[0]
            if self.rows:
                self.set_status(f"Those look like {KIND_NOUN[first]}, but this tab is for {KIND_NOUN[want]}. "
                                f"Clear the list or pick another conversion.", "warn")
                return
            target = {"word": "word2pdf", "excel": "excel2pdf", "ppt": "ppt2pdf", "img": "img2pdf",
                      "pdf": "pdf2word", "video": "compress_video"}[first]
            self.set_mode(target)
            want = first
        keep = [f for f, k in zip(files, kinds) if k == want]
        skipped = len(files) - len(keep)
        existing = {r.path.resolve() for r in self.rows}
        added = dupes = 0
        for f in keep:
            if f.resolve() in existing:
                dupes += 1
                continue
            existing.add(f.resolve())
            self._uid += 1
            row = FileRow(f, want, self._uid)
            row.removeRequested.connect(self._remove_row)
            row.moveRequested.connect(self._move_row)
            self.list_lay.insertWidget(self.list_lay.count() - 1, row)
            self.rows.append(row)
            added += 1
        self._refresh_list()
        bits = [f"Added {added} file(s)."] if added else []
        if dupes:
            bits.append(f"{dupes} already in the list.")
        if skipped:
            bits.append(f"Skipped {skipped} that don't match this conversion.")
        self.set_status(" ".join(bits) or "Nothing new to add.", "warn" if skipped and not added else "")

    def _remove_row(self, row: FileRow, refresh: bool = True):
        if row in self.rows:
            self.rows.remove(row)
        row.setParent(None)
        row.deleteLater()
        if refresh:
            self._refresh_list()
            if not self.rows:
                self.set_status("Add files to get started.")

    def _move_row(self, row: FileRow, delta: int):
        i = self.rows.index(row)
        j = i + delta
        if not 0 <= j < len(self.rows):
            return
        self.rows[i], self.rows[j] = self.rows[j], self.rows[i]
        self.list_lay.removeWidget(row)
        self.list_lay.insertWidget(j, row)
        self._refresh_list()

    def clear_all(self):
        if self.worker:
            return
        for r in list(self.rows):
            self._remove_row(r, refresh=False)
        self._refresh_list()
        self.set_status("Add files to get started.")

    def _refresh_list(self):
        n = len(self.rows)
        self.empty.setVisible(n == 0)
        self.list_header.setText(f"Files ({n})" if n else "Files")
        self.clear_btn.setVisible(n > 0)
        reorder = self.mode_key == "img2pdf" and n > 1
        for i, r in enumerate(self.rows):
            r.up.setVisible(reorder and i > 0)
            r.down.setVisible(reorder and i < n - 1)
        self.drop.setFixedHeight(84 if n else 104)  # compact once files are listed, leaving more room for them
        pending = sum(1 for r in self.rows if r.state in ("queued", "error"))
        self.go_btn.setEnabled(pending > 0 and not self.worker)
        if hasattr(self, "title"):
            self._update_title()

    def set_status(self, text: str, state: str = ""):
        self.status_line.setText(text)
        self.status_line.setToolTip(text)
        self.status_line.setProperty("state", state)
        repolish(self.status_line)

    # ---- output ----------------------------------------------------------- #
    def set_out_dir(self, path: Path):
        self.out_edit.setText(str(path))
        self.out_edit.setCursorPosition(0)  # show the start of the path, not the tail

    def choose_output(self):
        d = QFileDialog.getExistingDirectory(self, "Choose where to save converted files", self.out_edit.text())
        if d:
            self.set_out_dir(Path(d))

    def _output_dir(self) -> Path | None:
        out = Path(self.out_edit.text())
        try:
            out.mkdir(parents=True, exist_ok=True)
            probe = out / f".docshift_{uuid.uuid4().hex}"
            probe.write_bytes(b"")
            probe.unlink()
            return out
        except OSError:
            QMessageBox.warning(self, APP_NAME, f"Can't save to:\n{out}\n\nThe folder is missing or read-only. "
                                                f"Pick another one with Change….")
            return None

    # ---- run -------------------------------------------------------------- #
    def start(self):
        if self.worker:
            return
        pending = [r for r in self.rows if r.state in ("queued", "error")]
        if not pending:
            return
        out_dir = self._output_dir()
        if out_dir is None:
            return
        opts = self.panels[self.mode_key].values()
        if self.mode_key == "pdf2img" and opts.get("pages"):
            try:  # fail fast on a typo instead of after the first file
                cv.parse_pages(opts["pages"], 10 ** 6)
            except cv.ConversionError as e:
                QMessageBox.warning(self, APP_NAME, str(e))
                return
        if self.mode_key.startswith("compress_"):
            try:  # fail fast on a typo like "2mb" instead of after the first file
                compressors.parse_target(opts.get("target_mb") if opts.get("action") == "compress" else "")
            except cv.ConversionError as e:
                QMessageBox.warning(self, APP_NAME, str(e))
                return
            if opts.get("action") == "expand" and self.mode_key == "compress_video" and QMessageBox.question(
                    self, APP_NAME, "Decompressed (lossless) video is often 10-50 times larger than the original "
                                    "and can fill your disk quickly.\n\nContinue?") != QMessageBox.Yes:
                return
        if self.mode_key == "img2pdf" and opts.get("merge") and len(pending) > 1:
            tasks = [([r.uid for r in pending], [r.path for r in pending])]
        else:
            tasks = [([r.uid], [r.path]) for r in pending]
        for r in pending:
            r.set_state("queued")
        self._results = []
        self._tasks_total, self._tasks_done, self._task_frac, self._running_tasks = len(tasks), 0, 0.0, 0
        self.worker = Worker(self.mode_key, tasks, out_dir, opts)
        self.worker.task_started.connect(self._on_started)
        self.worker.task_done.connect(self._on_done)
        self.worker.task_failed.connect(self._on_failed)
        self.worker.task_progress.connect(self._on_progress)
        self.worker.all_done.connect(self._on_all_done)
        self._set_running(True)
        self.set_status(f"{self._verb()}… 0 of {len(tasks)}")
        self.worker.start()
        self.tick.start(80)

    def _set_running(self, on: bool):
        self.sidebar.setEnabled(not on)
        self.browse_btn.setEnabled(not on)
        self.change_btn.setEnabled(not on)
        self.clear_btn.setEnabled(not on)
        self.opt_card.setEnabled(not on)
        self.go_btn.setVisible(not on)
        self.cancel_btn.setVisible(on)
        self.cancel_btn.setEnabled(True)
        self.progress.setVisible(on)
        if on:
            self.progress.setValue(0)

    def cancel(self):
        if self.worker:
            self.worker.cancelled = True
            self.cancel_btn.setEnabled(False)
            self.set_status("Cancelling…", "warn")

    def _verb(self) -> str:
        return {"compress": "Compressing", "expand": "Decompressing"}.get(self._action(), "Converting")

    def _row(self, uid: int) -> FileRow | None:
        return next((r for r in self.rows if r.uid == uid), None)

    def _on_progress(self, fraction: float):
        if fraction > 0:  # real progress from the engine takes over from the eased estimate
            self._real_progress = True
            self._task_frac = max(self._task_frac, min(fraction, 1.0))
            self._update_bar()

    def _on_started(self, ids: list):
        self._task_frac = 0.0
        self._real_progress = False
        for uid in ids:
            r = self._row(uid)
            if r:
                r.set_state("running", f"{self._verb()}…")
        self.set_status(f"{self._verb()}… {self._tasks_done + 1} of {self._tasks_total}")

    def _on_done(self, ids: list, res: cv.Result):
        self._tasks_done += 1
        self._task_frac = 0.0
        self._real_progress = False
        show = res.reveal
        for uid in ids:
            r = self._row(uid)
            if r:
                label = "Saved" + (f" · {res.outputs[0].name}" if len(ids) == 1 and res.outputs else "")
                if len(ids) > 1:
                    label = f"Merged into {res.outputs[0].name}"
                if res.detail:
                    label = res.detail  # e.g. "12.40 MB -> 1.98 MB (-84%)"
                tip = "\n".join(x for x in (res.outputs[0].name, res.note, str(show)) if x)
                r.set_state("done", label, tip=tip, reveal_path=show)
        self._results.append(("ok", res))
        self._update_bar()

    def _on_failed(self, ids: list, message: str):
        self._tasks_done += 1
        self._task_frac = 0.0
        self._real_progress = False
        if message == "Cancelled":  # stopped by the user, not a failure
            for uid in ids:
                r = self._row(uid)
                if r:
                    r.set_state("queued")
            self._update_bar()
            return
        for uid in ids:
            r = self._row(uid)
            if r:
                r.set_state("error", message, tip=message)
        self._results.append(("err", message))
        self._update_bar()

    def _on_tick(self):
        if self.worker and self._tasks_done < self._tasks_total and not self._real_progress:
            self._task_frac += (0.92 - self._task_frac) * 0.035  # eases toward 92% while a file is being converted
            self._update_bar()

    def _update_bar(self):
        if self._tasks_total:
            self.progress.setValue(int((self._tasks_done + self._task_frac) / self._tasks_total * 1000))

    def _on_all_done(self):
        self.tick.stop()
        cancelled = bool(self.worker and self.worker.cancelled)
        self.worker.wait()
        self.worker = None
        self._set_running(False)
        for r in self.rows:  # anything cancelled before it started goes back to "Ready"
            if r.state == "running":
                r.set_state("queued")
        ok = [x for kind, x in self._results if kind == "ok"]
        bad = [x for kind, x in self._results if kind == "err"]
        out = self.out_edit.text()
        self.progress.setValue(1000 if not cancelled else self.progress.value())
        n_files = sum(len(x.outputs) for x in ok)
        if ok and not bad:
            what = "1 file" if n_files == 1 else f"{n_files} files"
            self.set_status(f"Done - {what} saved to {out}", "ok")
        elif ok and bad:
            self.set_status(f"{len(ok)} converted, {len(bad)} failed - see the red rows for the reason.", "warn")
        elif bad:
            self.set_status("Conversion failed - see the message on the file.", "err")
        if cancelled:
            self.set_status(f"Cancelled. {len(ok)} converted before stopping.", "warn")
        notes = [x.note for x in ok if x.note]
        if notes and not bad and not cancelled:
            self.set_status(self.status_line.fullText() + "  ·  " + notes[0], "ok")
        self._refresh_list()
        if ok and self.open_after.isChecked() and not cancelled:
            reveal(ok[-1].reveal)

    def closeEvent(self, e):
        if self.worker:
            if QMessageBox.question(self, APP_NAME, "A conversion is still running. Stop it and quit?") \
                    != QMessageBox.Yes:
                e.ignore()
                return
            self.worker.cancelled = True
            self.worker.wait(20000)
        e.accept()


def selftest(outdir: Path) -> int:
    """`DocShift.exe --selftest <folder>`: run one tiny conversion per engine and write selftest.log.
    Used to prove a packaged build carries every dependency (the windowed exe has no console to print to)."""
    from PIL import Image

    outdir.mkdir(parents=True, exist_ok=True)
    log: list[str] = []
    ok_all = True
    src = outdir / "st_input"
    src.mkdir(exist_ok=True)
    Image.new("RGB", (400, 300), (40, 110, 220)).save(src / "sample.png")
    (src / "sample.rtf").write_text(r"{\rtf1\ansi \b DocShift\b0  self-test paragraph.\par Second line.}",
                                    encoding="ascii")
    (src / "sample.csv").write_text("a,b,c\n1,2,3\n4,5,6\n7,8,9\n", encoding="ascii")
    ff = cv.ffmpeg_path()
    if ff:  # a 2-second test clip, made with the same bundled ffmpeg the app uses
        import subprocess
        subprocess.run([ff, "-y", "-loglevel", "error", "-f", "lavfi", "-i", "testsrc2=size=640x360:rate=24",
                        "-f", "lavfi", "-i", "sine=frequency=440", "-t", "2", "-pix_fmt", "yuv420p",
                        str(src / "sample.mp4")], stdin=subprocess.DEVNULL,
                       creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
    cv.com_init()
    office = cv.OfficeSession()
    steps = [("img2pdf", src / "sample.png", {"page": "fit"}), ("word2pdf", src / "sample.rtf", {}),
             ("excel2pdf", src / "sample.csv", {"fit_width": True})]
    made: dict[str, Path] = {}
    squeeze = {"action": "compress", "target_mb": "0.05"}
    try:
        for mode, path, opts in steps + [("pdf2img", None, {"format": "webp", "dpi": 72}),
                                         ("pdf2excel", None, {}), ("pdf2ppt", None, {"style": "editable"}),
                                         ("pdf2word", None, {}),
                                         ("compress_img", src / "sample.png", {**squeeze, "target_mb": "0.01"}),
                                         ("compress_pdf", None, {**squeeze, "target_mb": ""}),
                                         ("compress_video", src / "sample.mp4", {**squeeze, "target_mb": "0.1"}),
                                         ("compress_video", src / "sample.mp4", {"action": "expand"})]:
            try:
                if path is None:
                    path = made["word2pdf"]
                res = cv.run(mode, [path], outdir, opts, office)
                made[mode] = res.outputs[0]
                log.append(f"OK    {mode:14} -> {res.outputs[0].name}")
            except Exception as e:  # noqa: BLE001
                ok_all = False
                log.append(f"FAIL  {mode:14} -> {cv.friendly_error(e)}")
    finally:
        office.close()
        cv.com_uninit()
    (outdir / "selftest.log").write_text("\n".join(log) + ("\nALL OK" if ok_all else "\nFAILED") + "\n",
                                         encoding="utf-8")
    return 0 if ok_all else 1


def main():
    if len(sys.argv) > 2 and sys.argv[1] == "--selftest":
        sys.exit(selftest(Path(sys.argv[2])))
    try:
        ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID(APP_ID)  # own taskbar identity + icon
    except Exception:
        pass
    QApplication.setAttribute(Qt.AA_EnableHighDpiScaling, True)
    QApplication.setAttribute(Qt.AA_UseHighDpiPixmaps, True)
    app = QApplication(sys.argv)
    app.setApplicationName(APP_NAME)
    app.setFont(QFont("Segoe UI", 10))
    app.setWindowIcon(svg_icon(resource("assets/icon.svg")))
    win = MainWindow()
    win.show()
    sys.exit(app.exec_())


if __name__ == "__main__":
    main()
