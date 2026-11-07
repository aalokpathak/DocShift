import { useEffect, useRef, useState } from "react";
import { thumbnail, type Kind } from "../lib/api";
import { DEFAULT_EXT } from "../lib/kinds";
import { cx, FileIcon } from "./ui";

/** Lazy preview of an image / video / PDF page. Loads when scrolled into view; striped placeholder until then. */
export function Thumb({
  path,
  kind,
  page = null,
  size = 220,
  className,
  rotate = 0,
  fit = "cover",
  aspect,
}: {
  path: string;
  kind: Kind;
  page?: number | null;
  size?: number;
  className?: string;
  rotate?: number;
  fit?: "cover" | "contain";
  /** Width / height of the (unrotated) content, so the box has its final size before the image arrives. */
  aspect?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [src, setSrc] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setVisible(true);
          io.disconnect();
        }
      },
      { rootMargin: "300px" },
    );
    io.observe(el);
    return () => io.disconnect();
  }, []);

  useEffect(() => {
    if (!visible) return;
    let live = true;
    setFailed(false);
    thumbnail(path, page, size).then(
      (s) => live && setSrc(s),
      () => live && setFailed(true),
    );
    return () => {
      live = false;
    };
  }, [visible, path, page, size]);

  const sideways = rotate % 180 !== 0;
  const boxStyle = aspect ? { aspectRatio: sideways ? 1 / aspect : aspect, containerType: "size" as const } : undefined;
  // A quarter turn swaps the image's width and height so the rotated page still fits its box.
  const imgStyle = sideways
    ? { width: "100cqh", height: "100cqw", maxWidth: "none", transform: `rotate(${rotate}deg)` }
    : rotate
      ? { transform: `rotate(${rotate}deg)` }
      : undefined;

  return (
    <div ref={ref} style={boxStyle} className={cx("stripes relative grid place-items-center overflow-hidden", className)}>
      {src ? (
        <img
          src={src}
          alt=""
          draggable={false}
          className={cx(sideways ? "" : "absolute inset-0 size-full", "transition-transform duration-200", fit === "cover" ? "object-cover" : "object-contain")}
          style={imgStyle}
        />
      ) : failed ? (
        <FileIcon kind={kind} text={DEFAULT_EXT[kind]} />
      ) : null}
    </div>
  );
}
