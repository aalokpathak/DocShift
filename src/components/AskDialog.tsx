import { useEffect, useRef, useState } from "react";
import { useApp } from "../lib/store";
import { Button } from "./ui";

/** Shown while a job waits on "a file with this name already exists" (Settings > If a file exists > Ask). */
export function AskDialog() {
  const ask = useApp((s) => s.asks[0]);
  const answer = useApp((s) => s.answer);
  const [all, setAll] = useState(false);
  const first = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    setAll(false);
    first.current?.focus();
  }, [ask?.askId]);

  if (!ask) return null;
  const pick = (c: "replace" | "keep" | "skip") => answer(ask.askId, c, all);

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-black/30 p-6" role="dialog" aria-modal="true" aria-labelledby="ask-title">
      <div className="card fade-up flex w-[440px] max-w-full flex-col gap-3.5 p-5 shadow-[0_12px_40px_rgba(0,0,0,.18)]" onKeyDown={(e) => e.key === "Escape" && pick("skip")}>
        <div id="ask-title" className="text-[16px] font-semibold tracking-[-0.01em]">
          This file already exists
        </div>
        <div className="text-[13px] leading-relaxed text-muted">
          <span className="font-medium text-ink">{ask.name}</span> is already in <span className="break-all font-mono text-[12px] text-ink2">{ask.folder}</span>.
        </div>
        <label className="flex cursor-pointer items-center gap-2 text-[13px] text-ink2">
          <input type="checkbox" checked={all} onChange={(e) => setAll(e.target.checked)} className="size-4 accent-[var(--acc)]" />
          Do the same for the rest of this batch
        </label>
        <div className="mt-1 flex justify-end gap-2">
          <Button variant="ghost" onClick={() => pick("skip")}>
            Skip
          </Button>
          <Button onClick={() => pick("keep")}>Keep both</Button>
          <Button ref={first} variant="primary" onClick={() => pick("replace")}>
            Replace
          </Button>
        </div>
      </div>
    </div>
  );
}
