import type { FileInfo, Kind } from "./api";

export const KIND_LABEL: Record<Kind, { one: string; many: string }> = {
  word: { one: "Word document", many: "Word documents" },
  excel: { one: "Excel workbook", many: "Excel workbooks" },
  ppt: { one: "PowerPoint presentation", many: "PowerPoint presentations" },
  img: { one: "image", many: "images" },
  pdf: { one: "PDF", many: "PDFs" },
  video: { one: "video", many: "videos" },
};

export function formatSize(n: number): string {
  if (n < 1000) return `${n} B`;
  if (n < 1e6) return `${(n / 1e3).toFixed(1)} KB`;
  if (n < 1e9) return `${(n / 1e6).toFixed(2)} MB`;
  return `${(n / 1e9).toFixed(2)} GB`;
}

export function formatDate(ms?: number | null): string {
  if (!ms) return "";
  return new Date(ms).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}

/** `wallClock`: the value is a camera's local time stored as if it were UTC (EXIF has no time zone), so show it as-is. */
export function formatDateTime(ms?: number | null, wallClock = false): string {
  if (!ms) return "";
  return new Date(ms).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short", ...(wallClock ? { timeZone: "UTC" } : {}) });
}

export function formatDuration(sec?: number | null): string {
  if (!sec) return "";
  const s = Math.round(sec);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const r = s % 60;
  return h ? `${h}:${String(m).padStart(2, "0")}:${String(r).padStart(2, "0")}` : `${m}:${String(r).padStart(2, "0")}`;
}

export function folderOf(path: string): string {
  const i = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return i > 0 ? path.slice(0, i) : path;
}

export function plural(n: number, one: string, many?: string): string {
  return `${n} ${n === 1 ? one : many ?? one + "s"}`;
}

// ---------------------------------------------------------------------------------------------- sorting

export type SortKey = "added" | "name" | "taken" | "modified" | "created" | "size" | "pages";
export type SortDir = "asc" | "desc";

export const SORT_LABEL: Record<SortKey, string> = {
  added: "Order added",
  name: "File name",
  taken: "Date taken",
  modified: "Date modified",
  created: "Date created",
  size: "File size",
  pages: "Page count",
};

/** Natural order: "img2" comes before "img10", ignoring case. */
const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

export interface Sortable {
  info: FileInfo;
  seq: number; // position in which the file was added
}

function value(it: Sortable, key: SortKey): number | string {
  const i = it.info;
  switch (key) {
    case "added":
      return it.seq;
    case "name":
      return i.name;
    case "taken":
      return i.taken ?? i.modified ?? i.created ?? 0; // photos without EXIF fall back to the file date
    case "modified":
      return i.modified ?? 0;
    case "created":
      return i.created ?? 0;
    case "size":
      return i.size;
    case "pages":
      return i.pages ?? 0;
  }
}

export function sortItems<T extends Sortable>(items: T[], key: SortKey, dir: SortDir): T[] {
  const sign = dir === "asc" ? 1 : -1;
  return [...items].sort((a, b) => {
    const va = value(a, key);
    const vb = value(b, key);
    const c = typeof va === "string" ? collator.compare(va, vb as string) : (va as number) - (vb as number);
    // ties keep a stable, predictable order: by name, then by when they were added
    return sign * c || collator.compare(a.info.name, b.info.name) || a.seq - b.seq;
  });
}

export function pdfDetail(i: FileInfo): string {
  return [i.pages ? plural(i.pages, "page") : "", formatSize(i.size)].filter(Boolean).join(" · ");
}
