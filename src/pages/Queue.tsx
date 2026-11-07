import { Button, cx, FileIcon, PageTitle, Bar } from "../components/ui";
import { useApp, type JobState } from "../lib/store";
import { plural } from "../lib/files";
import type { Kind } from "../lib/api";

const KIND_OF_LIST: Record<string, Kind> = {
  "convert-from": "pdf",
  "convert-to": "word",
  merge: "pdf",
  organize: "pdf",
  "compress-pdf": "pdf",
  "compress-img": "img",
  "compress-video": "video",
};
const EXT: Record<Kind, string> = { pdf: "PDF", word: "DOC", excel: "XLS", ppt: "PPT", img: "IMG", video: "VID" };

export function Queue() {
  const jobs = useApp((s) => s.jobs);
  const cancel = useApp((s) => s.cancel);
  const clearFinished = useApp((s) => s.clearFinishedJobs);
  const list = Object.values(jobs).sort((a, b) => Number(b.running) - Number(a.running) || b.started - a.started);
  const running = list.filter((j) => j.running).length;

  return (
    <div className="flex flex-col gap-[18px]">
      <PageTitle
        title="Queue"
        sub={running ? `${plural(running, "job")} running. You can keep working while ${running === 1 ? "it finishes" : "these finish"}.` : "Nothing running. Jobs you start appear here."}
      >
        <Button disabled={list.length === running} onClick={clearFinished}>
          Clear finished
        </Button>
      </PageTitle>
      {list.length === 0 ? (
        <div className="card px-4 py-10 text-center text-[13px] text-muted">No jobs yet in this session.</div>
      ) : (
        <div className="card overflow-hidden">
          {list.map((j) => (
            <JobRow key={j.id} j={j} onCancel={() => cancel(j.id)} />
          ))}
        </div>
      )}
    </div>
  );
}

function JobRow({ j, onCancel }: { j: JobState; onCancel: () => void }) {
  const kind = KIND_OF_LIST[j.list];
  const failed = j.results.filter((r) => r.error).length;
  const ok = j.results.filter((r) => r.outcome).length;
  const status = j.running ? (j.cancelled ? "Stopping" : "Running") : j.cancelled ? "Cancelled" : failed && !ok ? "Failed" : failed ? "Partly done" : "Done";
  const name = j.names.length > 1 ? `${j.names[0]} and ${j.names.length - 1} more` : (j.names[0] ?? j.label);
  const detail = j.running ? `${j.finished} of ${plural(j.total, "file")} · ${Math.round(j.progress * 100)}%` : failed ? (j.results.find((r) => r.error)?.error ?? "") : ok ? (j.results[0].outcome?.detail ?? "") : "";
  return (
    <div className="grid grid-cols-[36px_minmax(0,1fr)_240px_110px] items-center gap-4 border-b border-row-line px-3.5 py-2.5 last:border-b-0">
      <FileIcon kind={kind} text={EXT[kind]} />
      <div className="min-w-0">
        <div className="truncate text-[13px] font-medium">{name}</div>
        <div className="text-[12px] text-muted">{j.label}</div>
      </div>
      <div className="flex min-w-0 flex-col gap-[5px]">
        <Bar value={j.running ? Math.max(j.progress, 0.03) : j.cancelled ? j.progress : 1} color={j.running ? "var(--acc)" : failed && !ok ? "var(--danger)" : "var(--acc2)"} />
        <span className="truncate font-mono text-[11.5px] text-faint" title={detail}>
          {detail}
        </span>
      </div>
      <div className="flex items-center justify-end gap-2">
        {j.running && !j.cancelled ? (
          <Button size="sm" onClick={onCancel}>
            Cancel
          </Button>
        ) : (
          <span className={cx("text-[12.5px] font-medium", status === "Done" ? "text-acc-ink" : status === "Failed" ? "text-danger" : "text-faint")}>{status}</span>
        )}
      </div>
    </div>
  );
}
