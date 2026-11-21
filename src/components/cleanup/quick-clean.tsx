import { BrushCleaning, Check, Loader2, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { CountUp } from "@/components/ui/count-up";
import { Button } from "@/components/ui/primitives";
import { backend, type CleanupProgress } from "@/lib/backend";
import { useI18n } from "@/lib/i18n";
import { formatBytes } from "@/lib/utils";

type State = "idle" | "measuring" | "confirm" | "running" | "done";

/**
 * "Clean only this": a few catalog categories measured, confirmed and run in place (the same
 * preview, one-shot token and history as the cleanup page). Used by the review bucket and the
 * disk map's badges.
 */
export function QuickClean({
  ids,
  label,
  route = "cleanup",
  unmeasurable = false,
  onDone,
}: {
  ids: string[];
  label?: string;
  /** Where "Elevate" reopens the app when a category needs admin. */
  route?: string;
  /** Nothing to measure (the component task): skip straight to the confirmation. */
  unmeasurable?: boolean;
  onDone?: (freed: number) => void;
}) {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const [state, setState] = useState<State>("idle");
  const [bytes, setBytes] = useState(0);
  const [live, setLive] = useState(0);
  const [msg, setMsg] = useState<string | null>(null);
  const [admin, setAdmin] = useState(false);

  useEffect(() => {
    if (state !== "running") return;
    const off = backend.on<CleanupProgress>("smart:progress", (p) => setLive(p.bytes));
    return () => void off.then((u) => u());
  }, [state]);

  const measure = async () => {
    setMsg(null);
    setState("measuring");
    try {
      const p = await backend.smartPreview(ids);
      const found =
        p.catalog.reduce((s, i) => s + i.bytes, 0) + p.system.reduce((s, i) => s + i.bytes, 0);
      const needsAdmin =
        p.catalog.some((i) => i.needs_admin) || p.system.some((i) => i.needs_admin);
      const nothing = found === 0 && !unmeasurable && p.catalog.every((i) => i.measurable);
      setAdmin(needsAdmin);
      setBytes(found);
      if (needsAdmin) {
        setState("idle");
      } else if (nothing) {
        setMsg(t("cleanup.preview.missing"));
        setState("idle");
      } else {
        setState("confirm");
      }
    } catch (e) {
      setMsg(errorText(e));
      setState("idle");
    }
  };

  const go = async () => {
    setLive(0);
    setState("running");
    try {
      const tok = await backend.smartConfirm(ids);
      const r = await backend.smartExecute(tok.token, ids);
      const items = r.records.flatMap((x) => x.items);
      const freed = items.reduce((s, i) => s + i.bytes_freed, 0);
      const failed = items.find((i) => i.error);
      setBytes(freed);
      setMsg(failed?.error ? errorText(new Error(failed.error)) : null);
      setState("done");
      onDone?.(freed);
    } catch (e) {
      setMsg(errorText(e));
      setState("idle");
    }
  };

  return (
    <span className="inline-flex flex-wrap items-center gap-2 text-xs">
      {state === "idle" ? (
        admin ? (
          <>
            <span className="text-warning">{t("cleanup.needs_admin")}</span>
            <ElevateButton route={route} />
          </>
        ) : (
          <Button
            variant="outline"
            size="sm"
            onClick={unmeasurable ? () => setState("confirm") : measure}
          >
            <BrushCleaning className="size-3.5" />
            {label ?? t("clean.quick.go")}
          </Button>
        )
      ) : null}
      {state === "measuring" ? (
        <Button variant="outline" size="sm" disabled>
          <Loader2 className="size-3.5 motion-safe:animate-spin" />
          {t("cleanup.preview.running")}
        </Button>
      ) : null}
      {state === "confirm" ? (
        <>
          <span className="font-semibold">
            {unmeasurable
              ? t("clean.quick.confirm_plain")
              : t("clean.quick.confirm", { size: formatBytes(bytes, locale) })}
          </span>
          <Button variant="primary" size="sm" onClick={go}>
            <Trash2 className="size-3.5" />
            {t("cleanup.confirm.go")}
          </Button>
          <Button variant="ghost" size="sm" onClick={() => setState("idle")}>
            {t("common.cancel")}
          </Button>
        </>
      ) : null}
      {state === "running" ? (
        <span className="inline-flex items-center gap-2" role="status">
          <Loader2 className="size-3.5 text-primary motion-safe:animate-spin" />
          <CountUp
            value={live}
            format={(n) => formatBytes(n, locale)}
            className="num font-semibold"
          />
        </span>
      ) : null}
      {state === "done" ? (
        <span className="inline-flex items-center gap-1.5 text-success" role="status">
          <Check className="size-3.5" />
          {t("cleanup.result.freed", { size: formatBytes(bytes, locale) })}
        </span>
      ) : null}
      {msg ? <span className="text-muted-foreground">{msg}</span> : null}
    </span>
  );
}
