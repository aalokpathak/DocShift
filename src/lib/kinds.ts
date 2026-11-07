import type { FileInfo, Kind } from "./api";

/** Hue of each file type's chip (OKLCH), as in the design. Lightness/chroma come from theme tokens. */
const HUE: Record<Kind | "txt", number> = { pdf: 25, word: 255, excel: 150, ppt: 55, img: 300, video: 200, txt: 80 };

export const DEFAULT_EXT: Record<Kind, string> = { pdf: "PDF", word: "DOCX", excel: "XLSX", ppt: "PPTX", img: "IMG", video: "MP4" };

export function chipColors(kind: Kind | "txt") {
  const h = HUE[kind];
  return { background: `oklch(var(--tl) var(--tc) ${h})`, color: `oklch(var(--il) 0.14 ${h})` };
}

/** Short label for a file's chip: its real extension when it fits, otherwise the kind's default. */
export function extLabel(info: Pick<FileInfo, "ext" | "kind">): string {
  const e = (info.ext || "").toUpperCase();
  if (e && e.length <= 4) return e === "JPEG" ? "JPG" : e;
  return info.kind ? DEFAULT_EXT[info.kind] : "FILE";
}

export const KIND_GROUP = (k: Kind | null | undefined): "pdf" | "office" | "img" | "video" | null =>
  !k ? null : k === "word" || k === "excel" || k === "ppt" ? "office" : k;
