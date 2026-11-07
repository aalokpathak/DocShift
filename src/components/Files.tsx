import { ArrowDown, ArrowUp, FolderOpen, GripVertical, X } from "lucide-react";
import type { ReactNode } from "react";
import type { Kind } from "../lib/api";
import { formatDuration, formatSize, plural } from "../lib/files";
import { pickFiles, pickFolder } from "../lib/intake";
import { showInFolder } from "../lib/open";
import { extLabel } from "../lib/kinds";
import { useApp, type Item, type JobState } from "../lib/store";
import { Thumb } from "./Thumb";
import { Bar, Button, cx, FileIcon, IconButton } from "./ui";

export function itemMeta(it: Item): string {
  const i = it.info;
  const bits: string[] = [];
  if (i.pages) bits.push(plural(i.pages, "page"));
  if (i.kind === "img" && i.width && i.height) bits.push(`${i.width} × ${i.height}`);
  if (i.kind === "video") {
    if (i.duration) bits.push(formatDuration(i.duration));
    if (i.width && i.height) bits.push(`${i.width} × ${i.height}`);
  }
  bits.push(formatSize(i.size));
  return bits.join(" · ");
}

/** Right-hand side of a file row: size, progress, result or error. */
export function RowStatus({ it }: { it: Item }) {
  if (it.status === "running")
    return (
      <div className="flex w-[200px] shrink-0 flex-col gap-[5px]">
        <Bar value={it.progress || 0.03} />
        <span className="font-mono text-[11.5px] text-faint">{it.progress > 0 ? `${Math.round(it.progress * 100)}%` : "Working…"}</span>
      </div>
    );
  if (it.status === "done")
    return (
      <div className="max-w-[260px] shrink-0 truncate text-right font-mono text-[12.5px] text-acc-ink" title={[it.detail, it.note].filter(Boolean).join("\n")}>
        {it.detail || "Saved"}
      </div>
    );
  if (it.status === "error")
    return (
      <div className="line-clamp-2 max-w-[280px] shrink-0 text-right text-[12px] text-danger" title={it.message}>
        {it.message}
      </div>
    );
  return <div className="shrink-0 text-right font-mono text-[12.5px] text-muted">{formatSize(it.info.size)}</div>;
}

/** A file row in the design's list style. Optional ordering controls for merge / images. */
export function FileLine({
  it,
  index,
  count,
  onRemove,
  onMove,
  dragHandle,
  dragging,
  thumb,
  extra,
}: {
  it: Item;
  index?: number;
  count?: number;
  onRemove?: () => void;
  onMove?: (delta: number) => void;
  dragHandle?: (e: React.PointerEvent) => void;
  dragging?: boolean;
  thumb?: boolean;
  extra?: ReactNode;
}) {
  const busy = it.status === "running";
  const kind = (it.info.kind ?? "pdf") as Kind;
  return (
    <div
      data-sort-index={index}
      className={cx(
        "flex items-center gap-3.5 border-b border-row-line px-3.5 py-[9px] last:border-b-0",
        dragging && "relative z-10 bg-sel shadow-[inset_3px_0_0_var(--acc)]",
      )}
    >
      {dragHandle && (
        <span onPointerDown={dragHandle} title="Drag to reorder" className="grid h-9 w-4 shrink-0 cursor-grab touch-none place-items-center text-faint hover:text-ink active:cursor-grabbing">
          <GripVertical className="size-4" />
        </span>
      )}
      {index !== undefined && <span className="w-4 shrink-0 text-right font-mono text-[12px] font-medium text-faint">{index + 1}</span>}
      {thumb ? (
        <Thumb path={it.info.path} kind={kind} page={kind === "pdf" ? 0 : null} size={120} className="h-14 w-11 shrink-0 rounded-[4px] border border-track" />
      ) : (
        <FileIcon kind={kind} text={extLabel(it.info)} />
      )}
      <div className="min-w-0 flex-1">
        <div className="truncate text-[13px] font-medium" title={it.info.path}>
          {it.info.name}
        </div>
        <div className="truncate text-[12px] text-muted">{itemMeta(it)}</div>
      </div>
      {extra}
      <RowStatus it={it} />
      {it.status === "done" && it.outputs?.[0] && (
        <IconButton label="Show in folder" onClick={() => void showInFolder(it.outputs![0])}>
          <FolderOpen className="size-3.5" />
        </IconButton>
      )}
      {onMove && (
        <>
          <IconButton label="Move up" disabled={busy || index === 0} onClick={() => onMove(-1)}>
            <ArrowUp className="size-3.5" />
          </IconButton>
          <IconButton label="Move down" disabled={busy || index === (count ?? 1) - 1} onClick={() => onMove(1)}>
            <ArrowDown className="size-3.5" />
          </IconButton>
        </>
      )}
      {onRemove && (
        <IconButton label={`Remove ${it.info.name}`} disabled={busy} onClick={onRemove}>
          <X className="size-3.5" />
        </IconButton>
      )}
    </div>
  );
}

/** Dashed "add more" target. Files dropped anywhere on the window land in the current screen too. */
export function AddMore({ text, kinds, onAdd, height = 52, big }: { text: string; kinds: Kind[]; onAdd: (infos: Awaited<ReturnType<typeof pickFiles>>) => void; height?: number; big?: boolean }) {
  const active = useApp((s) => s.dropActive);
  return (
    <button
      onClick={async () => onAdd(await pickFiles(kinds))}
      style={{ minHeight: height }}
      className={cx(
        "flex w-full cursor-pointer flex-col items-center justify-center gap-1 rounded-[10px] border-[1.5px] border-dashed text-[13px] transition-colors",
        active ? "border-acc bg-sel text-acc-ink" : "border-strong-line text-muted hover:border-acc hover:text-ink",
        big && "bg-surface py-10",
      )}
    >
      {big && <span className="text-[17px] font-semibold text-ink">{active ? "Release to add" : text}</span>}
      {big ? <span>or click to choose files</span> : active ? "Release to add" : text}
    </button>
  );
}

/** Footer of every working screen: where results go, then the action (or progress + cancel while running). */
export function SaveAndRun({ label, canRun, onRun, job, hint }: { label: string; canRun: boolean; onRun: () => void; job: JobState | null; hint?: ReactNode }) {
  const outDir = useApp((s) => s.outDir);
  const setSetting = useApp((s) => s.setSetting);
  const cancel = useApp((s) => s.cancel);
  const running = !!job?.running;
  return (
    <div className="flex flex-col gap-2.5 border-t border-row-line pt-3.5">
      <div className="flex items-center justify-between gap-3 text-[12.5px] text-muted">
        <span className="shrink-0">Save to</span>
        <button
          className="min-w-0 cursor-pointer truncate font-mono text-ink hover:text-acc-ink"
          title={`${outDir}\nClick to change`}
          disabled={running}
          onClick={async () => {
            const d = await pickFolder(outDir);
            if (d) setSetting("outDir", d);
          }}
        >
          {outDir || "…"}
        </button>
      </div>
      {hint}
      {running ? (
        <div className="flex flex-col gap-2">
          <div className="flex justify-between text-[12.5px]">
            <span>{job!.cancelled ? "Stopping…" : `Working · ${Math.min(job!.finished + 1, job!.total)} of ${job!.total}`}</span>
            <span className="font-mono text-faint">{Math.round(job!.progress * 100)}%</span>
          </div>
          <Bar value={job!.progress} />
          <Button size="md" disabled={job!.cancelled} onClick={() => cancel(job!.id)}>
            Cancel
          </Button>
        </div>
      ) : (
        <Button variant="primary" size="lg" disabled={!canRun} onClick={onRun}>
          {label}
        </Button>
      )}
    </div>
  );
}

/** Empty state for a screen that has no files yet. */
export function Empty({ title, sub, kinds, onAdd }: { title: string; sub: string; kinds: Kind[]; onAdd: (infos: Awaited<ReturnType<typeof pickFiles>>) => void }) {
  return (
    <div className="flex flex-col gap-2">
      <AddMore text={title} kinds={kinds} onAdd={onAdd} big />
      <p className="text-center text-[12px] text-faint">{sub}</p>
    </div>
  );
}
