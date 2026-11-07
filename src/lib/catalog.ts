import type { FileInfo, Kind } from "./api";
import { useApp, type CompressTab, type FromTarget, type ListKey, type OrganizeMode, type Screen } from "./store";

/** A place in the app: a screen plus the sub-mode it opens in. */
export interface Dest {
  screen: Screen;
  organizeMode?: OrganizeMode;
  compressTab?: CompressTab;
  fromTarget?: FromTarget;
  /** option presets to apply on arrival, by option scope */
  opts?: Record<string, Record<string, unknown>>;
}

export type NavId =
  | "home" | "smart" | "queue" | "history"
  | "conv-from" | "conv-to"
  | "merge" | "split" | "delete" | "extract" | "reorder"
  | "c-pdf" | "c-img" | "c-vid"
  | "settings";

export interface NavItem {
  id: NavId;
  label: string;
  dest: Dest;
}

export const NAV: { title?: string; items: NavItem[] }[] = [
  {
    items: [
      { id: "home", label: "Home", dest: { screen: "home" } },
      { id: "smart", label: "Smart dashboard", dest: { screen: "smart" } },
      { id: "queue", label: "Queue", dest: { screen: "queue" } },
      { id: "history", label: "Recent files", dest: { screen: "history" } },
    ],
  },
  {
    title: "Convert",
    items: [
      { id: "conv-from", label: "From PDF", dest: { screen: "convert-from" } },
      { id: "conv-to", label: "To PDF", dest: { screen: "convert-to" } },
    ],
  },
  {
    title: "Organize PDF",
    items: [
      { id: "merge", label: "Merge", dest: { screen: "merge" } },
      { id: "split", label: "Split", dest: { screen: "organize", organizeMode: "split" } },
      { id: "delete", label: "Delete pages", dest: { screen: "organize", organizeMode: "delete" } },
      { id: "extract", label: "Extract pages", dest: { screen: "organize", organizeMode: "extract" } },
      { id: "reorder", label: "Reorder & rotate", dest: { screen: "organize", organizeMode: "reorder" } },
    ],
  },
  {
    title: "Compress",
    items: [
      { id: "c-pdf", label: "PDF", dest: { screen: "compress", compressTab: "pdf" } },
      { id: "c-img", label: "Images", dest: { screen: "compress", compressTab: "img" } },
      { id: "c-vid", label: "Video", dest: { screen: "compress", compressTab: "video" } },
    ],
  },
  { items: [{ id: "settings", label: "Settings", dest: { screen: "settings" } }] },
];

/** Which sidebar entry is highlighted for the current screen. */
export function activeNav(s: { screen: Screen; organizeMode: OrganizeMode; compressTab: CompressTab }): NavId | null {
  switch (s.screen) {
    case "convert-from":
      return "conv-from";
    case "convert-to":
      return "conv-to";
    case "organize":
      return s.organizeMode;
    case "compress":
      return s.compressTab === "pdf" ? "c-pdf" : s.compressTab === "img" ? "c-img" : "c-vid";
    case "done":
      return null;
    default:
      return s.screen as NavId;
  }
}

export interface ToolCard {
  title: string;
  desc: string;
  chips: { kind: Kind | "txt"; text: string }[];
  dest: Dest;
  keywords: string;
}

const pdfChip = { kind: "pdf" as Kind, text: "PDF" };

export const TOOL_GROUPS: { title: string; items: ToolCard[] }[] = [
  {
    title: "Convert",
    items: [
      { title: "PDF to Word", desc: "Editable document", chips: [pdfChip, { kind: "word", text: "DOCX" }], dest: { screen: "convert-from", fromTarget: "word" }, keywords: "docx edit" },
      { title: "PDF to Excel", desc: "Tables become sheets", chips: [pdfChip, { kind: "excel", text: "XLSX" }], dest: { screen: "convert-from", fromTarget: "excel" }, keywords: "xlsx table spreadsheet" },
      { title: "PDF to PowerPoint", desc: "One slide per page", chips: [pdfChip, { kind: "ppt", text: "PPTX" }], dest: { screen: "convert-from", fromTarget: "ppt" }, keywords: "pptx slides" },
      { title: "PDF to Image", desc: "PNG or JPG per page", chips: [pdfChip, { kind: "img", text: "JPG" }], dest: { screen: "convert-from", fromTarget: "img" }, keywords: "png jpg webp picture" },
      { title: "PDF to Text", desc: "Plain text, scans too", chips: [pdfChip, { kind: "txt", text: "TXT" }], dest: { screen: "convert-from", fromTarget: "txt" }, keywords: "txt extract copy ocr scanned" },
      { title: "Searchable PDF", desc: "Read scanned pages (OCR)", chips: [pdfChip, pdfChip], dest: { screen: "convert-from", fromTarget: "ocr" }, keywords: "ocr scan scanned recognise recognize text search" },
      { title: "Word to PDF", desc: "Print-ready PDF", chips: [{ kind: "word", text: "DOCX" }, pdfChip], dest: { screen: "convert-to" }, keywords: "docx doc" },
      { title: "Excel to PDF", desc: "Print-ready PDF", chips: [{ kind: "excel", text: "XLSX" }, pdfChip], dest: { screen: "convert-to" }, keywords: "xlsx csv sheet" },
      { title: "PowerPoint to PDF", desc: "Print-ready PDF", chips: [{ kind: "ppt", text: "PPTX" }, pdfChip], dest: { screen: "convert-to" }, keywords: "pptx slides" },
      { title: "Images to PDF", desc: "Sort by name or date taken", chips: [{ kind: "img", text: "JPG" }, pdfChip], dest: { screen: "convert-to" }, keywords: "jpg png heic photos scan combine" },
      { title: "Combine into one PDF", desc: "Word, Excel, slides and images", chips: [{ kind: "word", text: "DOCX" }, { kind: "img", text: "JPG" }, pdfChip], dest: { screen: "convert-to", opts: { "convert-to": { combine: true } } }, keywords: "combine join bundle mixed office images" },
    ],
  },
  {
    title: "Organize PDF",
    items: [
      { title: "Merge", desc: "Combine several PDFs", chips: [pdfChip], dest: { screen: "merge" }, keywords: "join combine" },
      { title: "Split", desc: "Break into separate files", chips: [pdfChip], dest: { screen: "organize", organizeMode: "split" }, keywords: "cut separate" },
      { title: "Delete pages", desc: "Remove unwanted pages", chips: [pdfChip], dest: { screen: "organize", organizeMode: "delete" }, keywords: "remove" },
      { title: "Extract pages", desc: "Save pages as a new PDF", chips: [pdfChip], dest: { screen: "organize", organizeMode: "extract" }, keywords: "pick select" },
      { title: "Reorder & rotate", desc: "Arrange and turn pages", chips: [pdfChip], dest: { screen: "organize", organizeMode: "reorder" }, keywords: "organize order rotate move" },
    ],
  },
  {
    title: "Compress",
    items: [
      { title: "Compress PDF", desc: "Smaller to email or upload", chips: [pdfChip], dest: { screen: "compress", compressTab: "pdf" }, keywords: "shrink reduce size mb decompress" },
      { title: "Compress images", desc: "JPG, PNG, WebP", chips: [{ kind: "img", text: "JPG" }], dest: { screen: "compress", compressTab: "img" }, keywords: "shrink photo webp decompress" },
      { title: "Compress video", desc: "MP4, MOV, MKV", chips: [{ kind: "video", text: "MP4" }], dest: { screen: "compress", compressTab: "video" }, keywords: "shrink movie mp4 decompress" },
    ],
  },
];

/** The file list a destination works on. */
export function listFor(d: Dest): ListKey | null {
  switch (d.screen) {
    case "convert-from":
    case "convert-to":
    case "merge":
    case "organize":
      return d.screen;
    case "compress": {
      const tab = d.compressTab ?? useApp.getState().compressTab;
      return tab === "pdf" ? "compress-pdf" : tab === "img" ? "compress-img" : "compress-video";
    }
    default:
      return null;
  }
}

/** Navigate somewhere, optionally loading files into that screen (replacing what was there). */
export function openDest(d: Dest, files?: FileInfo[]) {
  const st = useApp.getState();
  const list = listFor(d);
  if (list && files?.length) st.setFiles(list, files);
  for (const [scope, values] of Object.entries(d.opts ?? {})) for (const [k, v] of Object.entries(values)) st.setOpt(scope, k, v);
  st.go(d.screen, {
    ...(d.organizeMode ? { organizeMode: d.organizeMode } : {}),
    ...(d.compressTab ? { compressTab: d.compressTab } : {}),
    ...(d.fromTarget ? { fromTarget: d.fromTarget } : {}),
  });
}

/** Where dropped files of a kind go when there's no better choice. */
export function defaultDest(kind: Kind): Dest {
  switch (kind) {
    case "pdf":
      return { screen: "convert-from" };
    case "video":
      return { screen: "compress", compressTab: "video" };
    default:
      return { screen: "convert-to" };
  }
}
