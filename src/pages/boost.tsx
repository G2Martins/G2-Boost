import {
  Cpu,
  Eraser,
  Gauge as GaugeIcon,
  Keyboard,
  MousePointer2,
  Network,
  Plug,
  RefreshCw,
  Server,
  Zap,
} from "lucide-react";
import { useEffect, useState } from "react";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  EmptyState,
  HelpHint,
  Switch,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/primitives";
import { DNS_PROVIDERS, TWEAKS, tweakDesc, tweakName, tweakWarn } from "@/data/catalogs";
import { backend, type MemoryStatus } from "@/lib/backend";
import { useI18n } from "@/lib/i18n";
import { cn, formatBytes } from "@/lib/utils";

const ICONS: Record<string, typeof Zap> = {
  game_mode: Zap,
  hags: GaugeIcon,
  input_lag: MousePointer2,
  keyboard: Keyboard,
  perf_counters: Cpu,
  power_plan: Plug,
  network_latency: Network,
  mouse: MousePointer2,
  background_services: Server,
};

type Preset = "default" | "high" | "custom";

export function BoostPage() {
  const { t } = useI18n();
  return (
    <PageHeaderWithTabs>
      <TabsContent value="tweaks">
        <PageBody className="px-0 pb-0">
          <TweaksTab />
        </PageBody>
      </TabsContent>
      <TabsContent value="ram">
        <PageBody className="px-0 pb-0">
          <RamTab />
        </PageBody>
      </TabsContent>
      <TabsContent value="dns">
        <PageBody className="px-0 pb-0">
          <DnsTab />
        </PageBody>
      </TabsContent>
      <span className="sr-only">{t("boost.title")}</span>
    </PageHeaderWithTabs>
  );
}

function PageHeaderWithTabs({ children }: { children: React.ReactNode }) {
  const { t } = useI18n();
  return (
    <PageHeader
      title={t("boost.title")}
      subtitle={t("boost.subtitle")}
      tabs={
        <Tabs defaultValue="tweaks">
          <TabsList>
            <TabsTrigger value="tweaks">{t("boost.tab.tweaks")}</TabsTrigger>
            <TabsTrigger value="ram">{t("boost.tab.ram")}</TabsTrigger>
            <TabsTrigger value="dns">{t("boost.tab.dns")}</TabsTrigger>
          </TabsList>
          {children}
        </Tabs>
      }
    />
  );
}

function TweaksTab() {
  const { t } = useI18n();
  const [preset, setPreset] = useState<Preset>("default");
  const [on, setOn] = useState<Record<string, boolean>>({});

  const applyPreset = (p: Preset) => {
    setPreset(p);
    if (p === "custom") return;
    setOn(Object.fromEntries(TWEAKS.map((tw) => [tw.id, tw.presets.includes(p)])));
  };

  return (
    <>
      <Card className="animate-rise flex flex-wrap items-center gap-3 py-4">
        <span className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
          {t("boost.presets")}
          <HelpHint text={t("boost.tweaks.help")} />
        </span>
        <div className="ml-auto flex gap-2">
          {(["default", "high", "custom"] as Preset[]).map((p) => (
            <button
              key={p}
              type="button"
              onClick={() => applyPreset(p)}
              className={cn(
                "rounded-md border px-4 py-2.5 font-display text-[13px] font-semibold transition-colors",
                preset === p
                  ? "border-ring bg-card-hover text-foreground"
                  : "border-border bg-background text-muted-foreground hover:text-foreground",
              )}
            >
              {t(`boost.preset.${p}` as const)}
            </button>
          ))}
        </div>
      </Card>
      <div className="grid grid-cols-1 gap-3 md:grid-cols-2 2xl:grid-cols-3">
        {TWEAKS.map((tw, i) => {
          const Icon = ICONS[tw.id] ?? Zap;
          return (
            <Card
              key={tw.id}
              className="panel-hover animate-rise flex flex-col gap-3"
              style={rise(i)}
            >
              <div className="flex items-start justify-between">
                <Icon className="size-6 text-foreground/90" strokeWidth={1.8} />
                <Switch
                  checked={!!on[tw.id]}
                  onCheckedChange={(v) => {
                    setPreset("custom");
                    setOn((s) => ({ ...s, [tw.id]: v }));
                  }}
                  aria-label={t(tweakName(tw.id))}
                />
              </div>
              <div>
                <h3 className="flex flex-wrap items-center gap-2 font-display text-[14px] font-semibold tracking-wide">
                  {t(tweakName(tw.id))}
                  {tw.beta ? (
                    <Badge className="bg-chart-1/20 text-chart-1">{t("common.beta")}</Badge>
                  ) : null}
                </h3>
                <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">
                  {t(tweakDesc(tw.id))}
                </p>
              </div>
              <details className="text-[11px]">
                <summary className="cursor-pointer text-muted-foreground underline decoration-dotted underline-offset-2 hover:text-foreground">
                  {t("common.not_recommended")}
                </summary>
                <p className="mt-1 text-muted-foreground">{t(tweakWarn(tw.id))}</p>
              </details>
              <div className="mt-auto flex items-center gap-2 text-[10px] text-muted-foreground">
                {tw.admin ? (
                  <Badge className="bg-card-hover text-muted-foreground">
                    {t("common.admin_required")}
                  </Badge>
                ) : null}
                <Badge className="bg-card-hover text-muted-foreground">{t("common.soon")}</Badge>
              </div>
            </Card>
          );
        })}
      </div>
    </>
  );
}

function RamTab() {
  const { t, locale } = useI18n();
  const [mem, setMem] = useState<MemoryStatus | null>(null);
  const [auto, setAuto] = useState(true);
  const [when, setWhen] = useState<"game" | "status">("game");

  useEffect(() => {
    let alive = true;
    const tick = () => backend.memoryStatus().then((m) => alive && m && setMem(m));
    tick();
    const id = setInterval(tick, 2000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  const used = mem ? mem.total_bytes - mem.avail_bytes : 0;
  const pct = mem ? Math.round((used / mem.total_bytes) * 100) : 0;

  return (
    <>
      <Card className="animate-rise">
        <CardTitle
          help={t("boost.ram.help")}
          right={
            <Button variant="primary" disabled title={t("common.soon")}>
              <Eraser className="size-4" />
              {t("boost.ram.clean")}
            </Button>
          }
        >
          {t("boost.ram.title")}
          <span className="ml-2 font-sans text-xs font-normal text-muted-foreground">
            {t("boost.ram.never")}
          </span>
        </CardTitle>
        <div className="h-5 w-full overflow-hidden rounded bg-border/60">
          <div
            className={cn(
              "h-full rounded transition-all duration-700",
              pct > 85 ? "bg-destructive" : "bg-chart-1",
            )}
            style={{ width: `${pct}%` }}
          />
        </div>
        <div className="mt-3 flex items-center gap-6 text-xs">
          <span className="flex items-center gap-2">
            <span className="size-3 rounded-sm bg-chart-1" />
            <b>{t("boost.ram.used")}</b>
            <span className="num text-muted-foreground">
              {mem ? formatBytes(used, locale) : "—"}
            </span>
          </span>
          <span className="flex items-center gap-2">
            <span className="size-3 rounded-sm border border-border" />
            <b>{t("boost.ram.free")}</b>
            <span className="num text-muted-foreground">
              {mem ? formatBytes(mem.avail_bytes, locale) : "—"}
            </span>
          </span>
          <span className="num ml-auto text-muted-foreground">
            {mem ? t("boost.ram.total", { total: formatBytes(mem.total_bytes, locale, 0) }) : "—"}
          </span>
        </div>
      </Card>

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <Card className="animate-rise" style={rise(1)}>
          <CardTitle>
            {t("boost.ram.routine")}
            <span className="ml-3 inline-flex items-center gap-2 font-sans text-xs font-medium">
              <Switch
                checked={auto}
                onCheckedChange={setAuto}
                aria-label={t("boost.ram.automate")}
              />
              {t("boost.ram.automate")}
            </span>
          </CardTitle>
          <p className="text-xs text-muted-foreground">{t("boost.ram.when")}</p>
          <div className="mt-3 flex flex-col gap-3 text-[13px]">
            <Radio
              checked={when === "game"}
              onChange={() => setWhen("game")}
              label={t("boost.ram.when.game")}
            />
            <div className="flex items-center justify-between gap-3">
              <Radio
                checked={when === "status"}
                onChange={() => setWhen("status")}
                label={t("boost.ram.when.status")}
              />
              <span className="rounded-md border border-border bg-background px-3 py-2 text-xs text-muted-foreground">
                {t("boost.ram.every", { hours: 12 })}
              </span>
            </div>
          </div>
          <p className="mt-4 rounded-md border border-chart-1/30 bg-chart-1/10 px-3 py-2.5 text-xs text-foreground/90">
            {t("boost.ram.notice")}
          </p>
        </Card>
        <Card className="animate-rise" style={rise(2)}>
          <CardTitle
            help={t("boost.ram.allowlist.help")}
            right={
              <Button variant="outline" size="sm" disabled title={t("common.soon")}>
                + {t("boost.ram.allowlist.add")}
              </Button>
            }
          >
            {t("boost.ram.allowlist")}
          </CardTitle>
          <EmptyState
            icon={<Server className="size-5" />}
            title={t("boost.ram.allowlist.empty")}
            body={t("boost.ram.allowlist.hint")}
          />
        </Card>
      </div>
    </>
  );
}

function Radio({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: () => void;
  label: string;
}) {
  return (
    <label className="flex cursor-pointer items-center gap-3">
      <input type="radio" checked={checked} onChange={onChange} className="peer sr-only" />
      <span className="flex size-4 items-center justify-center rounded-full border border-muted-foreground peer-checked:border-success">
        <span className={cn("size-2 rounded-full", checked ? "bg-success" : "bg-transparent")} />
      </span>
      <span className={cn(checked ? "font-semibold" : "text-muted-foreground")}>{label}</span>
    </label>
  );
}

function DnsTab() {
  const { t } = useI18n();
  return (
    <>
      <Card className="animate-rise flex items-center gap-3 py-4">
        <span className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
          {t("boost.dns.title")}
          <HelpHint text={t("boost.dns.help")} />
        </span>
        <span className="text-xs text-muted-foreground">{t("boost.dns.soon")}</span>
        <div className="ml-auto flex gap-2">
          <Button
            variant="outline"
            disabled
            title={t("common.soon")}
            aria-label={t("boost.dns.rank")}
          >
            <RefreshCw className="size-4" />
          </Button>
          <Button variant="outline" disabled title={t("common.soon")}>
            {t("boost.dns.restore")}
          </Button>
        </div>
      </Card>
      {DNS_PROVIDERS.map((p, i) => (
        <Card
          key={p.id}
          className="panel-hover animate-rise flex items-center gap-4 py-4"
          style={rise(i)}
        >
          <span className="num flex size-9 items-center justify-center rounded-md border border-border font-display font-semibold">
            –
          </span>
          <span className="font-display text-[14px] font-semibold">{p.name}</span>
          <span className="num text-xs text-muted-foreground">
            {p.primary} / {p.secondary}
          </span>
          <span className="ml-auto text-xs text-muted-foreground">{t("boost.dns.avg")}</span>
          <span className="num w-16 text-xs font-semibold">—</span>
          <Button variant="secondary" size="sm" disabled title={t("common.soon")}>
            {t("boost.dns.pick")}
          </Button>
        </Card>
      ))}
    </>
  );
}
