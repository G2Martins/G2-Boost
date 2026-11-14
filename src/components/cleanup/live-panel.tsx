import { Check, Square } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Rail } from "@/components/apps/updates-tab";
import { CountUp } from "@/components/ui/count-up";
import { Button, Card } from "@/components/ui/primitives";
import type { CleanupProgress } from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn, formatBytes } from "@/lib/utils";

/** `mm:ss` of a duration in milliseconds. */
function clock(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

/** Seconds on one item before the panel says it is a big one and the app is still on it. */
const SLOW_STEP_SECS = 20;

/**
 * What the core is doing right now: how far, bytes so far, the item in flight and for how
 * long, and Stop. The clock ticks every second even while one big item sends no new event,
 * so a long step never looks like a hang.
 */
export function LivePanel({
  live,
  running,
  ids,
  since,
  expected,
  nameOf,
  onStop,
  opaque,
  noteOf,
}: {
  live: CleanupProgress;
  /** Cleaning (true) or measuring. */
  running: boolean;
  /** The items in the order the core goes through them. */
  ids: string[];
  since: number;
  /** Bytes the preview found for this run (0 = progress by item count). */
  expected: number;
  nameOf: (id: string) => string;
  onStop: () => void;
  /** Items with nothing to count while they work (the rail sweeps instead). */
  opaque?: (id: string) => boolean;
  /** A note for an item that is known to be long. */
  noteOf?: (id: string) => I18nKey | undefined;
}) {
  const { t, locale } = useI18n();
  const [, tick] = useState(0);
  const [stopping, setStopping] = useState(false);
  const stepAt = useRef(Date.now());
  const current = live.current;
  useEffect(() => {
    stepAt.current = current ? Date.now() : 0;
  }, [current]);
  useEffect(() => {
    const id = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, []);

  const now = Date.now();
  const stepSecs = current ? Math.floor((now - stepAt.current) / 1000) : 0;
  const percent =
    current && opaque?.(current)
      ? null
      : Math.round(
          running && expected > 0
            ? Math.min(99, (100 * live.bytes) / expected)
            : (100 * live.done) / Math.max(1, live.total),
        );
  const position = Math.min(live.done + (current ? 1 : 0), live.total);
  const note = current ? noteOf?.(current) : undefined;

  return (
    <Card className="animate-rise border-primary/30">
      <div className="flex flex-wrap items-center gap-3">
        <span
          aria-hidden
          className="size-2 rounded-full bg-success motion-safe:animate-pulse-dot"
        />
        <span role="status" className="font-display text-[15px] font-semibold tracking-wide">
          {t(running ? "cleanup.live.cleaning" : "cleanup.live.measuring", {
            done: position,
            total: live.total,
          })}
        </span>
        <span className="num text-xs text-muted-foreground">{clock(now - since)}</span>
        <Button
          variant="outline"
          size="sm"
          className="ml-auto"
          disabled={stopping}
          onClick={() => {
            setStopping(true);
            onStop();
          }}
        >
          <Square className="size-3.5" />
          {t(stopping ? "cleanup.live.stopping" : "cleanup.live.stop")}
        </Button>
      </div>
      <p className="mt-3 flex flex-wrap items-baseline gap-x-2">
        <CountUp
          value={live.bytes}
          format={(n) => formatBytes(n, locale)}
          className="num text-2xl font-semibold"
        />
        <span className="num text-xs text-muted-foreground">
          {running
            ? t("cleanup.live.freed", { files: live.files.toLocaleString(locale) })
            : t("cleanup.live.found", { files: live.files.toLocaleString(locale) })}
        </span>
      </p>
      <div className="mt-3 flex">
        <Rail percent={percent} />
      </div>
      {current ? (
        <p className="num mt-2 text-xs text-muted-foreground">
          {t("cleanup.live.now", { item: nameOf(current), secs: stepSecs })}
        </p>
      ) : null}
      {note ? (
        <p className="mt-1 text-xs text-warning/90">{t(note)}</p>
      ) : current && stepSecs >= SLOW_STEP_SECS ? (
        <p className="mt-1 text-xs text-warning/90">{t("cleanup.live.slow")}</p>
      ) : null}
      <ul className="mt-3 flex flex-wrap gap-1.5" aria-label={t("cleanup.live.queue")}>
        {ids.map((id, i) => {
          const state = i < live.done ? "done" : id === current ? "active" : "pending";
          return (
            <li
              key={id}
              className={cn(
                "flex items-center gap-1 rounded-md border px-2 py-0.5 text-[11px] transition-colors duration-300",
                state === "done" && "border-success/30 text-success",
                state === "active" && "border-primary/60 bg-primary/10 text-foreground",
                state === "pending" && "border-border text-muted-foreground/70",
              )}
            >
              {state === "done" ? <Check className="size-3" aria-hidden /> : null}
              {nameOf(id)}
            </li>
          );
        })}
      </ul>
    </Card>
  );
}
