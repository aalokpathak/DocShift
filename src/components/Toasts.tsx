import { X } from "lucide-react";
import { useApp } from "../lib/store";
import { cx } from "./ui";

export function Toasts() {
  const toasts = useApp((s) => s.toasts);
  const dismiss = useApp((s) => s.dismissToast);
  return (
    <div className="pointer-events-none fixed bottom-4 right-4 z-40 flex w-[360px] flex-col gap-2" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className="card fade-up pointer-events-auto flex items-start gap-3 p-3 shadow-[0_8px_30px_rgba(0,0,0,.12)]">
          <span
            className={cx("mt-1.5 size-2 shrink-0 rounded-full", t.kind === "ok" ? "bg-acc2" : t.kind === "error" ? "bg-danger" : t.kind === "warn" ? "bg-warn-ink" : "bg-faint")}
          />
          <div className="flex-1 text-[13px] leading-snug">{t.text}</div>
          <button aria-label="Dismiss" onClick={() => dismiss(t.id)} className="cursor-pointer text-faint hover:text-ink">
            <X className="size-4" />
          </button>
        </div>
      ))}
    </div>
  );
}
