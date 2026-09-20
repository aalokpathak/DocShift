"""Build DocShift.exe.

    .venv\Scripts\python build.py

Steps: render assets/icon.svg -> assets/icon.ico (16-256 px), then run PyInstaller in one-file windowed mode.
The result is dist\DocShift.exe.
"""
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SVG = ROOT / "assets" / "icon.svg"
ICO = ROOT / "assets" / "icon.ico"


def make_icon():
    from PIL import Image
    from PyQt5.QtCore import QBuffer, QIODevice
    from PyQt5.QtGui import QGuiApplication, QImage, QPainter
    from PyQt5.QtSvg import QSvgRenderer

    app = QGuiApplication([])  # noqa: F841 - Qt needs an application object to render
    img = QImage(256, 256, QImage.Format_ARGB32)
    img.fill(0)
    painter = QPainter(img)
    painter.setRenderHint(QPainter.Antialiasing)
    QSvgRenderer(str(SVG)).render(painter)
    painter.end()
    buf = QBuffer()
    buf.open(QIODevice.WriteOnly)
    img.save(buf, "PNG")
    import io
    base = Image.open(io.BytesIO(bytes(buf.data()))).convert("RGBA")
    base.save(ICO, format="ICO", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    print(f"wrote {ICO.name}")


def main():
    make_icon()
    cmd = [
        sys.executable, "-m", "PyInstaller", "--noconfirm", "--clean", "--onefile", "--windowed",
        "--name", "DocShift", "--icon", str(ICO),
        "--add-data", f"{SVG};assets",
        "--exclude-module", "tkinter", "--exclude-module", "unittest", "--exclude-module", "pydoc",
        "--exclude-module", "PyQt5.QtWebEngine", "--exclude-module", "PyQt5.QtNetwork",
        "--exclude-module", "PyQt5.QtQml", "--exclude-module", "PyQt5.QtQuick", "--exclude-module", "PyQt5.QtSql",
        "--exclude-module", "PyQt5.QtMultimedia", "--exclude-module", "PyQt5.QtBluetooth",
        str(ROOT / "app.py"),
    ]
    subprocess.check_call(cmd, cwd=ROOT)
    print("\nBuilt:", ROOT / "dist" / "DocShift.exe")


if __name__ == "__main__":
    main()
