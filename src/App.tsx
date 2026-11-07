import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect } from "react";
import { AskDialog } from "./components/AskDialog";
import { Palette } from "./components/Palette";
import { Sidebar, TopBar } from "./components/Shell";
import { Toasts } from "./components/Toasts";
import { defaultOutputDir, engineStatus, ocrLanguages, onJobEvent, type FileInfo, type Kind } from "./lib/api";
import { inspect } from "./lib/intake";
import { useApp, type ListKey } from "./lib/store";
import { Compress } from "./pages/Compress";
import { Convert } from "./pages/Convert";
import { Done } from "./pages/Done";
import { History } from "./pages/History";
import { Home, intake } from "./pages/Home";
import { Merge } from "./pages/Merge";
import { Organize } from "./pages/Organize";
import { Queue } from "./pages/Queue";
import { Settings } from "./pages/Settings";
import { Smart } from "./pages/Smart";

function useTheme() {
  const theme = useApp((s) => s.theme);
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      document.documentElement.dataset.theme = theme === "dark" || (theme === "system" && mq.matches) ? "dark" : "light";
    };
    apply();
    void getCurrentWindow()
      .setTheme(theme === "system" ? null : theme)
      .catch(() => {});
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme]);
}

/** Which list (and which file kinds) the current screen takes when files are dropped on it. */
function dropTarget(): { list: ListKey; kinds: Kind[]; single?: boolean } | null {
  const s = useApp.getState();
  switch (s.screen) {
    case "convert-from":
      return { list: "convert-from", kinds: ["pdf"] };
    case "convert-to":
      return { list: "convert-to", kinds: ["word", "excel", "ppt", "img"] };
    case "merge":
      return { list: "merge", kinds: ["pdf"] };
    case "organize":
      return { list: "organize", kinds: ["pdf"], single: true };
    case "compress":
      return s.compressTab === "pdf" ? { list: "compress-pdf", kinds: ["pdf"] } : s.compressTab === "img" ? { list: "compress-img", kinds: ["img"] } : { list: "compress-video", kinds: ["video"] };
    default:
      return null;
  }
}

/** Files dropped anywhere in the window go to the screen that's open; anywhere else they land on the dashboard. */
async function handleDrop(paths: string[]) {
  const infos = await inspect(paths);
  if (!infos.length) return;
  const st = useApp.getState();
  const t = dropTarget();
  if (!t) {
    intake(infos);
    if (!["home", "smart"].includes(st.screen)) st.go("smart");
    return;
  }
  const fits = (i: FileInfo) => !!i.kind && t.kinds.includes(i.kind);
  const ok = infos.filter(fits);
  const other = infos.filter((i) => !fits(i));
  if (t.single && ok.length) st.setFiles(t.list, ok.slice(0, 1));
  else if (ok.length) st.addToList(t.list, ok);
  if (other.length) {
    if (!ok.length) {
      // nothing fits here: put them on the smart dashboard, which knows what to do with them
      intake(other);
      st.go("smart");
      st.toast({ kind: "info", text: "Those files don't fit this tool, so they were added to the Smart dashboard." });
    } else st.toast({ kind: "warn", text: `${other.length} file(s) skipped: they don't fit this tool.` });
  }
}

export default function App() {
  useTheme();
  const screen = useApp((s) => s.screen);

  useEffect(() => {
    const st = useApp.getState();
    if (!st.outDir) void defaultOutputDir().then((d) => useApp.getState().setSetting("outDir", d));
    void engineStatus()
      .then(st.setEngines)
      .catch(() => {});
    void ocrLanguages()
      .then(st.setOcrLangs)
      .catch(() => st.setOcrLangs([]));

    const offJobs = onJobEvent((e) => useApp.getState().onJobEvent(e));
    const offDrop = getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload;
      if (p.type === "enter" || p.type === "over") useApp.getState().setDropActive(true);
      else if (p.type === "leave") useApp.getState().setDropActive(false);
      else if (p.type === "drop") {
        useApp.getState().setDropActive(false);
        void handleDrop(p.paths);
      }
    });
    // a stray drop outside our handler would make the webview navigate to the file
    const block = (e: DragEvent) => e.preventDefault();
    window.addEventListener("dragover", block);
    window.addEventListener("drop", block);
    return () => {
      void offJobs.then((f) => f());
      void offDrop.then((f) => f());
      window.removeEventListener("dragover", block);
      window.removeEventListener("drop", block);
    };
  }, []);

  const page = {
    home: <Home />,
    smart: <Smart />,
    queue: <Queue />,
    history: <History />,
    "convert-from": <Convert />,
    "convert-to": <Convert />,
    merge: <Merge />,
    organize: <Organize />,
    compress: <Compress />,
    settings: <Settings />,
    done: <Done />,
  }[screen];

  return (
    <div className="flex h-full flex-col bg-bg">
      <TopBar />
      <div className="flex min-h-0 flex-1">
        <Sidebar />
        <main key={screen === "convert-to" ? "convert-from" : screen} className="fade-up min-w-0 flex-1 overflow-y-auto px-7 pb-8 pt-[22px]">
          {page}
        </main>
      </div>
      <Palette />
      <AskDialog />
      <Toasts />
    </div>
  );
}
