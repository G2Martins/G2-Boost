import { useCallback, useEffect, useState } from "react";
import { useErrorText } from "@/components/elevate-button";
import { Badge, Skeleton } from "@/components/ui/primitives";
import type { Restart, Tweak } from "@/data/catalogs";
import { backend, type FxLevel, inTauri, type TweakChange, type TweakState } from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

export type StateMap = Record<string, TweakState>;

/** Live tweak states; re-read when the window comes back (the user may have changed Settings). */
export function useTweakStates() {
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
  const put = useCallback((s: TweakState) => setStates((m) => ({ ...(m ?? {}), [s.id]: s })), []);
  return { states, put, refresh };
}

/** One tweak on or off (visual effects take a level; off restores the backup). */
export function setTweak(id: string, on: boolean, level?: FxLevel): Promise<TweakState> {
  if (id === "visual_fx") {
    return on ? backend.boostVisualFx(level ?? "balanced") : backend.boostVisualFxRevert();
  }
  return backend.boostApply(id, on);
}

/** Error text, plus the servicing slot being busy (a DISM job runs). */
export function useTweakErrorText() {
  const { t } = useI18n();
  const errorText = useErrorText();
  return useCallback(
    (e: unknown) =>
      e instanceof Error && e.message === "busy" ? t("tweaks.error.busy") : errorText(e),
    [t, errorText],
  );
}

/** Reading it needs administrator (a Windows feature): unknown, not "off". */
export const unknownNeedsAdmin = (s?: TweakState) =>
  s?.status === "unavailable" && s.detail === "needs_admin";

/** Can be turned on from here right now. */
export const applicable = (tw: Tweak, s: TweakState | undefined, elevated: boolean) =>
  (!tw.admin || elevated) &&
  s?.status !== "on" &&
  (s === undefined ? !inTauri : s.status !== "unavailable");

const RESTART_KEY: Record<Restart, I18nKey> = {
  reboot: "boost.state.reboot",
  signout: "boost.state.signout",
  explorer: "boost.state.explorer",
};

export function RestartBadge({ restart }: { restart?: Restart }) {
  const { t } = useI18n();
  return restart ? <Badge tone="neutral">{t(RESTART_KEY[restart])}</Badge> : null;
}

/** "Ver o que muda": the exact writes, read from the core's own definition of the tweak. */
export function TweakChanges({ id, className }: { id: string; className?: string }) {
  const { t } = useI18n();
  const [list, setList] = useState<TweakChange[] | null | undefined>(undefined);
  useEffect(() => {
    let alive = true;
    backend.tweakChanges(id).then((l) => alive && setList(l));
    return () => {
      alive = false;
    };
  }, [id]);
  const value = (c: TweakChange) => {
    if (c.kind === "plan") return t("tweaks.change.plan");
    if (c.kind === "visual_fx") return t("tweaks.change.visual_fx");
    if (c.value === null) return t("tweaks.change.removed");
    if (c.value === "disabled") return t("tweaks.change.disabled");
    if (c.value === "manual") return t("tweaks.change.manual");
    return c.value;
  };
  return (
    <div className={cn("rounded-md border border-border bg-black/25 px-3 py-2.5", className)}>
      {list === undefined ? (
        <div className="space-y-1.5">
          <span className="sr-only">{t("tweaks.changes.loading")}</span>
          <Skeleton className="h-3 w-4/5" />
          <Skeleton className="h-3 w-3/5" />
        </div>
      ) : list === null ? (
        <p className="text-[11px] text-muted-foreground">{t("tweaks.changes.app_only")}</p>
      ) : (
        <ul className="space-y-1.5">
          {list.map((c) => (
            <li key={`${c.kind}:${c.target}`} className="grid gap-0.5 text-[11px] leading-snug">
              <span className="text-[10px] font-semibold tracking-[0.08em] text-muted-foreground uppercase">
                {t(`tweaks.change.kind.${c.kind}` as I18nKey)}
              </span>
              <span className="break-all font-mono text-foreground/90">
                {c.target}
                <span className="text-muted-foreground"> → </span>
                <span className={c.value === null ? "text-warning" : "text-primary"}>
                  {value(c)}
                </span>
              </span>
            </li>
          ))}
        </ul>
      )}
      <p className="mt-2 text-[10px] text-muted-foreground">{t("tweaks.changes.note")}</p>
    </div>
  );
}
