import { openUrl } from "@tauri-apps/plugin-opener";
import { CircleCheck, Download, ExternalLink, Loader2, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Badge, Button, Card, CardTitle, Switch } from "@/components/ui/primitives";
import { backend, inTauri, type UpdateInfo } from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { formatBytes } from "@/lib/utils";

const ERRORS: Partial<Record<string, I18nKey>> = {
  offline: "update.error.offline",
  hash_mismatch: "update.error.hash",
  not_installed: "update.error.not_installed",
  up_to_date: "update.up_to_date",
};

/** Settings › G2 Boost updates: what GitHub has, the notes, one button to install. */
export function UpdateCard({
  auto,
  onAuto,
}: {
  auto: boolean | undefined;
  onAuto: (on: boolean) => void;
}) {
  const { t, locale } = useI18n();
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [phase, setPhase] = useState<"idle" | "checking" | "installing">("idle");
  const [error, setError] = useState<string | null>(null);

  const explain = useCallback(
    (e: unknown) => {
      const msg = e instanceof Error ? e.message : String(e);
      const k = ERRORS[msg];
      return k ? t(k) : t("update.error.generic", { error: msg });
    },
    [t],
  );

  const check = useCallback(async () => {
    setPhase("checking");
    setError(null);
    try {
      setInfo(await backend.updateCheck());
    } catch (e) {
      setError(explain(e));
    } finally {
      setPhase("idle");
    }
  }, [explain]);

  useEffect(() => {
    if (inTauri) void check();
  }, [check]);

  const install = async () => {
    setPhase("installing");
    setError(null);
    try {
      await backend.updateInstall(); // the app closes when the installer starts
    } catch (e) {
      setError(explain(e));
      setPhase("idle");
    }
  };

  return (
    <Card className="animate-rise lg:col-span-2">
      <CardTitle
        help={t("update.hint")}
        right={
          <Switch
            checked={!!auto}
            disabled={auto === undefined}
            onCheckedChange={onAuto}
            aria-label={t("update.auto")}
          />
        }
      >
        <RefreshCw className="size-4" />
        {t("update.title")}
      </CardTitle>
      <p className="text-xs text-muted-foreground">{t("update.auto.body")}</p>

      <div className="mt-4 flex flex-wrap items-center gap-3">
        <span className="text-[13px]">
          {t("update.current", { version: info?.current ?? "—" })}
        </span>
        {info ? (
          info.newer ? (
            <Badge tone="accent">{t("update.available", { version: info.latest })}</Badge>
          ) : (
            <span className="flex items-center gap-1.5 text-xs text-success">
              <CircleCheck className="size-3.5" />
              {t("update.up_to_date")}
            </span>
          )
        ) : null}
        <Button
          size="sm"
          variant="ghost"
          className="ml-auto"
          disabled={!inTauri || phase !== "idle"}
          onClick={check}
        >
          {phase === "checking" ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : (
            <RefreshCw className="size-3.5" />
          )}
          {t("update.check")}
        </Button>
      </div>

      {info?.newer ? (
        <div className="mt-4 grid gap-3">
          {info.notes ? (
            <div className="max-h-48 overflow-y-auto whitespace-pre-wrap rounded-md border border-border bg-background/50 px-3 py-2.5 text-xs leading-relaxed text-muted-foreground">
              {info.notes}
            </div>
          ) : null}
          <div className="flex flex-wrap items-center gap-2">
            {info.kind === "installed" ? (
              <Button variant="primary" tactile disabled={phase !== "idle"} onClick={install}>
                {phase === "installing" ? (
                  <Loader2 className="size-4 animate-spin" />
                ) : (
                  <Download className="size-4" />
                )}
                {phase === "installing"
                  ? t("update.installing")
                  : t("update.install", {
                      size: info.installer_size ? formatBytes(info.installer_size, locale) : "",
                    })}
              </Button>
            ) : null}
            <Button variant="outline" onClick={() => void openUrl(info.url)}>
              <ExternalLink className="size-4" />
              {t("update.page")}
            </Button>
          </div>
          <p className="text-[11px] text-muted-foreground">
            {info.kind === "installed"
              ? t("update.install.note")
              : t(`update.kind.${info.kind}` as I18nKey)}
          </p>
        </div>
      ) : null}
      {error ? <p className="mt-3 text-xs text-destructive">{error}</p> : null}
    </Card>
  );
}
