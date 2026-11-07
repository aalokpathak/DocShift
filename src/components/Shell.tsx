import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Moon, Search, Square, Sun, X } from "lucide-react";
import { useEffect, useState } from "react";
import { activeNav, NAV, openDest } from "../lib/catalog";
import { useApp, type Screen } from "../lib/store";
import { cx, Logo } from "./ui";

const TITLES: Record<Screen, string> = {
  home: "Home",
  smart: "Smart dashboard",
  queue: "Queue",
  history: "Recent files",
  "convert-from": "Convert",
  "convert-to": "Convert",
  merge: "Merge PDFs",
  organize: "Pages",
  compress: "Compress",
  settings: "Settings",
  done: "Finished",
};

const inTauri = "__TAURI_INTERNALS__" in window;

/** Minimise / maximise / close, drawn like Windows 11's caption buttons (the window has no system title bar). */
function WindowControls() {
  const [max, setMax] = useState(false);
  useEffect(() => {
    if (!inTauri) return;
    const w = getCurrentWindow();
    const sync = () => void w.isMaximized().then(setMax).catch(() => {});
    sync();
    const off = w.onResized(sync);
    return () => void off.then((f) => f());
  }, []);
  if (!inTauri) return null;
  const w = getCurrentWindow();
  const btn = "grid h-full w-[46px] cursor-default place-items-center text-ink2 transition-colors";
  return (
    <div className="-mr-4 ml-1 flex h-full shrink-0">
      <button className={cx(btn, "hover:bg-s1")} aria-label="Minimise" onClick={() => void w.minimize()}>
        <Minus className="size-4" strokeWidth={1.25} />
      </button>
      <button className={cx(btn, "hover:bg-s1")} aria-label={max ? "Restore" : "Maximise"} onClick={() => void w.toggleMaximize()}>
        {max ? <Copy className="size-3.5 -scale-x-100" strokeWidth={1.25} /> : <Square className="size-3.5" strokeWidth={1.25} />}
      </button>
      <button className={cx(btn, "hover:bg-[#c42b1c] hover:text-white")} aria-label="Close" onClick={() => void w.close()}>
        <X className="size-4" strokeWidth={1.25} />
      </button>
    </div>
  );
}

/** The window's title bar: drag to move, double-click to maximise; current screen, theme switch, tool search (Ctrl K). */
export function TopBar() {
  const screen = useApp((s) => s.screen);
  const theme = useApp((s) => s.theme);
  const setSetting = useApp((s) => s.setSetting);
  const setPalette = useApp((s) => s.setPalette);
  const dark = theme === "dark" || (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPalette(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setPalette]);

  return (
    <div data-tauri-drag-region className="flex h-11 shrink-0 select-none items-center gap-4 border-b border-line bg-chrome pl-4 pr-4">
      <div data-tauri-drag-region className="w-[212px] shrink-0" />
      <div data-tauri-drag-region className="flex-1 truncate text-center text-[13px] text-muted">
        DocShift — {TITLES[screen]}
      </div>
      <button
        onClick={() => setSetting("theme", dark ? "light" : "dark")}
        className="flex h-7 shrink-0 cursor-pointer items-center gap-1.5 rounded-[7px] border border-line bg-surface px-2.5 text-[12px] text-muted hover:text-ink"
      >
        {dark ? <Sun className="size-3.5" /> : <Moon className="size-3.5" />}
        {dark ? "Light mode" : "Dark mode"}
      </button>
      <button
        onClick={() => setPalette(true)}
        className="flex h-7 w-[220px] shrink-0 cursor-pointer items-center justify-between rounded-[7px] border border-line bg-surface px-2.5 text-[12px] text-faint hover:border-strong-line"
      >
        <span className="flex items-center gap-1.5">
          <Search className="size-3.5" /> Search tools
        </span>
        <span className="font-mono text-[11px]">Ctrl K</span>
      </button>
      <WindowControls />
    </div>
  );
}

export function Sidebar() {
  const screen = useApp((s) => s.screen);
  const organizeMode = useApp((s) => s.organizeMode);
  const compressTab = useApp((s) => s.compressTab);
  const jobs = useApp((s) => s.jobs);
  const inbox = useApp((s) => s.inbox.length);
  const active = activeNav({ screen, organizeMode, compressTab });
  const running = Object.values(jobs).filter((j) => j.running).length;
  const badge: Partial<Record<string, number>> = { queue: running, smart: inbox };

  return (
    <aside className="flex w-[228px] shrink-0 flex-col gap-4 overflow-y-auto border-r border-line bg-side px-2.5 py-4" aria-label="Navigation">
      <div className="flex items-center gap-2.5 px-2 pb-1">
        <Logo />
        <div className="text-[15px] font-semibold tracking-[-0.01em]">DocShift</div>
      </div>
      {NAV.map((g, gi) => (
        <nav key={gi} className="flex flex-col gap-px">
          {g.title && <div className="eyebrow px-2.5 pb-1.5">{g.title}</div>}
          {g.items.map((item) => {
            const on = active === item.id;
            const n = badge[item.id];
            return (
              <button
                key={item.id}
                onClick={() => openDest(item.dest)}
                aria-current={on ? "page" : undefined}
                className={cx(
                  "flex h-7 cursor-pointer items-center justify-between rounded-[7px] px-2.5 text-left text-[13px] transition-colors",
                  on ? "bg-seg-on font-medium text-ink shadow-[0_1px_2px_rgba(0,0,0,.06)]" : "text-ink2 hover:bg-s1",
                )}
              >
                <span>{item.label}</span>
                {!!n && <span className="rounded-[9px] bg-acc px-[7px] py-px font-mono text-[11px] font-medium text-white">{n}</span>}
              </button>
            );
          })}
        </nav>
      ))}
      <div className="mt-auto flex items-start gap-2 border-t border-line px-2.5 pt-3">
        <span className="mt-[5px] size-2 shrink-0 rounded-full bg-acc2" />
        <span className="text-[12px] leading-normal text-muted">Offline. Files never leave this computer.</span>
      </div>
    </aside>
  );
}
