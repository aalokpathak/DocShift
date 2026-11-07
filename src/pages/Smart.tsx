import { ArrowDown, ArrowUp, X } from "lucide-react";
import { useMemo, useState } from "react";
import { Button, Chip, cx, FileIcon, PageTitle, Seg } from "../components/ui";
import { openDest } from "../lib/catalog";
import { formatSize } from "../lib/files";
import { pickFiles } from "../lib/intake";
import { extLabel, KIND_GROUP } from "../lib/kinds";
import { useApp, type Item } from "../lib/store";
import { describe, suggestFor, type Action } from "../lib/suggest";
import { intake } from "./Home";

const FILTERS = {
  All: () => true,
  PDF: (i: Item) => i.info.kind === "pdf",
  Office: (i: Item) => KIND_GROUP(i.info.kind) === "office",
  Images: (i: Item) => i.info.kind === "img",
  Video: (i: Item) => i.info.kind === "video",
} as const;
type Filter = keyof typeof FILTERS;
type SortKey = "added" | "name" | "size" | "type";
const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

export function Smart() {
  const inbox = useApp((s) => s.inbox);
  const selId = useApp((s) => s.smartSel);
  const select = useApp((s) => s.selectSmart);
  const remove = useApp((s) => s.removeFromInbox);
  const clear = useApp((s) => s.clearInbox);
  const [filter, setFilter] = useState<Filter>("All");
  const [sort, setSort] = useState<SortKey>("added");
  const [asc, setAsc] = useState(true);

  const list = useMemo(() => {
    const key = (i: Item): string | number =>
      sort === "added" ? i.seq : sort === "name" ? i.info.name : sort === "size" ? i.info.size : `${i.info.kind}${i.info.name}`;
    return inbox.filter(FILTERS[filter]).sort((a, b) => {
      const x = key(a);
      const y = key(b);
      const c = typeof x === "string" ? collator.compare(x, y as string) : x - (y as number);
      return asc ? c : -c;
    });
  }, [inbox, filter, sort, asc]);

  const cur = list.find((i) => i.id === selId) ?? list[0];

  if (!inbox.length)
    return (
      <div className="flex flex-col gap-4">
        <PageTitle title="Smart dashboard" sub="Add files and DocShift suggests what you can do with each one." />
        <button
          onClick={async () => intake(await pickFiles())}
          className="flex min-h-[220px] cursor-pointer flex-col items-center justify-center gap-2 rounded-[14px] border-[1.5px] border-dashed border-strong-line bg-surface hover:border-acc"
        >
          <span className="text-[18px] font-semibold">Drop files here</span>
          <span className="text-[13px] text-muted">or click to choose them. PDF, Word, Excel, PowerPoint, images and video.</span>
        </button>
      </div>
    );

  return (
    <div className="flex flex-col gap-4">
      <PageTitle title="Smart dashboard" sub={`${inbox.length} ${inbox.length === 1 ? "file" : "files"} detected. Select one to see what you can do with it.`}>
        <Button size="md" variant="ghost" onClick={clear}>
          Clear
        </Button>
        <Button variant="primary" onClick={async () => intake(await pickFiles())}>
          Add files
        </Button>
      </PageTitle>

      <div className="flex flex-wrap items-center justify-between gap-4">
        <div className="flex flex-wrap gap-1.5">
          {(Object.keys(FILTERS) as Filter[]).map((f) => {
            const n = inbox.filter(FILTERS[f]).length;
            const on = filter === f;
            return (
              <button
                key={f}
                onClick={() => setFilter(f)}
                className={cx("h-7 cursor-pointer rounded-[14px] border px-[11px] text-[12.5px]", on ? "border-acc bg-sel text-acc-ink" : "border-btn-line bg-surface text-ink2")}
              >
                {f} <span className="font-mono opacity-70">{n}</span>
              </button>
            );
          })}
        </div>
        <div className="flex items-center gap-2">
          <span className="text-[12.5px] text-muted">Sort by</span>
          <Seg size="sm" value={sort} onChange={setSort} options={[["Added", "added"], ["Name", "name"], ["Size", "size"], ["Type", "type"]]} label="Sort by" />
          <button
            onClick={() => setAsc(!asc)}
            title="Reverse order"
            aria-label="Reverse order"
            className="grid size-8 cursor-pointer place-items-center rounded-[8px] border border-btn-line bg-surface text-ink2"
          >
            {asc ? <ArrowUp className="size-3.5" /> : <ArrowDown className="size-3.5" />}
          </button>
        </div>
      </div>

      <div className="grid grid-cols-[minmax(0,1fr)_400px] items-start gap-4">
        <div className="card overflow-hidden">
          {list.length === 0 && <div className="px-4 py-8 text-center text-[13px] text-muted">No {filter.toLowerCase()} files here.</div>}
          {list.map((it) => {
            const on = it.id === cur?.id;
            return (
              <div
                key={it.id}
                onClick={() => select(it.id)}
                className={cx("group grid cursor-pointer grid-cols-[36px_minmax(0,1fr)_80px_24px] items-center gap-3 border-b border-row-line px-3.5 py-[9px] last:border-b-0", on && "bg-sel shadow-[inset_3px_0_0_var(--acc)]")}
              >
                <FileIcon kind={it.info.kind!} text={extLabel(it.info)} />
                <div className="min-w-0">
                  <div className="truncate text-[13px] font-medium" title={it.info.path}>
                    {it.info.name}
                  </div>
                  <div className="truncate text-[12px] text-muted">{describe(it.info).split(" · ").slice(1, -1).join(" · ") || it.info.ext.toUpperCase()}</div>
                </div>
                <div className="text-right font-mono text-[12.5px] text-muted">{formatSize(it.info.size)}</div>
                <button
                  aria-label={`Remove ${it.info.name}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    remove(it.id);
                  }}
                  className="grid size-6 cursor-pointer place-items-center rounded-[6px] text-faint opacity-0 hover:bg-s1 hover:text-ink group-hover:opacity-100"
                >
                  <X className="size-3.5" />
                </button>
              </div>
            );
          })}
        </div>
        {cur && <Panel item={cur} inbox={inbox} />}
      </div>
    </div>
  );
}

function Panel({ item, inbox }: { item: Item; inbox: Item[] }) {
  const group = KIND_GROUP(item.info.kind);
  const same = inbox.filter((i) => KIND_GROUP(i.info.kind) === group).map((i) => i.info);
  const s = suggestFor(item.info, same.length);
  const run = (a: Action) => openDest(a.dest, a.scope === "group" ? same : [item.info]);

  return (
    <div className="card sticky top-0 flex flex-col gap-4 p-4">
      <div className="flex items-center gap-3">
        <FileIcon kind={item.info.kind!} text={extLabel(item.info)} size={44} />
        <div className="min-w-0">
          <div className="truncate text-[14px] font-semibold" title={item.info.path}>
            {item.info.name}
          </div>
          <div className="mt-0.5 text-[12px] text-muted">{describe(item.info)}</div>
        </div>
      </div>

      <button onClick={() => run(s.primary.action)} className="flex cursor-pointer items-center justify-between gap-3 rounded-[10px] border border-acc bg-sel px-3.5 py-3 text-left">
        <span>
          <span className="eyebrow block !text-[10px] !text-acc-ink">Suggested</span>
          <span className="mt-[3px] block text-[13.5px] font-semibold">{s.primary.label}</span>
          <span className="mt-0.5 block text-[12px] text-muted">{s.primary.why}</span>
        </span>
        <span className="text-[18px] text-acc-ink">→</span>
      </button>

      {s.groups.filter((g) => g.actions.length).map((g) => (
        <div key={g.title} className="flex flex-col gap-2">
          <div className="eyebrow">{g.title}</div>
          <div className="flex flex-wrap gap-1.5">
            {g.actions.map((a) => (
              <button
                key={a.label}
                onClick={() => run(a)}
                className="flex h-8 cursor-pointer items-center gap-[7px] rounded-[8px] border border-btn-line bg-surface px-2.5 text-[12.5px] hover:border-acc"
              >
                {a.chip && <Chip kind={a.chip.kind} text={a.chip.text} small />}
                {a.label}
              </button>
            ))}
          </div>
        </div>
      ))}

      {same.length > 1 && (
        <div className="flex items-center justify-between gap-2.5 border-t border-row-line pt-3.5">
          <span className="text-[12.5px] text-muted">
            {same.length} {s.groupName} found
          </span>
          <div className="flex gap-1.5">
            {s.bulk.map((b) => (
              <Button key={b.label} size="sm" onClick={() => run(b)}>
                {b.label}
              </Button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
