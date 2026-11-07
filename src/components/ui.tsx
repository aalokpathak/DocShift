import { Loader2 } from "lucide-react";
import type { ButtonHTMLAttributes, CSSProperties, ReactNode, Ref } from "react";
import type { Kind } from "../lib/api";
import { chipColors } from "../lib/kinds";

export function cx(...parts: (string | false | null | undefined)[]): string {
  return parts.filter(Boolean).join(" ");
}

type BtnProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "ghost" | "danger";
  size?: "sm" | "md" | "lg";
  busy?: boolean;
  ref?: Ref<HTMLButtonElement>;
};

/** Buttons as drawn in the design: 30 / 34 / 40 px tall, 7–8 px radius, green primary. */
export function Button({ variant = "secondary", size = "md", busy, className, children, disabled, ...rest }: BtnProps) {
  const sizes = { sm: "h-[30px] px-3 text-[12.5px] rounded-[7px]", md: "h-[34px] px-3.5 text-[13px] rounded-[8px]", lg: "h-10 px-[18px] text-[14px] rounded-[8px]" }[size];
  const variants = {
    primary: "bg-acc text-white font-medium border border-transparent hover:brightness-110",
    secondary: "bg-surface text-ink border border-btn-line hover:border-strong-line",
    ghost: "bg-transparent text-muted border border-transparent hover:text-ink hover:bg-s1",
    danger: "bg-surface text-danger border border-btn-line hover:border-danger",
  }[variant];
  return (
    <button
      className={cx("inline-flex shrink-0 cursor-pointer items-center justify-center gap-1.5 whitespace-nowrap transition-colors disabled:cursor-not-allowed disabled:opacity-45", sizes, variants, className)}
      disabled={disabled || busy}
      {...rest}
    >
      {busy && <Loader2 className="size-3.5 animate-spin" />}
      {children}
    </button>
  );
}

export function IconButton({ label, className, children, ...rest }: ButtonHTMLAttributes<HTMLButtonElement> & { label: string }) {
  return (
    <button
      aria-label={label}
      title={label}
      className={cx(
        "grid size-[30px] shrink-0 cursor-pointer place-items-center rounded-[7px] border border-line bg-surface text-ink2 transition-colors hover:border-strong-line disabled:cursor-not-allowed disabled:opacity-35",
        className,
      )}
      {...rest}
    >
      {children}
    </button>
  );
}

/** Segmented control (grey track, white raised "on" segment). */
export function Seg<T extends string | number>({
  value,
  options,
  onChange,
  size = "md",
  stretch,
  label,
}: {
  value: T;
  options: readonly (readonly [string, T])[];
  onChange: (v: T) => void;
  size?: "sm" | "md";
  stretch?: boolean;
  label?: string;
}) {
  return (
    <div role="radiogroup" aria-label={label} className={cx("flex gap-[2px] rounded-[8px] bg-track p-[3px]", stretch && "w-full")}>
      {options.map(([text, v]) => {
        const on = v === value;
        return (
          <button
            key={String(v)}
            role="radio"
            aria-checked={on}
            onClick={() => onChange(v)}
            className={cx(
              "cursor-pointer whitespace-nowrap rounded-[6px] px-2.5 transition-colors",
              size === "sm" ? "h-[26px] text-[12.5px]" : "h-[30px] px-3.5 text-[13px]",
              stretch && "flex-1",
              on ? "bg-seg-on font-medium text-ink shadow-[0_1px_2px_rgba(0,0,0,.08)]" : "text-muted hover:text-ink",
            )}
          >
            {text}
          </button>
        );
      })}
    </div>
  );
}

export function Toggle({ on, onChange, label, disabled }: { on: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button
      role="switch"
      aria-checked={on}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!on)}
      className={cx("h-5 w-[34px] shrink-0 cursor-pointer rounded-[10px] p-[2px] transition-colors disabled:cursor-not-allowed disabled:opacity-45", on ? "bg-acc" : "bg-strong-line")}
    >
      <span className={cx("block size-4 rounded-full bg-white shadow-[0_1px_2px_rgba(0,0,0,.2)] transition-transform", on && "translate-x-[14px]")} />
    </button>
  );
}

/** A row with a label (and optional description) on the left and a control on the right. */
export function Row({ label, desc, children, className }: { label: ReactNode; desc?: ReactNode; children?: ReactNode; className?: string }) {
  return (
    <div className={cx("flex items-center justify-between gap-4", className)}>
      <div className="min-w-0">
        <div className="text-[13px]">{label}</div>
        {desc && <div className="mt-0.5 text-[12px] text-muted">{desc}</div>}
      </div>
      {children}
    </div>
  );
}

/** Radio indicator ring used by option cards and lists. */
export function Ring({ on }: { on: boolean }) {
  return <span className="size-4 shrink-0 rounded-full" style={{ border: on ? "5px solid var(--acc)" : "1.5px solid var(--strong-line)" }} />;
}

/** File-type chip ("PDF", "DOCX" …) in the type's colour. */
export function Chip({ kind, text, small }: { kind: Kind | "txt"; text: string; small?: boolean }) {
  return (
    <span className={cx("inline-block rounded-[5px] font-mono font-medium", small ? "px-[5px] py-[2px] text-[9.5px]" : "px-1.5 py-[3px] text-[10px]")} style={chipColors(kind)}>
      {text}
    </span>
  );
}

/** Square file icon with the extension in it. */
export function FileIcon({ kind, text, size = 36 }: { kind: Kind | "txt"; text: string; size?: number }) {
  const style: CSSProperties = { ...chipColors(kind), width: size, height: size, fontSize: size > 40 ? 11 : 10 };
  return (
    <div className="grid shrink-0 place-items-center rounded-[8px] font-mono font-medium" style={style}>
      {text}
    </div>
  );
}

export function Bar({ value, className, color = "var(--acc)" }: { value: number; className?: string; color?: string }) {
  const pct = Math.max(0, Math.min(1, value)) * 100;
  return (
    <div className={cx("h-1.5 overflow-hidden rounded-[3px] bg-track", className)} role="progressbar" aria-valuenow={Math.round(pct)} aria-valuemin={0} aria-valuemax={100}>
      <div className="h-full rounded-[3px] transition-[width] duration-300" style={{ width: `${pct}%`, background: color }} />
    </div>
  );
}

export function PageTitle({ title, sub, children }: { title: ReactNode; sub?: ReactNode; children?: ReactNode }) {
  return (
    <div className="flex items-end justify-between gap-5">
      <div className="min-w-0">
        <div className="text-[20px] font-semibold tracking-[-0.015em]">{title}</div>
        {sub && <div className="mt-1 text-[13px] text-muted">{sub}</div>}
      </div>
      {children && <div className="flex shrink-0 items-center gap-2">{children}</div>}
    </div>
  );
}

export function Logo({ size = 28 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 28 28" className="block shrink-0" aria-hidden>
      <rect width="28" height="28" rx="7" style={{ fill: "var(--acc)" }} />
      <rect x="5.5" y="5" width="11" height="14" rx="2" fill="none" stroke="#fff" strokeOpacity="0.6" strokeWidth="1.6" />
      <rect x="11.5" y="9" width="11" height="14" rx="2" fill="#fff" />
      <path d="M14.5 16h5M17.5 14l2 2-2 2" fill="none" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" style={{ stroke: "var(--acc)" }} />
    </svg>
  );
}
