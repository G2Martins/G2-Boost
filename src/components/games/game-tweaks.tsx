import { Cpu, MonitorCog, TriangleAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { useErrorText } from "@/components/elevate-button";
import { Badge, Button, Modal, Switch } from "@/components/ui/primitives";
import {
  backend,
  type GameEntry,
  type GameTweaks,
  type GameTweaksInfo,
  type Settings,
} from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";

export const NO_TWEAKS: GameTweaks = {
  free_core0: false,
  physical_only: false,
  perf_cores_only: false,
  gpu_high_perf: false,
};

type Key = keyof GameTweaks;

/** Which options mean something on this PC (the rest are hidden, not just disabled). */
function applicable(info: GameTweaksInfo): { cpu: Key[]; gpu: boolean } {
  const cpu: Key[] = [];
  if (info.cpu.hybrid) cpu.push("perf_cores_only");
  if (info.cpu.physical > 1) cpu.push("free_core0");
  if (info.cpu.smt) cpu.push("physical_only");
  return { cpu, gpu: info.gpus > 1 };
}

function Row({
  k,
  on,
  onChange,
  badge,
  disabled,
  note,
}: {
  k: Key;
  on: boolean;
  onChange: (on: boolean) => void;
  badge: "recommended" | "advanced";
  disabled?: boolean;
  note?: string;
}) {
  const { t } = useI18n();
  return (
    <li className="flex items-start gap-4 py-3">
      <div className="min-w-0 flex-1">
        <p className="flex flex-wrap items-center gap-2 text-sm font-semibold">
          {t(`gametweaks.${k}` as I18nKey)}
          <Badge tone={badge === "recommended" ? "accent" : "neutral"}>
            {t(`gametweaks.${badge}` as I18nKey)}
          </Badge>
        </p>
        <p className="mt-0.5 text-xs leading-relaxed text-muted-foreground">
          {t(`gametweaks.${k}.desc` as I18nKey)}
        </p>
        {note ? <p className="mt-1 text-[11px] text-warning">{note}</p> : null}
      </div>
      <Switch
        checked={on}
        disabled={disabled}
        onCheckedChange={onChange}
        aria-label={t(`gametweaks.${k}` as I18nKey)}
        className="mt-0.5"
      />
    </li>
  );
}

/** "Ajustes do jogo": CPU cores while in Game Mode + dedicated GPU for its exe. */
export function GameTweaksDialog({
  game,
  open,
  current,
  info,
  onClose,
  onSaved,
}: {
  game: GameEntry | null;
  open: boolean;
  current: GameTweaks;
  info: GameTweaksInfo | null;
  onClose: () => void;
  onSaved: (s: Settings) => void;
}) {
  const { t } = useI18n();
  const errorText = useErrorText();
  const [draft, setDraft] = useState<GameTweaks>(current);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  // Fresh draft every time the dialog opens.
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset only on open, not on every render
  useEffect(() => {
    if (!open) return;
    setDraft(current);
    setErr(null);
  }, [open, game?.id]);

  const set = (k: Key, on: boolean) => setDraft((d) => ({ ...d, [k]: on }));
  const opts = info ? applicable(info) : { cpu: [], gpu: false };
  // A preference written earlier stays reachable (to turn it off) even if the PC changed.
  opts.gpu ||= current.gpu_high_perf;
  const changed = (Object.keys(NO_TWEAKS) as Key[]).some((k) => draft[k] !== current[k]);

  const save = async () => {
    if (!game) return;
    setBusy(true);
    setErr(null);
    try {
      onSaved(await backend.gameTweaksSet(game.id, draft));
      onClose();
    } catch (e) {
      const m = e instanceof Error ? e.message : String(e);
      setErr(
        m === "no_exe" || m === "unknown_game"
          ? t(`gametweaks.error.${m}` as I18nKey)
          : errorText(e),
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={open && game !== null}
      onClose={() => !busy && onClose()}
      title={t("gametweaks.title", { game: game?.name ?? "" })}
      closeLabel={t("gametweaks.cancel")}
      footer={
        <>
          <Button variant="ghost" disabled={busy} onClick={onClose}>
            {t("gametweaks.cancel")}
          </Button>
          <Button variant="primary" disabled={busy || !changed} onClick={save}>
            {t("gametweaks.save")}
          </Button>
        </>
      }
    >
      {info ? (
        <div className="flex flex-col gap-4">
          <p className="num text-xs text-muted-foreground">
            {t("gametweaks.cpu", { physical: info.cpu.physical, logical: info.cpu.logical })}
            {info.cpu.hybrid
              ? `, ${t("gametweaks.cpu.hybrid", { perf: info.cpu.perf_logical })}`
              : ""}
          </p>

          {opts.cpu.length > 0 ? (
            <section>
              <p className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                <Cpu className="size-3.5" />
                {t("gametweaks.section.cpu")}
              </p>
              <ul className="divide-y divide-border">
                {opts.cpu.map((k) => (
                  <Row key={k} k={k} on={draft[k]} onChange={(on) => set(k, on)} badge="advanced" />
                ))}
              </ul>
              {game && !game.enabled ? (
                <p className="mt-1 flex gap-1.5 text-[11px] text-warning">
                  <TriangleAlert className="mt-px size-3.5 shrink-0" />
                  {t("gametweaks.mode_off")}
                </p>
              ) : null}
              <p className="mt-1 text-[11px] leading-relaxed text-muted-foreground">
                {t("gametweaks.combine")}
              </p>
            </section>
          ) : null}

          {opts.gpu ? (
            <section>
              <p className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                <MonitorCog className="size-3.5" />
                {t("gametweaks.section.gpu")}
              </p>
              <ul>
                <Row
                  k="gpu_high_perf"
                  on={draft.gpu_high_perf}
                  onChange={(on) => set("gpu_high_perf", on)}
                  badge="recommended"
                  disabled={!game?.main_exe && !current.gpu_high_perf}
                  note={game?.main_exe ? undefined : t("gametweaks.gpu.no_exe")}
                />
              </ul>
            </section>
          ) : null}

          {opts.cpu.length === 0 && !opts.gpu ? (
            <p className="text-sm text-muted-foreground">{t("gametweaks.none")}</p>
          ) : (
            <p className="rounded-md border border-border bg-background/60 px-3 py-2 text-[11px] leading-relaxed text-muted-foreground">
              {t("gametweaks.honest")}
            </p>
          )}
          {err ? <p className="text-xs text-destructive">{err}</p> : null}
        </div>
      ) : (
        <p className="text-sm text-muted-foreground">{t("gametweaks.none")}</p>
      )}
    </Modal>
  );
}
