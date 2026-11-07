import { Check, X } from "lucide-react";
import { Button, FileIcon, cx } from "../components/ui";
import { openDest } from "../lib/catalog";
import { inspect } from "../lib/intake";
import { openResult, showInFolder } from "../lib/open";
import { extLabel } from "../lib/kinds";
import { useApp } from "../lib/store";
import type { Kind } from "../lib/api";

const kindOfName = (name: string): Kind | "txt" => {
  const e = name.split(".").pop()?.toLowerCase() ?? "";
  if (e === "pdf") return "pdf";
  if (e === "txt") return "txt";
  if (["doc", "docx"].includes(e)) return "word";
  if (["xls", "xlsx"].includes(e)) return "excel";
  if (["ppt", "pptx"].includes(e)) return "ppt";
  if (["mp4", "mkv", "mov"].includes(e)) return "video";
  return "img";
};

/** Shown after a job finishes while you're still on its screen: the results and what to do next. */
export function Done() {
  const done = useApp((s) => s.done);
  const go = useApp((s) => s.go);
  if (!done) return null;
  const pdfs = done.outputs.filter((o) => o.name.toLowerCase().endsWith(".pdf")).map((o) => o.path);
  const failed = done.errors.length > 0 && done.outputs.length === 0;

  const withResults = async (dest: Parameters<typeof openDest>[0], paths: string[]) => openDest(dest, await inspect(paths));

  return (
    <div className="mx-auto mt-[30px] flex max-w-[620px] flex-col gap-[18px]">
      <div className="flex flex-col items-center gap-3 text-center">
        <div className={cx("grid size-14 place-items-center rounded-full", failed ? "bg-danger-soft text-danger" : "bg-sel text-acc-ink")}>
          {failed ? <X className="size-7" /> : <Check className="size-7" />}
        </div>
        <div className="text-[24px] font-semibold tracking-[-0.015em]">{done.title}</div>
        <div className="text-[13px] text-muted">{done.note}</div>
      </div>

      {done.outputs.length > 0 && (
        <div className="card overflow-hidden">
          {done.outputs.map((o) => {
            const kind = kindOfName(o.name);
            return (
              <div key={o.path} className="flex items-center gap-3.5 border-b border-row-line px-3.5 py-2.5 last:border-b-0">
                <FileIcon kind={kind} text={o.name.includes(".") ? extLabel({ ext: o.name.split(".").pop()!, kind: kind === "txt" ? null : kind }) : "DIR"} />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[14px] font-medium" title={o.path}>
                    {o.name}
                  </div>
                  <div className="truncate text-[12px] text-muted">{o.meta || o.path}</div>
                </div>
                <Button size="sm" variant="primary" onClick={() => void openResult(o.path)}>
                  Open
                </Button>
                <Button size="sm" onClick={() => void showInFolder(o.path)}>
                  Show in folder
                </Button>
              </div>
            );
          })}
        </div>
      )}

      {done.errors.length > 0 && (
        <div className="card overflow-hidden">
          {done.errors.map((e, i) => (
            <div key={i} className="border-b border-row-line px-3.5 py-2.5 text-[13px] text-danger last:border-b-0">
              {e}
            </div>
          ))}
        </div>
      )}

      <div className="flex flex-col gap-2.5">
        <div className="text-[13px] text-muted">{failed ? "Try again or start something else" : "Do more with the result"}</div>
        <div className="flex flex-wrap gap-2">
          {pdfs.length > 0 && done.category !== "Compress" && <Button onClick={() => void withResults({ screen: "compress", compressTab: "pdf" }, pdfs)}>Compress</Button>}
          {pdfs.length > 0 && <Button onClick={() => void withResults({ screen: "merge" }, pdfs)}>Merge with other PDFs</Button>}
          {pdfs.length === 1 && <Button onClick={() => void withResults({ screen: "organize", organizeMode: "delete" }, pdfs)}>Edit pages</Button>}
          <Button onClick={() => go("home")}>Start something new</Button>
        </div>
      </div>
    </div>
  );
}
