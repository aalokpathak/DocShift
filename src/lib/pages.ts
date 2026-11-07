export interface PageSel {
  index: number;
  rotate: number;
}

export type RangeResult = { ok: true; groups: number[][] } | { ok: false; error: string };

/**
 * "1-3, 5, 8-10" -> [[0,1,2],[4],[7,8,9]] (zero-based). Each comma-separated piece becomes its own group, which is
 * exactly what splitting needs; for selecting pages the groups are simply flattened.
 */
export function parseRanges(spec: string, count: number): RangeResult {
  const pieces = spec.split(/[,;]+/).map((p) => p.trim()).filter(Boolean);
  if (!pieces.length) return { ok: false, error: "Type some pages, for example 1-3, 5." };
  const groups: number[][] = [];
  for (const piece of pieces) {
    const m = /^(\d+)\s*(?:-\s*(\d+))?$/.exec(piece);
    if (!m) return { ok: false, error: `"${piece}" is not a valid page range. Use something like 1-3, 5.` };
    let a = Number(m[1]);
    let b = Number(m[2] ?? m[1]);
    if (a > b) [a, b] = [b, a];
    if (a < 1 || b > count) return { ok: false, error: `Pages ${piece} are outside this PDF, which has ${count} page(s).` };
    groups.push(Array.from({ length: b - a + 1 }, (_, i) => a - 1 + i));
  }
  return { ok: true, groups };
}

export function chunk(count: number, n: number): number[][] {
  const out: number[][] = [];
  for (let i = 0; i < count; i += n) out.push(Array.from({ length: Math.min(n, count - i) }, (_, k) => i + k));
  return out;
}

/** "1-3" style label for a group of page indexes. */
export function groupLabel(g: number[]): string {
  if (!g.length) return "";
  const sorted = [...g].sort((a, b) => a - b);
  const parts: string[] = [];
  let start = sorted[0];
  let prev = start;
  for (const v of sorted.slice(1).concat(Infinity)) {
    if (v !== prev + 1) {
      parts.push(start === prev ? `${start + 1}` : `${start + 1}–${prev + 1}`);
      start = v;
    }
    prev = v;
  }
  return parts.join(", ");
}
