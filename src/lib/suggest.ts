// Smart dashboard: what to suggest for a file, and what else can be done with it.
import type { FileInfo, Kind } from "./api";
import type { Dest } from "./catalog";
import { formatSize, plural } from "./files";
import { KIND_GROUP } from "./kinds";

export interface Action {
  label: string;
  chip?: { kind: Kind | "txt"; text: string };
  dest: Dest;
  /** which files the action takes: just this one, or all files of the same group */
  scope: "one" | "group";
}

export interface Suggestions {
  primary: { label: string; why: string; action: Action };
  groups: { title: string; actions: Action[] }[];
  bulk: Action[];
  groupName: string;
}

const MB = 1_000_000;
const one = (label: string, dest: Dest, chip?: Action["chip"]): Action => ({ label, dest, chip, scope: "one" });
const all = (label: string, dest: Dest): Action => ({ label, dest, scope: "group" });

export function describe(f: FileInfo): string {
  const bits: string[] = [];
  switch (f.kind) {
    case "pdf":
      bits.push("PDF");
      if (f.pages) bits.push(plural(f.pages, "page"));
      if (f.scanned) bits.push("scanned");
      break;
    case "word":
      bits.push("Word document");
      break;
    case "excel":
      bits.push("Excel workbook");
      break;
    case "ppt":
      bits.push("PowerPoint presentation");
      break;
    case "img":
      bits.push(`Image · ${f.ext.toUpperCase()}`);
      if (f.width && f.height) bits.push(`${f.width} × ${f.height}`);
      break;
    case "video":
      bits.push("Video");
      if (f.width && f.height) bits.push(`${f.width} × ${f.height}`);
      if (f.duration) bits.push(f.duration < 60 ? `${Math.round(f.duration)} s` : `${Math.floor(f.duration / 60)} min ${Math.round(f.duration % 60)} s`);
      break;
  }
  bits.push(formatSize(f.size));
  return bits.join(" · ");
}

export function suggestFor(f: FileInfo, sameGroupCount: number): Suggestions {
  const group = KIND_GROUP(f.kind);
  const toPdf: Dest = { screen: "convert-to" };
  switch (group) {
    case "pdf": {
      const fromTo = (t: "word" | "excel" | "ppt" | "img" | "txt" | "ocr"): Dest => ({ screen: "convert-from", fromTarget: t });
      const big = f.size > 10 * MB;
      return {
        primary: f.scanned
          ? { label: "Make it searchable", why: "This PDF is scanned: its pages are pictures, so the text can't be searched or copied yet.", action: one("Searchable PDF", fromTo("ocr")) }
          : big
          ? { label: "Compress PDF", why: `${formatSize(f.size)} is large for email. Most mail services stop at 20–25 MB.`, action: one("Compress PDF", { screen: "compress", compressTab: "pdf" }) }
          : sameGroupCount > 1
            ? { label: `Merge ${sameGroupCount} PDFs`, why: "Several PDFs found. Combine them into one file in the order you choose.", action: all("Merge", { screen: "merge" }) }
            : { label: "Convert to Word", why: "Edit the text and tables in Word.", action: one("Word", fromTo("word")) },
        groups: [
          {
            title: "Convert",
            actions: [
              one("Word", fromTo("word"), { kind: "word", text: "DOCX" }),
              one("Excel", fromTo("excel"), { kind: "excel", text: "XLSX" }),
              one("PowerPoint", fromTo("ppt"), { kind: "ppt", text: "PPTX" }),
              one("Images", fromTo("img"), { kind: "img", text: "JPG" }),
              one("Text", fromTo("txt"), { kind: "txt", text: "TXT" }),
              one("Searchable PDF", fromTo("ocr"), { kind: "pdf", text: "OCR" }),
            ],
          },
          {
            title: "Organize",
            actions: [
              all("Merge", { screen: "merge" }),
              one("Split", { screen: "organize", organizeMode: "split" }),
              one("Delete pages", { screen: "organize", organizeMode: "delete" }),
              one("Extract pages", { screen: "organize", organizeMode: "extract" }),
              one("Reorder & rotate", { screen: "organize", organizeMode: "reorder" }),
            ],
          },
          { title: "Compress", actions: [one("Compress PDF", { screen: "compress", compressTab: "pdf" }, { kind: "pdf", text: "PDF" })] },
        ],
        bulk: [all("Merge them", { screen: "merge" }), all("Compress all", { screen: "compress", compressTab: "pdf" })],
        groupName: "PDFs",
      };
    }
    case "office": {
      const why =
        f.kind === "excel" ? "Locks the layout for sharing and printing." : f.kind === "ppt" ? "Anyone can open it without PowerPoint." : "Ready to send or sign.";
      return {
        primary: { label: "Convert to PDF", why, action: one("PDF", toPdf) },
        groups: [{ title: "Convert", actions: [one("PDF", toPdf, { kind: "pdf", text: "PDF" })] }],
        bulk: [all("All to PDF", toPdf)],
        groupName: "Office files",
      };
    }
    case "img": {
      const imgTab = (fmt?: string): Dest => ({ screen: "compress", compressTab: "img", opts: fmt ? { "compress-img": { format: fmt, action: "compress" } } : undefined });
      const primary =
        sameGroupCount > 1
          ? { label: `Combine ${sameGroupCount} images into a PDF`, why: "Sorted by name, date taken or the order you added them.", action: all("Images to PDF", toPdf) }
          : f.size > 2 * MB
            ? { label: "Compress image", why: `${formatSize(f.size)} is more than most websites and chat apps need.`, action: one("Compress image", imgTab()) }
            : { label: "Convert to PDF", why: "Easy to share, print or attach.", action: one("PDF", toPdf) };
      return {
        primary,
        groups: [
          { title: "Convert", actions: [one("PDF", toPdf, { kind: "pdf", text: "PDF" }), one("JPG", imgTab("jpg"), { kind: "img", text: "JPG" }), one("WebP", imgTab("webp"), { kind: "img", text: "WEBP" })] },
          { title: "Compress", actions: [one("Compress image", imgTab(), { kind: "img", text: f.ext.toUpperCase().slice(0, 4) || "IMG" })] },
        ],
        bulk: [all("Combine into one PDF", toPdf), all("Compress all", imgTab())],
        groupName: "images",
      };
    }
    default: {
      const vid = (level?: string, action = "compress", resolution?: number): Dest => ({
        screen: "compress",
        compressTab: "video",
        opts: { "compress-video": { action, ...(level ? { level, target_mb: "" } : {}), ...(resolution !== undefined ? { resolution } : {}) } },
      });
      const short = Math.min(f.width ?? 0, f.height ?? 0);
      return {
        primary: {
          label: "Compress video",
          why: f.size > 25 * MB ? `${formatSize(f.size)} is too large for most email and chat apps.` : "Make it smaller to share quickly.",
          action: one("Compress video", vid()),
        },
        groups: [
          { title: "Compress", actions: [one("Light", vid("light", "compress", 0)), one("Balanced", vid("balanced", "compress", 0)), one("Smallest", vid("strong", "compress", 0)), one("To a size limit", vid())] },
          {
            title: "Resize",
            actions: [
              ...(short > 1080 ? [one("Shrink to 1080p", vid("light", "compress", 1080))] : []),
              ...(short > 720 ? [one("Shrink to 720p", vid("balanced", "compress", 720))] : []),
              ...(short > 480 ? [one("Shrink to 480p", vid("balanced", "compress", 480))] : []),
            ],
          },
          { title: "Other", actions: [one("Lossless copy", vid(undefined, "expand"))] },
        ],
        bulk: [all("Compress all", vid())],
        groupName: "videos",
      };
    }
  }
}
