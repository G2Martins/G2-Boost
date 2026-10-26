import { BadgeInfo, Loader2, RotateCcw, ShieldCheck } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Logo } from "@/components/shell/app-shell";
import { Button } from "@/components/ui/primitives";
import { backend } from "@/lib/backend";
import { useI18n } from "@/lib/i18n";

/**
 * "Sempre abrir como administrador": our own screen explains the request, then Windows asks.
 * The UAC prompt dims over this screen (instead of a bare desktop). Yes = the elevated copy opens
 * and this one quits; No = this copy carries on in limited mode.
 */
export function AdminGate({ route, onSkip }: { route: string; onSkip: () => void }) {
  const { t } = useI18n();
  const [phase, setPhase] = useState<"asking" | "declined">("asking");
  const ask = useCallback(() => {
    setPhase("asking");
    // Let this screen paint first: the prompt shows a dimmed snapshot of what is on screen.
    window.setTimeout(() => {
      backend.relaunchElevated(route).catch(() => setPhase("declined"));
    }, 700);
  }, [route]);
  useEffect(ask, [ask]);

  return (
    <div className="carbon fixed inset-0 z-[70] flex items-center justify-center p-6">
      <div className="banner absolute inset-0 opacity-70" aria-hidden />
      <section
        aria-live="polite"
        className="panel animate-rise relative flex w-full max-w-md flex-col items-center gap-5 bg-surface/90 p-8 text-center"
      >
        <span className="admin-gate-mark relative flex size-18 items-center justify-center rounded-2xl border border-border-strong bg-background">
          <Logo className="size-11" />
        </span>
        <div>
          <h1 className="font-display text-xl font-semibold tracking-wide">
            {t("admin.gate.title")}
          </h1>
          <p className="mt-2 text-[13px] leading-relaxed text-muted-foreground">
            {phase === "asking" ? t("admin.gate.body") : t("admin.gate.declined")}
          </p>
        </div>
        <ul className="grid w-full gap-2.5 text-left text-xs">
          <li className="flex gap-3">
            <ShieldCheck className="mt-0.5 size-4 shrink-0 text-success" />
            {t("admin.gate.point.confirm")}
          </li>
          <li className="flex gap-3">
            <RotateCcw className="mt-0.5 size-4 shrink-0 text-primary" />
            {t("admin.gate.point.undo")}
          </li>
          <li className="flex gap-3">
            <BadgeInfo className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
            {t("admin.gate.point.publisher")}
          </li>
        </ul>
        {phase === "asking" ? (
          <p className="flex items-center gap-2 text-xs text-muted-foreground">
            <Loader2 className="size-3.5 animate-spin" />
            {t("admin.gate.waiting")}
          </p>
        ) : (
          <div className="flex w-full flex-wrap justify-center gap-2">
            <Button variant="primary" tactile onClick={onSkip}>
              {t("admin.gate.skip")}
            </Button>
            <Button variant="outline" onClick={ask}>
              {t("admin.gate.retry")}
            </Button>
          </div>
        )}
        <p className="text-[11px] text-muted-foreground/80">{t("admin.gate.settings")}</p>
      </section>
    </div>
  );
}
