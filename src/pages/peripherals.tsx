import { Keyboard, Mouse, MousePointerClick, Undo2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { Fader } from "@/components/periph/fader";
import { inputClass } from "@/components/profiles/shared";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import { Badge, Button, Card, CardTitle, LoadingScope, Switch } from "@/components/ui/primitives";
import {
  backend,
  inTauri,
  type PeriphSetting,
  type PeriphState,
  type TweakState,
} from "@/lib/backend";
import { useBoostStates, useElevated } from "@/lib/boost-hooks";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { navigate } from "@/lib/nav";
import { cn } from "@/lib/utils";

/** `WHEEL_PAGESCROLL`: one screen per notch. */
const WHEEL_PAGE = 4294967295;
/** Shown (disabled) outside the app. */
const SAMPLE: PeriphState = {
  mouse_speed: 10,
  wheel_lines: 3,
  double_click: 500,
  kb_delay: 1,
  kb_rate: 31,
  originals: [],
};
const SHORTCUTS = [
  "alt_shift_off",
  "narrator_shortcut_off",
  "accessibility_shortcuts_off",
] as const;

export function PeripheralsPage() {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const elevated = useElevated();
  const { states, put } = useBoostStates();
  const [st, setSt] = useState<PeriphState | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [rowErr, setRowErr] = useState<Record<string, string>>({});

  const load = useCallback(async () => {
    const s = await backend.periphState();
    if (s) setSt(s);
  }, []);
  useEffect(() => {
    load();
    // Changed in Windows Settings meanwhile? Read again when the window comes back.
    const onBack = () => document.visibilityState === "visible" && load();
    window.addEventListener("focus", onBack);
    document.addEventListener("visibilitychange", onBack);
    return () => {
      window.removeEventListener("focus", onBack);
      document.removeEventListener("visibilitychange", onBack);
    };
  }, [load]);

  const run = async (job: () => Promise<PeriphState>) => {
    setErr(null);
    try {
      setSt(await job());
    } catch (e) {
      setErr(errorText(e));
      await load(); // snap the sliders back to what Windows has
    }
  };
  const set = (s: PeriphSetting, v: number) => run(() => backend.periphSet(s, v));
  const revert = (s: PeriphSetting | null) => run(() => backend.periphRevert(s));

  /** The tweak flow (backup first), through the profiles step so the history records it. */
  const toggle = async (id: string, on: boolean) => {
    setBusy(id);
    setRowErr((m) => ({ ...m, [id]: "" }));
    try {
      const r = await backend.profilesStep({ id, op: on ? "on" : "off", level: null }, "@periph");
      if (r.state) put(r.state);
      if (!r.ok) setRowErr((m) => ({ ...m, [id]: errorText(new Error(r.error ?? "")) }));
      if (id === "keyboard") await load(); // the preset moves both sliders
    } catch (e) {
      setRowErr((m) => ({ ...m, [id]: errorText(e) }));
    } finally {
      setBusy(null);
    }
  };

  const cur = st ?? SAMPLE;
  const off = !st;
  const orig = (s: PeriphSetting) => st?.originals.find((o) => o.setting === s)?.value;
  const undoLabel = (s: PeriphSetting, text: (v: number) => string) => {
    const v = orig(s);
    return v === undefined ? null : t("periph.original", { value: text(v) });
  };
  const pageScroll = cur.wheel_lines >= WHEEL_PAGE;
  const num = (n: number) => String(n);
  const lines = (v: number) =>
    v >= WHEEL_PAGE
      ? t("periph.page_scroll")
      : v === 1
        ? t("periph.unit.line")
        : t("periph.unit.lines", { n: v });
  const ms = (v: number) => t("periph.unit.ms", { n: v });
  const delayMs = (v: number) => ms(250 * (v + 1));
  const perSecond = (v: number) =>
    t("periph.unit.per_second", {
      n: new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(2.5 + (27.5 * v) / 31),
    });
  const needsAdmin = SHORTCUTS.some((id) => states?.[id]?.admin) && !elevated;

  return (
    <LoadingScope loading={inTauri && !st}>
      <PageHeader
        title={t("nav.peripherals")}
        subtitle={t("periph.subtitle")}
        actions={
          st && st.originals.length > 0 ? (
            <Button variant="ghost" size="sm" onClick={() => revert(null)}>
              <Undo2 className="size-4" />
              {t("periph.original_all")}
            </Button>
          ) : null
        }
      />
      <PageBody>
        {err ? <p className="text-xs text-destructive">{err}</p> : null}
        {!inTauri ? (
          <p className="text-xs text-muted-foreground">{t("periph.error.app_only")}</p>
        ) : null}

        <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
          <Card className="animate-rise flex flex-col gap-5">
            <CardTitle
              right={
                <Button variant="outline" size="sm" onClick={() => navigate("mousetest")}>
                  <MousePointerClick className="size-4" />
                  {t("periph.mouse.test")}
                </Button>
              }
            >
              <Mouse className="size-4" />
              {t("periph.mouse.title")}
            </CardTitle>
            <Fader
              label={t("periph.speed")}
              hint={t("periph.speed.hint")}
              value={cur.mouse_speed}
              min={1}
              max={20}
              disabled={off}
              format={num}
              marks={[{ value: 10, label: t("periph.speed.one_to_one") }]}
              ends={[t("periph.speed.slow"), t("periph.speed.fast")]}
              original={undoLabel("mouse_speed", num)}
              onCommit={(v) => set("mouse_speed", v)}
              onRevert={() => revert("mouse_speed")}
            />
            <TweakRow
              id="mouse"
              title={t("periph.accel")}
              desc={t("periph.accel.desc")}
              state={states?.mouse}
              loaded={!!states}
              elevated={elevated}
              busy={busy === "mouse"}
              error={rowErr.mouse}
              onToggle={(on) => toggle("mouse", on)}
            />
            <Fader
              label={t("periph.wheel")}
              hint={pageScroll ? t("periph.wheel.page") : t("periph.wheel.hint")}
              value={pageScroll ? 3 : cur.wheel_lines}
              min={1}
              max={100}
              disabled={off}
              format={(v) => (pageScroll && v === 3 ? t("periph.page_scroll") : lines(v))}
              ends={["1", "100"]}
              original={undoLabel("wheel_lines", lines)}
              onCommit={(v) => set("wheel_lines", v)}
              onRevert={() => revert("wheel_lines")}
            />
            <div className="flex flex-col gap-3">
              <Fader
                label={t("periph.dblclick")}
                hint={t("periph.dblclick.hint")}
                value={cur.double_click}
                min={200}
                max={900}
                step={10}
                disabled={off}
                format={ms}
                ends={[t("periph.dblclick.fast"), t("periph.dblclick.slow")]}
                original={undoLabel("double_click", ms)}
                onCommit={(v) => set("double_click", v)}
                onRevert={() => revert("double_click")}
              />
              <DoubleClickZone />
            </div>
          </Card>

          <Card className="animate-rise flex flex-col gap-5" style={rise(1)}>
            <CardTitle>
              <Keyboard className="size-4" />
              {t("periph.keyboard.title")}
            </CardTitle>
            <Fader
              label={t("periph.kb.delay")}
              hint={t("periph.kb.delay.hint")}
              value={cur.kb_delay}
              min={0}
              max={3}
              disabled={off}
              format={delayMs}
              ends={[t("periph.kb.delay.short"), t("periph.kb.delay.long")]}
              original={undoLabel("kb_delay", delayMs)}
              onCommit={(v) => set("kb_delay", v)}
              onRevert={() => revert("kb_delay")}
            />
            <Fader
              label={t("periph.kb.rate")}
              hint={t("periph.kb.rate.hint")}
              value={cur.kb_rate}
              min={0}
              max={31}
              disabled={off}
              format={perSecond}
              ends={[t("periph.kb.rate.slow"), t("periph.kb.rate.fast")]}
              original={undoLabel("kb_rate", perSecond)}
              onCommit={(v) => set("kb_rate", v)}
              onRevert={() => revert("kb_rate")}
            />
            <div className="flex flex-col gap-1.5">
              <label htmlFor="periph-try" className="text-[13px] font-semibold">
                {t("periph.kb.try")}
              </label>
              <input
                id="periph-try"
                type="text"
                autoComplete="off"
                spellCheck={false}
                placeholder={t("periph.kb.try.placeholder")}
                className={cn(inputClass, "num w-full")}
              />
            </div>
            <TweakRow
              id="keyboard"
              title={t("periph.kb.fast")}
              desc={t("periph.kb.fast.desc")}
              state={states?.keyboard}
              loaded={!!states}
              elevated={elevated}
              busy={busy === "keyboard"}
              error={rowErr.keyboard}
              onToggle={(on) => toggle("keyboard", on)}
            />
          </Card>
        </div>

        <Card className="animate-rise" style={rise(2)}>
          <CardTitle
            help={t("periph.shortcuts.help")}
            right={needsAdmin ? <ElevateButton route="peripherals" /> : null}
          >
            {t("periph.shortcuts.title")}
          </CardTitle>
          <div className="divide-y divide-border">
            {SHORTCUTS.map((id) => (
              <TweakRow
                key={id}
                id={id}
                title={t(`periph.shortcut.${id}` as I18nKey)}
                desc={t(`periph.shortcut.${id}.desc` as I18nKey)}
                state={states?.[id]}
                loaded={!!states}
                elevated={elevated}
                busy={busy === id}
                error={rowErr[id]}
                onToggle={(on) => toggle(id, on)}
                missingNote={t("periph.shortcuts.unavailable")}
              />
            ))}
          </div>
        </Card>
      </PageBody>
    </LoadingScope>
  );
}

/** A tweak as a labelled switch. Missing from this build = disabled with a note (never hidden state). */
function TweakRow({
  id,
  title,
  desc,
  state,
  loaded,
  elevated,
  busy,
  error,
  onToggle,
  missingNote,
}: {
  id: string;
  title: string;
  desc: string;
  state?: TweakState;
  loaded: boolean;
  elevated: boolean;
  busy: boolean;
  error?: string;
  onToggle: (on: boolean) => void;
  missingNote?: string;
}) {
  const { t } = useI18n();
  const present = !!state && state.status !== "unavailable";
  const locked = !present || (state.admin && !elevated);
  return (
    <div className="flex items-start gap-4 py-3 first:pt-0 last:pb-0" data-tweak={id}>
      <div className="min-w-0 flex-1">
        <p className="flex flex-wrap items-center gap-2 text-[13px] font-semibold">
          {title}
          {state?.admin ? <Badge tone="neutral">{t("common.admin_required")}</Badge> : null}
        </p>
        <p className="mt-0.5 text-xs leading-relaxed text-muted-foreground">{desc}</p>
        {loaded && !state && missingNote ? (
          <p className="mt-1 text-[11px] text-muted-foreground">{missingNote}</p>
        ) : null}
        {error ? <p className="mt-1 text-xs text-destructive">{error}</p> : null}
      </div>
      <Switch
        checked={state?.status === "on"}
        disabled={locked || busy}
        onCheckedChange={onToggle}
        aria-label={title}
      />
    </div>
  );
}

/** Double-click here: the webview follows the system speed, so what you see is what games get. */
function DoubleClickZone() {
  const { t } = useI18n();
  const [hits, setHits] = useState(0);
  useEffect(() => {
    if (hits === 0) return;
    const id = window.setTimeout(() => setHits(0), 1500);
    return () => window.clearTimeout(id);
  }, [hits]);
  return (
    <button
      type="button"
      onDoubleClick={() => setHits((n) => n + 1)}
      className={cn(
        "press flex h-12 items-center justify-center rounded-md border border-dashed text-xs font-semibold transition-colors duration-200 ease-out-expo",
        hits > 0
          ? "border-success/60 bg-success/10 text-success"
          : "border-border-strong text-muted-foreground hover:text-foreground",
      )}
    >
      {hits > 0 ? t("periph.dblclick.ok") : t("periph.dblclick.test")}
    </button>
  );
}
