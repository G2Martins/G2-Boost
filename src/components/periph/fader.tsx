import { Undo2 } from "lucide-react";
import { type CSSProperties, type ReactNode, useEffect, useId, useState } from "react";
import { cn } from "@/lib/utils";

/**
 * Native range input dressed as a mixing-desk fader: lit track up to the cap, a notch in the cap,
 * tick marks under it and optional marks (the 1:1 point). The value is committed when the pointer
 * or the key is released, so dragging does not write to Windows on every step.
 */
const RANGE = [
  "h-6 w-full cursor-pointer appearance-none bg-transparent disabled:cursor-not-allowed disabled:opacity-40",
  "[&::-webkit-slider-runnable-track]:h-1.5 [&::-webkit-slider-runnable-track]:rounded-full",
  "[&::-webkit-slider-runnable-track]:bg-[linear-gradient(90deg,hsl(var(--primary)/0.4),hsl(var(--primary))_var(--fill),hsl(var(--foreground)/0.09)_var(--fill))]",
  "[&::-webkit-slider-thumb]:-mt-2 [&::-webkit-slider-thumb]:h-5.5 [&::-webkit-slider-thumb]:w-3",
  "[&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-[3px]",
  "[&::-webkit-slider-thumb]:border [&::-webkit-slider-thumb]:border-border-strong",
  "[&::-webkit-slider-thumb]:bg-foreground",
  "[&::-webkit-slider-thumb]:bg-[image:linear-gradient(90deg,transparent_44%,hsl(var(--primary))_44%,hsl(var(--primary))_56%,transparent_56%)]",
  "[&::-webkit-slider-thumb]:shadow-[0_2px_6px_hsl(0_0%_0%/0.55)]",
].join(" ");

export type Mark = { value: number; label: string };

export function Fader({
  label,
  hint,
  value,
  min,
  max,
  step = 1,
  disabled,
  format,
  marks = [],
  ends,
  original,
  onCommit,
  onRevert,
}: {
  label: string;
  hint?: ReactNode;
  value: number;
  min: number;
  max: number;
  step?: number;
  disabled?: boolean;
  format: (v: number) => string;
  marks?: Mark[];
  ends: [string, string];
  /** Button text when a saved original differs from the value now ("back to the original (10)"). */
  original?: string | null;
  onCommit: (v: number) => void;
  onRevert?: () => void;
}) {
  const id = useId();
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const at = (v: number) => ((v - min) / (max - min)) * 100;
  const span = max - min;
  const ticks =
    span <= 20
      ? Array.from({ length: span + 1 }, (_, i) => min + i)
      : [0, 1, 2, 3, 4].map((i) => min + (span * i) / 4);
  const commit = () => {
    if (!disabled && draft !== value) onCommit(draft);
  };
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-baseline justify-between gap-3">
        <label htmlFor={id} className="text-[13px] font-semibold">
          {label}
        </label>
        <output htmlFor={id} className="num text-[13px] font-semibold">
          {format(draft)}
        </output>
      </div>
      <input
        id={id}
        type="range"
        min={min}
        max={max}
        step={step}
        value={draft}
        disabled={disabled}
        aria-valuetext={format(draft)}
        style={{ "--fill": `${at(draft)}%` } as CSSProperties}
        className={RANGE}
        onChange={(e) => setDraft(Number(e.target.value))}
        onPointerUp={commit}
        onKeyUp={commit}
      />
      <div aria-hidden className="relative mx-1.5 h-4">
        {ticks.map((v) => (
          <span
            key={v}
            className="absolute top-0 h-1.5 w-px bg-foreground/25"
            style={{ left: `${at(v)}%` }}
          />
        ))}
        {marks.map((m) => (
          <span
            key={m.value}
            className="absolute top-0 flex -translate-x-1/2 flex-col items-center"
            style={{ left: `${at(m.value)}%` }}
          >
            <span className="h-2.5 w-px bg-primary" />
            <span className="num text-[10px] font-semibold text-primary">{m.label}</span>
          </span>
        ))}
      </div>
      <div className="flex justify-between text-[10px] text-muted-foreground">
        <span>{ends[0]}</span>
        <span>{ends[1]}</span>
      </div>
      {hint ? <p className="text-[11px] leading-relaxed text-muted-foreground">{hint}</p> : null}
      {original && onRevert ? (
        <button
          type="button"
          onClick={onRevert}
          className={cn(
            "press -ml-1.5 inline-flex w-fit items-center gap-1.5 rounded-md px-1.5 py-1 text-xs font-semibold text-primary",
            "hover:bg-primary/10",
          )}
        >
          <Undo2 className="size-3.5" />
          {original}
        </button>
      ) : null}
    </div>
  );
}
