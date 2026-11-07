import { useMemo, useState } from "react";
import { FileIcon, PageTitle, Seg, cx } from "../components/ui";
import { formatSize } from "../lib/files";
import { useApp, type HistoryEntry } from "../lib/store";
import { errText, openFile, type Kind } from "../lib/api";
import { showInFolder } from "../lib/open";

type Col = "File" | "Action" | "Size" | "When";
const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

const kindOf = (name: string): Kind | "txt" => {
  const e = name.split(".").pop()?.toLowerCase() ?? "";
  if (e === "txt") return "txt";
  return e === "pdf" ? "pdf" : e.startsWith("doc") ? "word" : e.startsWith("xls") ? "excel" : e.startsWith("ppt") ? "ppt" : ["mp4", "mkv", "mov"].includes(e) ? "video" : "img";
};

function when(ms: number): string {
  const d = new Date(ms);
  const today = new Date();
  const yest = new Date(today.getTime() - 86_400_000);
  const time = d.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  if (d.toDateString() === today.toDateString()) return `Today, ${time}`;
  if (d.toDateString() === yest.toDateString()) return "Yesterday";
  return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

export function History() {
  const history = useApp((s) => s.history);
  const keep = useApp((s) => s.keepHistory);
  const remove = useApp((s) => s.removeHistory);
  const toast = useApp((s) => s.toast);
  const [cat, setCat] = useState<"All" | HistoryEntry["category"]>("All");
  const [sort, setSort] = useState<Col>("When");
  const [asc, setAsc] = useState(false);

  const rows = useMemo(() => {
    const key = (h: HistoryEntry): string | number => (sort === "File" ? h.name : sort === "Action" ? h.action : sort === "Size" ? (h.size ?? 0) : h.when);
    return history
      .filter((h) => cat === "All" || h.category === cat)
      .sort((a, b) => {
        const x = key(a);
        const y = key(b);
        const c = typeof x === "string" ? collator.compare(x, y as string) : x - (y as number);
        return asc ? c : -c;
      });
  }, [history, cat, sort, asc]);

  const header = (c: Col) => (
    <button
      onClick={() => {
        if (sort === c) setAsc(!asc);
        else {
          setSort(c);
          setAsc(c === "File" || c === "Action");
        }
      }}
      className={cx("eyebrow cursor-pointer text-left", c === "Size" && "text-right", sort === c && "!text-ink")}
    >
      {c} {sort === c ? (asc ? "↑" : "↓") : ""}
    </button>
  );

  const open = async (h: HistoryEntry) => {
    try {
      await openFile(h.path);
    } catch (e) {
      const gone = errText(e).includes("isn't there");
      toast({ kind: gone ? "warn" : "error", text: gone ? `${h.name} isn't there any more. It may have been moved or deleted.` : errText(e) });
      if (gone) remove(h.path);
    }
  };

  return (
    <div className="flex flex-col gap-[18px]">
      <PageTitle title="Recent files" sub={keep ? "Files DocShift made, kept on this computer for 30 days. Clear anytime in Settings." : "The recent files list is turned off in Settings."}>
        <Seg value={cat} onChange={setCat} options={[["All", "All"], ["Convert", "Convert"], ["Organize", "Organize"], ["Compress", "Compress"]]} label="Filter" />
      </PageTitle>
      <div className="card overflow-hidden">
        <div className="grid grid-cols-[36px_minmax(0,1fr)_170px_90px_110px_110px] gap-3.5 border-b border-track px-3.5 py-2.5">
          <span />
          {header("File")}
          {header("Action")}
          {header("Size")}
          {header("When")}
          <span />
        </div>
        {rows.length === 0 && <div className="px-4 py-10 text-center text-[13px] text-muted">Nothing here yet. Results you create will be listed here.</div>}
        {rows.map((h) => (
          <div key={h.path} className="grid grid-cols-[36px_minmax(0,1fr)_170px_90px_110px_110px] items-center gap-3.5 border-b border-row-line px-3.5 py-2 last:border-b-0 hover:bg-soft">
            <FileIcon kind={kindOf(h.name)} text={(h.name.split(".").pop() ?? "").toUpperCase().slice(0, 4)} />
            <div className="min-w-0">
              <div className="truncate text-[13px] font-medium" title={h.path}>
                {h.name}
              </div>
              <div className="truncate text-[12px] text-muted">{h.meta}</div>
            </div>
            <span className="truncate text-[13px] text-ink2">{h.action}</span>
            <span className="text-right font-mono text-[12.5px] text-muted">{h.size != null ? formatSize(h.size) : "—"}</span>
            <span className="text-[13px] text-muted">{when(h.when)}</span>
            <span className="flex justify-end gap-3 text-[13px]">
              <button className="cursor-pointer text-acc hover:text-acc-ink" onClick={() => void open(h)}>
                Open
              </button>
              <button className="cursor-pointer text-muted hover:text-ink" onClick={() => void showInFolder(h.path)}>
                Folder
              </button>
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
