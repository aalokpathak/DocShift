# Third-party notices

DocShift is built on the work of many open-source projects. The most significant ones:

## Bundled in the release executables

| Component | Used for | License |
|---|---|---|
| [FFmpeg](https://ffmpeg.org/) (the *essentials* build from [gyan.dev](https://www.gyan.dev/ffmpeg/builds/), includes x264) | Video compression and decompression, decoding HEIC / AVIF photos | GNU GPL v3. DocShift starts it as a separate program; it is not linked into DocShift's code. Source code and build information are available from the links above. |
| [PDFium](https://pdfium.googlesource.com/pdfium/) via [pdfium-binaries](https://github.com/bblanchon/pdfium-binaries) | Rendering PDF pages, reading PDF text | BSD-3-Clause / Apache-2.0 (see `src-tauri/binaries/PDFIUM-LICENSE.txt`) |
| [Tauri](https://tauri.app/) 2 and its plugins | Application shell | MIT / Apache-2.0 |
| [React](https://react.dev/) 19, [Zustand](https://github.com/pmndrs/zustand) | User interface | MIT |
| [Tailwind CSS](https://tailwindcss.com/) 4 | Styling | MIT |
| [Lucide](https://lucide.dev/) | Icons | ISC |
| [IBM Plex Sans / Mono](https://github.com/IBM/plex) via Fontsource | Interface typeface | SIL Open Font License 1.1 |

## Rust crates (statically linked)

[lopdf](https://crates.io/crates/lopdf) (MIT), [pdfium-render](https://crates.io/crates/pdfium-render) (MIT / Apache-2.0),
[image](https://crates.io/crates/image) (MIT / Apache-2.0), [jpeg-encoder](https://crates.io/crates/jpeg-encoder) (MIT / Apache-2.0),
[webp](https://crates.io/crates/webp) (MIT; libwebp is BSD-3-Clause), [png](https://crates.io/crates/png) (MIT / Apache-2.0),
[color_quant](https://crates.io/crates/color_quant) (MIT), [tiff](https://crates.io/crates/tiff) (MIT), [kamadak-exif](https://crates.io/crates/kamadak-exif) (BSD-2-Clause),
[rust_xlsxwriter](https://crates.io/crates/rust_xlsxwriter) (MIT / Apache-2.0), [zip](https://crates.io/crates/zip) (MIT),
[zstd](https://crates.io/crates/zstd) (MIT), [rayon](https://crates.io/crates/rayon) (MIT / Apache-2.0),
[windows](https://crates.io/crates/windows) (MIT / Apache-2.0), [serde](https://crates.io/crates/serde) (MIT / Apache-2.0),
[regex](https://crates.io/crates/regex) (MIT / Apache-2.0), [anyhow](https://crates.io/crates/anyhow) (MIT / Apache-2.0).

## Not bundled, used when present

Microsoft Word, Excel and PowerPoint (driven through COM to export Office documents faithfully) and LibreOffice (fallback).
Text recognition uses the OCR engine that is part of Windows 10/11 (Windows.Media.Ocr) and its installed language packs.
These are not distributed with DocShift and remain subject to their own licenses.

## Redistribution

Because the executables include a GPL build of FFmpeg, redistributing them means honouring that license for FFmpeg
(provide its license text and a way to obtain its source). The links in the table above are that pointer.
