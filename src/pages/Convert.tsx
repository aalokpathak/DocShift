import { useEffect, useRef, useState } from "react";
import { AddMore, Empty, FileLine, SaveAndRun } from "../components/Files";
import { Thumb } from "../components/Thumb";
import { Button, Chip, cx, PageTitle, Ring, Row, Seg, Toggle } from "../components/ui";
import { useSortable } from "../components/useSortable";
import type { Kind } from "../lib/api";
import { SORT_LABEL, sortItems, type SortDir, type SortKey } from "../lib/files";
import { useLatestJob, runEach, runnable } from "../lib/runner";
import { useApp, type FromTarget } from "../lib/store";

export function Convert() {
  const screen = useApp((s) => s.screen);
  const go = useApp((s) => s.go);
  const from = screen === "convert-from";
  return (
    <div className="flex flex-col gap-[18px]">
      <PageTitle title="Convert" sub="Turn PDFs into Word, Excel, PowerPoint, images or text, or turn documents and images into PDF.">
        <Seg
          value={from ? "from" : "to"}
          onChange={(v) => go(v === "from" ? "convert-from" : "convert-to")}
          options={[
            ["From PDF", "from"],
            ["To PDF", "to"],
          ]}
          label="Direction"
        />
      </PageTitle>
      {from ? <FromPdf /> : <ToPdf />}
    </div>
  );
}

const TARGETS: { id: FromTarget; label: string; chip: { kind: Kind | "txt"; text: string }; mode: string; noun: string }[] = [
  { id: "word", label: "Word document", chip: { kind: "word", text: "DOCX" }, mode: "pdf2word", noun: "Word" },
  { id: "excel", label: "Excel workbook", chip: { kind: "excel", text: "XLSX" }, mode: "pdf2excel", noun: "Excel" },
  { id: "ppt", label: "PowerPoint presentation", chip: { kind: "ppt", text: "PPTX" }, mode: "pdf2ppt", noun: "PowerPoint" },
  { id: "img", label: "Images, one per page", chip: { kind: "img", text: "JPG" }, mode: "pdf2img", noun: "images" },
  { id: "txt", label: "Plain text", chip: { kind: "txt", text: "TXT" }, mode: "pdf2txt", noun: "text" },
  { id: "ocr", label: "Searchable PDF (read scanned pages)", chip: { kind: "pdf", text: "PDF" }, mode: "pdf_ocr", noun: "searchable PDF" },
];

/** Targets that can read scanned pages first (OCR). */
const OCR_TARGETS: FromTarget[] = ["word", "excel", "ppt", "txt"];

function FromPdf() {
  const items = useApp((s) => s.lists["convert-from"]);
  const target = useApp((s) => s.fromTarget);
  const engines = useApp((s) => s.engines);
  const ocrLang = useApp((s) => s.ocrLang);
  const opt = useApp((s) => s.options["convert-from"]) ?? {};
  const setOpt = useApp((s) => s.setOpt);
  const addToList = useApp((s) => s.addToList);
  const setFiles = useApp((s) => s.setFiles);
  const removeFromList = useApp((s) => s.removeFromList);
  const job = useLatestJob("convert-from");
  const [pagesMode, setPagesMode] = useState<"all" | "choose">(opt.pages ? "choose" : "all");

  const t = TARGETS.find((x) => x.id === target)!;
  const o = <T,>(k: string, d: T) => (opt[k] === undefined ? d : (opt[k] as T));
  const set = (k: string, v: unknown) => setOpt("convert-from", k, v);
  const first = items[0];
  const todo = runnable(items);

  const noWord = !!engines && !engines.word;
  const canOcr = engines?.ocr !== false;
  const ocrOn = OCR_TARGETS.includes(target) && canOcr && o("ocr", true);
  // Word's own importer keeps the layout; without Word, DocShift writes flowing text itself
  const layout = target === "word" && noWord ? "flow" : o<string>(target === "txt" ? "txt_layout" : "layout_word", "keep");

  const run = () => {
    const base: Record<string, unknown> = { ocr: ocrOn, ocr_lang: ocrLang };
    const options: Record<string, unknown> =
      target === "excel"
        ? { ...base, layout: o("layout", "pages") }
        : target === "ppt"
          ? { ...base, style: o("style", "exact"), dpi: o("dpi", 150) }
          : target === "img"
            ? { format: o("format", "png"), dpi: o("dpi", 200), pages: pagesMode === "choose" ? o("pages", "") : "" }
            : target === "ocr"
              ? { ocr_lang: ocrLang, pdfa: o("pdfa_ocr", false) }
              : { ...base, layout };
    void runEach({ list: "convert-from", screen: "convert-from", label: target === "ocr" ? "Searchable PDF" : `PDF to ${t.noun}`, category: "Convert", mode: t.mode, items: todo, options });
  };

  return (
    <div className="grid grid-cols-[minmax(0,1fr)_400px] items-start gap-5">
      <div className="card flex flex-col gap-3.5 p-[18px]">
        <div className="eyebrow">Source</div>
        {!first ? (
          <Empty title="Drop a PDF here" sub="Several PDFs are converted one after another." kinds={["pdf"]} onAdd={(f) => setFiles("convert-from", f)} />
        ) : (
          <>
            <Thumb path={first.info.path} kind="pdf" page={0} size={600} fit="contain" className="h-[300px] rounded-[8px] border border-track" />
            <div className="-mx-[18px] border-t border-row-line">
              {items.map((it) => (
                <FileLine key={it.id} it={it} onRemove={() => removeFromList("convert-from", it.id)} />
              ))}
            </div>
            <AddMore text="Add more PDFs" kinds={["pdf"]} onAdd={(f) => addToList("convert-from", f)} height={44} />
          </>
        )}
      </div>

      <div className="card flex flex-col gap-[18px] p-[18px]">
        <div className="flex flex-col gap-2">
          <div className="eyebrow">Convert to</div>
          {TARGETS.map((x) => {
            const on = x.id === target;
            return (
              <button
                key={x.id}
                onClick={() => useApp.setState({ fromTarget: x.id })}
                className={cx("flex cursor-pointer items-center gap-3 rounded-[9px] border px-3 py-2.5 text-left", on ? "border-acc bg-sel" : "border-card-line bg-surface hover:border-strong-line")}
                role="radio"
                aria-checked={on}
              >
                <Ring on={on} />
                <span className="w-[46px] text-center">
                  <Chip kind={x.chip.kind} text={x.chip.text} />
                </span>
                <span className="flex-1 text-[13px]">{x.label}</span>
              </button>
            );
          })}
        </div>

        <div className="flex flex-col gap-3 border-t border-row-line pt-3">
          {(target === "word" || target === "txt") && (
            <Row
              label="Layout"
              desc={
                target === "word" && noWord
                  ? "Microsoft Word wasn't found, so DocShift writes flowing text"
                  : layout === "flow"
                    ? "Paragraphs and headings that reflow when you edit"
                    : target === "word"
                      ? "Microsoft Word rebuilds pages, tables and columns"
                      : "Spacing keeps columns and tables lined up"
              }
            >
              <Seg
                size="sm"
                value={layout}
                onChange={(v) => set(target === "txt" ? "txt_layout" : "layout_word", v)}
                options={target === "word" && noWord ? [["Flowing text", "flow"]] : [["Keep layout", "keep"], ["Flowing text", "flow"]]}
              />
            </Row>
          )}
          {OCR_TARGETS.includes(target) && (
            <Row
              label="Read scanned pages"
              desc={canOcr ? "Turns pictures of text into real text, using Windows' built-in recognition" : "Needs a Windows language with text recognition (Settings > Time & language)"}
            >
              <Toggle label="Read scanned pages" on={ocrOn} onChange={(v) => set("ocr", v)} disabled={!canOcr} />
            </Row>
          )}
          {target === "ocr" && (
            <>
              <p className="text-[12px] leading-relaxed text-muted">
                {canOcr
                  ? "Pages stay exactly as they look. Recognised text is added invisibly on top, so you can search, select and copy it. Pages that already have text are left alone."
                  : "Windows has no text-recognition language installed. Add one in Windows Settings > Time & language > Language & region."}
              </p>
              <Row label="Archive format (PDF/A)" desc="For long-term storage and records">
                <Toggle label="Archive format (PDF/A)" on={o("pdfa_ocr", false)} onChange={(v) => set("pdfa_ocr", v)} />
              </Row>
            </>
          )}
          {target === "excel" && (
            <Row label="Layout">
              <Seg size="sm" value={o("layout", "pages")} onChange={(v) => set("layout", v)} options={[["Sheet per page", "pages"], ["One sheet", "single"]]} />
            </Row>
          )}
          {target === "ppt" && (
            <>
              <Row label="Slides" desc={o<string>("style", "exact") === "editable" ? "Text becomes real, editable text boxes" : "Each page as an exact picture"}>
                <Seg size="sm" value={o("style", "exact")} onChange={(v) => set("style", v)} options={[["Exact look", "exact"], ["Editable text", "editable"]]} />
              </Row>
              <Row label="Image quality">
                <Seg size="sm" value={o("dpi", 150)} onChange={(v) => set("dpi", v)} options={[["Standard", 150], ["High", 220]]} />
              </Row>
            </>
          )}
          {target === "img" && (
            <>
              <Row label="Format">
                <Seg size="sm" value={o("format", "png")} onChange={(v) => set("format", v)} options={[["PNG", "png"], ["JPG", "jpg"], ["WebP", "webp"], ["TIFF", "tiff"]]} />
              </Row>
              <Row label="Resolution">
                <Seg size="sm" value={o("dpi", 200)} onChange={(v) => set("dpi", v)} options={[["72", 72], ["150", 150], ["200", 200], ["300 dpi", 300]]} />
              </Row>
              <Row label="Pages">
                <Seg size="sm" value={pagesMode} onChange={setPagesMode} options={[["All", "all"], ["Choose…", "choose"]]} />
              </Row>
              {pagesMode === "choose" && <input className="field" placeholder="e.g. 1-3, 5" value={o("pages", "")} onChange={(e) => set("pages", e.target.value)} aria-label="Pages to convert" />}
            </>
          )}
        </div>

        <SaveAndRun
          label={target === "ocr" ? (todo.length > 1 ? `Make ${todo.length} PDFs searchable` : "Make searchable") : todo.length > 1 ? `Convert ${todo.length} files to ${t.noun}` : `Convert to ${t.noun}`}
          canRun={todo.length > 0 && !(target === "ocr" && !canOcr)}
          onRun={run}
          job={job}
        />
      </div>
    </div>
  );
}

// --------------------------------------------------------------------------------------------- To PDF

const MODE: Record<string, string> = { word: "word2pdf", excel: "excel2pdf", ppt: "ppt2pdf" };
const SORTS: SortKey[] = ["added", "name", "taken", "modified", "created", "size"];

function ToPdf() {
  const items = useApp((s) => s.lists["convert-to"]);
  const engines = useApp((s) => s.engines);
  const opt = useApp((s) => s.options["convert-to"]) ?? {};
  const setOpt = useApp((s) => s.setOpt);
  const addToList = useApp((s) => s.addToList);
  const setList = useApp((s) => s.setList);
  const removeFromList = useApp((s) => s.removeFromList);
  const toast = useApp((s) => s.toast);
  const runJob = useApp((s) => s.runJob);
  const job = useLatestJob("convert-to");
  const [sortKey, setSortKey] = useState<SortKey | "manual">("added");
  const [dir, setDir] = useState<SortDir>("asc");

  const o = <T,>(k: string, d: T) => (opt[k] === undefined ? d : (opt[k] as T));
  const set = (k: string, v: unknown) => setOpt("convert-to", k, v);
  const add = (infos: Parameters<typeof addToList>[1]) => {
    const ok = infos.filter((i) => i.kind && i.kind !== "pdf" && i.kind !== "video");
    if (ok.length < infos.length) toast({ kind: "warn", text: "Only Word, Excel, PowerPoint and image files can be converted to PDF here." });
    addToList("convert-to", ok);
  };

  const images = items.filter((i) => i.info.kind === "img");
  const allImages = items.length > 0 && images.length === items.length;
  // all-images lists combine by default (scans, photos); mixed lists only when asked
  const combine = items.length > 1 && o("combine", allImages);
  const has = (k: Kind) => items.some((i) => i.info.kind === k);
  const todo = runnable(items);

  // keep the chosen sort applied as files are added; clearing resets to "order added"
  const lastCount = useRef(items.length);
  useEffect(() => {
    if (items.length === 0) {
      setSortKey("added");
      setDir("asc");
    } else if (items.length !== lastCount.current && sortKey !== "manual") setList("convert-to", (l) => sortItems(l, sortKey, dir));
    lastCount.current = items.length;
  }, [items.length]); // eslint-disable-line react-hooks/exhaustive-deps

  const move = (from: number, to: number) => {
    setSortKey("manual");
    setList("convert-to", (l) => {
      const n = [...l];
      const [m] = n.splice(from, 1);
      n.splice(to, 0, m);
      return n;
    });
  };
  const { dragging, containerRef, startDrag } = useSortable(items.length, move);
  const applySort = (k: SortKey, d: SortDir) => {
    setSortKey(k);
    setDir(d);
    setList("convert-to", (l) => sortItems(l, k, d));
  };

  const run = async () => {
    const group = `to-pdf-${Date.now()}`;
    const quality = o("quality", "standard");
    const pdfa = o("pdfa", false);
    const name = String(o("name", "")).trim();
    if (combine && !allImages) {
      // Office files, images (and PDFs) in list order, one bookmark per file
      const options = { quality, pdfa, bookmarks: o("bookmarks", true), fit_width: o("fit_width", true), page: o("page", "fit"), margin: o("margin", "small"), output_name: name };
      const taskItems: Record<string, string[]> = { combined: todo.map((i) => i.id) };
      await runJob({ list: "convert-to", screen: "convert-to", label: "Combine into one PDF", category: "Convert", spec: { mode: "combine_pdf", tasks: [{ id: "combined", inputs: todo.map((i) => i.info.path) }], options }, taskItems, group });
      return;
    }
    for (const kind of ["word", "excel", "ppt"] as const) {
      const list = todo.filter((i) => i.info.kind === kind);
      if (!list.length) continue;
      const options = { quality, pdfa, bookmarks: o("bookmarks", true), fit_width: o("fit_width", true) };
      const taskItems: Record<string, string[]> = {};
      const tasks = list.map((it) => ((taskItems[it.id] = [it.id]), { id: it.id, inputs: [it.info.path] }));
      await runJob({ list: "convert-to", screen: "convert-to", label: "Convert to PDF", category: "Convert", spec: { mode: MODE[kind], tasks, options }, taskItems, group });
    }
    const imgs = todo.filter((i) => i.info.kind === "img");
    if (imgs.length) {
      const options = { quality, pdfa, page: o("page", "fit"), margin: o("margin", "small"), output_name: combine ? name : "" };
      const taskItems: Record<string, string[]> = {};
      const tasks = combine && imgs.length > 1
        ? ((taskItems.combined = imgs.map((i) => i.id)), [{ id: "combined", inputs: imgs.map((i) => i.info.path) }])
        : imgs.map((it) => ((taskItems[it.id] = [it.id]), { id: it.id, inputs: [it.info.path] }));
      await runJob({ list: "convert-to", screen: "convert-to", label: combine && imgs.length > 1 ? "Images to PDF" : "Convert to PDF", category: "Convert", spec: { mode: "img2pdf", tasks, options }, taskItems, group });
    }
  };

  const missing = ([["word", "Word", engines?.word], ["excel", "Excel", engines?.excel], ["ppt", "PowerPoint", engines?.powerpoint]] as const).filter(
    ([k, , ok]) => has(k) && engines && !ok && !engines.libreoffice,
  );

  return (
    <div className="grid grid-cols-[minmax(0,1fr)_400px] items-start gap-5">
      <div className="card flex flex-col gap-3.5 p-[18px]">
        <div className="flex items-center justify-between gap-3">
          <div className="eyebrow">Files</div>
          {items.length > 1 && (
            <div className="flex items-center gap-2">
              <span className="text-[12px] text-muted">Order</span>
              <select
                className="field !h-[30px] !w-auto cursor-pointer text-[12.5px]"
                value={sortKey}
                aria-label="Sort by"
                onChange={(e) => applySort(e.target.value as SortKey, dir)}
              >
                {sortKey === "manual" && <option value="manual">Custom (dragged)</option>}
                {SORTS.map((k) => (
                  <option key={k} value={k}>
                    {SORT_LABEL[k]}
                  </option>
                ))}
              </select>
              <Button size="sm" disabled={sortKey === "manual"} onClick={() => sortKey !== "manual" && applySort(sortKey, dir === "asc" ? "desc" : "asc")}>
                {dir === "asc" ? "↑" : "↓"}
              </Button>
            </div>
          )}
        </div>
        {items.length === 0 ? (
          <Empty title="Drop files here" sub="Word, Excel, PowerPoint and images (JPG, PNG, HEIC, WebP, TIFF and more)." kinds={["word", "excel", "ppt", "img"]} onAdd={add} />
        ) : (
          <>
            <div ref={containerRef as React.RefObject<HTMLDivElement>} className="-mx-[18px] border-y border-row-line">
              {items.map((it, i) => (
                <FileLine
                  key={it.id}
                  it={it}
                  index={combine && items.length > 1 ? i : undefined}
                  count={items.length}
                  thumb={it.info.kind === "img"}
                  dragHandle={items.length > 1 && !job?.running ? startDrag(i) : undefined}
                  dragging={dragging === i}
                  onRemove={() => removeFromList("convert-to", it.id)}
                />
              ))}
            </div>
            <AddMore text="Add more files" kinds={["word", "excel", "ppt", "img"]} onAdd={add} height={44} />
          </>
        )}
      </div>

      <div className="card flex flex-col gap-3.5 p-[18px]">
        <div className="eyebrow">PDF options</div>
        <div className="flex flex-col gap-2">
          <span className="text-[13px]">Output</span>
          <Seg
            stretch
            size="sm"
            value={combine ? "combine" : "each"}
            onChange={(v) => set("combine", v === "combine")}
            options={[["One PDF per file", "each"], ["Combine into one", "combine"]]}
          />
          {combine && (
            <input
              className="field"
              placeholder={`${items[0].info.name.replace(/\.[^.]+$/, "")}_${allImages ? "merged" : "combined"}`}
              value={o("name", "")}
              onChange={(e) => set("name", e.target.value)}
              aria-label="File name"
            />
          )}
          {combine && !allImages && <span className="text-[11.5px] text-faint">In the order shown, with a bookmark for each file.</span>}
        </div>
        <div className="flex flex-col gap-2">
          <span className="text-[13px]">Quality</span>
          <Seg stretch size="sm" value={o("quality", "standard")} onChange={(v) => set("quality", v)} options={[["Small", "small"], ["Standard", "standard"], ["Print", "print"]]} />
          <span className="text-[11.5px] text-faint">
            {o<string>("quality", "standard") === "small"
              ? "Smallest file: pictures are compressed more. Text stays sharp."
              : o<string>("quality", "standard") === "print"
                ? "Pictures kept at full resolution, as in the original."
                : "Good for screen and office printing, at a sensible size."}
          </span>
        </div>
        {has("img") && (
          <Row label="Page size">
            <Seg size="sm" value={o("page", "fit")} onChange={(v) => set("page", v)} options={[["Fit image", "fit"], ["A4", "a4"], ["Letter", "letter"]]} />
          </Row>
        )}
        <Row label="Archive format (PDF/A)" desc="For long-term storage and records">
          <Toggle label="Archive format (PDF/A)" on={o("pdfa", false)} onChange={(v) => set("pdfa", v)} />
        </Row>
        {has("word") && (
          <Row label="Bookmarks from headings" desc="Word documents">
            <Toggle label="Bookmarks from headings" on={o("bookmarks", true)} onChange={(v) => set("bookmarks", v)} />
          </Row>
        )}
        {has("excel") && (
          <Row label="Fit sheets to page width" desc="Stops wide sheets splitting across pages">
            <Toggle label="Fit sheets to page width" on={o("fit_width", true)} onChange={(v) => set("fit_width", v)} />
          </Row>
        )}
        {missing.length > 0 && <p className="text-[12px] text-danger">Microsoft {missing.map((m) => m[1]).join(" and ")} wasn't found, so those files can't be converted.</p>}
        <SaveAndRun
          label={combine ? "Combine into one PDF" : todo.length > 1 ? `Convert ${todo.length} files to PDF` : "Convert to PDF"}
          canRun={todo.length > 0}
          onRun={() => void run()}
          job={job}
        />
      </div>
    </div>
  );
}
