import { openUrl } from "@tauri-apps/plugin-opener";
import type { ReactNode } from "react";
import { Button, Seg, Toggle } from "../components/ui";
import { defaultOutputDir } from "../lib/api";
import { pickFolder } from "../lib/intake";
import { useApp } from "../lib/store";
import pkg from "../../package.json";

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-2">
      <div className="eyebrow">{title}</div>
      <div className="card">{children}</div>
    </div>
  );
}

function SRow({ label, desc, children }: { label: string; desc?: string; children?: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-5 border-b border-row-line px-3.5 py-2.5 last:border-b-0">
      <div className={children ? "min-w-0 shrink-0 basis-[38%]" : "min-w-0"}>
        <div className="text-[13px]">{label}</div>
        {desc && <div className="mt-0.5 text-[12px] text-muted">{desc}</div>}
      </div>
      {children}
    </div>
  );
}

export function Settings() {
  const s = useApp();
  const e = s.engines;
  const engineRows: [string, boolean | undefined, string][] = [
    ["Microsoft Word", e?.word, "Word to PDF, PDF to Word"],
    ["Microsoft Excel", e?.excel, "Excel to PDF"],
    ["Microsoft PowerPoint", e?.powerpoint, "PowerPoint to PDF"],
    ["LibreOffice", e?.libreoffice, "Used for Office files when Microsoft Office isn't installed"],
    ["PDF engine", e?.pdf, "Page previews, PDF to images, Excel and PowerPoint"],
    ["Video engine", e?.video, "Video compression and HEIC / AVIF photos"],
    ["Text recognition", e?.ocr, "Reads scanned pages, built into Windows"],
  ];
  const langs = s.ocrLangs;

  return (
    <div className="flex max-w-[720px] flex-col gap-[18px]">
      <div className="text-[20px] font-semibold tracking-[-0.015em]">Settings</div>

      <Section title="Saving">
        <SRow label="Save files to">
          <div className="flex min-w-0 items-center gap-2.5">
            <span className="truncate font-mono text-[12.5px]" title={s.outDir}>
              {s.outDir}
            </span>
            <Button
              size="sm"
              onClick={async () => {
                const d = await pickFolder(s.outDir);
                if (d) s.setSetting("outDir", d);
              }}
            >
              Change
            </Button>
            <Button size="sm" variant="ghost" onClick={async () => s.setSetting("outDir", await defaultOutputDir())}>
              Downloads
            </Button>
          </div>
        </SRow>
        <SRow
          label="If a file with the same name exists"
          desc={s.onExists === "keep" ? "The new file gets (1), (2) and so on." : s.onExists === "replace" ? "The old file is overwritten." : "DocShift asks each time."}
        >
          <Seg size="sm" value={s.onExists} onChange={(v) => s.setSetting("onExists", v)} options={[["Keep both", "keep"], ["Replace", "replace"], ["Ask", "ask"]]} />
        </SRow>
        <SRow label="Show the result in Explorer when finished">
          <Toggle label="Show the result in Explorer when finished" on={s.openAfter} onChange={(v) => s.setSetting("openAfter", v)} />
        </SRow>
      </Section>

      <Section title="Defaults">
        <SRow label="Compression level" desc="Used when you open Compress">
          <Seg size="sm" value={s.defLevel} onChange={(v) => s.setSetting("defLevel", v)} options={[["Light", "light"], ["Balanced", "balanced"], ["Smallest", "strong"]]} />
        </SRow>
        <SRow
          label="Text recognition language"
          desc={langs && !langs.length ? "No language with text recognition is installed in Windows." : "For scanned pages. More languages: Windows Settings > Time & language."}
        >
          <select
            className="field !h-[30px] !w-auto min-w-[180px] cursor-pointer text-[12.5px]"
            value={s.ocrLang}
            onChange={(ev) => s.setSetting("ocrLang", ev.target.value)}
            aria-label="Text recognition language"
            disabled={!langs?.length}
          >
            <option value="">Same as Windows</option>
            {langs?.map((l) => (
              <option key={l.tag} value={l.tag}>
                {l.name}
              </option>
            ))}
          </select>
        </SRow>
      </Section>

      <Section title="Privacy">
        <SRow label="Works fully offline" desc="DocShift never uploads files and needs no account." />
        <SRow label="Keep recent files list" desc="Stored on this computer for 30 days">
          <Toggle label="Keep recent files list" on={s.keepHistory} onChange={(v) => s.setSetting("keepHistory", v)} />
        </SRow>
        <SRow label="Clear recent files">
          <Button size="sm" disabled={!s.history.length} onClick={s.clearHistory}>
            Clear
          </Button>
        </SRow>
      </Section>

      <Section title="Appearance">
        <SRow label="Theme">
          <Seg size="sm" value={s.theme} onChange={(v) => s.setSetting("theme", v)} options={[["Light", "light"], ["Dark", "dark"], ["System", "system"]]} />
        </SRow>
      </Section>

      <Section title="Engines on this computer">
        {engineRows.map(([name, ok, why]) => (
          <SRow key={name} label={name} desc={why}>
            <span className={ok ? "text-[12.5px] font-medium text-acc-ink" : "text-[12.5px] text-faint"}>{e ? (ok ? "Available" : "Not found") : "Checking…"}</span>
          </SRow>
        ))}
      </Section>

      <Section title="About">
        <SRow label={`DocShift ${pkg.version}`} desc="Tauri 2 · Rust · React 19. Bundles FFmpeg (GPL) and PDFium. Office files are exported by your own Microsoft Office.">
          <Button size="sm" onClick={() => void openUrl("https://github.com/aalokpathak/DocShift")}>
            GitHub
          </Button>
        </SRow>
      </Section>
    </div>
  );
}
