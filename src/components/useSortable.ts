import { useCallback, useEffect, useRef, useState } from "react";

/**
 * Pointer-based drag-to-reorder for lists and grids.
 *
 * Native HTML5 drag & drop doesn't work inside a Tauri window on Windows (the OS file-drop handler takes over), so
 * this tracks the pointer itself. Items need `data-sort-index={i}`; the grab handle gets `onPointerDown`.
 * While dragging, the item is moved live to wherever the pointer is, and the list scrolls near its edges.
 */
export function useSortable(count: number, onMove: (from: number, to: number) => void) {
  const [dragging, setDragging] = useState<number | null>(null);
  const containerRef = useRef<HTMLElement | null>(null);
  const current = useRef<number | null>(null);
  const pointer = useRef({ x: 0, y: 0 });
  const raf = useRef(0);
  const moveRef = useRef(onMove);
  moveRef.current = onMove;

  const itemAt = useCallback((x: number, y: number): number | null => {
    const root = containerRef.current;
    if (!root) return null;
    let best: number | null = null;
    let bestDist = Infinity;
    root.querySelectorAll<HTMLElement>("[data-sort-index]").forEach((el) => {
      const r = el.getBoundingClientRect();
      const idx = Number(el.dataset.sortIndex);
      if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) {
        best = idx;
        bestDist = -1;
      } else if (bestDist >= 0) {
        const d = Math.hypot(Math.max(r.left - x, 0, x - r.right), Math.max(r.top - y, 0, y - r.bottom));
        if (d < bestDist) {
          bestDist = d;
          best = idx;
        }
      }
    });
    return best;
  }, []);

  const scrollParent = useCallback((): HTMLElement | null => {
    let el = containerRef.current?.parentElement ?? null;
    while (el) {
      const oy = getComputedStyle(el).overflowY;
      if ((oy === "auto" || oy === "scroll") && el.scrollHeight > el.clientHeight) return el;
      el = el.parentElement;
    }
    return null;
  }, []);

  useEffect(() => {
    if (dragging === null) return;
    const onMovePtr = (e: PointerEvent) => {
      pointer.current = { x: e.clientX, y: e.clientY };
      const over = itemAt(e.clientX, e.clientY);
      if (over !== null && current.current !== null && over !== current.current) {
        moveRef.current(current.current, over);
        current.current = over;
        setDragging(over);
      }
    };
    const end = () => {
      current.current = null;
      setDragging(null);
    };
    const tick = () => {
      const sp = scrollParent();
      if (sp) {
        const r = sp.getBoundingClientRect();
        const y = pointer.current.y;
        if (y < r.top + 48) sp.scrollTop -= Math.min(18, (r.top + 48 - y) / 2 + 2);
        else if (y > r.bottom - 48) sp.scrollTop += Math.min(18, (y - (r.bottom - 48)) / 2 + 2);
      }
      raf.current = requestAnimationFrame(tick);
    };
    raf.current = requestAnimationFrame(tick);
    window.addEventListener("pointermove", onMovePtr);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
    document.body.style.cursor = "grabbing";
    return () => {
      cancelAnimationFrame(raf.current);
      window.removeEventListener("pointermove", onMovePtr);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      document.body.style.cursor = "";
    };
  }, [dragging !== null, itemAt, scrollParent]); // eslint-disable-line react-hooks/exhaustive-deps

  const startDrag = (index: number) => (e: React.PointerEvent) => {
    if (e.button !== 0 || count < 2) return;
    e.preventDefault();
    pointer.current = { x: e.clientX, y: e.clientY };
    current.current = index;
    setDragging(index);
  };

  return { dragging, containerRef, startDrag };
}
