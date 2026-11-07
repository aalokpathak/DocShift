import { Search } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { NAV, openDest, TOOL_GROUPS, type Dest } from "../lib/catalog";
import { useApp } from "../lib/store";
import { Chip, cx } from "./ui";

interface Entry {
  title: string;
  hint: string;
  dest: Dest;
  chips: { kind: Parameters<typeof Chip>[0]["kind"]; text: string }[];
  keywords: string;
}

const ENTRIES: Entry[] = [
  ...TOOL_GROUPS.flatMap((g) => g.items.map((t) => ({ title: t.title, hint: `${g.title} · ${t.desc}`, dest: t.dest, chips: t.chips, keywords: t.keywords }))),
  ...NAV.flatMap((g) => g.items)
    .filter((i) => ["home", "smart", "queue", "history", "settings"].includes(i.id))
    .map((i) => ({ title: i.label, hint: "Go to", dest: i.dest, chips: [], keywords: i.id })),
];

/** Ctrl K: type to find any tool or screen. */
export function Palette() {
  const open = useApp((s) => s.paletteOpen);
  const setOpen = useApp((s) => s.setPalette);
  const [q, setQ] = useState("");
  const [idx, setIdx] = useState(0);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setQ("");
      setIdx(0);
      setTimeout(() => input.current?.focus(), 0);
    }
  }, [open]);

  const results = useMemo(() => {
    const words = q.toLowerCase().split(/\s+/).filter(Boolean);
    return ENTRIES.filter((e) => {
      const hay = `${e.title} ${e.hint} ${e.keywords}`.toLowerCase();
      return words.every((w) => hay.includes(w));
    });
  }, [q]);

  if (!open) return null;
  const pick = (e: Entry) => {
    setOpen(false);
    openDest(e.dest);
  };

  return (
    <div className="fixed inset-0 z-50 flex justify-center bg-black/25 pt-[12vh]" onMouseDown={() => setOpen(false)}>
      <div className="card fade-up h-fit w-[520px] overflow-hidden shadow-[0_20px_60px_rgba(0,0,0,.25)]" onMouseDown={(e) => e.stopPropagation()} role="dialog" aria-label="Search tools">
        <div className="flex items-center gap-2 border-b border-row-line px-3.5">
          <Search className="size-4 text-faint" />
          <input
            ref={input}
            value={q}
            onChange={(e) => {
              setQ(e.target.value);
              setIdx(0);
            }}
            onKeyDown={(e) => {
              if (e.key === "Escape") setOpen(false);
              else if (e.key === "ArrowDown") {
                e.preventDefault();
                setIdx((i) => Math.min(i + 1, results.length - 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setIdx((i) => Math.max(i - 1, 0));
              } else if (e.key === "Enter" && results[idx]) pick(results[idx]);
            }}
            placeholder="Search tools, e.g. merge, compress video, word"
            className="h-11 flex-1 bg-transparent text-[14px] outline-none placeholder:text-faint"
            aria-label="Search tools"
          />
          <span className="font-mono text-[11px] text-faint">Esc</span>
        </div>
        <div className="max-h-[360px] overflow-y-auto p-1.5">
          {results.length === 0 && <div className="px-3 py-6 text-center text-[13px] text-muted">No tool matches “{q}”.</div>}
          {results.map((e, i) => (
            <button
              key={e.title}
              onMouseEnter={() => setIdx(i)}
              onClick={() => pick(e)}
              className={cx("flex w-full cursor-pointer items-center gap-3 rounded-[8px] px-3 py-2 text-left", i === idx ? "bg-sel" : "")}
            >
              <span className="flex w-[92px] shrink-0 items-center gap-1">
                {e.chips.map((c, k) => (
                  <span key={k} className="flex items-center gap-1">
                    {k > 0 && <span className="text-[11px] text-faint">→</span>}
                    <Chip kind={c.kind} text={c.text} small />
                  </span>
                ))}
              </span>
              <span className="flex-1">
                <span className="block text-[13px] font-medium">{e.title}</span>
                <span className="block text-[12px] text-muted">{e.hint}</span>
              </span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
