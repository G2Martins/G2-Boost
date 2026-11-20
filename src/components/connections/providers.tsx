import { Globe, KeyRound, Lock, RefreshCw, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { ExternalLink } from "@/components/safe-markdown";
import { rise } from "@/components/shell/page-header";
import { Badge, Button, Card, CardTitle, selectClass } from "@/components/ui/primitives";
import {
  type CheckState,
  DEFAULT_MODELS,
  PROVIDER_WEB,
  PROVIDERS,
  type Provider,
  validModel,
} from "@/lib/ai";
import { inTauri } from "@/lib/backend";
import { useI18n } from "@/lib/i18n";
import { useLuli } from "@/lib/luli";
import { cn } from "@/lib/utils";

/** Where each provider hands out keys (shown as the domain, opened in the system browser). */
const KEY_URL: Record<Provider, string> = {
  anthropic: "https://console.anthropic.com/settings/keys",
  openai: "https://platform.openai.com/api-keys",
  gemini: "https://aistudio.google.com/apikey",
  nvidia: "https://build.nvidia.com/",
};

type Shown = CheckState | "none" | "unchecked";

const LED: Record<Shown, string> = {
  none: "bg-muted-foreground/40",
  unchecked: "bg-muted-foreground/70",
  testing: "bg-foreground/70 motion-safe:animate-pulse",
  ok: "bg-success shadow-[0_0_8px_hsl(var(--success)/0.7)]",
  unverified: "bg-success/70",
  invalid: "bg-destructive shadow-[0_0_8px_hsl(var(--destructive)/0.6)]",
  quota: "bg-warning",
  offline: "bg-warning/80",
  error: "bg-destructive",
};

const TEXT: Record<Shown, string> = {
  none: "text-muted-foreground",
  unchecked: "text-muted-foreground",
  testing: "text-foreground",
  ok: "text-success",
  unverified: "text-success",
  invalid: "text-destructive",
  quota: "text-warning",
  offline: "text-warning",
  error: "text-destructive",
};

/** Conexões › Provedores: one port per AI provider (key, live status, model). */
export function ProvidersTab() {
  const { t } = useI18n();
  return (
    <>
      <p className="max-w-3xl text-xs leading-relaxed text-muted-foreground">
        {t("conn.prov.intro")}
      </p>
      <div className="grid gap-4 xl:grid-cols-2">
        {PROVIDERS.map((p, i) => (
          <ProviderCard key={p} p={p} i={i} />
        ))}
      </div>
    </>
  );
}

function ProviderCard({ p, i }: { p: Provider; i: number }) {
  const { t, locale } = useI18n();
  const luli = useLuli();
  const has = !!luli.keys[p];
  const check = luli.checks[p];
  const shown: Shown = !has ? "none" : (check?.state ?? "unchecked");
  const active = luli.provider === p;
  const [draft, setDraft] = useState("");
  const [msg, setMsg] = useState<string | null>(null);

  const test = () => {
    setMsg(null);
    void luli.testKey(p);
  };
  const save = async () => {
    const err = await luli.saveKey(p, draft);
    setMsg(err);
    if (!err) {
      setDraft("");
      void luli.testKey(p); // a new key is checked at once
    }
  };

  const when = check?.at
    ? new Date(check.at).toLocaleString(locale, { dateStyle: "short", timeStyle: "short" })
    : null;

  return (
    <Card
      className={cn(
        "animate-rise flex flex-col gap-4",
        active && "shadow-[inset_0_0_0_1px_hsl(var(--primary)/0.45)]",
      )}
      style={rise(i)}
      data-provider={p}
    >
      <CardTitle
        className="mb-0"
        right={
          active ? (
            <Badge tone="accent">{t("conn.prov.active")}</Badge>
          ) : (
            <Button
              size="sm"
              variant="ghost"
              disabled={!has}
              onClick={() => luli.switchProvider(p)}
            >
              {t("conn.prov.use")}
            </Button>
          )
        }
      >
        <span
          className={cn("size-2 shrink-0 rounded-full transition-colors", LED[shown])}
          aria-hidden
        />
        {t(`ai.provider.${p}` as const)}
      </CardTitle>

      {/* status line: what the last check (or question) said, and when */}
      <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1" aria-live="polite">
        <span className={cn("text-[13px] font-semibold", TEXT[shown])}>
          {shown === "error"
            ? t("conn.prov.state.error", { status: String(check?.status ?? "?") })
            : t(`conn.prov.state.${shown}` as const)}
        </span>
        {when && shown !== "none" && shown !== "testing" ? (
          <span className="num text-[11px] text-muted-foreground">
            {t("conn.prov.checked", { when })}
          </span>
        ) : null}
        <span className="ml-auto flex items-center gap-1 text-[11px] text-muted-foreground">
          <Globe className="size-3.5" />
          {PROVIDER_WEB[p] ? t("conn.prov.web.yes") : t("conn.prov.web.no")}
        </span>
      </div>
      {shown !== "none" && shown !== "testing" && shown !== "unchecked" ? (
        <p className="-mt-2 text-[11px] text-muted-foreground">
          {t(`conn.prov.state.${shown}.hint` as const)}
        </p>
      ) : null}

      {/* key */}
      <div className="rounded-md border border-border bg-background/60 p-3">
        <p className="flex items-center gap-2 text-xs">
          {has ? (
            <Lock className="size-3.5 text-success" />
          ) : (
            <KeyRound className="size-3.5 text-muted-foreground" />
          )}
          <span className={has ? "text-success" : "text-muted-foreground"}>
            {has ? t("ai.key.stored") : t("ai.key.none")}
          </span>
          {has ? (
            <span className="ml-auto flex gap-1">
              <Button
                size="sm"
                variant="outline"
                disabled={shown === "testing" || !inTauri}
                onClick={test}
              >
                <RefreshCw
                  className={cn("size-3.5", shown === "testing" && "motion-safe:animate-spin")}
                />
                {t("conn.prov.test")}
              </Button>
              <Button
                size="sm"
                variant="ghost"
                aria-label={t("ai.key.delete")}
                onClick={() => void luli.deleteKey(p)}
              >
                <Trash2 className="size-3.5" />
              </Button>
            </span>
          ) : null}
        </p>
        <form
          className="mt-2 flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (draft.trim()) void save();
          }}
        >
          <input
            type="password"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            placeholder={t(`ai.key.placeholder.${p}` as const)}
            aria-label={t("conn.prov.key_label", { provider: t(`ai.provider.${p}` as const) })}
            autoComplete="off"
            spellCheck={false}
            className="h-8 min-w-0 flex-1 rounded-md border border-border bg-background px-2.5 font-mono text-xs outline-none focus:border-ring"
          />
          <Button
            type="submit"
            size="sm"
            variant={has ? "ghost" : "outline"}
            disabled={!draft.trim() || !inTauri}
          >
            {has ? t("conn.prov.replace") : t("ai.key.save")}
          </Button>
        </form>
        {msg ? <p className="mt-2 text-[11px] text-destructive">{msg}</p> : null}
        <p className="mt-2 text-[11px] leading-relaxed text-muted-foreground">
          {t(`conn.prov.get.${p}` as const)}{" "}
          <ExternalLink url={KEY_URL[p]} label={t("conn.prov.keys_page")} />
        </p>
      </div>

      <ModelPicker p={p} listed={check?.models ?? []} />
    </Card>
  );
}

/** The listed chat models when a test listed them; free text (validated) otherwise or on demand. */
function ModelPicker({ p, listed }: { p: Provider; listed: string[] }) {
  const { t } = useI18n();
  const luli = useLuli();
  const current = luli.models[p];
  const [custom, setCustom] = useState(false);
  const [draft, setDraft] = useState(current);
  useEffect(() => setDraft(current), [current]);
  const valid = validModel(p, draft.trim());
  const id = `model-${p}`;
  const options = listed.includes(current) ? listed : [current, ...listed];
  return (
    <div>
      <label
        className="block text-[10px] font-semibold uppercase tracking-wider text-muted-foreground"
        htmlFor={id}
      >
        {t("ai.model")}
      </label>
      {listed.length > 0 && !custom ? (
        <select
          id={id}
          className={cn(selectClass, "mt-1 w-full font-mono text-xs")}
          value={current}
          onChange={(e) => {
            if (e.target.value === "") setCustom(true);
            else luli.setModel(p, e.target.value);
          }}
        >
          {options.map((m) => (
            <option key={m} value={m}>
              {m === DEFAULT_MODELS[p] ? `${m} · ${t("conn.prov.default")}` : m}
            </option>
          ))}
          <option value="">{t("conn.prov.custom")}</option>
        </select>
      ) : (
        <div className="mt-1 flex gap-2">
          <input
            id={id}
            value={draft}
            onChange={(e) => {
              setDraft(e.target.value);
              const v = e.target.value.trim();
              if (validModel(p, v)) luli.setModel(p, v);
            }}
            spellCheck={false}
            maxLength={80}
            aria-invalid={!valid}
            className={cn(
              "h-9 min-w-0 flex-1 rounded-md border bg-background px-3 font-mono text-xs outline-none focus:border-ring",
              valid ? "border-border" : "border-destructive/60",
            )}
          />
          {listed.length > 0 ? (
            <Button size="sm" variant="ghost" className="h-9" onClick={() => setCustom(false)}>
              {t("conn.prov.list")}
            </Button>
          ) : null}
        </div>
      )}
      <p className={cn("mt-1 text-[11px]", valid ? "text-muted-foreground" : "text-destructive")}>
        {!valid
          ? t("conn.prov.model_invalid")
          : listed.length > 0
            ? t("conn.prov.models_listed", { count: String(listed.length) })
            : t("conn.prov.models_hint")}
      </p>
    </div>
  );
}
