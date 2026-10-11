"""DocShift compression / expansion engines for PDF, images and video. No GUI code.

Every engine accepts an optional size limit ("target"). With a target the engine searches for the best quality that
still fits under it; without one it uses the chosen compression level. Sizes use 1 MB = 1,000,000 bytes, which is
the stricter convention, so a file that fits is accepted by sites using either definition.

"Decompress" cannot bring back detail that lossy compression threw away. It rewrites the file in an uncompressed or
lossless form (useful for editing, inspecting or re-processing) and the result is larger.
"""
from __future__ import annotations

import io
import math
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

import converters as cv
from converters import ConversionError, Control, Result

MB = 1_000_000
_NOWIN = getattr(subprocess, "CREATE_NO_WINDOW", 0)


# --------------------------------------------------------------------------- #
# Shared helpers
# --------------------------------------------------------------------------- #
def fmt_size(n: float) -> str:
    for unit in ("B", "KB", "MB", "GB"):
        if n < 1000 or unit == "GB":
            return f"{n:.0f} {unit}" if unit == "B" else f"{n:.2f} MB" if unit == "MB" else f"{n:.1f} {unit}"
        n /= 1000
    return f"{n} B"


def parse_target(text) -> int | None:
    """'2' / '2.5' / '2,5' -> bytes. Blank -> None (no size limit)."""
    t = str(text or "").strip().replace(",", ".")
    if not t:
        return None
    try:
        value = float(t)
    except ValueError:
        raise ConversionError("The size limit must be a number of MB, for example 2 or 0.5.")
    if value <= 0:
        raise ConversionError("The size limit must be greater than zero.")
    if value < 0.005:
        raise ConversionError("That size limit is too small to be useful.")
    return int(value * MB)


def _detail(before: int, after: int) -> str:
    pct = f" ({(after / before - 1) * 100:+.0f}%)" if before else ""
    return f"{fmt_size(before)} → {fmt_size(after)}{pct}"


def _deliver(src: Path, produced: Path, out_dir: Path, tag: str, ext: str, note: str = "",
             keep_if_larger: bool = True) -> Result:
    """Move the produced file next to the others as '<name>_<tag><ext>'.

    If compression made the file no smaller, hand back an unchanged copy instead of something worse."""
    before, after = src.stat().st_size, produced.stat().st_size
    if keep_if_larger and after >= before:
        shutil.copyfile(src, produced)
        after, ext = before, src.suffix
        note = (note + " " if note else "") + "It was already well compressed, so an unchanged copy was saved."
    out = cv._publish(produced, out_dir, f"{src.stem}_{tag}", ext)
    return Result([out], note, _detail(before, after))


def _already_under(src: Path, target: int | None, out_dir: Path, tag: str) -> Result | None:
    if target is not None and src.stat().st_size <= target:
        out = cv.unique_path(out_dir, f"{src.stem}_{tag}", src.suffix)
        out_dir.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(src, out)
        size = src.stat().st_size
        return Result([out], f"Already under {fmt_size(target)}, so it was left unchanged.", _detail(size, size))
    return None


def _too_big(target: int, got: int, what: str = "") -> ConversionError:
    return ConversionError(f"Couldn't get {what}under {fmt_size(target)} - the smallest result was {fmt_size(got)}. "
                           f"Try a larger limit.")


def _best_quality(encode, target: int, lo: int, hi: int, ctl: Control, max_iter: int = 7):
    """Find the highest quality in lo..hi whose output still fits `target` bytes.

    Returns (quality, data) on success, or (None, data_at_lo) when even `lo` is too big. File size grows smoothly with
    quality, so interpolating on log(size) needs about half the encodes of a plain bisection."""
    low = encode(lo)
    if len(low) > target:
        return None, low
    top = encode(hi)
    if len(top) <= target:
        return hi, top
    best_q, best = lo, low
    s_lo, s_hi = len(low), len(top)
    for _ in range(max_iter):
        if hi - lo <= 1 or best_q and len(best) >= 0.97 * target:
            break
        ctl.check()
        frac = (math.log(target) - math.log(s_lo)) / (math.log(s_hi) - math.log(s_lo)) if s_hi > s_lo else 0.5
        q = min(max(round(lo + frac * (hi - lo)), lo + 1), hi - 1)
        data = encode(q)
        if len(data) <= target:
            lo, s_lo, best_q, best = q, len(data), q, data
        else:
            hi, s_hi = q, len(data)
    return best_q, best


def _shrink_factor(target: int, size: int) -> float:
    """How much to scale each side so a file of `size` bytes lands just under `target` (size ~ pixel count)."""
    return min(0.92, math.sqrt(target / size) * 0.97)


# --------------------------------------------------------------------------- #
# Images
# --------------------------------------------------------------------------- #
_IMG_Q = {"light": 88, "balanced": 75, "strong": 55}
_IMG_MAX_SIDE = {"light": None, "balanced": 3000, "strong": 2000}
_PNG_COLORS = {"light": None, "balanced": 256, "strong": 64}
_SCALES = [1.0, 0.85, 0.7, 0.55, 0.42, 0.3, 0.2, 0.12]
_FMT_EXT = {"JPEG": ".jpg", "PNG": ".png", "WEBP": ".webp"}


def _has_alpha(im) -> bool:
    return im.mode in ("RGBA", "LA", "PA") or (im.mode == "P" and "transparency" in im.info)


def _flatten(im):
    from PIL import Image
    if _has_alpha(im):
        rgba = im.convert("RGBA")
        bg = Image.new("RGB", rgba.size, (255, 255, 255))
        bg.paste(rgba, mask=rgba.getchannel("A"))
        return bg
    return im.convert("RGB")


def _encode(im, fmt: str, quality: int | None = None, colors: int | None = None, icc=None) -> bytes:
    from PIL import Image
    buf = io.BytesIO()
    extra = {"icc_profile": icc} if icc else {}
    if fmt == "JPEG":
        _flatten(im).save(buf, "JPEG", quality=quality or 75, optimize=True, progressive=True, **extra)
    elif fmt == "WEBP":
        out = im.convert("RGBA") if _has_alpha(im) else im.convert("RGB")
        out.save(buf, "WEBP", quality=quality or 75, method=4, **extra)
    else:  # PNG
        out = im.convert("RGBA") if _has_alpha(im) else im.convert("RGB")
        if colors:
            method = Image.Quantize.FASTOCTREE if out.mode == "RGBA" else Image.Quantize.MEDIANCUT
            out = out.quantize(colors, method=method)
        out.save(buf, "PNG", optimize=True, **extra)
    return buf.getvalue()


def _scaled(im, scale: float):
    from PIL import Image
    if scale >= 1:
        return im
    return im.resize((max(1, round(im.width * scale)), max(1, round(im.height * scale))), Image.LANCZOS)


def _open_image(src: Path):
    from PIL import Image, ImageOps
    try:
        im = Image.open(src)
        im.load()
    except Exception:
        raise ConversionError("This file could not be opened as an image. It may be damaged.")
    if getattr(im, "n_frames", 1) > 1:
        raise ConversionError("Animated or multi-page images aren't supported here.")
    fmt = im.format
    icc = im.info.get("icc_profile")
    return ImageOps.exif_transpose(im), fmt, icc


def compress_image(src: Path, out_dir: Path, opts: dict, ctl: Control) -> Result:
    target = parse_target(opts.get("target_mb"))
    if (done := _already_under(src, target, out_dir, "compressed")):
        return done
    level = opts.get("level", "balanced")
    im, in_fmt, icc = _open_image(src)
    choice = opts.get("format", "same")
    fmt = {"jpg": "JPEG", "webp": "WEBP", "png": "PNG"}.get(choice) or (in_fmt if in_fmt in _FMT_EXT else "JPEG")

    if target is None:
        cap = _IMG_MAX_SIDE[level]
        if cap and max(im.size) > cap:
            im = _scaled(im, cap / max(im.size))
        data = _encode(im, fmt, _IMG_Q[level], _PNG_COLORS[level] if fmt == "PNG" else None, icc)
        note = ""
    else:
        data, note = _fit_image(im, fmt, target, icc, ctl, src.stat().st_size)

    tmp = Path(tempfile.mkdtemp(prefix="docshift_"))
    try:
        produced = tmp / "out"
        produced.write_bytes(data)
        return _deliver(src, produced, out_dir, "compressed", _FMT_EXT[fmt], note)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def _fit_image(im, fmt: str, target: int, icc, ctl: Control, src_size: int = 0) -> tuple[bytes, str]:
    """Keep as much resolution and quality as possible under `target` bytes.

    Quality is searched first; the picture is only shrunk when even a decent minimum quality doesn't fit."""
    scale = 1.0
    size = target
    for _ in range(10):
        ctl.check()
        work = _scaled(im, scale)
        if min(work.size) < 16:
            break
        if fmt == "PNG":
            data, colors = None, None
            ladder = ([None] if src_size and target >= 0.6 * src_size else []) + [256, 128, 64]
            for colors in ladder:
                ctl.check()
                data = _encode(work, "PNG", colors=colors, icc=icc)
                if len(data) <= target:
                    return data, _scale_note(scale, colors)
            size = len(data)
        else:
            q, data = _best_quality(lambda q: _encode(work, fmt, q, icc=icc), target, 35, 95, ctl)
            if q is not None:
                return data, _scale_note(scale)
            size = len(data)
        scale *= _shrink_factor(target, size)
    raise _too_big(target, size, "this image ")


def _scale_note(scale: float, colors: int | None = None) -> str:
    bits = []
    if scale < 1:
        bits.append(f"resized to {scale * 100:.0f}% to fit")
    if colors:
        bits.append(f"reduced to {colors} colours")
    return ("Image was " + " and ".join(bits) + ".") if bits else ""


def expand_image(src: Path, out_dir: Path, opts: dict, ctl: Control) -> Result:
    im, _fmt, icc = _open_image(src)
    fmt = opts.get("expand_format", "png")
    ext = {"png": ".png", "tiff": ".tiff", "bmp": ".bmp"}[fmt]
    tmp = Path(tempfile.mkdtemp(prefix="docshift_"))
    try:
        produced = tmp / "out"
        extra = {}
        if fmt == "png":
            extra = dict(format="PNG", compress_level=6)
            out = im if im.mode in ("RGB", "RGBA", "L", "LA") else im.convert("RGBA" if _has_alpha(im) else "RGB")
        elif fmt == "tiff":
            extra = dict(format="TIFF", compression=None)
            out = im if im.mode in ("RGB", "RGBA", "L") else im.convert("RGBA" if _has_alpha(im) else "RGB")
        else:
            extra = dict(format="BMP")
            out = _flatten(im)
        out.save(produced, **extra)
        return _deliver(src, produced, out_dir, "expanded", ext,
                        "Detail already lost to earlier compression can't be recovered.", keep_if_larger=False)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


# --------------------------------------------------------------------------- #
# PDF
# --------------------------------------------------------------------------- #
# (rewrite images above this dpi, down to this dpi, at this JPEG quality) for the three presets
_PDF_PRESETS = {"light": (250, 200, 85), "balanced": (200, 150, 70), "strong": (150, 110, 50)}
# With a size limit: try these image resolutions from sharpest to coarsest, searching JPEG quality in each
_PDF_DPI_TIERS = [250, 200, 150, 110, 80, 60, 45]


def _pdf_bytes(src: Path, step: tuple[int, int, int]) -> bytes:
    import pymupdf
    thr, dpi, quality = step
    doc = pymupdf.open(str(src))
    try:
        try:
            doc.rewrite_images(dpi_threshold=thr, dpi_target=dpi, quality=quality)
        except Exception:
            pass  # a PDF without usable images still benefits from the clean-up below
        try:
            doc.subset_fonts()
        except Exception:
            pass
        try:
            return doc.tobytes(garbage=4, deflate=True, deflate_images=True, deflate_fonts=True, use_objstms=1)
        except TypeError:
            return doc.tobytes(garbage=4, deflate=True)
    finally:
        doc.close()


def _pdf_flattened(src: Path, dpi: int, quality: int) -> bytes:
    """Last resort: every page becomes a JPEG. Text stops being selectable, but the size drops a lot."""
    import pymupdf
    old, new = pymupdf.open(str(src)), pymupdf.open()
    try:
        for page in old:
            pix = page.get_pixmap(dpi=dpi, alpha=False)
            out = new.new_page(width=page.rect.width, height=page.rect.height)
            out.insert_image(out.rect, stream=pix.tobytes("jpeg", jpg_quality=quality))
        return new.tobytes(garbage=4, deflate=True)
    finally:
        old.close()
        new.close()


def compress_pdf(src: Path, out_dir: Path, opts: dict, ctl: Control) -> Result:
    cv._open_pdf(src).close()  # clear errors for damaged / password-protected files
    target = parse_target(opts.get("target_mb"))
    if (done := _already_under(src, target, out_dir, "compressed")):
        return done
    note = ""
    if target is None:
        data = _pdf_bytes(src, _PDF_PRESETS[opts.get("level", "balanced")])
    else:
        data, smallest = None, None
        for n, dpi in enumerate(_PDF_DPI_TIERS):
            ctl.check()
            ctl.report(n / (len(_PDF_DPI_TIERS) + 1))
            q, candidate = _best_quality(lambda q: _pdf_bytes(src, (int(dpi * 1.2), dpi, q)), target, 35, 90, ctl, 5)
            smallest = len(candidate) if smallest is None else min(smallest, len(candidate))
            if q is not None:
                data = candidate
                break
        if data is None:
            for dpi, quality in ((130, 60), (100, 50), (80, 45)):
                ctl.check()
                candidate = _pdf_flattened(src, dpi, quality)
                smallest = min(smallest, len(candidate))
                if len(candidate) <= target:
                    data = candidate
                    note = "Pages were converted to images to fit, so the text is no longer selectable."
                    break
        if data is None:
            raise _too_big(target, smallest)
    tmp = Path(tempfile.mkdtemp(prefix="docshift_"))
    try:
        produced = tmp / "out.pdf"
        produced.write_bytes(data)
        return _deliver(src, produced, out_dir, "compressed", ".pdf", note)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def expand_pdf(src: Path, out_dir: Path, opts: dict, ctl: Control) -> Result:
    import pymupdf
    doc = cv._open_pdf(src)
    tmp = Path(tempfile.mkdtemp(prefix="docshift_"))
    try:
        produced = tmp / "out.pdf"
        doc.save(str(produced), expand=255, deflate=False, garbage=0)
        return _deliver(src, produced, out_dir, "expanded", ".pdf",
                        "All streams are stored uncompressed, which makes the file larger but easy to inspect or edit.",
                        keep_if_larger=False)
    finally:
        doc.close()
        shutil.rmtree(tmp, ignore_errors=True)


# --------------------------------------------------------------------------- #
# Video (ffmpeg)
# --------------------------------------------------------------------------- #
_VIDEO_CRF = {"light": 23, "balanced": 27, "strong": 31}
_VIDEO_MAX_H = {"light": None, "balanced": 1080, "strong": 720}
_HEIGHTS = [2160, 1440, 1080, 720, 540, 480, 360, 240]


class _Probe:
    duration: float | None = None
    width = height = 0
    fps = 30.0
    has_audio = False


def _ffmpeg() -> str:
    path = cv.ffmpeg_path()
    if not path:
        raise ConversionError("The video engine (ffmpeg) is missing, so videos can't be processed.")
    return path


def _probe(ff: str, src: Path) -> _Probe:
    p = subprocess.run([ff, "-hide_banner", "-i", str(src)], capture_output=True, text=True, errors="replace",
                       stdin=subprocess.DEVNULL, timeout=60, creationflags=_NOWIN)
    text = p.stderr or ""
    info = _Probe()
    if (m := re.search(r"Duration:\s*(\d+):(\d+):(\d+(?:\.\d+)?)", text)):
        info.duration = int(m[1]) * 3600 + int(m[2]) * 60 + float(m[3])
    line = next((l for l in text.splitlines() if "Video:" in l and "attached pic" not in l), None)
    if line is None:
        raise ConversionError("No video track was found in this file.")
    if (m := re.search(r"[\s,](\d{2,5})x(\d{2,5})(?=[\s,\[])", line)):
        info.width, info.height = int(m[1]), int(m[2])
    if (m := re.search(r"(\d+(?:\.\d+)?)\s*fps", line)):
        info.fps = max(1.0, min(float(m[1]), 120.0))
    if (m := re.search(r"rotation of (-?\d+(?:\.\d+)?)", text)) and abs(round(float(m[1]))) % 180 == 90:
        info.width, info.height = info.height, info.width  # phone video stored sideways
    info.has_audio = "Audio:" in text
    if not info.width or not info.height:
        raise ConversionError("Couldn't read this video's size. The file may be damaged.")
    return info


def _run_ffmpeg(ff: str, args: list, tmp: Path, duration: float | None, ctl: Control, lo: float, hi: float):
    """Run one ffmpeg pass, mapping its progress onto the lo..hi slice of the overall progress bar."""
    cmd = [ff, "-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-progress", "pipe:1", "-nostats"] + args
    log = tmp / "ffmpeg.log"
    with open(log, "wb") as err:
        proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=err, stdin=subprocess.DEVNULL, cwd=tmp,
                                text=True, errors="replace", bufsize=1, creationflags=_NOWIN)
        try:
            for line in proc.stdout:
                ctl.check()
                if duration and line.startswith("out_time_us="):
                    try:
                        ctl.report(lo + (hi - lo) * min(1.0, int(line.split("=")[1]) / 1e6 / duration))
                    except ValueError:
                        pass  # "N/A" before the first frame
            rc = proc.wait()
        except BaseException:
            proc.kill()
            proc.wait()
            raise
    if rc != 0:
        tail = [l.strip() for l in log.read_text(errors="replace").splitlines() if l.strip()][-2:]
        raise ConversionError("The video could not be processed: " + (" ".join(tail) or f"ffmpeg exit code {rc}"))
    ctl.report(hi)


def _pick_height(info: _Probe, video_bps: int) -> int | None:
    """Tallest standard height that still gets ~0.05 bits per pixel; None keeps the original size."""
    def enough(h: int) -> bool:
        w = info.width * h / info.height
        return video_bps / (w * h * info.fps) >= 0.05
    if enough(info.height):
        return None
    for h in _HEIGHTS:
        if h < info.height and enough(h):
            return h
    return min(240, info.height)


def _vf(height: int | None) -> str:
    # H.264 needs even dimensions; setsar keeps pixels square after resizing
    return f"scale=-2:{height},setsar=1" if height else "scale=trunc(iw/2)*2:trunc(ih/2)*2,setsar=1"


def compress_video(src: Path, out_dir: Path, opts: dict, ctl: Control) -> Result:
    target = parse_target(opts.get("target_mb"))
    if (done := _already_under(src, target, out_dir, "compressed")):
        return done
    ff = _ffmpeg()
    info = _probe(ff, src)
    level = opts.get("level", "balanced")
    tmp = Path(tempfile.mkdtemp(prefix="docshift_"))
    try:
        out = tmp / "out.mp4"
        src_arg = str(src.resolve())
        mux = ["-pix_fmt", "yuv420p", "-movflags", "+faststart"]
        if target is None:
            cap = _VIDEO_MAX_H[level]
            height = cap if cap and info.height > cap else None
            _run_ffmpeg(ff, ["-i", src_arg, "-map", "0:v:0", "-map", "0:a:0?", "-vf", _vf(height),
                             "-c:v", "libx264", "-preset", "fast", "-crf", str(_VIDEO_CRF[level]), *mux,
                             "-c:a", "aac", "-b:a", "96k" if level == "strong" else "128k", str(out)],
                        tmp, info.duration, ctl, 0.0, 1.0)
            note = ""
        else:
            note = _two_pass(ff, src_arg, out, tmp, info, target, ctl, mux)
        return _deliver(src, out, out_dir, "compressed", ".mp4", note)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def _two_pass(ff: str, src_arg: str, out: Path, tmp: Path, info: _Probe, target: int, ctl: Control,
              mux: list) -> str:
    if not info.duration:
        raise ConversionError("Couldn't read this video's length, which is needed to hit a size limit.")
    audio_bps = 128_000 if info.has_audio else 0
    total_bps = target * 8 * 0.94 / info.duration  # keep ~6% in hand for container overhead
    if info.has_audio and total_bps < 300_000:
        audio_bps = 64_000 if total_bps >= 120_000 else 32_000
    video_bps = int(total_bps - audio_bps)
    if video_bps < 40_000:
        need = (40_000 + audio_bps) * info.duration / 8 / 0.94 / MB
        length = f"{info.duration:.0f}-second" if info.duration < 90 else f"{info.duration / 60:.1f}-minute"
        raise ConversionError(f"A {length} video can't fit in {fmt_size(target)}. "
                              f"It needs at least about {max(need, 0.05):.2f} MB.")
    height = _pick_height(info, video_bps)
    vf = _vf(height)
    enc = ["-c:v", "libx264", "-preset", "fast", "-passlogfile", "ffpass"]
    audio = (["-c:a", "aac", "-b:a", str(audio_bps)] + (["-ac", "1"] if audio_bps <= 32_000 else [])
             if info.has_audio else ["-an"])
    _run_ffmpeg(ff, ["-i", src_arg, "-map", "0:v:0", "-vf", vf, *enc, "-b:v", str(video_bps), "-pass", "1",
                     "-an", "-f", "null", "-"], tmp, info.duration, ctl, 0.0, 0.4)
    size = 0
    for attempt in range(3):
        lo = 0.4 + 0.2 * attempt
        _run_ffmpeg(ff, ["-i", src_arg, "-map", "0:v:0", "-map", "0:a:0?", "-vf", vf, *enc, "-b:v", str(video_bps),
                         "-pass", "2", *mux, *audio, str(out)], tmp, info.duration, ctl, lo, lo + 0.2)
        size = out.stat().st_size
        if size <= target:
            ctl.report(1.0)
            return f"Resized to {height}p to fit." if height else ""
        video_bps = int(video_bps * target / size * 0.96)  # rate control overshot: tighten and redo pass 2
        if video_bps < 20_000:
            break
    raise _too_big(target, size, "this video ")


def expand_video(src: Path, out_dir: Path, opts: dict, ctl: Control) -> Result:
    ff = _ffmpeg()
    info = _probe(ff, src)
    tmp = Path(tempfile.mkdtemp(prefix="docshift_"))
    try:
        out = tmp / "out.mkv"
        _run_ffmpeg(ff, ["-i", str(src.resolve()), "-map", "0:v:0", "-map", "0:a:0?", "-c:v", "ffv1", "-level", "3",
                         "-c:a", "pcm_s16le", str(out)], tmp, info.duration, ctl, 0.0, 1.0)
        return _deliver(src, out, out_dir, "expanded", ".mkv",
                        "Saved losslessly (FFV1 in MKV). It is much larger and the quality of the original is "
                        "unchanged, not improved.", keep_if_larger=False)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


# --------------------------------------------------------------------------- #
# Dispatcher
# --------------------------------------------------------------------------- #
_ENGINES = {
    ("compress_pdf", "compress"): compress_pdf, ("compress_pdf", "expand"): expand_pdf,
    ("compress_img", "compress"): compress_image, ("compress_img", "expand"): expand_image,
    ("compress_video", "compress"): compress_video, ("compress_video", "expand"): expand_video,
}


def run(mode: str, src: Path, out_dir: Path, opts: dict, ctl: Control) -> Result:
    engine = _ENGINES[(mode, opts.get("action", "compress"))]
    return engine(src, out_dir, opts, ctl)
