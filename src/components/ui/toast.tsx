import { X } from "lucide-react";
import { useSyncExternalStore } from "react";
import { reducedMotion } from "@/components/ui/count-up";
import type { Tone } from "@/components/ui/primitives";
import { cn } from "@/lib/utils";

type Item = {
  id: number;
  text: string;
  tone: Tone;
  action?: { label: string; run: () => void };
  state: "in" | "out";
};

// ponytail: module-level store (one Toaster per app); a context only if two shells ever coexist.
let seq = 0;
let items: Item[] = [];
const subs = new Set<() => void>();
const emit = () => {
  for (const s of subs) s();
};

/** Short confirmation at the bottom-right ("Ajuste aplicado", with an optional "Desfazer"). */
export function toast(
  text: string,
  opts: { tone?: Tone; action?: { label: string; run: () => void }; duration?: number } = {},
) {
  const id = ++seq;
  items = [
    ...items,
    { id, text, tone: opts.tone ?? "neutral", action: opts.action, state: "in" as const },
  ].slice(-4);
  emit();
  window.setTimeout(() => dismiss(id), opts.duration ?? 5000);
}

function dismiss(id: number) {
  if (!items.some((t) => t.id === id && t.state === "in")) return;
  const drop = () => {
    items = items.filter((t) => t.id !== id);
    emit();
  };
  if (reducedMotion()) return drop();
  items = items.map((t) => (t.id === id ? { ...t, state: "out" } : t));
  emit();
  window.setTimeout(drop, 220);
}

const subscribe = (cb: () => void) => {
  subs.add(cb);
  return () => subs.delete(cb);
};

export function Toaster({ closeLabel }: { closeLabel: string }) {
  const list = useSyncExternalStore(subscribe, () => items);
  return (
    <section
      aria-live="polite"
      className="pointer-events-none fixed right-4 bottom-4 z-[60] flex w-84 flex-col gap-2"
    >
      {list.map((t) => (
        <div
          key={t.id}
          data-state={t.state}
          className={cn("toast pointer-events-auto", `tone-${t.tone}`)}
        >
          <p className="flex-1 pt-0.5 text-[13px] leading-snug">{t.text}</p>
          {t.action ? (
            <button
              type="button"
              onClick={() => {
                t.action?.run();
                dismiss(t.id);
              }}
              className="press shrink-0 rounded-md px-2 py-1 text-xs font-semibold text-primary hover:bg-primary/10"
            >
              {t.action.label}
            </button>
          ) : null}
          <button
            type="button"
            onClick={() => dismiss(t.id)}
            aria-label={closeLabel}
            className="press -m-1 flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-foreground/6 hover:text-foreground"
          >
            <X className="size-3.5" />
          </button>
        </div>
      ))}
    </section>
  );
}
