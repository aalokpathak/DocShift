import { create } from "zustand";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { answerAsk, cancelJob, checkOutputDir, errText, inspectFiles, startJob, type EngineStatus, type FileInfo, type JobEvent, type JobSpec, type OcrLanguage, type OnExists, type Outcome } from "./api";
import { formatSize } from "./files";

export type Theme = "light" | "dark" | "system";
export type Level = "light" | "balanced" | "strong";
export type Screen =
  | "home"
  | "smart"
  | "queue"
  | "history"
  | "convert-from"
  | "convert-to"
  | "merge"
  | "organize"
  | "compress"
  | "settings"
  | "done";
export type OrganizeMode = "delete" | "split" | "extract" | "reorder";
export type CompressTab = "pdf" | "img" | "video";
export type FromTarget = "word" | "excel" | "ppt" | "img" | "txt" | "ocr";
export type Category = "Convert" | "Organize" | "Compress";

/** File lists, one per working screen. Compress keeps one list per tab. */
export type ListKey = "convert-from" | "convert-to" | "merge" | "organize" | "compress-pdf" | "compress-img" | "compress-video";

export interface Item {
  id: string;
  info: FileInfo;
  /** order in which the file was added (for "Order added" sorting) */
  seq: number;
  status: "ready" | "running" | "done" | "error";
  progress: number;
  message?: string;
  detail?: string;
  note?: string;
  outputs?: string[];
}

export interface TaskResult {
  taskId: string;
  outcome?: Outcome;
  error?: string;
  /** the user chose not to replace an existing file */
  skipped?: string;
}

/** A job is waiting for the user to decide what happens to a file name that's taken. */
export interface Ask {
  askId: number;
  jobId: string;
  name: string;
  folder: string;
}

export interface JobState {
  id: string;
  list: ListKey;
  screen: Screen;
  label: string;
  category: Category;
  /** task id -> ids of the list items it covers */
  taskItems: Record<string, string[]>;
  names: string[];
  /** task id -> names of the input files, for "From report.pdf" labels */
  taskNames: Record<string, string[]>;
  total: number;
  finished: number;
  progress: number;
  running: boolean;
  cancelled: boolean;
  results: TaskResult[];
  started: number;
  /** jobs started together by one click (e.g. Word + Excel + images to PDF) finish as one result */
  group?: string;
}

export interface HistoryEntry {
  path: string;
  name: string;
  action: string;
  category: Category;
  meta: string;
  size: number | null;
  when: number;
}

export interface DoneView {
  title: string;
  note: string;
  outputs: { path: string; name: string; meta: string }[];
  errors: string[];
  category: Category;
}

export interface Toast {
  id: number;
  kind: "info" | "ok" | "warn" | "error";
  text: string;
}

// ------------------------------------------------------------------------------------------- persistence

const SETTINGS_KEY = "docshift.settings.v3";
const HISTORY_KEY = "docshift.history.v1";
const MONTH = 30 * 86_400_000;

interface Settings {
  theme: Theme;
  outDir: string;
  openAfter: boolean;
  defLevel: Level;
  keepHistory: boolean;
  /** what to do when an output file name is taken */
  onExists: OnExists;
  /** OCR language tag ("" = Windows' own language) */
  ocrLang: string;
}
const DEFAULTS: Settings = { theme: "system", outDir: "", openAfter: false, defLevel: "balanced", keepHistory: true, onExists: "keep", ocrLang: "" };

function load<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? { ...fallback, ...JSON.parse(raw) } : fallback;
  } catch {
    return fallback;
  }
}
function save(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* private mode etc.: it just won't persist */
  }
}
function loadHistory(): HistoryEntry[] {
  try {
    const list = JSON.parse(localStorage.getItem(HISTORY_KEY) ?? "[]") as HistoryEntry[];
    return list.filter((h) => Date.now() - h.when < MONTH);
  } catch {
    return [];
  }
}

// ------------------------------------------------------------------------------------------------ store

let seqCounter = 0;
let idCounter = 0;
let toastCounter = 0;
export const newItem = (info: FileInfo): Item => ({ id: `f${++idCounter}`, info, seq: ++seqCounter, status: "ready", progress: 0 });

const baseName = (p: string) => p.split(/[\\/]/).pop() ?? p;

interface State extends Settings {
  screen: Screen;
  organizeMode: OrganizeMode;
  compressTab: CompressTab;
  fromTarget: FromTarget;
  engines: EngineStatus | null;
  inbox: Item[];
  /** selected file on the smart dashboard */
  smartSel: string | null;
  lists: Record<ListKey, Item[]>;
  options: Record<string, Record<string, unknown>>;
  jobs: Record<string, JobState>;
  history: HistoryEntry[];
  done: DoneView | null;
  toasts: Toast[];
  dropActive: boolean;
  paletteOpen: boolean;
  asks: Ask[];
  ocrLangs: OcrLanguage[] | null;

  go: (s: Screen, patch?: Partial<Pick<State, "organizeMode" | "compressTab" | "fromTarget">>) => void;
  setSetting: <K extends keyof Settings>(k: K, v: Settings[K]) => void;
  setEngines: (e: EngineStatus) => void;
  setDropActive: (v: boolean) => void;
  setPalette: (v: boolean) => void;
  setOcrLangs: (l: OcrLanguage[]) => void;
  answer: (askId: number, choice: "replace" | "keep" | "skip", all: boolean) => void;

  addToInbox: (infos: FileInfo[]) => Item[];
  removeFromInbox: (id: string) => void;
  clearInbox: () => void;
  selectSmart: (id: string | null) => void;

  /** Appends files (deduplicated) to a list. Returns the items that were added. */
  addToList: (key: ListKey, infos: FileInfo[]) => Item[];
  /** Replaces a list's files (used when opening a tool from the dashboard). */
  setFiles: (key: ListKey, infos: FileInfo[]) => void;
  setList: (key: ListKey, fn: (items: Item[]) => Item[]) => void;
  removeFromList: (key: ListKey, id: string) => void;
  clearList: (key: ListKey) => void;
  opt: <T>(scope: string, key: string, fallback: T) => T;
  setOpt: (scope: string, key: string, v: unknown) => void;

  runJob: (a: { list: ListKey; screen: Screen; label: string; category: Category; spec: Omit<JobSpec, "outDir">; taskItems: Record<string, string[]>; group?: string }) => Promise<string | null>;
  cancel: (jobId: string) => void;
  clearFinishedJobs: () => void;
  onJobEvent: (e: JobEvent) => void;

  clearHistory: () => void;
  removeHistory: (path: string) => void;

  toast: (t: Omit<Toast, "id">) => void;
  dismissToast: (id: number) => void;
}

const settings0 = load<Settings>(SETTINGS_KEY, DEFAULTS);
const emptyLists = (): Record<ListKey, Item[]> => ({
  "convert-from": [],
  "convert-to": [],
  merge: [],
  organize: [],
  "compress-pdf": [],
  "compress-img": [],
  "compress-video": [],
});

export const useApp = create<State>((set, get) => ({
  ...settings0,
  screen: "home",
  organizeMode: "delete",
  compressTab: "pdf",
  fromTarget: "word",
  engines: null,
  inbox: [],
  smartSel: null,
  lists: emptyLists(),
  options: {},
  jobs: {},
  history: settings0.keepHistory ? loadHistory() : [],
  done: null,
  toasts: [],
  dropActive: false,
  paletteOpen: false,
  asks: [],
  ocrLangs: null,

  go: (screen, patch) => set({ screen, ...(patch ?? {}) }),
  setSetting: (k, v) => {
    set({ [k]: v } as Partial<State>);
    const s = get();
    save(SETTINGS_KEY, { theme: s.theme, outDir: s.outDir, openAfter: s.openAfter, defLevel: s.defLevel, keepHistory: s.keepHistory, onExists: s.onExists, ocrLang: s.ocrLang });
    if (k === "keepHistory" && !v) get().clearHistory();
  },
  setEngines: (engines) => set({ engines }),
  setDropActive: (dropActive) => set({ dropActive }),
  setPalette: (paletteOpen) => set({ paletteOpen }),
  setOcrLangs: (ocrLangs) => set({ ocrLangs }),
  answer: (askId, choice, all) => {
    set((s) => ({ asks: s.asks.filter((a) => a.askId !== askId) }));
    void answerAsk(askId, choice, all).catch(() => {});
  },

  addToInbox: (infos) => {
    const have = new Set(get().inbox.map((i) => i.info.path.toLowerCase()));
    const fresh = infos.filter((i) => !have.has(i.path.toLowerCase())).map(newItem);
    if (fresh.length) set((s) => ({ inbox: [...s.inbox, ...fresh], smartSel: fresh[0].id }));
    return fresh;
  },
  removeFromInbox: (id) => set((s) => ({ inbox: s.inbox.filter((i) => i.id !== id), smartSel: s.smartSel === id ? null : s.smartSel })),
  clearInbox: () => set({ inbox: [], smartSel: null }),
  selectSmart: (smartSel) => set({ smartSel }),

  addToList: (key, infos) => {
    const have = new Set(get().lists[key].map((i) => i.info.path.toLowerCase()));
    const fresh = infos.filter((i) => !i.error && !have.has(i.path.toLowerCase())).map(newItem);
    if (fresh.length) set((s) => ({ lists: { ...s.lists, [key]: [...s.lists[key], ...fresh] } }));
    const broken = infos.find((i) => i.error);
    if (broken) get().toast({ kind: "warn", text: `${broken.name}: ${broken.error}` });
    return fresh;
  },
  setFiles: (key, infos) => {
    if (Object.values(get().jobs).some((j) => j.running && j.list === key)) {
      get().addToList(key, infos);
      return;
    }
    set((s) => ({ lists: { ...s.lists, [key]: infos.filter((i) => !i.error).map(newItem) } }));
  },
  setList: (key, fn) => set((s) => ({ lists: { ...s.lists, [key]: fn(s.lists[key]) } })),
  removeFromList: (key, id) => set((s) => ({ lists: { ...s.lists, [key]: s.lists[key].filter((i) => i.id !== id) } })),
  clearList: (key) => set((s) => ({ lists: { ...s.lists, [key]: [] } })),
  opt: (scope, key, fallback) => {
    const v = get().options[scope]?.[key];
    return (v === undefined ? fallback : v) as never;
  },
  setOpt: (scope, key, v) => set((s) => ({ options: { ...s.options, [scope]: { ...s.options[scope], [key]: v } } })),

  runJob: async ({ list, screen, label, category, spec, taskItems, group }) => {
    const { outDir, onExists } = get();
    try {
      await checkOutputDir(outDir);
    } catch (e) {
      get().toast({ kind: "error", text: errText(e) });
      return null;
    }
    try {
      const id = await startJob({ ...spec, outDir, onExists });
      const ids = new Set(Object.values(taskItems).flat());
      const items = get().lists[list];
      const names = items.filter((i) => ids.has(i.id)).map((i) => i.info.name);
      const taskNames = Object.fromEntries(Object.entries(taskItems).map(([t, its]) => [t, items.filter((i) => its.includes(i.id)).map((i) => i.info.name)]));
      const job: JobState = {
        id, list, screen, label, category, taskItems, names, taskNames, total: spec.tasks.length, finished: 0, progress: 0,
        running: true, cancelled: false, results: [], started: Date.now(), group,
      };
      set((s) => ({
        jobs: { ...s.jobs, [id]: job },
        lists: { ...s.lists, [list]: s.lists[list].map((i) => (ids.has(i.id) ? { ...i, status: "running", progress: 0, message: undefined } : i)) },
      }));
      return id;
    } catch (e) {
      get().toast({ kind: "error", text: errText(e) });
      return null;
    }
  },

  cancel: (jobId) => {
    set((s) => ({ jobs: { ...s.jobs, [jobId]: { ...s.jobs[jobId], cancelled: true } }, asks: s.asks.filter((a) => a.jobId !== jobId) }));
    void cancelJob(jobId);
  },
  clearFinishedJobs: () => set((s) => ({ jobs: Object.fromEntries(Object.entries(s.jobs).filter(([, j]) => j.running)) })),

  onJobEvent: (e) => {
    const job = get().jobs[e.jobId];
    if (!job) return;
    const patchItems = (taskId: string, fn: (i: Item) => Item) =>
      set((s) => {
        const ids = new Set(job.taskItems[taskId] ?? []);
        return { lists: { ...s.lists, [job.list]: s.lists[job.list].map((i) => (ids.has(i.id) ? fn(i) : i)) } };
      });
    const patchJob = (fn: (j: JobState) => JobState) => set((s) => ({ jobs: { ...s.jobs, [e.jobId]: fn(s.jobs[e.jobId]) } }));

    switch (e.type) {
      case "taskStarted":
        patchItems(e.taskId, (i) => ({ ...i, status: "running", progress: 0 }));
        break;
      case "progress":
        patchItems(e.taskId, (i) => ({ ...i, progress: e.fraction }));
        patchJob((j) => ({ ...j, progress: Math.max(j.progress, (j.finished + e.fraction) / j.total) }));
        break;
      case "taskDone":
        patchItems(e.taskId, (i) => ({ ...i, status: "done", progress: 1, detail: e.outcome.detail, note: e.outcome.note, outputs: e.outcome.outputs, message: undefined }));
        patchJob((j) => ({ ...j, finished: j.finished + 1, progress: (j.finished + 1) / j.total, results: [...j.results, { taskId: e.taskId, outcome: e.outcome }] }));
        break;
      case "taskFailed":
        patchItems(e.taskId, (i) =>
          e.cancelled || e.skipped ? { ...i, status: "ready", progress: 0, message: undefined } : { ...i, status: "error", message: e.message, progress: 0 },
        );
        patchJob((j) => ({
          ...j, finished: j.finished + 1, progress: (j.finished + 1) / j.total,
          results: e.cancelled ? j.results : [...j.results, e.skipped ? { taskId: e.taskId, skipped: e.message } : { taskId: e.taskId, error: e.message }],
        }));
        break;
      case "ask":
        set((s) => ({ asks: [...s.asks, { askId: e.askId, jobId: e.jobId, name: e.name, folder: e.folder }] }));
        break;
      case "jobDone":
        patchJob((j) => ({ ...j, running: false, progress: e.cancelled ? j.progress : 1 }));
        void finishJob(get().jobs[e.jobId], e.cancelled);
        break;
    }
  },

  clearHistory: () => {
    set({ history: [] });
    save(HISTORY_KEY, []);
  },
  removeHistory: (path) => {
    const history = get().history.filter((h) => h.path !== path);
    set({ history });
    save(HISTORY_KEY, history);
  },

  toast: (t) => {
    const id = ++toastCounter;
    set((s) => ({ toasts: [...s.toasts.slice(-3), { ...t, id }] }));
    setTimeout(() => get().dismissToast(id), t.kind === "error" ? 9000 : 5000);
  },
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));

/** After a job: record history, optionally reveal the result, and show the "finished" screen if the user is still there. */
async function finishJob(job: JobState, cancelled: boolean) {
  const st = useApp.getState();
  const mine = (j: JobState) =>
    j.results.flatMap((x) => {
      const o = x.outcome;
      if (!o) return [];
      const from = j.taskNames[x.taskId] ?? [];
      return o.outputs.map((path, i, all) => ({
        path,
        name: baseName(path),
        // one task that wrote several files (a split): label each part; conversions say what they came from
        meta: all.length > 1 ? `Part ${i + 1} of ${all.length}` : j.category === "Convert" && from.length ? `From ${from.length === 1 ? from[0] : `${from.length} files`}` : o.detail || o.note,
      }));
    });
  await recordHistory(job, mine(job));

  // A group finishes when its last job does; report all of it together.
  const group = job.group ? Object.values(st.jobs).filter((j) => j.group === job.group) : [job];
  if (group.some((j) => j.running)) return;
  const results = group.flatMap((j) => j.results);
  const total = group.reduce((n, j) => n + j.total, 0);
  const ok = results.filter((r) => r.outcome);
  const errors = results.filter((r) => r.error).map((r) => r.error!);
  const skipped = results.filter((r) => r.skipped).length;
  const outputs = group.flatMap(mine);
  cancelled = cancelled || group.some((j) => j.cancelled);

  if (cancelled) {
    st.toast({ kind: "warn", text: `Cancelled. ${ok.length} finished before stopping.` });
    return;
  }

  if (st.openAfter && outputs[0]) void revealItemInDir(outputs[0].path).catch(() => {});

  const skipNote = skipped ? `${skipped === 1 ? "1 file was" : `${skipped} files were`} skipped because a file with that name already exists.` : "";
  const title = !ok.length && skipped && !errors.length ? "Nothing was replaced" : errors.length && !ok.length ? "That didn't work" : doneTitle(job, ok.length, outputs.length);
  const note = [
    errors.length && ok.length ? `${errors.length} of ${total} failed. Originals are unchanged.` : errors.length ? "Nothing was saved. Originals are unchanged." : ok.map((r) => r.outcome!.note).find(Boolean) || (skipped ? "" : "Originals are unchanged."),
    skipNote,
  ].filter(Boolean).join(" ");
  const view: DoneView = { title, note, outputs, errors, category: job.category };

  if (useApp.getState().screen === job.screen) useApp.setState({ done: view, screen: "done" });
  else {
    useApp.setState({ done: view });
    if (!ok.length && skipped && !errors.length) st.toast({ kind: "info", text: `${job.label}: ${skipNote}` });
    else if (errors.length && !ok.length) st.toast({ kind: "error", text: `${job.label}: ${errors[0]}` });
    else st.toast({ kind: errors.length ? "warn" : "ok", text: `${job.label} finished. ${outputs.length === 1 ? outputs[0].name : `${outputs.length} files`} saved.` });
  }
}

async function recordHistory(job: JobState, outputs: { path: string; name: string; meta: string }[]) {
  if (!useApp.getState().keepHistory || !outputs.length) return;
  let sizes: Record<string, number> = {};
  try {
    const infos = await inspectFiles(outputs.map((o) => o.path));
    sizes = Object.fromEntries(infos.filter((i) => !i.error).map((i) => [i.path, i.size]));
  } catch {
    /* sizes are optional */
  }
  const now = Date.now();
  const fresh: HistoryEntry[] = outputs.map((o) => ({ path: o.path, name: o.name, action: job.label, category: job.category, meta: o.meta, size: sizes[o.path] ?? null, when: now }));
  const history = [...fresh, ...useApp.getState().history.filter((h) => !fresh.some((f) => f.path === h.path))].slice(0, 300);
  useApp.setState({ history });
  save(HISTORY_KEY, history);
}

function doneTitle(job: JobState, ok: number, files = ok): string {
  const l = job.label;
  if (l.startsWith("PDF to ")) return ok > 1 ? `Converted ${ok} files to ${l.slice(7)}` : `Converted to ${l.slice(7)}`;
  if (l === "Convert to PDF") return ok > 1 ? `Converted ${ok} files to PDF` : "Converted to PDF";
  if (l === "Images to PDF" || l === "Combine into one PDF" || l === "Searchable PDF") return "Your PDF is ready";
  if (l.startsWith("Merge")) return l.replace("Merge", "Merged");
  if (l === "Split PDF") return `Split into ${files} files`;
  if (l === "Delete pages") return "Pages removed";
  if (l === "Extract pages") return "Pages extracted";
  if (l === "Reorder pages") return "Pages rearranged";
  if (l.startsWith("Decompress")) return ok > 1 ? `Decompressed ${ok} files` : "Decompressed";
  if (l.startsWith("Compress")) return ok > 1 ? `Compressed ${ok} files` : "Compressed";
  return `${l}: done`;
}

export { formatSize };
