import { useState } from "react";
import { Button, Chip, cx, FileIcon } from "../components/ui";
import { openDest, TOOL_GROUPS } from "../lib/catalog";
import { pickFiles, pickFolder, inspect } from "../lib/intake";
import { extLabel, KIND_GROUP } from "../lib/kinds";
import { useApp } from "../lib/store";
import { describe, suggestFor } from "../lib/suggest";
import type { FileInfo } from "../lib/api";

/** Adds files to the dashboard: one file stays here with quick actions, several open the smart dashboard. */
export function intake(infos: FileInfo[]) {
  const st = useApp.getState();
  const fresh = st.addToInbox(infos.filter((i) => i.kind && !i.error));
  const skipped = infos.filter((i) => !i.kind || i.error);
  if (skipped.length) st.toast({ kind: "warn", text: `${skipped.length === 1 ? skipped[0].name : `${skipped.length} files`} can't be used here. DocShift works with PDF, Office files, images and video.` });
  if (fresh.length > 1) st.go("smart");
}

export function Home() {
  const inbox = useApp((s) => s.inbox);
  const dropActive = useApp((s) => s.dropActive);
  const [busy, setBusy] = useState(false);
  const last = inbox[inbox.length - 1];

  const choose = async () => {
    setBusy(true);
    try {
      intake(await pickFiles());
    } finally {
      setBusy(false);
    }
  };
  const folder = async () => {
    const d = await pickFolder();
    if (d) intake(await inspect([d]));
  };

  return (
    <div className="flex flex-col gap-[30px]">
      <div
        className={cx(
          "flex flex-col items-center gap-3 rounded-[14px] border-[1.5px] border-dashed bg-surface px-8 pb-[18px] pt-7 text-center transition-colors",
          dropActive ? "border-acc bg-sel" : "border-strong-line",
        )}
      >
        <div className="text-[22px] font-semibold tracking-[-0.02em]">{dropActive ? "Release to add" : "Drop files here"}</div>
        <div className="max-w-[460px] text-[14px] leading-normal text-muted">
          PDF, Word, Excel, PowerPoint, images or video. DocShift suggests what you can do with them.
        </div>
        <div className="mt-1 flex gap-2.5">
          <Button variant="primary" size="lg" busy={busy} onClick={choose}>
            Choose files
          </Button>
          <Button size="lg" onClick={folder}>
            Open folder
          </Button>
        </div>
        {last ? <JustAdded info={last.info} count={inbox.length} /> : <div className="h-2" />}
      </div>

      {TOOL_GROUPS.map((cat) => (
        <section key={cat.title} className="flex flex-col gap-3">
          <h2 className="text-[15px] font-semibold">{cat.title}</h2>
          <div className="grid grid-cols-4 gap-2.5">
            {cat.items.map((t) => (
              <button
                key={t.title}
                onClick={() => openDest(t.dest)}
                className="card flex cursor-pointer flex-col gap-3 p-3.5 text-left transition-shadow hover:border-strong-line hover:shadow-[0_2px_8px_rgba(0,0,0,.05)]"
              >
                <span className="flex items-center gap-1.5">
                  {t.chips.map((c, i) => (
                    <span key={i} className="flex items-center gap-1.5">
                      {i > 0 && <span className="text-[12px] text-faint">→</span>}
                      <Chip kind={c.kind} text={c.text} />
                    </span>
                  ))}
                </span>
                <span>
                  <span className="block text-[14px] font-medium">{t.title}</span>
                  <span className="mt-0.5 block text-[12px] text-muted">{t.desc}</span>
                </span>
              </button>
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}

/** The most recently added file with its top suggestions, right under the drop area. */
function JustAdded({ info, count }: { info: FileInfo; count: number }) {
  const inbox = useApp((s) => s.inbox);
  const sameGroup = inbox.filter((i) => KIND_GROUP(i.info.kind) === KIND_GROUP(info.kind)).map((i) => i.info);
  const sug = suggestFor(info, sameGroup.length);
  const quick = [sug.primary.action, ...sug.groups.flatMap((g) => g.actions)].filter((a, i, arr) => arr.findIndex((b) => b.label === a.label) === i).slice(0, 3);
  return (
    <div className="mt-4 flex items-center gap-3.5 self-stretch rounded-[10px] bg-bg px-3.5 py-3 text-left">
      <FileIcon kind={info.kind!} text={extLabel(info)} />
      <div className="min-w-0 flex-1">
        <div className="truncate text-[13px] font-medium">{info.name}</div>
        <div className="truncate text-[12px] text-muted">
          Just added · {describe(info)}
          {count > 1 ? ` · ${count} files on the dashboard` : ""}
        </div>
      </div>
      <button className="shrink-0 cursor-pointer text-[12px] text-acc hover:text-acc-ink" onClick={() => useApp.getState().go("smart")}>
        All options
      </button>
      <div className="flex shrink-0 gap-1.5">
        {quick.map((a) => (
          <Button key={a.label} size="sm" onClick={() => openDest(a.dest, a.scope === "group" ? sameGroup : [info])}>
            {a.label === "PDF" ? "To PDF" : a.label === "Word" ? "To Word" : a.label}
          </Button>
        ))}
      </div>
    </div>
  );
}
