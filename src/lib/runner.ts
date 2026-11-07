// Glue between screens, the store and the backend.
import { errText, validatePages, validateTarget } from "./api";
import { useApp, type Category, type Item, type ListKey, type Screen } from "./store";

/** The most recent job for a list, so a screen shows live progress even after navigating away and back. */
export function useLatestJob(list: ListKey) {
  return useApp((s) => {
    // a running job wins (several can run at once for one click); otherwise the most recent one
    let best: (typeof s.jobs)[string] | null = null;
    for (const j of Object.values(s.jobs)) {
      if (j.list !== list) continue;
      if (!best || (j.running && !best.running) || (j.running === best.running && j.started > best.started)) best = j;
    }
    return best;
  });
}

/** Files a run would process: anything waiting or failed; finished ones only if nothing else is pending. */
export function runnable(items: Item[]): Item[] {
  const fresh = items.filter((i) => i.status === "ready" || i.status === "error");
  return fresh.length ? fresh : items.filter((i) => i.status === "done");
}

/** Values the backend would reject, caught before starting so the message is immediate. */
export async function validate(values: Record<string, unknown>): Promise<string | null> {
  try {
    if (values.action !== "expand" && typeof values.target_mb === "string" && values.target_mb.trim()) await validateTarget(values.target_mb);
    if (typeof values.pages === "string" && values.pages.trim()) await validatePages(values.pages, 100000);
  } catch (e) {
    return errText(e);
  }
  return null;
}

/** One task per file, all with the same backend mode. */
export async function runEach(a: { list: ListKey; screen: Screen; label: string; category: Category; mode: string; items: Item[]; options: Record<string, unknown> }) {
  const st = useApp.getState();
  const bad = await validate(a.options);
  if (bad) {
    st.toast({ kind: "error", text: bad });
    return null;
  }
  const taskItems: Record<string, string[]> = {};
  const tasks = a.items.map((it) => {
    taskItems[it.id] = [it.id];
    return { id: it.id, inputs: [it.info.path] };
  });
  return st.runJob({ list: a.list, screen: a.screen, label: a.label, category: a.category, spec: { mode: a.mode, tasks, options: a.options }, taskItems });
}
