import { RotateCcw, RotateCw, Trash2, Undo2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Empty } from "../components/Files";
import { Thumb } from "../components/Thumb";
import { Button, cx, PageTitle, Seg } from "../components/ui";
import { useSortable } from "../components/useSortable";
import { pdfInfo, type PdfInfo } from "../lib/api";
import { formatSize, plural } from "../lib/files";
import { pickFiles, pickFolder } from "../lib/intake";
import { groupLabel, parseRanges, type PageSel } from "../lib/pages";
import { useLatestJob } from "../lib/runner";
import { useApp, type OrganizeMode } from "../lib/store";

interface OrgPage {
  index: number;
  rotate: number;
  removed: boolean;
}

const HINT: Record<OrganizeMode, string> = {
  delete: "Click pages to remove them.",
  split: "Click a page to split after it.",
  extract: "Click pages to pick them.",
  reorder: "Drag pages into a new order. Rotate or remove them with the buttons under each page.",
};

export function Organize() {
  const mode = useApp((s) => s.organizeMode);
  const go = useApp((s) => s.go);
  const items = useApp((s) => s.lists.organize);
  const setFiles = useApp((s) => s.setFiles);
  const runJob = useApp((s) => s.runJob);
  const toast = useApp((s) => s.toast);
  const outDir = useApp((s) => s.outDir);
  const setSetting = useApp((s) => s.setSetting);
  const cancel = useApp((s) => s.cancel);
  const job = useLatestJob("organize");
  const running = !!job?.running;
  const file = items[0];

  const [info, setInfo] = useState<PdfInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [del, setDel] = useState<Set<number>>(new Set());
  const [ext, setExt] = useState<Set<number>>(new Set());
  const [splits, setSplits] = useState<Set<number>>(new Set()); // split AFTER these page indexes
  const [org, setOrg] = useState<OrgPage[]>([]);
  const [typed, setTyped] = useState("");
  const [every, setEvery] = useState("");

  useEffect(() => {
    setInfo(null);
    setError(null);
    setDel(new Set());
    setExt(new Set());
    setSplits(new Set());
    if (!file) return;
    let live = true;
    pdfInfo(file.info.path).then(
      (i) => {
        if (!live) return;
        setInfo(i);
        setOrg(Array.from({ length: i.pages }, (_, index) => ({ index, rotate: 0, removed: false })));
      },
      (e) => live && setError(typeof e === "string" ? e : "This PDF could not be read."),
    );
    return () => {
      live = false;
    };
  }, [file?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const n = info?.pages ?? 0;
  const toggle = (set: (f: (s: Set<number>) => Set<number>) => void, i: number) =>
    set((s) => {
      const next = new Set(s);
      if (next.has(i)) next.delete(i);
      else next.add(i);
      return next;
    });

  const ranges = useMemo(() => {
    const cuts = [...splits].filter((i) => i < n - 1).sort((a, b) => a - b);
    const groups: number[][] = [];
    let start = 0;
    for (const c of [...cuts, n - 1]) {
      groups.push(Array.from({ length: c - start + 1 }, (_, k) => start + k));
      start = c + 1;
    }
    return n ? groups : [];
  }, [splits, n]);

  const move = (from: number, to: number) =>
    setOrg((l) => {
      const next = [...l];
      const [m] = next.splice(from, 1);
      next.splice(to, 0, m);
      return next;
    });
  const { dragging, containerRef, startDrag } = useSortable(org.length, move);

  // ---- what the button will do
  const plan = useMemo((): { groups: PageSel[][]; tag: string; summary: string; button: string } | { error: string } => {
    const sel = (pages: number[]) => pages.map((index) => ({ index, rotate: 0 }));
    if (!n) return { error: "" };
    if (mode === "delete") {
      const keep = Array.from({ length: n }, (_, i) => i).filter((i) => !del.has(i));
      if (!del.size) return { error: "Click the pages you want to remove." };
      if (!keep.length) return { error: "That would remove every page." };
      return { groups: [sel(keep)], tag: "edited", summary: `${plural(del.size, "page")} removed · ${keep.length} remain`, button: "Save new PDF" };
    }
    if (mode === "extract") {
      const pages = [...ext].sort((a, b) => a - b);
      if (!pages.length) return { error: "Click the pages you want to keep." };
      return { groups: [sel(pages)], tag: "extract", summary: `${plural(pages.length, "page")} selected: ${groupLabel(pages)}`, button: "Extract to new PDF" };
    }
    if (mode === "split") {
      if (ranges.length < 2) return { error: "Click a page to split after it." };
      return {
        groups: ranges.map(sel),
        tag: "part",
        summary: `Splits into ${ranges.length} files: pages ${ranges.map(groupLabel).join(", ")}`,
        button: `Split into ${ranges.length} files`,
      };
    }
    const kept = org.filter((p) => !p.removed);
    if (!kept.length) return { error: "Every page is removed." };
    if (!org.some((p, i) => p.index !== i || p.rotate || p.removed)) return { error: "Move, rotate or remove a page first." };
    return { groups: [kept.map((p) => ({ index: p.index, rotate: p.rotate }))], tag: "organized", summary: `${plural(kept.length, "page")} in the new order`, button: "Save new PDF" };
  }, [mode, n, del, ext, ranges, org]);

  const run = () => {
    if (!file || "error" in plan) return;
    void runJob({
      list: "organize",
      screen: "organize",
      label: { delete: "Delete pages", extract: "Extract pages", split: "Split PDF", reorder: "Reorder pages" }[mode],
      category: "Organize",
      spec: { mode: "pdf_pages", tasks: [{ id: "pages", inputs: [file.info.path] }], options: { groups: plan.groups, tag: plan.tag } },
      taskItems: { pages: [file.id] },
    });
  };

  const reset = () => {
    if (mode === "delete") setDel(new Set());
    else if (mode === "extract") setExt(new Set());
    else if (mode === "split") setSplits(new Set());
    else setOrg(Array.from({ length: n }, (_, index) => ({ index, rotate: 0, removed: false })));
  };

  const applyTyped = () => {
    const r = parseRanges(typed, n);
    if (!r.ok) return toast({ kind: "warn", text: r.error });
    const pages = new Set(r.groups.flat());
    if (mode === "delete") setDel(pages);
    else setExt(pages);
  };
  const applyEvery = () => {
    const k = Number(every);
    if (!Number.isInteger(k) || k < 1) return toast({ kind: "warn", text: "Enter a whole number of pages, 1 or more." });
    setSplits(new Set(Array.from({ length: n }, (_, i) => i).filter((i) => (i + 1) % k === 0)));
  };

  const open = async () => {
    const picked = await pickFiles(["pdf"]);
    if (!picked.length) return;
    setFiles("organize", picked.slice(0, 1));
    if (picked.length > 1) toast({ kind: "info", text: "Pages are edited one PDF at a time, so the first one was opened." });
  };

  return (
    <div className="flex flex-col gap-5">
      <PageTitle
        title={mode === "reorder" ? "Reorder & rotate pages" : "Split & delete pages"}
        sub={file ? `${file.info.name} · ${info ? plural(info.pages, "page") : "reading…"}. ${HINT[mode]}` : "Open a PDF to work with its pages."}
      >
        <Seg
          value={mode}
          onChange={(m) => go("organize", { organizeMode: m })}
          options={[["Delete", "delete"], ["Split", "split"], ["Extract", "extract"], ["Reorder", "reorder"]]}
          label="Mode"
        />
      </PageTitle>

      {!file ? (
        <Empty title="Drop a PDF here" sub="Its pages appear as thumbnails so you can pick, cut or arrange them." kinds={["pdf"]} onAdd={(f) => f.length && setFiles("organize", f.slice(0, 1))} />
      ) : (
        <>
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-[12.5px] text-muted">{formatSize(file.info.size)}</span>
            <Button size="sm" disabled={running} onClick={open}>
              Open another PDF
            </Button>
            <span className="flex-1" />
            {(mode === "delete" || mode === "extract") && info && (
              <>
                <input
                  className="field !h-[30px] !w-44 text-[12.5px]"
                  placeholder="Type pages, e.g. 1-3, 7"
                  value={typed}
                  onChange={(e) => setTyped(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && applyTyped()}
                  aria-label="Pages"
                />
                <Button size="sm" onClick={applyTyped}>
                  Select
                </Button>
              </>
            )}
            {mode === "split" && info && (
              <>
                <span className="text-[12.5px] text-muted">Every</span>
                <input
                  className="field !h-[30px] !w-14 text-center text-[12.5px]"
                  inputMode="numeric"
                  value={every}
                  onChange={(e) => /^\d{0,4}$/.test(e.target.value) && setEvery(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && applyEvery()}
                  aria-label="Pages per file"
                />
                <span className="text-[12.5px] text-muted">pages</span>
                <Button size="sm" onClick={applyEvery}>
                  Apply
                </Button>
                <Button size="sm" onClick={() => setSplits(new Set(Array.from({ length: n }, (_, i) => i)))}>
                  Each page
                </Button>
              </>
            )}
          </div>

          {error && <div className="rounded-[10px] bg-danger-soft p-3 text-[13px] text-danger">{error}</div>}

          {info && mode !== "reorder" && (
            <div className="card grid grid-cols-[repeat(auto-fill,minmax(104px,1fr))] gap-x-[18px] gap-y-[22px] p-[22px]">
              {Array.from({ length: n }, (_, i) => {
                const removed = mode === "delete" && del.has(i);
                const checked = mode === "extract" && ext.has(i);
                const cut = mode === "split" && splits.has(i) && i < n - 1;
                const [w, h] = info.sizes[i] ?? [595, 842];
                return (
                  <button
                    key={i}
                    disabled={running}
                    onClick={() => toggle(mode === "delete" ? setDel : mode === "extract" ? setExt : setSplits, i)}
                    className="relative flex cursor-pointer flex-col items-center gap-1.5"
                    aria-pressed={removed || checked || cut}
                    aria-label={`Page ${i + 1}`}
                  >
                    <div
                      className={cx("relative w-full overflow-hidden rounded-[4px] border-2 bg-white", removed ? "border-danger" : checked ? "border-acc" : "border-track")}
                    >
                      <Thumb path={file.info.path} kind="pdf" page={i} size={220} fit="contain" aspect={w / h} className={cx("w-full", removed && "opacity-55")} />
                      {removed && (
                        <span className="absolute inset-0 grid place-items-center">
                          <span className="rounded-[4px] bg-danger px-1.5 py-[3px] font-mono text-[10px] font-medium text-white">REMOVE</span>
                        </span>
                      )}
                      {checked && <span className="absolute right-[5px] top-[5px] grid size-[18px] place-items-center rounded-full bg-acc text-[11px] text-white">✓</span>}
                    </div>
                    {cut && <span className="absolute -right-[11px] bottom-[18px] -top-1.5 w-[3px] rounded-[2px] bg-acc" />}
                    <span className="font-mono text-[12px] text-muted">{i + 1}</span>
                  </button>
                );
              })}
            </div>
          )}

          {info && mode === "reorder" && (
            <div ref={containerRef as React.RefObject<HTMLDivElement>} className="card grid grid-cols-[repeat(auto-fill,minmax(112px,1fr))] gap-x-[18px] gap-y-[22px] p-[22px]">
              {org.map((p, pos) => {
                const [w, h] = info.sizes[p.index] ?? [595, 842];
                return (
                  <div
                    key={p.index}
                    data-sort-index={pos}
                    onPointerDown={running ? undefined : startDrag(pos)}
                    className={cx("flex cursor-grab touch-none flex-col items-center gap-1.5 active:cursor-grabbing", dragging === pos && "opacity-70")}
                  >
                    <div className={cx("w-full overflow-hidden rounded-[4px] border-2 bg-white", p.removed ? "border-danger" : dragging === pos ? "border-acc" : "border-track")}>
                      <Thumb path={file.info.path} kind="pdf" page={p.index} size={220} fit="contain" rotate={p.rotate} aspect={w / h} className={cx("w-full", p.removed && "opacity-40")} />
                    </div>
                    <div className="flex w-full items-center justify-between" onPointerDown={(e) => e.stopPropagation()}>
                      <span className="font-mono text-[12px] text-muted">
                        {pos + 1}
                        {p.index !== pos && <span className="text-faint"> ({p.index + 1})</span>}
                      </span>
                      <span className="flex">
                        <button aria-label="Rotate left" className="cursor-pointer rounded p-0.5 text-faint hover:text-ink" onClick={() => setOrg((l) => l.map((x, i) => (i === pos ? { ...x, rotate: (x.rotate + 270) % 360 } : x)))}>
                          <RotateCcw className="size-3.5" />
                        </button>
                        <button aria-label="Rotate right" className="cursor-pointer rounded p-0.5 text-faint hover:text-ink" onClick={() => setOrg((l) => l.map((x, i) => (i === pos ? { ...x, rotate: (x.rotate + 90) % 360 } : x)))}>
                          <RotateCw className="size-3.5" />
                        </button>
                        <button
                          aria-label={p.removed ? "Restore page" : "Remove page"}
                          className={cx("cursor-pointer rounded p-0.5", p.removed ? "text-acc-ink" : "text-faint hover:text-danger")}
                          onClick={() => setOrg((l) => l.map((x, i) => (i === pos ? { ...x, removed: !x.removed } : x)))}
                        >
                          {p.removed ? <Undo2 className="size-3.5" /> : <Trash2 className="size-3.5" />}
                        </button>
                      </span>
                    </div>
                  </div>
                );
              })}
            </div>
          )}

          {info && (
            <div className="card sticky bottom-0 z-10 flex items-center justify-between gap-5 px-[18px] py-3.5 shadow-[0_-6px_20px_rgba(0,0,0,.08)]">
              <div className="min-w-0 text-[13px]">
                {running ? (
                  <span>Working… {Math.round((job?.progress ?? 0) * 100)}%</span>
                ) : "error" in plan ? (
                  <span className="text-muted">{plan.error}</span>
                ) : (
                  plan.summary
                )}
                <div className="mt-0.5 truncate text-[12px] text-faint" title={outDir}>
                  Saves to{" "}
                  <button
                    className="cursor-pointer font-mono hover:text-acc-ink"
                    disabled={running}
                    onClick={async () => {
                      const d = await pickFolder(outDir);
                      if (d) setSetting("outDir", d);
                    }}
                  >
                    {outDir}
                  </button>
                  . The original is left unchanged.
                </div>
              </div>
              <div className="flex shrink-0 gap-2">
                {running ? (
                  <Button size="lg" onClick={() => job && cancel(job.id)}>
                    Cancel
                  </Button>
                ) : (
                  <>
                    <Button size="lg" onClick={reset}>
                      Reset
                    </Button>
                    <Button size="lg" variant="primary" disabled={"error" in plan} onClick={run}>
                      {"error" in plan ? { delete: "Save new PDF", extract: "Extract to new PDF", split: "Split PDF", reorder: "Save new PDF" }[mode] : plan.button}
                    </Button>
                  </>
                )}
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}
