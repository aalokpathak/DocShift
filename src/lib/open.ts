// Opening results: errors are shown instead of silently doing nothing.
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { errText, openFile } from "./api";
import { useApp } from "./store";

export async function openResult(path: string): Promise<boolean> {
  try {
    await openFile(path);
    return true;
  } catch (e) {
    useApp.getState().toast({ kind: "error", text: errText(e) });
    return false;
  }
}

export async function showInFolder(path: string): Promise<void> {
  try {
    await revealItemInDir(path);
  } catch (e) {
    useApp.getState().toast({ kind: "error", text: `Couldn't show it in Explorer: ${errText(e)}` });
  }
}
