import { type ReactNode, useEffect, useState } from "react";
import { LuliMark } from "@/components/luli-card";
import { SafeMarkdown } from "@/components/safe-markdown";
import { type I18nKey, useI18n } from "@/lib/i18n";

function reducedMotion(): boolean {
  try {
    return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  } catch {
    return false;
  }
}

/** Phrase for each stretch of the wait, so a slow answer never looks like a frozen app. */
function phase(seconds: number, web: boolean): I18nKey {
  if (seconds < 3) return "ai.think.read";
  if (seconds < 8) return web ? "ai.think.web" : "ai.think.think";
  if (seconds < 20) return "ai.think.check";
  return "ai.think.slow";
}

/** Luli is working: her mark turns, the phrase shimmers, the seconds count up. */
export function LuliThinking({ web }: { web: boolean }) {
  const { t } = useI18n();
  const [seconds, setSeconds] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setSeconds((s) => s + 1), 1000);
    return () => clearInterval(id);
  }, []);
  return (
    <div className="flex items-center gap-3 py-1">
      <LuliMark className="luli-thinking size-7" />
      <span role="status" className="think-shimmer text-sm font-medium">
        {t(phase(seconds, web))}
      </span>
      <span className="font-mono text-[11px] tabular-nums text-muted-foreground" aria-hidden>
        {seconds}s
      </span>
    </div>
  );
}

/**
 * Typewriter for an answer that arrives now (streamed in chunks or all at once): the text is
 * revealed at a steady pace that speeds up when a big chunk lands. Answers reopened from the
 * history, and users who asked for less motion, see the whole text at once. `after` (actions,
 * sources, next questions) shows when the typing ends.
 */
export function TypeText({
  text,
  live,
  pending,
  after,
}: {
  text: string;
  /** Born while Luli was answering: only then it types. */
  live: boolean;
  /** More text may still stream in (keeps the caret). */
  pending?: boolean;
  after?: ReactNode;
}) {
  const [typing] = useState(() => live && !reducedMotion());
  const [shown, setShown] = useState(() => (typing ? 0 : text.length));
  useEffect(() => {
    if (!typing || shown >= text.length) return;
    const raf = requestAnimationFrame(() =>
      setShown(Math.min(text.length, shown + Math.max(3, Math.ceil((text.length - shown) / 45)))),
    );
    return () => cancelAnimationFrame(raf);
  }, [typing, shown, text]);
  const done = !typing || (shown >= text.length && !pending);
  return (
    <>
      <SafeMarkdown text={typing ? text.slice(0, shown) : text} />
      {done ? (
        after
      ) : (
        <span
          className="type-caret ml-0.5 inline-block h-4 w-1.5 bg-primary align-middle"
          aria-hidden
        />
      )}
    </>
  );
}
