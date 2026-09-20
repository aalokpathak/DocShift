<div align="center">

<img src="assets/icon.svg" width="120" alt="DocShift logo">

# DocShift

**Convert Word, Excel, PowerPoint and images to PDF — and PDF back again — with the layout intact.**

[![Download](https://img.shields.io/badge/Download-DocShift.exe-6366F1?style=for-the-badge&logo=windows&logoColor=white)](https://github.com/aalokpathak/DocShift/releases/latest/download/DocShift.exe)

![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11-blue)
![Python](https://img.shields.io/badge/python-3.10%2B-3776AB)
![UI](https://img.shields.io/badge/UI-PyQt5-41CD52)

<img src="docs/screenshot-dark.png" width="820" alt="DocShift, dark theme">

</div>

## Download

Grab **[DocShift.exe](https://github.com/aalokpathak/DocShift/releases/latest/download/DocShift.exe)** from the
[Releases page](https://github.com/aalokpathak/DocShift/releases/latest). It is a single portable file: no installer,
no Python needed. Just double-click it.

> Windows may show a SmartScreen prompt the first time because the exe isn't code-signed. Choose
> **More info → Run anyway**.

## What it converts

| To PDF | From PDF |
|---|---|
| Word → PDF (`.doc` `.docx` `.rtf` `.odt` `.txt`) | PDF → Word (editable `.docx`) |
| Excel → PDF (`.xls` `.xlsx` `.csv`) | PDF → Excel (tables become real cells) |
| PowerPoint → PDF (`.ppt` `.pptx` `.odp`) | PDF → PowerPoint (editable text, or exact page images) |
| Images → PDF (JPG, PNG, WEBP, BMP, GIF, TIFF) | PDF → Images (PNG, JPG, WEBP, TIFF, BMP, 72–600 dpi) |

## Features

- **Faithful layout.** Word, Excel and PowerPoint are exported by Microsoft Office itself, the same engine as
  *Save as PDF*, so fonts, headers, footers, tables, charts and images match the original.
- **Browse or drag & drop**, one file or many. The app switches to the right conversion when you drop a file type.
- **Choose where to save.** Defaults to your Downloads folder; change it to any folder. Existing files are never
  overwritten (` (1)`, ` (2)`, …).
- **Smart options per conversion.** Fit wide Excel sheets to one page, merge and reorder images into a single PDF,
  pick image format / DPI / page range, and more.
- **Per-file status**, progress bar and a Cancel button. One bad file never stops the batch.
- **Light and dark themes**, keyboard accessible.
- **Safe by default.** Macros in opened Office files are always disabled; password-protected files are detected
  instantly and reported instead of hanging.

<div align="center">
<img src="docs/screenshot-light.png" width="820" alt="DocShift, light theme, Images to PDF">
</div>

## How it works

| Conversion | Engine |
|---|---|
| Word / Excel / PowerPoint → PDF | Microsoft Office via COM automation. Falls back to LibreOffice if Office isn't installed. |
| Images → PDF | Pillow + PyMuPDF. JPG/PNG are embedded untouched (no re-compression). |
| PDF → Word | Word's PDF reflow engine. |
| PDF → Excel | PyMuPDF table detection plus a custom detector for tables without ruling lines; numbers stay numbers. |
| PDF → PowerPoint | PyMuPDF + python-pptx. *Editable* mode rebuilds text as real text boxes over a clean background image. |
| PDF → Images | PyMuPDF rendering. |

## Requirements

- Windows 10 / 11
- **Microsoft Office** (Word, Excel, PowerPoint) for the best results. Word is required for *PDF → Word*.
  Image conversions and PDF → Excel / PowerPoint / Images work without Office.

## Limitations

- **Scanned PDFs have no text layer and there is no OCR**, so they convert to page images only.
- PDF → Word / Excel / PowerPoint is inherently approximate, because a PDF stores positioned glyphs, not paragraphs
  or cells. Office → PDF is exact.
- Password-protected files must be unlocked first.
- Windows only (Office automation uses COM).

## Run from source / build the exe

```bash
git clone https://github.com/aalokpathak/DocShift.git
cd DocShift
python -m venv .venv
.venv\Scripts\pip install -r requirements.txt

.venv\Scripts\python app.py          # run
.venv\Scripts\python build.py        # build dist\DocShift.exe (also generates icon.ico from assets/icon.svg)
```

`DocShift.exe --selftest <folder>` runs one conversion per engine and writes `selftest.log`, which is handy for
checking a build.

## Project layout

```
app.py          GUI (PyQt5): window, drag & drop, options, worker thread
converters.py   all conversion logic, no GUI code
assets/         icon.svg (source of the app icon)
build.py        icon generation + PyInstaller build
docs/           screenshots
```

## Built with

Python · PyQt5 · PyMuPDF · Pillow · python-pptx · openpyxl · pywin32 · PyInstaller
