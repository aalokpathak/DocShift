import { open } from "@tauri-apps/plugin-dialog";
import { errText, inspectFiles, type FileInfo, type Kind } from "./api";
import { KIND_LABEL } from "./files";
import { useApp } from "./store";

const EXTS: Record<Kind, string[]> = {
  pdf: ["pdf"],
  word: ["doc", "docx", "docm", "dot", "dotx", "rtf", "odt", "txt"],
  excel: ["xls", "xlsx", "xlsm", "xlsb", "csv", "ods"],
  ppt: ["ppt", "pptx", "pptm", "pps", "ppsx", "odp"],
  img: ["jpg", "jpeg", "jfif", "png", "webp", "heic", "heif", "hif", "avif", "bmp", "gif", "tif", "tiff", "ico"],
  video: ["mp4", "mov", "mkv", "avi", "webm", "wmv", "flv", "m4v", "mpg", "mpeg", "3gp"],
};

export const extList = (kind: Kind) => EXTS[kind].map((e) => e.toUpperCase());

/** Native file picker. With no kinds, offers every supported type. Returns inspected files. */
export async function pickFiles(kinds?: Kind[]): Promise<FileInfo[]> {
  const list = kinds ?? (Object.keys(EXTS) as Kind[]);
  const filters = [
    { name: kinds ? kinds.map((k) => KIND_LABEL[k].many).join(", ") : "All supported files", extensions: list.flatMap((k) => EXTS[k]) },
    { name: "All files", extensions: ["*"] },
  ];
  const sel = await open({ multiple: true, filters });
  if (!sel) return [];
  return inspect(Array.isArray(sel) ? sel : [sel]);
}

export async function pickFolder(defaultPath?: string): Promise<string | null> {
  const sel = await open({ directory: true, multiple: false, defaultPath });
  return typeof sel === "string" ? sel : null;
}

export async function inspect(paths: string[]): Promise<FileInfo[]> {
  try {
    return await inspectFiles(paths);
  } catch (e) {
    useApp.getState().toast({ kind: "error", text: errText(e) });
    return [];
  }
}
