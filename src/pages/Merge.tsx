import { AddMore, Empty, FileLine, SaveAndRun } from "../components/Files";
import { PageTitle, Row, Seg, Toggle } from "../components/ui";
import { useSortable } from "../components/useSortable";
import { plural, sortItems, type SortKey } from "../lib/files";
import { useLatestJob } from "../lib/runner";
import { useApp } from "../lib/store";
import { useState } from "react";

export function Merge() {
  const items = useApp((s) => s.lists.merge);
  const name = useApp((s) => (s.options.merge?.name as string) ?? "");
  const bookmarks = useApp((s) => (s.options.merge?.bookmarks as boolean | undefined) ?? true);
  const blank = useApp((s) => (s.options.merge?.blank as boolean | undefined) ?? false);
  const setOpt = useApp((s) => s.setOpt);
  const addToList = useApp((s) => s.addToList);
  const setList = useApp((s) => s.setList);
  const removeFromList = useApp((s) => s.removeFromList);
  const runJob = useApp((s) => s.runJob);
  const job = useLatestJob("merge");
  const [sort, setSort] = useState<"name" | "pages" | "size" | "">("");

  const move = (from: number, to: number) => {
    if (to < 0 || to >= items.length) return;
    setSort("");
    setList("merge", (l) => {
      const n = [...l];
      const [m] = n.splice(from, 1);
      n.splice(to, 0, m);
      return n;
    });
  };
  const { dragging, containerRef, startDrag } = useSortable(items.length, move);
  const applySort = (k: "name" | "pages" | "size") => {
    setSort(k);
    const key: SortKey = k;
    // names A→Z; pages and size largest first, as in the design
    setList("merge", (l) => sortItems(l, key, k === "name" ? "asc" : "desc"));
  };
  const pages = items.reduce((n, i) => n + (i.info.pages ?? 0), 0);
  const running = !!job?.running;

  const run = () =>
    runJob({
      list: "merge",
      screen: "merge",
      label: `Merge ${items.length} PDFs`,
      category: "Organize",
      spec: { mode: "pdf_merge", tasks: [{ id: "merge", inputs: items.map((i) => i.info.path) }], options: { output_name: name.trim(), bookmarks, blank } },
      taskItems: { merge: items.map((i) => i.id) },
    });

  return (
    <div className="flex flex-col gap-[18px]">
      <PageTitle title="Merge PDFs" sub="Files are combined top to bottom. Drag, use the arrows or sort to change the order.">
        {items.length > 1 && (
          <>
            <span className="text-[12.5px] text-muted">Sort by</span>
            <Seg size="sm" value={sort} onChange={(v) => v && applySort(v)} options={[["Name", "name"], ["Pages", "pages"], ["Size", "size"]]} label="Sort by" />
          </>
        )}
      </PageTitle>
      <div className="grid grid-cols-[minmax(0,1fr)_320px] items-start gap-5">
        <div className="flex flex-col gap-2">
          {items.length === 0 ? (
            <Empty title="Drop PDFs here" sub="Add two or more PDFs to merge them into one." kinds={["pdf"]} onAdd={(f) => addToList("merge", f)} />
          ) : (
            <>
              <div ref={containerRef as React.RefObject<HTMLDivElement>} className="card overflow-hidden">
                {items.map((it, i) => (
                  <FileLine
                    key={it.id}
                    it={it}
                    index={i}
                    count={items.length}
                    thumb
                    dragHandle={running ? undefined : startDrag(i)}
                    dragging={dragging === i}
                    onMove={(d) => move(i, i + d)}
                    onRemove={() => removeFromList("merge", it.id)}
                  />
                ))}
              </div>
              <AddMore text="Drop more PDFs or click to add" kinds={["pdf"]} onAdd={(f) => addToList("merge", f)} />
            </>
          )}
        </div>
        <div className="card flex flex-col gap-4 p-[18px]">
          <div className="flex flex-col gap-1.5">
            <span className="text-[12.5px] text-muted">File name</span>
            <input className="field" placeholder="merged" value={name} onChange={(e) => setOpt("merge", "name", e.target.value)} aria-label="File name" />
          </div>
          <Row label="Bookmark for each file" desc="Jump to where each file starts">
            <Toggle label="Bookmark for each file" on={bookmarks} onChange={(v) => setOpt("merge", "bookmarks", v)} />
          </Row>
          <Row label="Blank page between files" desc="Handy for double-sided printing">
            <Toggle label="Blank page between files" on={blank} onChange={(v) => setOpt("merge", "blank", v)} />
          </Row>
          <p className="text-[12px] leading-relaxed text-muted">Pages, text, images and links are kept.</p>
          <div className="flex justify-between border-t border-row-line pt-3.5 text-[13px]">
            <span className="text-muted">Result</span>
            <span>{items.length ? `${plural(pages + (blank ? items.length - 1 : 0), "page")} from ${plural(items.length, "file")}` : "—"}</span>
          </div>
          {items.length === 1 && <p className="text-[12px] text-warn-ink">Add at least one more PDF.</p>}
          <SaveAndRun label={items.length > 1 ? `Merge ${items.length} files` : "Merge files"} canRun={items.length > 1} onRun={() => void run()} job={job} />
        </div>
      </div>
    </div>
  );
}
