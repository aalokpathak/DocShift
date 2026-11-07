// Development only: lets the UI run in a plain browser (npm run dev, open /?mock) with a fake backend, so layouts can be
// reviewed without the native shell. Never included in production builds (guarded by import.meta.env.DEV in main.tsx).
import { emit } from "@tauri-apps/api/event";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { FileInfo } from "./api";

const DAY = 86_400_000;
const now = Date.now();

function file(name: string, kind: FileInfo["kind"], size: number, extra: Partial<FileInfo> = {}): FileInfo {
  return {
    path: `C:\\Users\\You\\Documents\\${name}`, name, ext: name.split(".").pop()!, kind, size,
    created: now - 40 * DAY, modified: now - 12 * DAY, ...extra,
  };
}

const SAMPLE: Record<string, FileInfo> = {
  "Annual report.pdf": file("Annual report.pdf", "pdf", 4_210_000, { pages: 24 }),
  "Contract.pdf": file("Contract.pdf", "pdf", 880_000, { pages: 6, modified: now - 3 * DAY }),
  "Budget.xlsx": file("Budget.xlsx", "excel", 96_000),
  "Proposal.docx": file("Proposal.docx", "word", 240_000),
  "Pitch deck.pptx": file("Pitch deck.pptx", "ppt", 3_900_000),
  "IMG_2041.jpg": file("IMG_2041.jpg", "img", 4_800_000, { width: 4032, height: 3024, taken: now - 30 * DAY, modified: now - 1 * DAY }),
  "IMG_2042.jpg": file("IMG_2042.jpg", "img", 5_100_000, { width: 4032, height: 3024, taken: now - 31 * DAY, modified: now - 2 * DAY }),
  "IMG_10.jpg": file("IMG_10.jpg", "img", 3_200_000, { width: 4032, height: 3024, taken: now - 29 * DAY, modified: now - 4 * DAY }),
  "Receipt scan.png": file("Receipt scan.png", "img", 900_000, { width: 1700, height: 2200, modified: now - 6 * DAY }),
  "Holiday.mp4": file("Holiday.mp4", "video", 128_000_000, { duration: 94, width: 1920, height: 1080 }),
};

function svg(label: string, hue: number, w = 300, h = 220): string {
  const s = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="hsl(${hue},70%,60%)"/><stop offset="1" stop-color="hsl(${hue + 50},70%,45%)"/></linearGradient></defs><rect width="100%" height="100%" fill="url(#g)"/><text x="50%" y="52%" fill="white" font-family="Segoe UI" font-size="${Math.round(h / 4)}" font-weight="700" text-anchor="middle">${label}</text></svg>`;
  return "data:image/svg+xml;base64," + btoa(s);
}
function pageSvg(n: number): string {
  const lines = Array.from({ length: 11 }, (_, i) => `<rect x="24" y="${70 + i * 22}" width="${150 - ((i * 37 + n * 11) % 60)}" height="7" rx="3" fill="#cfd4e6"/>`).join("");
  const s = `<svg xmlns="http://www.w3.org/2000/svg" width="200" height="283" viewBox="0 0 200 283"><rect width="200" height="283" fill="white"/><rect x="24" y="24" width="90" height="14" rx="3" fill="#5457ee"/><rect x="24" y="44" width="60" height="8" rx="3" fill="#9aa1c4"/>${lines}</svg>`;
  return "data:image/svg+xml;base64," + btoa(s);
}

export function installMock() {
  mockWindows("main");
  let seq = 0;
  mockIPC(
    (cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      switch (cmd) {
        case "inspect_files":
          return (a.paths as string[]).map((p) => {
            const name = p.split(/[\\/]/).pop()!;
            return SAMPLE[name] ?? file(name, name.endsWith(".pdf") ? "pdf" : null, 100_000, name.endsWith(".pdf") ? { pages: 3 } : { error: "Not a supported file type." });
          });
        case "thumbnail": {
          const path = String(a.path);
          if (path.toLowerCase().endsWith(".pdf")) return pageSvg(Number(a.page ?? 0) + 1);
          const hue = [...path].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 7);
          return svg(path.split(/[\\/]/).pop()!.slice(0, 8), hue);
        }
        case "pdf_info": {
          const name = String(a.path).split(/[\\/]/).pop()!;
          const pages = SAMPLE[name]?.pages ?? 8;
          return { pages, sizes: Array.from({ length: pages }, (_, i) => (i === 3 ? [842, 595] : [595, 842])), noText: false };
        }
        case "engine_status":
          return { word: true, excel: true, powerpoint: false, libreoffice: false, video: true, pdf: true, ocr: true };
        case "ocr_languages":
          return [{ tag: "en-US", name: "English (United States)" }, { tag: "en-GB", name: "English (United Kingdom)" }];
        case "open_path":
        case "answer_ask":
          return null;
        case "default_output_dir":
          return "C:\\Users\\You\\Downloads";
        case "check_output_dir":
        case "validate_target":
          return null;
        case "validate_pages":
          return [];
        case "cancel_job":
          return null;
        case "start_job": {
          const spec = a.spec as { tasks: { id: string; inputs: string[] }[]; mode: string };
          const jobId = `job${++seq}`;
          setTimeout(async () => {
            for (const t of spec.tasks) {
              await emit("docshift://job", { type: "taskStarted", jobId, taskId: t.id });
              for (let i = 1; i <= 10; i++) {
                await new Promise((r) => setTimeout(r, 140));
                await emit("docshift://job", { type: "progress", jobId, taskId: t.id, fraction: i / 10 });
              }
              const out = t.inputs[0].replace(/\.[^.]+$/, "") + "_done.pdf";
              await emit("docshift://job", { type: "taskDone", jobId, taskId: t.id, outcome: { outputs: [out], note: "", detail: "5.22 MB \u2192 491.0 KB (-91%)" } });
            }
            await emit("docshift://job", { type: "jobDone", jobId, cancelled: false });
          }, 50);
          return jobId;
        }
        default:
          if (cmd.startsWith("plugin:dialog|open")) {
            const opts = (a.options ?? {}) as { filters?: { extensions: string[] }[]; directory?: boolean };
            if (opts.directory) return "C:\Users\You\Documents\Converted";
            const exts = opts.filters?.[0]?.extensions ?? [];
            const all = Object.values(SAMPLE);
            const pick = exts.length > 30 ? all.slice(0, 9) : exts.includes("jpg") && exts.length < 12 ? all.filter((f) => ["jpg", "png"].includes(f.ext)) : exts.includes("pdf") && exts.length === 1 ? all.filter((f) => f.ext === "pdf") : exts.includes("mp4") ? all.filter((f) => f.ext === "mp4") : all.slice(0, 5);
            return pick.map((f) => f.path);
          }
          if (cmd.startsWith("plugin:opener|")) return null;
          return null; // window theme etc.
      }
    },
    { shouldMockEvents: true },
  );
}
