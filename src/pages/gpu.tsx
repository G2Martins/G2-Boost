import { Cpu, FlaskConical, Loader2, MonitorCog, RefreshCw, Sparkles, Undo2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useErrorText } from "@/components/elevate-button";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import { LiveLine } from "@/components/ui/live-line";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  EmptyState,
  Modal,
  Skeleton,
  Switch,
  selectClass,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/primitives";
import {
  type AmdChange,
  type AmdFeature,
  type AmdGpu,
  backend,
  type GameEntry,
  type Gpu,
  type GpuOverview,
  type MonitorSample,
  type NvProfile,
  type NvSetting,
} from "@/lib/backend";
import { boot } from "@/lib/boot";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn, formatBytes } from "@/lib/utils";

const WINDOW = 60;
const FPS_MIN = 20;
const FPS_MAX = 1000;
/** Recommendation chips read as a sentence, not as an uppercase tag. */
const CHIP = "normal-case tracking-normal text-[11px]";

/** Core error codes of this page → text; everything else goes through the shared mapper. */
function useGpuError() {
  const { t } = useI18n();
  const base = useErrorText();
  return useCallback(
    (e: unknown) => {
      const msg = e instanceof Error ? e.message : String(e);
      if (msg === "no_nvidia") return t("gpu.error.no_nvidia");
      if (msg === "unknown_game") return t("gpu.error.unknown_game");
      if (msg === "no_exe" || msg === "bad_exe") return t("gpu.error.no_exe");
      if (msg === "no_amd") return t("gpu.amd.error.no_amd");
      if (msg === "helper_failed") return t("gpu.amd.error");
      return base(e);
    },
    [t, base],
  );
}

export function GpuPage() {
  const { t } = useI18n();
  // undefined = loading; null = outside the app or nothing answered.
  const [info, setInfo] = useState<GpuOverview | null | undefined>(undefined);
  useEffect(() => {
    let alive = true;
    backend.gpuInfo().then((i) => alive && setInfo(i));
    return () => {
      alive = false;
    };
  }, []);
  const gpu = info?.section ? (info.gpus.find((g) => g.vendor === info.section) ?? null) : null;

  return (
    <>
      <PageHeader title={t("gpu.title")} subtitle={t("gpu.subtitle")} />
      <PageBody>
        {info === undefined ? (
          <>
            <Skeleton className="h-52 w-full" />
            <Skeleton className="h-96 w-full" />
          </>
        ) : !gpu ? (
          <Card className="animate-rise">
            <EmptyState
              icon={<MonitorCog className="size-5" />}
              title={t("gpu.none.title")}
              body={t("gpu.none.body")}
            />
          </Card>
        ) : (
          <>
            <Hero gpu={gpu} driver={info?.nvidia ?? null} />
            {info?.section === "nvidia" ? <NvidiaSection /> : null}
            {info?.section === "amd" ? <AmdSection /> : null}
            {info?.section === "intel" ? <IntelSection /> : null}
          </>
        )}
      </PageBody>
    </>
  );
}

/* ---------------- header card: identity + live readouts ---------------- */

function Hero({ gpu, driver }: { gpu: Gpu; driver: GpuOverview["nvidia"] }) {
  const { t, locale } = useI18n();
  const [samples, setSamples] = useState<MonitorSample[]>([]);
  useEffect(() => {
    let alive = true;
    let off: (() => void) | null = null;
    backend.monitorStart();
    backend
      .on<MonitorSample>("monitor:tick", (s) => {
        if (alive) setSamples((prev) => [...prev.slice(-(WINDOW - 1)), s]);
      })
      .then((u) => {
        off = u;
      });
    return () => {
      alive = false;
      off?.();
      backend.monitorStop();
    };
  }, []);
  const last = samples.at(-1) ?? null;
  const meta = [
    driver
      ? t("gpu.driver", { version: driver.version, branch: driver.branch })
      : gpu.driver
        ? t("gpu.driver.plain", { version: gpu.driver })
        : null,
    gpu.vram_bytes > 0 ? t("gpu.vram", { size: formatBytes(gpu.vram_bytes, locale, 0) }) : null,
    gpu.pcie,
  ].filter((m): m is string => !!m);

  return (
    <Card className="animate-rise halo grid grid-cols-1 items-center gap-x-10 gap-y-5 lg:grid-cols-[minmax(0,1fr)_minmax(0,460px)]">
      <div className="min-w-0">
        <p className="text-[10px] font-semibold uppercase tracking-[0.14em] text-muted-foreground">
          {t("gpu.hero.eyebrow")}
        </p>
        <h2 className="mt-1 truncate font-display text-[22px] font-semibold tracking-wide">
          {gpu.name}
        </h2>
        <div className="rule-accent mt-2 w-48" aria-hidden />
        <p className="num mt-3 flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
          {meta.map((m) => (
            <span key={m}>{m}</span>
          ))}
        </p>
        <dl className="mt-6 grid grid-cols-3 gap-x-7 gap-y-3 sm:grid-cols-5">
          <Readout
            label={t("gpu.live.usage")}
            value={last ? Math.round(last.gpu_pct) : null}
            unit="%"
            ready={!!last}
          />
          <Readout
            label={t("gpu.live.temp")}
            value={last?.gpu_temp_c ?? null}
            unit="°C"
            ready={!!last}
          />
          <Readout
            label={t("gpu.live.clock")}
            value={last?.gpu_core_mhz ?? null}
            unit="MHz"
            ready={!!last}
          />
          <Readout
            label={t("gpu.live.power")}
            value={last?.gpu_power_w != null ? Math.round(last.gpu_power_w) : null}
            unit="W"
            ready={!!last}
          />
          <Readout
            label={t("gpu.live.vram")}
            text={last ? formatBytes(last.gpu_mem_bytes, locale) : null}
            ready={!!last}
          />
        </dl>
      </div>
      <LiveLine
        className="h-auto w-full"
        series={[{ values: samples.map((s) => s.gpu_pct), color: "hsl(var(--chart-1))" }]}
        window={WINDOW}
        seq={samples.at(-1)?.seq}
        max={100}
        label={t("gpu.live.chart")}
      />
    </Card>
  );
}

function Readout({
  label,
  value,
  unit,
  text,
  ready,
}: {
  label: string;
  value?: number | null;
  unit?: string;
  text?: string | null;
  ready: boolean;
}) {
  const shown = text ?? (value != null ? String(value) : null);
  return (
    <div className="min-w-16">
      <dt className="text-[11px] text-muted-foreground">{label}</dt>
      <dd className="num mt-0.5 font-display text-xl font-semibold">
        {!ready ? (
          <Skeleton className="mt-1 h-6 w-12" />
        ) : shown === null ? (
          <span className="text-muted-foreground">—</span>
        ) : (
          <>
            {shown}
            {unit && value != null ? (
              <span className="ml-0.5 font-sans text-xs font-normal text-muted-foreground">
                {unit}
              </span>
            ) : null}
          </>
        )}
      </dd>
    </div>
  );
}

/* ---------------- NVIDIA: driver profiles ---------------- */

function NvidiaSection() {
  const { t } = useI18n();
  return (
    <Card className="animate-rise" style={rise(1)}>
      <Tabs defaultValue={boot.startTab === "game" ? "game" : "global"}>
        <TabsList className="-mx-5 -mt-1 mb-1 px-5">
          <TabsTrigger value="global">{t("gpu.tab.global")}</TabsTrigger>
          <TabsTrigger value="game">{t("gpu.tab.game")}</TabsTrigger>
        </TabsList>
        <TabsContent value="global">
          <NvProfilePanel game={null} />
        </TabsContent>
        <TabsContent value="game">
          <GameProfile />
        </TabsContent>
      </Tabs>
    </Card>
  );
}

function GameProfile() {
  const { t } = useI18n();
  const [games, setGames] = useState<GameEntry[] | null>(null);
  const [game, setGame] = useState<string>("");
  useEffect(() => {
    let alive = true;
    backend.gamesList().then((g) => {
      if (!alive) return;
      // A profile needs the game's executable: launcher entries without one are left out.
      setGames((g ?? []).filter((x) => x.main_exe || x.id.startsWith("exe:")));
    });
    return () => {
      alive = false;
    };
  }, []);
  return (
    <div className="pt-4">
      <div className="flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
        <span aria-hidden>{t("gpu.game.pick")}</span>
        {games === null ? (
          <Skeleton className="h-9 w-64" />
        ) : (
          <select
            aria-label={t("gpu.game.pick")}
            value={game}
            onChange={(e) => setGame(e.target.value)}
            disabled={games.length === 0}
            className={cn(selectClass, "min-w-64 text-foreground")}
          >
            <option value="">
              {games.length === 0 ? t("gpu.game.empty") : t("gpu.game.choose")}
            </option>
            {games.map((g) => (
              <option key={g.id} value={g.id}>
                {g.name}
              </option>
            ))}
          </select>
        )}
      </div>
      {game ? <NvProfilePanel key={game} game={game} /> : null}
    </div>
  );
}

function NvProfilePanel({ game }: { game: string | null }) {
  const { t } = useI18n();
  const errText = useGpuError();
  const [view, setView] = useState<NvProfile | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [confirm, setConfirm] = useState(false);

  const load = useCallback(() => {
    setView(null);
    setMsg(null);
    backend
      .gpuNvRead(game)
      .then(setView)
      .catch((e) => setMsg({ ok: false, text: errText(e) }));
  }, [game, errText]);
  useEffect(() => {
    load();
  }, [load]);

  const run = async (key: string, f: () => Promise<NvProfile>, done: string) => {
    setBusy(key);
    setMsg(null);
    try {
      setView(await f());
      setMsg({ ok: true, text: done });
    } catch (e) {
      setMsg({ ok: false, text: errText(e) });
    } finally {
      setBusy(null);
    }
  };

  const label = (s: NvSetting, v: string) =>
    s.id === "fps_limit"
      ? v === "0"
        ? t("gpu.nv.fps_limit.off")
        : t("gpu.nv.fps_limit.value", { n: v })
      : t(`gpu.nv.${s.id}.${v}` as I18nKey);
  const pending = view?.settings.filter((s) => s.recommended !== null && !s.good) ?? [];

  return (
    <div className="pt-4">
      <CardTitle
        help={game === null ? t("gpu.global.help") : t("gpu.game.help")}
        right={
          <div className="flex items-center gap-2">
            <Button
              variant="ghost"
              size="sm"
              onClick={load}
              disabled={busy !== null}
              aria-label={t("gpu.refresh")}
            >
              <RefreshCw className="size-4" />
            </Button>
            <Button
              variant="primary"
              size="sm"
              disabled={!view || busy !== null || pending.length === 0}
              title={view && pending.length === 0 ? t("gpu.recommended.none") : undefined}
              onClick={() => setConfirm(true)}
            >
              <Sparkles className="size-4" />
              {t("gpu.recommended.apply")}
            </Button>
          </div>
        }
      >
        {game === null ? t("gpu.global.title") : t("gpu.game.title")}
      </CardTitle>

      <p className="-mt-2 mb-2 text-xs text-muted-foreground">
        {view === null
          ? " "
          : view.profile
            ? t("gpu.profile", { name: view.profile })
            : t("gpu.game.no_profile")}
      </p>
      {msg ? (
        <p
          role="status"
          className={cn("mb-2 text-xs", msg.ok ? "text-success" : "text-destructive")}
        >
          {msg.text}
        </p>
      ) : null}

      <div className="divide-y divide-border">
        {view === null
          ? [0, 1, 2, 3, 4, 5, 6].map((k) => <Skeleton key={k} className="my-4 h-10 w-full" />)
          : view.settings.map((s, i) => (
              <NvRow
                key={s.id}
                s={s}
                index={i}
                busy={busy}
                label={label}
                onApply={(v) =>
                  run(s.id, () => backend.gpuNvApply(game, s.id, v), t("gpu.applied"))
                }
                onRevert={() => run(s.id, () => backend.gpuNvRevert(game, s.id), t("gpu.reverted"))}
              />
            ))}
      </div>
      <p className="mt-3 text-[11px] text-muted-foreground">{t("gpu.restart_note")}</p>

      <Modal
        open={confirm}
        onClose={() => setConfirm(false)}
        closeLabel={t("common.cancel")}
        title={t("gpu.recommended.title")}
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirm(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                setConfirm(false);
                void run("all", () => backend.gpuNvRecommended(game), t("gpu.recommended.done"));
              }}
            >
              {t("gpu.recommended.apply")}
            </Button>
          </>
        }
      >
        <p className="text-muted-foreground">{t("gpu.recommended.body")}</p>
        <ul className="mt-3 space-y-1.5">
          {pending.map((s) => (
            <li key={s.id} className="flex flex-wrap items-baseline justify-between gap-x-4">
              <span>{t(`gpu.nv.${s.id}` as I18nKey)}</span>
              <span className="num text-xs">
                <span className="text-muted-foreground">
                  {s.value === null ? t("gpu.custom", { raw: s.raw }) : label(s, s.value)}
                </span>
                {" → "}
                <span className="text-primary">{s.recommended ? label(s, s.recommended) : ""}</span>
              </span>
            </li>
          ))}
        </ul>
      </Modal>
    </div>
  );
}

function NvRow({
  s,
  index,
  busy,
  label,
  onApply,
  onRevert,
}: {
  s: NvSetting;
  index: number;
  busy: string | null;
  label: (s: NvSetting, v: string) => string;
  onApply: (v: string) => void;
  onRevert: () => void;
}) {
  const { t } = useI18n();
  const name = t(`gpu.nv.${s.id}` as I18nKey);
  const working = busy === s.id || busy === "all";
  return (
    <div
      className="animate-rise grid grid-cols-1 gap-3 py-3.5 md:grid-cols-[minmax(0,1fr)_220px_240px] md:items-center"
      style={rise(index)}
    >
      <div className="min-w-0">
        <p className="flex items-center gap-2 text-sm font-semibold">
          {name}
          {s.user_set ? (
            <Badge tone="neutral" title={t("gpu.user_set.hint")}>
              {t("gpu.user_set")}
            </Badge>
          ) : null}
          {working ? <Loader2 className="size-3.5 animate-spin text-muted-foreground" /> : null}
        </p>
        <p className="mt-1 max-w-2xl text-xs leading-relaxed text-muted-foreground">
          {t(`gpu.nv.${s.id}.hint` as I18nKey)}
        </p>
      </div>
      {s.id === "fps_limit" ? (
        <FpsInput value={s.value ?? "0"} disabled={busy !== null} label={name} onApply={onApply} />
      ) : (
        <select
          aria-label={name}
          value={s.value ?? ""}
          disabled={busy !== null}
          onChange={(e) => e.target.value && onApply(e.target.value)}
          className={cn(selectClass, "w-full text-foreground")}
        >
          {s.value === null ? (
            <option value="" disabled>
              {t("gpu.custom", { raw: s.raw })}
            </option>
          ) : null}
          {s.options.map((o) => (
            <option key={o} value={o}>
              {label(s, o)}
            </option>
          ))}
        </select>
      )}
      <div className="flex flex-wrap items-center gap-2 md:justify-end">
        {s.recommended === null ? (
          <Badge tone="neutral" className={CHIP}>
            {t("gpu.recommended.depends")}
          </Badge>
        ) : s.good ? (
          <Badge tone="success" className={CHIP}>
            {t("gpu.recommended.ok")}
          </Badge>
        ) : (
          <Badge tone="accent" className={CHIP}>
            {t("gpu.recommended", { value: label(s, s.recommended) })}
          </Badge>
        )}
        {s.backed_up ? (
          <Button size="sm" variant="ghost" disabled={busy !== null} onClick={onRevert}>
            <Undo2 className="size-3.5" />
            {t("gpu.revert")}
          </Button>
        ) : null}
      </div>
    </div>
  );
}

function FpsInput({
  value,
  disabled,
  label,
  onApply,
}: {
  value: string;
  disabled: boolean;
  label: string;
  onApply: (v: string) => void;
}) {
  const { t } = useI18n();
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const n = Number(draft);
  const valid = /^\d+$/.test(draft) && (n === 0 || (n >= FPS_MIN && n <= FPS_MAX));
  const dirty = draft !== value;
  return (
    <div className="flex items-center gap-2">
      <input
        type="number"
        inputMode="numeric"
        min={0}
        max={FPS_MAX}
        value={draft}
        disabled={disabled}
        aria-label={label}
        aria-invalid={!valid}
        title={t("gpu.nv.fps_limit.range", { min: FPS_MIN, max: FPS_MAX })}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && valid && dirty) onApply(String(n));
        }}
        className={cn(selectClass, "num w-24 text-foreground", !valid && "border-destructive")}
      />
      {dirty ? (
        <Button
          size="sm"
          variant="secondary"
          disabled={disabled || !valid}
          onClick={() => onApply(String(n))}
        >
          {t("gpu.apply")}
        </Button>
      ) : (
        <span className="text-xs text-muted-foreground">
          {n === 0 ? t("gpu.nv.fps_limit.off") : t("gpu.nv.fps_limit.unit")}
        </span>
      )}
    </div>
  );
}

/* ---------------- AMD (beta, helper process) ---------------- */

function AmdSection() {
  const { t } = useI18n();
  const errText = useGpuError();
  const [gpus, setGpus] = useState<AmdGpu[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirm, setConfirm] = useState<AmdGpu | null>(null);

  const load = useCallback(() => {
    setGpus(null);
    setError(null);
    backend
      .gpuAmdRead()
      .then(setGpus)
      .catch((e) => setError(errText(e)));
  }, [errText]);
  useEffect(() => {
    load();
  }, [load]);

  const set = async (index: number, changes: AmdChange[]) => {
    setBusy(true);
    setError(null);
    try {
      setGpus(await backend.gpuAmdSet(index, changes));
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
    }
  };
  const pending = (g: AmdGpu) =>
    g.features.filter((f) => f.supported && f.recommended !== null && f.enabled !== f.recommended);
  // Hybrid laptops: the discrete card first.
  const shown = gpus ? [...gpus].sort((a, b) => Number(b.discrete) - Number(a.discrete)) : null;

  return (
    <Card className="animate-rise" style={rise(1)}>
      <CardTitle
        right={
          <Button
            variant="ghost"
            size="sm"
            onClick={load}
            disabled={busy}
            aria-label={t("gpu.refresh")}
          >
            <RefreshCw className="size-4" />
          </Button>
        }
      >
        {t("gpu.amd.title")}
        <Badge tone="warning">{t("gpu.amd.beta")}</Badge>
      </CardTitle>
      <p className="-mt-2 mb-3 flex items-start gap-2 text-xs text-muted-foreground">
        <FlaskConical className="mt-0.5 size-3.5 shrink-0" />
        {t("gpu.amd.beta_note")}
      </p>
      {error ? (
        <div className="flex flex-wrap items-center gap-3 py-2">
          <p role="status" className="text-sm text-destructive">
            {error}
          </p>
          <Button size="sm" variant="outline" onClick={load}>
            {t("gpu.amd.retry")}
          </Button>
        </div>
      ) : null}
      {shown === null && !error ? <Skeleton className="h-64 w-full" /> : null}
      {shown?.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t("gpu.amd.error")}</p>
      ) : null}
      {shown?.map((g) => (
        <section key={g.index} className="mt-2">
          <header className="flex flex-wrap items-center justify-between gap-3 border-b border-border pb-2">
            <span className="font-display text-sm font-semibold tracking-wide">{g.name}</span>
            <Button
              size="sm"
              variant="primary"
              disabled={busy || pending(g).length === 0}
              title={pending(g).length === 0 ? t("gpu.recommended.none") : undefined}
              onClick={() => setConfirm(g)}
            >
              <Sparkles className="size-4" />
              {t("gpu.recommended.apply")}
            </Button>
          </header>
          <div className="divide-y divide-border">
            {g.features.map((f, i) => (
              <AmdRow
                key={f.feature}
                f={f}
                index={i}
                busy={busy}
                onChange={(c) => set(g.index, [c])}
              />
            ))}
          </div>
        </section>
      ))}
      <Modal
        open={confirm !== null}
        onClose={() => setConfirm(null)}
        closeLabel={t("common.cancel")}
        title={t("gpu.recommended.title")}
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirm(null)}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                const g = confirm;
                setConfirm(null);
                if (g)
                  void set(
                    g.index,
                    pending(g).map((f) => ({
                      feature: f.feature,
                      enabled: f.recommended === true,
                    })),
                  );
              }}
            >
              {t("gpu.recommended.apply")}
            </Button>
          </>
        }
      >
        <p className="text-muted-foreground">{t("gpu.amd.recommended.body")}</p>
        <ul className="mt-3 space-y-1.5">
          {(confirm ? pending(confirm) : []).map((f) => (
            <li key={f.feature} className="flex items-baseline justify-between gap-4">
              <span>{t(`gpu.amd.${f.feature}` as I18nKey)}</span>
              <span className="text-xs">
                <span className="text-muted-foreground">
                  {t(f.enabled ? "gpu.amd.on" : "gpu.amd.off")}
                </span>
                {" → "}
                <span className="text-primary">
                  {t(f.recommended ? "gpu.amd.on" : "gpu.amd.off")}
                </span>
              </span>
            </li>
          ))}
        </ul>
      </Modal>
    </Card>
  );
}

function AmdRow({
  f,
  index,
  busy,
  onChange,
}: {
  f: AmdFeature;
  index: number;
  busy: boolean;
  onChange: (c: AmdChange) => void;
}) {
  const { t } = useI18n();
  const name = t(`gpu.amd.${f.feature}` as I18nKey);
  const [draft, setDraft] = useState(f.value != null ? String(f.value) : "");
  useEffect(() => setDraft(f.value != null ? String(f.value) : ""), [f.value]);
  const ranged =
    f.supported && f.range !== null && (f.feature === "frtc" || f.feature === "sharpening");
  const n = Number(draft);
  const valid = !!f.range && /^\d+$/.test(draft) && n >= f.range[0] && n <= f.range[1];
  return (
    <div
      className="animate-rise grid grid-cols-1 gap-3 py-3.5 md:grid-cols-[minmax(0,1fr)_220px_240px] md:items-center"
      style={rise(index)}
    >
      <div className="min-w-0">
        <p className="text-sm font-semibold">{name}</p>
        <p className="mt-1 max-w-2xl text-xs leading-relaxed text-muted-foreground">
          {t(`gpu.amd.${f.feature}.hint` as I18nKey)}
        </p>
      </div>
      <div className="flex items-center gap-3">
        {f.supported ? (
          <Switch
            checked={f.enabled}
            disabled={busy}
            aria-label={name}
            onCheckedChange={(on) => onChange({ feature: f.feature, enabled: on })}
          />
        ) : (
          <span className="text-xs text-muted-foreground">{t("gpu.amd.unsupported")}</span>
        )}
        {ranged && f.range ? (
          <span className="flex items-center gap-1.5">
            <input
              type="number"
              min={f.range[0]}
              max={f.range[1]}
              value={draft}
              disabled={busy}
              aria-label={`${name} · ${t("gpu.amd.value")}`}
              aria-invalid={!valid}
              onChange={(e) => setDraft(e.target.value)}
              className={cn(
                selectClass,
                "num w-20 text-foreground",
                !valid && "border-destructive",
              )}
            />
            {draft !== String(f.value ?? "") ? (
              <Button
                size="sm"
                variant="secondary"
                disabled={busy || !valid}
                onClick={() => onChange({ feature: f.feature, value: n })}
              >
                {t("gpu.apply")}
              </Button>
            ) : null}
          </span>
        ) : null}
      </div>
      <div className="flex items-center gap-2 md:justify-end">
        {!f.supported ? null : f.recommended === null ? (
          <Badge tone="neutral" className={CHIP}>
            {t("gpu.recommended.taste")}
          </Badge>
        ) : f.enabled === f.recommended ? (
          <Badge tone="success" className={CHIP}>
            {t("gpu.recommended.ok")}
          </Badge>
        ) : (
          <Badge tone="accent" className={CHIP}>
            {t("gpu.recommended", { value: t(f.recommended ? "gpu.amd.on" : "gpu.amd.off") })}
          </Badge>
        )}
      </div>
    </div>
  );
}

/* ---------------- Intel: info only for now ---------------- */

function IntelSection() {
  const { t } = useI18n();
  return (
    <Card className="animate-rise" style={rise(1)}>
      <EmptyState
        icon={<Cpu className="size-5" />}
        title={t("gpu.intel.title")}
        body={t("gpu.intel.body")}
        action={<Badge tone="neutral">{t("gpu.intel.soon")}</Badge>}
      />
    </Card>
  );
}
