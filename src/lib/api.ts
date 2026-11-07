// Typed wrappers around the Rust commands and the job event stream.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Kind = "word" | "excel" | "ppt" | "img" | "pdf" | "video";

export interface FileInfo {
  path: string;
  name: string;
  ext: string;
  kind: Kind | null;
  size: number;
  created?: number | null;
  modified?: number | null;
  taken?: number | null;
  pages?: number | null;
  /** PDF with no selectable text on its first pages */
  scanned?: boolean | null;
  width?: number | null;
  height?: number | null;
  duration?: number | null;
  error?: string | null;
}

export interface PdfInfo {
  pages: number;
  sizes: [number, number][];
  noText: boolean;
}

export interface EngineStatus {
  word: boolean;
  excel: boolean;
  powerpoint: boolean;
  libreoffice: boolean;
  video: boolean;
  pdf: boolean;
  /** Windows has an OCR language installed */
  ocr: boolean;
}

export interface OcrLanguage {
  tag: string;
  name: string;
}

export type OnExists = "keep" | "replace" | "ask";

export interface Outcome {
  outputs: string[];
  note: string;
  detail: string;
}

export interface TaskSpec {
  id: string;
  inputs: string[];
}

export interface JobSpec {
  mode: string;
  tasks: TaskSpec[];
  outDir: string;
  options: Record<string, unknown>;
  onExists?: OnExists;
}

export type JobEvent =
  | { type: "taskStarted"; jobId: string; taskId: string }
  | { type: "progress"; jobId: string; taskId: string; fraction: number }
  | { type: "taskDone"; jobId: string; taskId: string; outcome: Outcome }
  | { type: "taskFailed"; jobId: string; taskId: string; message: string; cancelled: boolean; skipped?: boolean }
  | { type: "ask"; jobId: string; taskId: string; askId: number; name: string; folder: string }
  | { type: "jobDone"; jobId: string; cancelled: boolean };

export const inspectFiles = (paths: string[]) => invoke<FileInfo[]>("inspect_files", { paths });
export const pdfInfo = (path: string) => invoke<PdfInfo>("pdf_info", { path });
export const engineStatus = () => invoke<EngineStatus>("engine_status");
export const defaultOutputDir = () => invoke<string>("default_output_dir");
export const checkOutputDir = (dir: string) => invoke<void>("check_output_dir", { dir });
export const validateTarget = (text: string) => invoke<void>("validate_target", { text });
export const validatePages = (spec: string, count: number) => invoke<number[]>("validate_pages", { spec, count });
export const startJob = (spec: JobSpec) => invoke<string>("start_job", { spec });
export const cancelJob = (jobId: string) => invoke<void>("cancel_job", { jobId });
export const ocrLanguages = () => invoke<OcrLanguage[]>("ocr_languages");
export const answerAsk = (askId: number, choice: "replace" | "keep" | "skip", all: boolean) => invoke<void>("answer_ask", { askId, choice, all });
/** Opens a file or folder in its default app; rejects with a readable message. */
export const openFile = (path: string) => invoke<void>("open_path", { path });

export function onJobEvent(cb: (e: JobEvent) => void): Promise<UnlistenFn> {
  return listen<JobEvent>("docshift://job", (e) => cb(e.payload));
}

// ---- thumbnails: cached, and limited to a few at a time so scrolling a big PDF stays smooth ----

const cache = new Map<string, Promise<string>>();
let active = 0;
const waiting: (() => void)[] = [];
const MAX_PARALLEL = 3;

function slot(): Promise<void> {
  if (active < MAX_PARALLEL) {
    active++;
    return Promise.resolve();
  }
  return new Promise((res) => waiting.push(() => { active++; res(); }));
}
function release() {
  active--;
  waiting.shift()?.();
}

export function thumbnail(path: string, page: number | null, size: number): Promise<string> {
  const key = `${path}|${page ?? ""}|${size}`;
  let p = cache.get(key);
  if (!p) {
    p = (async () => {
      await slot();
      try {
        return await invoke<string>("thumbnail", { path, page, size });
      } finally {
        release();
      }
    })();
    p.catch(() => cache.delete(key)); // allow a retry after a failure
    cache.set(key, p);
  }
  return p;
}

/** Message text from whatever a rejected invoke() carries. */
export function errText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : "Something went wrong.";
}
