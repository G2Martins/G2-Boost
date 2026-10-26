import {
  Cpu,
  FileCog,
  FolderOpen,
  MonitorCog,
  RotateCcw,
  Trash2,
  TriangleAlert,
  Zap,
} from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { useErrorText } from "@/components/elevate-button";
import { Badge, Button, Modal, Switch } from "@/components/ui/primitives";
import { toast } from "@/components/ui/toast";
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
  high_priority: false,
};

type Key = keyof GameTweaks;
type CpuKey = "perf_cores_only" | "free_core0" | "physical_only";

/** The game's stored options (older files have no `high_priority`). */
export function optionsOf(s: Settings | null, id: string): GameTweaks {
  return { ...NO_TWEAKS, ...s?.game_tweaks[id] };
}

/** Which CPU/GPU options mean something on this PC (the rest are hidden, not just disabled). */
function applicable(info: GameTweaksInfo): { cpu: CpuKey[]; gpu: boolean } {
  const cpu: CpuKey[] = [];
  if (info.cpu.hybrid) cpu.push("perf_cores_only");
  if (info.cpu.physical > 1) cpu.push("free_core0");
  if (info.cpu.smt) cpu.push("physical_only");
  return { cpu, gpu: info.gpus > 1 };
}

function Row({
  label,
  desc,
  on,
  onChange,
  badge,
  disabled,
  note,
}: {
  label: string;
  desc: string;
  on: boolean;
  onChange: (on: boolean) => void;
  badge?: "recommended" | "advanced";
  disabled?: boolean;
  note?: string;
}) {
  const { t } = useI18n();
  return (
    <li className="flex items-start gap-4 py-3">
      <div className="min-w-0 flex-1">
        <p className="flex flex-wrap items-center gap-2 text-sm font-semibold">
          {label}
          {badge ? (
            <Badge tone={badge === "recommended" ? "accent" : "neutral"}>
              {t(`gametweaks.${badge}` as I18nKey)}
            </Badge>
          ) : null}
        </p>
        <p className="mt-0.5 text-xs leading-relaxed text-muted-foreground">{desc}</p>
        {note ? <p className="mt-1 text-[11px] text-warning">{note}</p> : null}
      </div>
      <Switch
        checked={on}
        disabled={disabled}
        onCheckedChange={onChange}
        aria-label={label}
        className="mt-0.5"
      />
    </li>
  );
}

function Section({
  icon,
  title,
  children,
}: {
  icon: ReactNode;
  title: string;
  children: ReactNode;
}) {
  return (
    <section>
      <p className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
        {icon}
        {title}
      </p>
      {children}
    </section>
  );
}

/**
 * "Ajustes" of one game: Game Mode for it and its priority, CPU cores while in Game Mode,
 * dedicated GPU for its exe (saved with Salvar), and which exe it uses (applied at once).
 */
export function GameTweaksDialog({
  game,
  open,
  settings,
  info,
  onClose,
  onSaved,
}: {
  game: GameEntry | null;
  open: boolean;
  settings: Settings | null;
  info: GameTweaksInfo | null;
  onClose: () => void;
  onSaved: (s: Settings) => void;
}) {
  const { t } = useI18n();
  const errorText = useErrorText();
  const current = optionsOf(settings, game?.id ?? "");
  const modeOn = !!game && !!settings?.game_mode_games.includes(game.id);
  const [draft, setDraft] = useState<GameTweaks>(current);
  const [draftMode, setDraftMode] = useState(modeOn);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  // Fresh draft every time the dialog opens (or switches game).
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset only on open, not on every render
  useEffect(() => {
    if (!open) return;
    setDraft(current);
    setDraftMode(modeOn);
    setErr(null);
  }, [open, game?.id]);

  const set = (k: Key, on: boolean) => setDraft((d) => ({ ...d, [k]: on }));
  const opts = info ? applicable(info) : { cpu: [] as CpuKey[], gpu: false };
  // A preference written earlier stays reachable (to turn it off) even if the PC changed.
  opts.gpu ||= current.gpu_high_perf;
  const globalPriority = !!settings?.game_mode.high_priority;
  const tweaksChanged = (Object.keys(NO_TWEAKS) as Key[]).some((k) => draft[k] !== current[k]);
  const chosen = game ? settings?.game_exe_override[game.id] : undefined;

  const fail = (e: unknown) => {
    const m = e instanceof Error ? e.message : String(e);
    setErr(
      ["no_exe", "unknown_game", "outside_folder", "bad_exe"].includes(m)
        ? t(`gametweaks.error.${m}` as I18nKey)
        : m === "app-only"
          ? t("common.app_only")
          : errorText(e),
    );
  };

  /** Runs one action with the dialog busy; errors stay in the dialog. */
  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setErr(null);
    try {
      await action();
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  };

  const save = () =>
    run(async () => {
      if (!game || !settings) return;
      let s = settings;
      if (draftMode !== modeOn) {
        const ids = new Set(s.game_mode_games);
        if (draftMode) ids.add(game.id);
        else ids.delete(game.id);
        s = await backend.settingsSet({ ...s, game_mode_games: [...ids] });
      }
      if (tweaksChanged) s = await backend.gameTweaksSet(game.id, draft);
      onSaved(s);
      onClose();
    });

  const changeExe = () =>
    run(async () => {
      if (!game) return;
      const s = await backend.pickExe(game.id);
      if (!s) return; // cancelled in the Windows dialog
      onSaved(s);
      toast(t("gametweaks.exe.changed", { game: game.name }), { tone: "success" });
    });

  const resetExe = () =>
    run(async () => {
      if (!game) return;
      onSaved(await backend.gameExeReset(game.id));
      toast(t("gametweaks.exe.restored", { game: game.name }), { tone: "success" });
    });

  const remove = () =>
    run(async () => {
      if (!game) return;
      onSaved(await backend.gamesRemove(game.id));
      toast(t("gametweaks.removed", { game: game.name }), { tone: "neutral" });
      onClose();
    });

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
          <Button
            variant="primary"
            disabled={busy || !settings || (!tweaksChanged && draftMode === modeOn)}
            onClick={save}
          >
            {t("gametweaks.save")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <Section icon={<Zap className="size-3.5" />} title={t("games.mode.title")}>
          <ul className="divide-y divide-border">
            <Row
              label={t("gametweaks.mode")}
              desc={t("gametweaks.mode.desc")}
              on={draftMode}
              onChange={setDraftMode}
              disabled={!settings}
            />
            <Row
              label={t("gametweaks.high_priority")}
              desc={t("gametweaks.high_priority.desc")}
              on={draft.high_priority || globalPriority}
              onChange={(on) => set("high_priority", on)}
              disabled={!settings || globalPriority}
              note={globalPriority ? t("gametweaks.high_priority.global") : undefined}
            />
          </ul>
          {draftMode ? null : (
            <p className="mt-1 flex gap-1.5 text-[11px] text-warning">
              <TriangleAlert className="mt-px size-3.5 shrink-0" />
              {t("gametweaks.mode_off.all")}
            </p>
          )}
        </Section>

        {info ? (
          <p className="num text-xs text-muted-foreground">
            {t("gametweaks.cpu", { physical: info.cpu.physical, logical: info.cpu.logical })}
            {info.cpu.hybrid
              ? `, ${t("gametweaks.cpu.hybrid", { perf: info.cpu.perf_logical })}`
              : ""}
          </p>
        ) : null}

        {opts.cpu.length > 0 ? (
          <Section icon={<Cpu className="size-3.5" />} title={t("gametweaks.section.cpu")}>
            <ul className="divide-y divide-border">
              {opts.cpu.map((k) => (
                <Row
                  key={k}
                  label={t(`gametweaks.${k}` as I18nKey)}
                  desc={t(`gametweaks.${k}.desc` as I18nKey)}
                  on={draft[k]}
                  onChange={(on) => set(k, on)}
                  badge="advanced"
                />
              ))}
            </ul>
            <p className="mt-1 text-[11px] leading-relaxed text-muted-foreground">
              {t("gametweaks.combine")}
            </p>
          </Section>
        ) : null}

        {opts.gpu ? (
          <Section icon={<MonitorCog className="size-3.5" />} title={t("gametweaks.section.gpu")}>
            <ul>
              <Row
                label={t("gametweaks.gpu_high_perf")}
                desc={t("gametweaks.gpu_high_perf.desc")}
                on={draft.gpu_high_perf}
                onChange={(on) => set("gpu_high_perf", on)}
                badge="recommended"
                disabled={!game?.main_exe && !current.gpu_high_perf}
                note={game?.main_exe ? undefined : t("gametweaks.gpu.no_exe")}
              />
            </ul>
          </Section>
        ) : null}

        <Section icon={<FileCog className="size-3.5" />} title={t("gametweaks.section.exe")}>
          <div className="mt-2 flex flex-col gap-2.5 rounded-md border border-border bg-background/60 p-3">
            <p className="flex min-w-0 items-center gap-2">
              <span
                className="num min-w-0 flex-1 truncate text-left text-[11px] text-foreground/85"
                title={game?.main_exe ?? undefined}
                dir="rtl"
              >
                {/* rtl keeps the file name visible when the path is cut */}
                <bdi>{game?.main_exe ?? t("gametweaks.exe.none")}</bdi>
              </span>
              {chosen ? <Badge tone="accent">{t("gametweaks.exe.custom")}</Badge> : null}
            </p>
            <div className="flex flex-wrap gap-2">
              <Button size="sm" variant="outline" disabled={busy || !settings} onClick={changeExe}>
                <FolderOpen className="size-3.5" />
                {t("gametweaks.exe.change")}
              </Button>
              {chosen ? (
                <Button size="sm" variant="ghost" disabled={busy} onClick={resetExe}>
                  <RotateCcw className="size-3.5" />
                  {t("gametweaks.exe.reset")}
                </Button>
              ) : null}
              {game?.launcher === "custom" ? (
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={busy}
                  onClick={remove}
                  className="ml-auto hover:text-destructive"
                >
                  <Trash2 className="size-3.5" />
                  {t("gametweaks.remove")}
                </Button>
              ) : null}
            </div>
          </div>
          <p className="mt-1.5 text-[11px] leading-relaxed text-muted-foreground">
            {t("gametweaks.exe.desc")} {t("gametweaks.instant")}
          </p>
        </Section>

        <p className="rounded-md border border-border bg-background/60 px-3 py-2 text-[11px] leading-relaxed text-muted-foreground">
          {t("gametweaks.honest")}
        </p>
        {err ? <p className="text-xs text-destructive">{err}</p> : null}
      </div>
    </Modal>
  );
}
