import { useEffect, useRef, useState } from "react";

export const reducedMotion = () =>
  typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/**
 * Tween a number toward `target` (rAF, ease-out cubic). The first value counts up from 0;
 * later changes tween from what is on screen. `null` = no value yet (the caller shows a
 * skeleton, never a placeholder 0 — so there is no 0→value flash). Reduced motion: no tween.
 */
export function useCountUp(target: number | null, duration = 600): number | null {
  const [shown, setShown] = useState<number | null>(target === null ? null : 0);
  const current = useRef<number>(0);
  useEffect(() => {
    if (target === null) {
      setShown(null);
      return;
    }
    const start = current.current;
    if (start === target || reducedMotion()) {
      current.current = target;
      setShown(target);
      return;
    }
    let raf = 0;
    const t0 = performance.now();
    const tick = (now: number) => {
      const p = Math.min(1, (now - t0) / duration);
      const v = start + (target - start) * (1 - (1 - p) ** 3);
      current.current = v;
      setShown(v);
      if (p < 1) raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [target, duration]);
  return shown;
}

export function CountUp({
  value,
  format,
  className,
}: {
  value: number;
  format: (n: number) => string;
  className?: string;
}) {
  const shown = useCountUp(value) ?? value;
  return <span className={className}>{format(shown)}</span>;
}
