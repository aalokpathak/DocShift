import { pickFolder } from "../lib/intake";
import { AddMore, Empty, itemMeta } from "../components/Files";
import { Bar, Button, cx, FileIcon, IconButton, PageTitle, Ring, Seg } from "../components/ui";
import { formatSize } from "../lib/files";
import { extLabel } from "../lib/kinds";
import { runEach, runnable, useLatestJob } from "../lib/runner";
import { useApp, type CompressTab, type Item, type Level, type ListKey } from "../lib/store";
import { X } from "lucide-react";
import type { Kind } from "../lib/api";

const TABS: Record<CompressTab, { list: ListKey; mode: string; kind: Kind; noun: string; scope: string }> = {
  pdf: { list: "compress-pdf", mode: "pdf_compress", kind: "pdf", noun: "PDFs", scope: "compress-pdf" },
  img: { list: "compress-img", mode: "img_compress", kind: "img", noun: "images", scope: "compress-img" },
  video: { list: "compress-video", mode: "video_compress", kind: "video", noun: "videos", scope: "compress-video" },
};

const LEVELS: Record<CompressTab, [Level, string, string][]> = {
  pdf: [["light", "Light", "Best quality, small savings"], ["balanced", "Balanced", "Good for email and sharing"], ["strong", "Smallest", "Lower image quality"]],
  img: [["light", "Light", "No visible difference"], ["balanced", "Balanced", "Great for web and chat"], ["strong", "Smallest", "Thumbnails and previews"]],
  video: [["light", "Light", "Near-original quality"], ["balanced", "Balanced", "Good for sharing, up to 1080p"], ["strong", "Smallest", "Quick to send, up to 720p"]],
};

/** "5.24 MB → 495.1 KB (-91%)" -> 0.09 */
function ratioOf(detail?: string): number | null {
  const m = detail?.match(/\(([+-]?\d+)%\)/);
  return m ? Math.max(0, 1 + Number(m[1]) / 100) : null;
}

export function Compress() {
  const tab = useApp((s) => s.compressTab);
  const go = useApp((s) => s.go);
  const t = TABS[tab];
  const items = useApp((s) => s.lists[t.list]);
  const opts = useApp((s) => s.options[t.scope]) ?? {};
  const defLevel = useApp((s) => s.defLevel);
  const setOpt = useApp((s) => s.setOpt);
  const addToList = useApp((s) => s.addToList);
  const removeFromList = useApp((s) => s.removeFromList);
  const engines = useApp((s) => s.engines);
  const outDir = useApp((s) => s.outDir);
  const setSetting = useApp((s) => s.setSetting);
  const cancel = useApp((s) => s.cancel);
  const job = useLatestJob(t.list);

  const o = <T,>(k: string, d: T) => (opts[k] === undefined ? d : (opts[k] as T));
  const set = (k: string, v: unknown) => setOpt(t.scope, k, v);
  const action = o<"compress" | "expand">("action", "compress");
  const level = o<Level>("level", defLevel);
  const target = o("target_mb", "");
  const expand = action === "expand";
  const todo = runnable(items);
  const running = !!job?.running;
  const before = items.reduce((n, i) => n + i.info.size, 0);
  const done = items.filter((i) => i.status === "done" && ratioOf(i.detail) !== null);
  const after = done.reduce((n, i) => n + i.info.size * ratioOf(i.detail)!, 0);
  const doneBefore = done.reduce((n, i) => n + i.info.size, 0);
  const add = (infos: Parameters<typeof addToList>[1]) => {
    const ok = infos.filter((i) => i.kind === t.kind);
    if (ok.length < infos.length) useApp.getState().toast({ kind: "warn", text: `This tab compresses ${t.noun}. Switch tabs for other files.` });
    addToList(t.list, ok);
  };

  const run = () =>
    runEach({
      list: t.list,
      screen: "compress",
      label: expand ? `Decompress ${tab === "img" ? "images" : tab === "pdf" ? "PDF" : "video"}` : `Compress ${tab === "img" ? "images" : tab === "pdf" ? "PDF" : "video"}`,
      category: "Compress",
      mode: t.mode,
      items: todo,
      options: { action, level, target_mb: expand ? "" : target, format: o("format", "same"), expand_format: o("expand_format", "png"), resolution: tab === "video" ? o("resolution", 0) : 0 },
    });

  const verb = expand ? "Decompress" : "Compress";
  return (
    <div className="flex flex-col gap-[18px]">
      <PageTitle title={verb} sub={expand ? "Rewrite files in a lossless or uncompressed form, for editing or archiving." : "Make files smaller to email, upload or store."}>
        <Seg value={tab} onChange={(v) => go("compress", { compressTab: v })} options={[["PDF", "pdf"], ["Images", "img"], ["Video", "video"]]} label="File type" />
      </PageTitle>

      {tab === "video" && engines && !engines.video && <div className="rounded-[10px] bg-danger-soft p-3 text-[13px] text-danger">The video engine couldn't be started, so video tools aren't available.</div>}

      <div className="flex flex-wrap items-center gap-x-6 gap-y-3">
        <div className="flex items-center gap-2.5">
          <span className="text-[13px]">Mode</span>
          <Seg size="sm" value={action} onChange={(v) => set("action", v)} options={[["Compress", "compress"], ["Decompress", "expand"]]} label="Mode" />
        </div>
        {!expand && (
          <div className="flex items-center gap-2.5">
            <span className="text-[13px]">Size limit</span>
            <div className="relative">
              <input
                className="field !h-[32px] !w-[110px] pr-9 font-mono"
                inputMode="decimal"
                placeholder="None"
                value={target}
                onChange={(e) => /^\d{0,5}([.,]\d{0,3})?$/.test(e.target.value) && set("target_mb", e.target.value)}
                aria-label="Size limit in MB"
              />
              <span className="pointer-events-none absolute right-2.5 top-1/2 -translate-y-1/2 font-mono text-[12px] text-faint">MB</span>
            </div>
            <span className="max-w-[300px] text-[12px] leading-snug text-muted">
              {target ? "Keeps the best quality that fits under this size." : "Optional. For upload forms with a size limit."}
            </span>
          </div>
        )}
        {tab === "video" && !expand && (
          <div className="flex items-center gap-2.5">
            <span className="text-[13px]">Resolution</span>
            <Seg
              size="sm"
              value={o("resolution", 0)}
              onChange={(v) => set("resolution", v)}
              options={[["Auto", 0], ["1080p", 1080], ["720p", 720], ["480p", 480]]}
              label="Resolution"
            />
          </div>
        )}
        {tab === "img" && (
          <div className="flex items-center gap-2.5">
            <span className="text-[13px]">Save as</span>
            {expand ? (
              <Seg size="sm" value={o("expand_format", "png")} onChange={(v) => set("expand_format", v)} options={[["PNG", "png"], ["TIFF", "tiff"], ["BMP", "bmp"]]} />
            ) : (
              <Seg size="sm" value={o("format", "same")} onChange={(v) => set("format", v)} options={[["Keep format", "same"], ["JPG", "jpg"], ["WebP", "webp"], ["PNG", "png"]]} />
            )}
          </div>
        )}
      </div>

      {!expand && (
        <div className={cx("grid grid-cols-3 gap-2.5 transition-opacity", target && "opacity-50")}>
          {LEVELS[tab].map(([id, title, desc]) => {
            const on = level === id && !target;
            return (
              <button
                key={id}
                onClick={() => {
                  set("level", id);
                  set("target_mb", "");
                }}
                className={cx("flex cursor-pointer flex-col gap-1 rounded-[10px] border-[1.5px] bg-surface px-4 py-3.5 text-left", on ? "border-acc" : "border-card-line hover:border-strong-line")}
                role="radio"
                aria-checked={on}
              >
                <span className="flex items-center justify-between">
                  <span className="text-[14px] font-medium">{title}</span>
                  <Ring on={on} />
                </span>
                <span className="text-[12.5px] text-muted">{desc}</span>
              </button>
            );
          })}
        </div>
      )}
      {expand && (
        <p className="text-[12.5px] text-muted">
          {tab === "pdf" && "Every stream is stored uncompressed: bigger, but easy to inspect or edit."}
          {tab === "img" && "Saved in a lossless or uncompressed format. Detail lost to earlier compression can't be recovered."}
          {tab === "video" && "Saved losslessly (FFV1 in MKV). Expect a file many times larger; quality isn't improved."}
        </p>
      )}

      {items.length === 0 ? (
        <Empty title={`Drop ${t.noun} here`} sub={tab === "video" ? "MP4, MOV, MKV, AVI, WebM and more." : tab === "img" ? "JPG, PNG, WebP, HEIC, AVIF, BMP, TIFF." : "Any PDF."} kinds={[t.kind]} onAdd={add} />
      ) : (
        <div className="card overflow-hidden">
          {items.map((it) => (
            <CompRow key={it.id} it={it} onRemove={() => removeFromList(t.list, it.id)} />
          ))}
          <div className="border-t border-row-line px-3.5 py-2">
            <AddMore text={`Add more ${t.noun}`} kinds={[t.kind]} onAdd={add} height={40} />
          </div>
          <div className="flex items-center justify-between gap-4 bg-soft px-4 py-3.5">
            <div className="min-w-0 text-[13px]">
              {running ? (
                <div className="flex w-[320px] flex-col gap-1.5">
                  <span>
                    Working · {Math.min(job!.finished + 1, job!.total)} of {job!.total}
                    <span className="ml-2 font-mono text-faint">{Math.round(job!.progress * 100)}%</span>
                  </span>
                  <Bar value={job!.progress} />
                </div>
              ) : done.length ? (
                <span>
                  <span className="font-mono">
                    {formatSize(doneBefore)} → {formatSize(Math.round(after))}
                  </span>{" "}
                  {after < doneBefore && <span className="font-medium text-acc-ink">({Math.round((1 - after / doneBefore) * 100)}% smaller)</span>}
                </span>
              ) : (
                <span>
                  Total <span className="font-mono">{formatSize(before)}</span>
                  {target && !expand ? <span className="text-muted"> · each file to under {target} MB</span> : null}
                </span>
              )}
              <div className="mt-0.5 truncate text-[12px] text-faint">
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
                . Originals are kept.
              </div>
            </div>
            {running ? (
              <Button size="lg" disabled={job!.cancelled} onClick={() => cancel(job!.id)}>
                Cancel
              </Button>
            ) : (
              <Button size="lg" variant="primary" disabled={!todo.length || (tab === "video" && !!engines && !engines.video)} onClick={() => void run()}>
                {verb} {todo.length} {todo.length === 1 ? "file" : "files"}
              </Button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function CompRow({ it, onRemove }: { it: Item; onRemove: () => void }) {
  const r = ratioOf(it.detail);
  return (
    <div className="grid grid-cols-[36px_minmax(0,1fr)_200px_190px_30px] items-center gap-3.5 border-b border-row-line px-3.5 py-[9px]">
      <FileIcon kind={it.info.kind!} text={extLabel(it.info)} />
      <div className="min-w-0">
        <div className="truncate text-[13px] font-medium" title={it.info.path}>
          {it.info.name}
        </div>
        <div className={cx("truncate text-[12px]", it.status === "error" ? "text-danger" : "text-muted")} title={it.message ?? it.note}>
          {it.status === "error" ? it.message : it.note || itemMeta(it)}
        </div>
      </div>
      {it.status === "running" ? <Bar value={it.progress || 0.03} /> : <Bar value={r ?? 0} color="var(--acc2)" />}
      <div className="truncate whitespace-nowrap text-right font-mono text-[12.5px]">
        {it.status === "done" && it.detail ? (
          <>
            <span className="text-faint">{it.detail.split(" → ")[0]}</span> → {it.detail.split(" → ")[1]?.replace(/\s*\(.*\)$/, "")}
          </>
        ) : it.status === "running" ? (
          <span className="text-faint">{it.progress > 0 ? `${Math.round(it.progress * 100)}%` : "Working…"}</span>
        ) : (
          <span className="text-muted">{formatSize(it.info.size)}</span>
        )}
      </div>
      <IconButton label={`Remove ${it.info.name}`} disabled={it.status === "running"} onClick={onRemove}>
        <X className="size-3.5" />
      </IconButton>
    </div>
  );
}
