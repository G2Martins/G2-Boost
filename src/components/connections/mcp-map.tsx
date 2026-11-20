import { CloudDownload, Network, RefreshCw, ShieldAlert } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { rise } from "@/components/shell/page-header";
import { ExeIcon } from "@/components/ui/exe-icon";
import { Badge, Button, Card, CardTitle, EmptyState, Skeleton } from "@/components/ui/primitives";
import { backend, inTauri, type McpClientMap, type McpServer } from "@/lib/backend";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/**
 * Conexões › MCP: what the AI apps on this PC start, drawn as a bus per app with its servers wired
 * below. Read-only: G2 Boost never starts or edits these servers.
 */
export function McpMapCard({ style }: { style?: React.CSSProperties }) {
  const { t } = useI18n();
  const [maps, setMaps] = useState<McpClientMap[] | null>(null);
  const [busy, setBusy] = useState(false);
  const load = useCallback(async () => {
    setBusy(true);
    setMaps((await backend.mcpMap()) ?? []);
    setBusy(false);
  }, []);
  useEffect(() => {
    void load();
  }, [load]);

  const found = (maps ?? []).filter((m) => m.config || m.servers.length > 0);
  const missing = (maps ?? []).filter((m) => !m.config && m.servers.length === 0);
  const total = found.reduce((n, m) => n + m.servers.length, 0);
  const downloads = found.reduce((n, m) => n + m.servers.filter((s) => s.downloads).length, 0);

  return (
    <Card className="animate-rise" style={style}>
      <CardTitle
        help={t("conn.mcp.hint")}
        right={
          <Button
            size="sm"
            variant="ghost"
            disabled={busy || !inTauri}
            onClick={() => void load()}
            aria-label={t("conn.mcp.refresh")}
          >
            <RefreshCw className={cn("size-4", busy && "motion-safe:animate-spin")} />
          </Button>
        }
      >
        <Network className="size-4" />
        {t("conn.mcp.title")}
      </CardTitle>
      <p className="max-w-3xl text-xs leading-relaxed text-muted-foreground">
        {t("conn.mcp.body")}
      </p>

      {maps === null ? (
        <div className="mt-4 space-y-2">
          <Skeleton className="h-10" />
          <Skeleton className="h-10" />
        </div>
      ) : found.length === 0 ? (
        <EmptyState
          icon={<Network className="size-5" />}
          title={t("conn.mcp.empty.title")}
          body={inTauri ? t("conn.mcp.empty.body") : t("conn.app_only")}
        />
      ) : (
        <>
          <p className="mt-4 text-[11px] text-muted-foreground">
            {t("conn.mcp.summary", { count: String(total), apps: String(found.length) })}
            {downloads > 0 ? (
              <span className="text-warning">
                {" "}
                · {t("conn.mcp.summary_downloads", { count: String(downloads) })}
              </span>
            ) : null}
          </p>
          <div className="mt-3 grid items-start gap-3 xl:grid-cols-2">
            {found.map((m, i) => (
              <ClientBus key={m.client} m={m} i={i} />
            ))}
          </div>
        </>
      )}

      {missing.length > 0 && maps !== null && inTauri ? (
        <p className="mt-3 text-[11px] text-muted-foreground">
          {t("conn.mcp.searched", {
            list: missing.map((m) => t(`conn.mcp.client.${m.client}` as const)).join(", "),
          })}
        </p>
      ) : null}
      <p className="mt-4 flex items-start gap-2 rounded-md border border-border bg-background/50 p-3 text-[11px] leading-relaxed text-muted-foreground">
        <ShieldAlert className="mt-0.5 size-3.5 shrink-0 text-warning" />
        {t("conn.mcp.safety")}
      </p>
    </Card>
  );
}

function ClientBus({ m, i }: { m: McpClientMap; i: number }) {
  const { t } = useI18n();
  const name = t(`conn.mcp.client.${m.client}` as const);
  // The app's own servers first, then what its projects declare.
  const servers = [...m.servers].sort(
    (a, b) => Number(a.scope === "project") - Number(b.scope === "project"),
  );
  return (
    <section
      className="animate-rise rounded-md border border-border bg-background/40"
      style={rise(i)}
    >
      <header className="flex items-center gap-3 px-3 pt-3 pb-2">
        <ExeIcon path={m.exe} name={name} className="size-6" />
        <div className="min-w-0 flex-1">
          <p className="font-display text-[13px] font-semibold tracking-wide">{name}</p>
          <p className="text-[11px] text-muted-foreground">
            {servers.length === 0
              ? t("conn.mcp.none")
              : t("conn.mcp.count", { count: String(servers.length) })}
          </p>
        </div>
        {m.error ? <Badge tone="warning">{t(`conn.mcp.error.${m.error}` as const)}</Badge> : null}
      </header>
      {servers.length > 0 ? (
        // The bus: one hairline down from the app, a short tick to each server it starts.
        <ul className="relative mr-3 mb-3 ml-6 border-l border-border-strong">
          {servers.map((s) => (
            <ServerRow key={`${s.scope}:${s.name}`} s={s} />
          ))}
        </ul>
      ) : null}
    </section>
  );
}

function ServerRow({ s }: { s: McpServer }) {
  const { t } = useI18n();
  const target = s.command ?? s.host;
  return (
    <li className="relative flex flex-wrap items-center gap-x-2 gap-y-1 py-1.5 pl-4 before:absolute before:top-[17px] before:left-0 before:h-px before:w-3 before:bg-border-strong">
      <span className={cn("font-mono text-[12px] font-semibold", s.g2boost && "text-primary")}>
        {s.name}
      </span>
      {s.transport && s.transport !== "unknown" ? (
        <Badge tone="neutral">{s.transport}</Badge>
      ) : null}
      {target ? (
        <span className="min-w-0 truncate font-mono text-[11px] text-muted-foreground">
          {target}
        </span>
      ) : null}
      {s.g2boost ? <Badge tone="accent">{t("conn.mcp.flag.g2boost")}</Badge> : null}
      {s.downloads ? (
        <Badge tone="warning" title={t("conn.mcp.flag.downloads.hint")}>
          <CloudDownload className="mr-1 size-3" />
          {t("conn.mcp.flag.downloads")}
        </Badge>
      ) : null}
      {s.missing ? <Badge tone="danger">{t("conn.mcp.flag.missing")}</Badge> : null}
      {s.scope === "project" ? (
        <span className="text-[11px] text-muted-foreground">
          {t("conn.mcp.projects", { count: String(s.projects) })}
        </span>
      ) : null}
    </li>
  );
}
