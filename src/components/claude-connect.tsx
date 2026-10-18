import { Check, ChevronDown, Copy, Plug } from "lucide-react";
import { type CSSProperties, useCallback, useEffect, useState } from "react";
import { useErrorText } from "@/components/elevate-button";
import { Badge, Button, Card, CardTitle, Modal } from "@/components/ui/primitives";
import { backend, type ClaudeClient, type ClaudeTarget, inTauri } from "@/lib/backend";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

const CLIENTS: ClaudeClient[] = ["desktop", "code"];

/** One-click registration of `G2 Boost.exe --mcp` with Claude Desktop (Cowork) and Claude Code. */
export function ClaudeConnectCard({ style }: { style?: CSSProperties }) {
  const { t } = useI18n();
  const errorText = useErrorText();
  const [targets, setTargets] = useState<ClaudeTarget[] | null>(null);
  const [exe, setExe] = useState<string | null>(null);
  const [ask, setAsk] = useState<{ client: ClaudeClient; on: boolean } | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ client: ClaudeClient; text: string; ok: boolean } | null>(null);
  const [manual, setManual] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);

  const load = useCallback(() => {
    backend.claudeTargets().then((x) => setTargets(x ?? []));
  }, []);
  useEffect(() => {
    load();
    backend.appInfo().then((i) => setExe(i?.exe ?? null));
  }, [load]);

  const run = async () => {
    if (!ask) return;
    setBusy(true);
    try {
      const next = ask.on
        ? await backend.claudeConnect(ask.client)
        : await backend.claudeDisconnect(ask.client);
      setTargets(next);
      setMsg({
        client: ask.client,
        ok: true,
        text: ask.on ? t(`claude.done.${ask.client}` as const) : t("claude.removed"),
      });
    } catch (e) {
      const m = e instanceof Error ? e.message : String(e);
      setMsg({
        client: ask.client,
        ok: false,
        text:
          m === "unreadable_config"
            ? t("claude.error.unreadable")
            : m === "not_installed"
              ? t("claude.missing")
              : errorText(e),
      });
    } finally {
      setBusy(false);
      setAsk(null);
    }
  };

  const copy = async (k: string, text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(k);
      setTimeout(() => setCopied((c) => (c === k ? null : c)), 1500);
    } catch {
      /* clipboard unavailable: the text stays selectable */
    }
  };

  const path = exe ?? "C:\\…\\G2 Boost.exe";
  const cli = `claude mcp add --scope user g2boost -- "${path}" --mcp`;
  const json = JSON.stringify(
    { mcpServers: { g2boost: { command: path, args: ["--mcp"] } } },
    null,
    2,
  );
  const askPaths = ask ? (targets ?? []).filter((x) => x.client === ask.client && x.path) : [];

  return (
    <Card className="animate-rise" style={style}>
      <CardTitle help={t("claude.hint")}>
        <Plug className="size-4" />
        {t("claude.title")}
      </CardTitle>
      <p className="text-xs text-muted-foreground">{t("claude.body")}</p>

      <ul className="mt-3 space-y-2">
        {CLIENTS.map((c) => {
          const rows = (targets ?? []).filter((x) => x.client === c);
          const installed = rows.some((x) => x.path);
          const connected = installed && rows.every((x) => !x.path || x.connected);
          const here = connected && rows.every((x) => !x.path || x.points_here);
          return (
            <li key={c} className="rounded-md border border-border bg-background/50 p-2.5">
              <div className="flex flex-wrap items-center gap-2">
                <span className="font-display text-[13px] font-semibold tracking-wide">
                  {t(`claude.client.${c}` as const)}
                </span>
                {targets === null ? null : !installed ? (
                  <Badge tone="neutral">{t("claude.state.missing")}</Badge>
                ) : here ? (
                  <Badge tone="success">{t("claude.state.on")}</Badge>
                ) : connected ? (
                  <Badge tone="warning">{t("claude.state.elsewhere")}</Badge>
                ) : (
                  <Badge tone="neutral">{t("claude.state.off")}</Badge>
                )}
                <span className="ml-auto flex gap-1.5">
                  {installed && connected ? (
                    <Button
                      size="sm"
                      variant="ghost"
                      disabled={busy || !inTauri}
                      onClick={() => setAsk({ client: c, on: false })}
                    >
                      {t("claude.remove")}
                    </Button>
                  ) : null}
                  {installed && !here ? (
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={busy || !inTauri}
                      onClick={() => setAsk({ client: c, on: true })}
                    >
                      {connected ? t("claude.repoint") : t("claude.connect")}
                    </Button>
                  ) : null}
                </span>
              </div>
              {msg?.client === c ? (
                <p
                  className={cn("mt-1.5 text-[11px]", msg.ok ? "text-success" : "text-destructive")}
                >
                  {msg.text}
                </p>
              ) : null}
            </li>
          );
        })}
      </ul>
      <p className="mt-2 text-[11px] text-muted-foreground">{t("claude.try")}</p>

      <button
        type="button"
        onClick={() => setManual((v) => !v)}
        className="mt-3 flex items-center gap-1 text-[11px] text-muted-foreground hover:text-foreground"
      >
        <ChevronDown className={cn("size-3.5 transition-transform", manual && "rotate-180")} />
        {t("claude.manual")}
      </button>
      {manual
        ? [
            { k: "cli", label: "Claude Code", text: cli },
            { k: "json", label: "claude_desktop_config.json", text: json },
          ].map((b) => (
            <div key={b.k} className="mt-2">
              <p className="flex items-center text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                {b.label}
                <button
                  type="button"
                  onClick={() => copy(b.k, b.text)}
                  aria-label={t("claude.copy")}
                  className="press ml-auto rounded p-1 hover:bg-foreground/[0.06] hover:text-foreground"
                >
                  {copied === b.k ? (
                    <Check className="size-3.5 text-success" />
                  ) : (
                    <Copy className="size-3.5" />
                  )}
                </button>
              </p>
              <pre className="mt-1 overflow-x-auto rounded-md border border-border bg-background/70 p-2 font-mono text-[10px] text-foreground/90">
                {b.text}
              </pre>
            </div>
          ))
        : null}

      <Modal
        open={ask !== null}
        onClose={() => setAsk(null)}
        closeLabel={t("common.cancel")}
        title={
          ask?.on
            ? t("claude.confirm.connect", { client: t(`claude.client.${ask.client}` as const) })
            : t("claude.confirm.remove", {
                client: ask ? t(`claude.client.${ask.client}` as const) : "",
              })
        }
        footer={
          <>
            <Button variant="ghost" onClick={() => setAsk(null)}>
              {t("common.cancel")}
            </Button>
            <Button variant="primary" disabled={busy} onClick={run}>
              {ask?.on ? t("claude.connect") : t("claude.remove")}
            </Button>
          </>
        }
      >
        <p className="text-sm">{t("claude.confirm.body")}</p>
        <ul className="mt-2 space-y-1">
          {askPaths.map((x) => (
            <li key={x.path} className="break-all font-mono text-[11px] text-muted-foreground">
              {x.path}
            </li>
          ))}
        </ul>
        {ask?.on ? (
          <p className="mt-2 text-xs text-muted-foreground">
            {t(`claude.confirm.after.${ask.client}` as const)}
          </p>
        ) : null}
      </Modal>
    </Card>
  );
}
