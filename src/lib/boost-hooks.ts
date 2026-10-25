import { useCallback, useEffect, useState } from "react";
import { backend, type TweakState } from "@/lib/backend";

export type StateMap = Record<string, TweakState>;

/** Live state of every tweak (re-read when the window comes back: Windows Settings may have changed). */
export function useBoostStates() {
  const [states, setStates] = useState<StateMap | null>(null);
  const refresh = useCallback(async () => {
    const list = await backend.boostState();
    if (list) setStates(Object.fromEntries(list.map((s) => [s.id, s])));
  }, []);
  useEffect(() => {
    refresh();
    const onBack = () => document.visibilityState === "visible" && refresh();
    window.addEventListener("focus", onBack);
    document.addEventListener("visibilitychange", onBack);
    return () => {
      window.removeEventListener("focus", onBack);
      document.removeEventListener("visibilitychange", onBack);
    };
  }, [refresh]);
  return {
    states,
    refresh,
    put: (s: TweakState) => setStates((m) => ({ ...(m ?? {}), [s.id]: s })),
  };
}

/** Whether the app runs as administrator (false until known, and outside the app). */
export function useElevated() {
  const [elevated, setElevated] = useState(false);
  useEffect(() => {
    let alive = true;
    backend.appInfo().then((i) => alive && i && setElevated(i.elevated));
    return () => {
      alive = false;
    };
  }, []);
  return elevated;
}

/** The last non-null value: a closing dialog keeps its content while it fades out. */
export function useLatest<T>(value: T | null): T | null {
  const [last, setLast] = useState<T | null>(value);
  useEffect(() => {
    if (value !== null) setLast(value);
  }, [value]);
  return value ?? last;
}
