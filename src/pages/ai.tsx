import {
  Bot,
  ChevronDown,
  KeyRound,
  Lock,
  RefreshCw,
  Send,
  ShieldCheck,
  Sparkles,
  Trash2,
  Wrench,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useErrorText } from "@/components/elevate-button";
import { ExternalLink, SafeMarkdown } from "@/components/safe-markdown";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import { Badge, Button, Card, CardTitle, HelpHint } from "@/components/ui/primitives";
import { cleanupName, TWEAKS, tweakName } from "@/data/catalogs";
import {
  type Answer,
  askStateless,
  ClaudeChat,
  DEFAULT_MODELS,
  isTimeout,
  PROVIDERS,
  type Provider,
  systemPrompt,
  TRUSTED_DOMAINS,
  type Turn,
} from "@/lib/ai";
import { backend, inTauri, type StartupItem } from "@/lib/backend";
import { useI18n } from "@/lib/i18n";
import { navigate } from "@/lib/nav";
import { cn } from "@/lib/utils";

type Message = { role: "user" | "assistant"; text: string; answer?: Answer; error?: string };

const LS_PROVIDER = "g2boost.ai.provider";
const lsModel = (p: Provider) => `g2boost.ai.model.${p}`;
function readLs(k: string): string | null {
  try {
    return localStorage.getItem(k);
  } catch {
    return null;
  }
}
function writeLs(k: string, v: string) {
  try {
    localStorage.setItem(k, v);
  } catch {
    /* storage unavailable */
  }
}

export function AiPage() {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const [provider, setProvider] = useState<Provider>(() => {
    const v = readLs(LS_PROVIDER);
    return v && (PROVIDERS as string[]).includes(v) ? (v as Provider) : "anthropic";
  });
  const [models, setModels] = useState<Record<Provider, string>>(() => ({
    anthropic: readLs(lsModel("anthropic")) || DEFAULT_MODELS.anthropic,
    openai: readLs(lsModel("openai")) || DEFAULT_MODELS.openai,
    gemini: readLs(lsModel("gemini")) || DEFAULT_MODELS.gemini,
  }));
  const [keys, setKeys] = useState<Record<string, boolean>>({});
  const [keyDraft, setKeyDraft] = useState("");
  const [context, setContext] = useState<string | null>(null);
  const [contextBusy, setContextBusy] = useState(false);
  const [showContext, setShowContext] = useState(false);
  const [messages, setMessages] = useState<Message[]>([]);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [streaming, setStreaming] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const claude = useRef<ClaudeChat | null>(null);
  // The request in flight: aborted on unmount and on provider/model/context switch, so a late
  // answer never lands in a transcript it no longer belongs to.
  const inflight = useRef<AbortController | null>(null);
  const cancel = useCallback(() => {
    inflight.current?.abort();
    inflight.current = null;
  }, []);
  useEffect(() => cancel, [cancel]);
  const endRef = useRef<HTMLDivElement | null>(null);

  const loadContext = useCallback(async () => {
    cancel();
    setContextBusy(true);
    try {
      const c = await backend.aiContext();
      setContext(JSON.stringify(c));
    } catch (e) {
      setNote(errorText(e));
    } finally {
      setContextBusy(false);
    }
    claude.current = null;
    setMessages([]);
  }, [errorText, cancel]);

  useEffect(() => {
    backend.aiKeyStatus().then((k) => k && setKeys(k));
    if (inTauri) void loadContext();
  }, [loadContext]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: scroll whenever the transcript grows
  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [messages, streaming]);

  const switchProvider = (p: Provider) => {
    cancel();
    setProvider(p);
    writeLs(LS_PROVIDER, p);
    setKeyDraft("");
    setNote(null);
    claude.current = null;
    setMessages([]);
  };

  const saveKey = async () => {
    setNote(null);
    try {
      await backend.aiKeySet(provider, keyDraft);
      setKeyDraft("");
      setKeys((k) => ({ ...k, [provider]: true }));
      setNote(t("ai.key.saved"));
    } catch (e) {
      setNote(
        e instanceof Error && e.message === "invalid_key" ? t("ai.key.invalid") : errorText(e),
      );
    }
  };

  const deleteKey = async () => {
    await backend.aiKeyDelete(provider).catch(() => undefined);
    setKeys((k) => ({ ...k, [provider]: false }));
  };

  const games = useMemo(() => {
    if (!context) return [] as string[];
    try {
      const c = JSON.parse(context) as { steam_games?: { name?: string }[] };
      return (c.steam_games ?? [])
        .map((g) => g.name ?? "")
        .filter(Boolean)
        .slice(0, 3);
    } catch {
      return [];
    }
  }, [context]);

  const quick = [
    t("ai.quick.analyze"),
    t("ai.quick.upgrade"),
    t("ai.quick.temps"),
    ...games.map((g) => t("ai.quick.fps", { game: g })),
  ];

  const send = async (question: string) => {
    const q = question.trim();
    if (!q || busy || !context) return;
    if (!keys[provider]) {
      setNote(t("ai.key.missing"));
      return;
    }
    setNote(null);
    setDraft("");
    setBusy(true);
    setStreaming("");
    const prior = messages;
    setMessages([...prior, { role: "user", text: q }]);
    const system = systemPrompt(locale, context);
    const ctrl = new AbortController();
    inflight.current = ctrl;
    try {
      let answer: Answer;
      if (provider === "anthropic") {
        if (!claude.current) claude.current = new ClaudeChat(models.anthropic, system);
        answer = await claude.current.ask(q, setStreaming, ctrl.signal);
      } else {
        const turns: Turn[] = [
          ...prior.filter((m) => !m.error).map((m) => ({ role: m.role, text: m.text })),
          { role: "user", text: q },
        ];
        answer = await askStateless(
          provider,
          models[provider],
          system,
          turns,
          setStreaming,
          ctrl.signal,
        );
      }
      if (!ctrl.signal.aborted)
        setMessages((m) => [...m, { role: "assistant", text: answer.text, answer }]);
    } catch (e) {
      if (!ctrl.signal.aborted) {
        const msg = e instanceof Error ? e.message : String(e);
        const text = isTimeout(e)
          ? t("ai.error.timeout")
          : msg === "refusal"
            ? t("ai.error.refusal")
            : msg === "no_key"
              ? t("ai.key.missing")
              : msg === "empty_response"
                ? t("ai.error.empty")
                : t("ai.error.generic", { error: msg });
        setMessages((m) => [...m, { role: "assistant", text: "", error: text }]);
      }
      claude.current = null; // start clean after a failed Claude turn (history stays append-only)
    } finally {
      if (inflight.current === ctrl) inflight.current = null;
      setBusy(false);
      setStreaming("");
    }
  };

  const contextKb = context ? (new TextEncoder().encode(context).length / 1024).toFixed(1) : "—";

  return (
    <>
      <PageHeader title={t("ai.title")} subtitle={t("ai.subtitle")} />
      <PageBody>
        <div className="grid grid-cols-1 gap-4 xl:grid-cols-[320px_1fr]">
          {/* ---------------- session rail ---------------- */}
          <div className="flex flex-col gap-4">
            <Card className="animate-rise">
              <CardTitle help={t("ai.provider.hint")}>
                <Bot className="size-4" />
                {t("ai.provider")}
              </CardTitle>
              <div className="grid grid-cols-3 gap-1.5 rounded-lg border border-border bg-background/60 p-1">
                {PROVIDERS.map((p) => (
                  <button
                    key={p}
                    type="button"
                    onClick={() => switchProvider(p)}
                    className={cn(
                      "rounded-md px-2 py-2 font-display text-[12px] font-semibold tracking-wide transition-colors",
                      provider === p
                        ? "bg-primary/15 text-primary shadow-[inset_0_0_0_1px_hsl(var(--primary)/0.45)]"
                        : "text-muted-foreground hover:text-foreground",
                    )}
                  >
                    {t(`ai.provider.${p}` as const)}
                    <span
                      className={cn(
                        "ml-1.5 inline-block size-1.5 rounded-full align-middle",
                        keys[p] ? "bg-success" : "bg-muted-foreground/40",
                      )}
                      aria-hidden
                    />
                  </button>
                ))}
              </div>
              <label
                className="mt-4 block text-[10px] font-semibold uppercase tracking-wider text-muted-foreground"
                htmlFor="ai-model"
              >
                {t("ai.model")}
              </label>
              <input
                id="ai-model"
                value={models[provider]}
                onChange={(e) => {
                  const v = e.target.value.trim();
                  cancel();
                  claude.current = null; // the next Claude turn uses the new model
                  setModels((m) => ({ ...m, [provider]: v }));
                  writeLs(lsModel(provider), v);
                  claude.current = null;
                }}
                spellCheck={false}
                maxLength={64}
                className="mt-1 h-9 w-full rounded-md border border-border bg-background px-3 font-mono text-xs outline-none focus:border-ring"
              />
              <div className="mt-4 rounded-md border border-border bg-background/60 p-3">
                <p className="flex items-center gap-2 text-xs">
                  {keys[provider] ? (
                    <Lock className="size-3.5 text-success" />
                  ) : (
                    <KeyRound className="size-3.5 text-muted-foreground" />
                  )}
                  <span className={keys[provider] ? "text-success" : "text-muted-foreground"}>
                    {keys[provider] ? t("ai.key.stored") : t("ai.key.none")}
                  </span>
                  <HelpHint text={t("ai.key.hint")} />
                </p>
                <div className="mt-2 flex gap-2">
                  <input
                    type="password"
                    value={keyDraft}
                    onChange={(e) => setKeyDraft(e.target.value)}
                    placeholder={t(`ai.key.placeholder.${provider}` as const)}
                    autoComplete="off"
                    spellCheck={false}
                    className="h-8 min-w-0 flex-1 rounded-md border border-border bg-background px-2.5 font-mono text-xs outline-none focus:border-ring"
                  />
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={!keyDraft.trim() || !inTauri}
                    onClick={saveKey}
                  >
                    {t("ai.key.save")}
                  </Button>
                </div>
                {keys[provider] ? (
                  <button
                    type="button"
                    onClick={deleteKey}
                    className="mt-2 flex items-center gap-1 text-[11px] text-muted-foreground hover:text-destructive"
                  >
                    <Trash2 className="size-3" />
                    {t("ai.key.delete")}
                  </button>
                ) : null}
              </div>
              {note ? <p className="mt-3 text-xs text-foreground/90">{note}</p> : null}
            </Card>

            <Card className="animate-rise" style={rise(1)}>
              <CardTitle
                help={t("ai.context.hint")}
                right={
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={contextBusy || !inTauri}
                    onClick={loadContext}
                    aria-label={t("ai.context.refresh")}
                  >
                    <RefreshCw className={cn("size-4", contextBusy && "animate-spin")} />
                  </Button>
                }
              >
                <ShieldCheck className="size-4" />
                {t("ai.context")}
              </CardTitle>
              <p className="text-xs text-muted-foreground">
                {t("ai.context.summary", {
                  kb: contextKb,
                  provider: t(`ai.provider.${provider}` as const),
                })}
              </p>
              <button
                type="button"
                onClick={() => setShowContext((v) => !v)}
                className="mt-2 flex items-center gap-1 text-[11px] text-muted-foreground hover:text-foreground"
              >
                <ChevronDown
                  className={cn("size-3.5 transition-transform", showContext && "rotate-180")}
                />
                {showContext ? t("ai.context.hide") : t("ai.context.show")}
              </button>
              {showContext && context ? (
                <pre className="mt-2 max-h-72 overflow-auto rounded-md border border-border bg-background/70 p-2 font-mono text-[10px] leading-snug text-muted-foreground">
                  {JSON.stringify(JSON.parse(context), null, 2)}
                </pre>
              ) : null}
              <p className="mt-3 text-[11px] text-muted-foreground">
                {t("ai.sources", { list: TRUSTED_DOMAINS.slice(0, 6).join(", ") })}
              </p>
            </Card>

            <McpCard />
          </div>

          {/* ---------------- console ---------------- */}
          <Card className="animate-rise flex min-h-[560px] flex-col" style={rise(2)}>
            <CardTitle>
              <Sparkles className="size-4 text-primary" />
              {t("ai.console")}
              <span className="ml-2 font-mono text-[11px] font-normal text-muted-foreground">
                {models[provider]}
              </span>
            </CardTitle>

            <div className="flex-1 space-y-4 overflow-y-auto pr-1">
              {messages.length === 0 && !busy ? (
                <div className="flex h-full flex-col items-start justify-center gap-3 py-10">
                  <p className="font-display text-lg font-semibold">{t("ai.empty.title")}</p>
                  <p className="max-w-xl text-sm text-muted-foreground">{t("ai.empty.body")}</p>
                </div>
              ) : null}
              {messages.map((m, i) =>
                m.role === "user" ? (
                  // biome-ignore lint/suspicious/noArrayIndexKey: append-only transcript
                  <div key={i} className="flex justify-end">
                    <p className="max-w-[80%] rounded-lg rounded-br-sm bg-card-hover px-3.5 py-2 text-sm">
                      {m.text}
                    </p>
                  </div>
                ) : (
                  // biome-ignore lint/suspicious/noArrayIndexKey: append-only transcript
                  <div key={i} className="border-l-2 border-primary/60 pl-4">
                    {m.error ? (
                      <p className="text-sm text-destructive">{m.error}</p>
                    ) : (
                      <SafeMarkdown text={m.text} />
                    )}
                    {m.answer ? <AnswerExtras answer={m.answer} /> : null}
                  </div>
                ),
              )}
              {busy ? (
                <div className="border-l-2 border-primary/60 pl-4">
                  {streaming ? (
                    <SafeMarkdown text={streaming} />
                  ) : (
                    <p className="text-sm text-muted-foreground">{t("ai.thinking")}</p>
                  )}
                  <span
                    className="ml-0.5 inline-block h-4 w-1.5 animate-pulse bg-primary align-middle"
                    aria-hidden
                  />
                </div>
              ) : null}
              <div ref={endRef} />
            </div>

            <div className="mt-4 flex flex-wrap gap-1.5">
              {quick.map((q) => (
                <button
                  key={q}
                  type="button"
                  disabled={busy || !context}
                  onClick={() => send(q)}
                  className="rounded-full border border-border px-3 py-1 text-[11px] text-muted-foreground transition-colors hover:border-primary/50 hover:text-foreground disabled:opacity-40"
                >
                  {q}
                </button>
              ))}
            </div>
            <form
              className="mt-3 flex gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                void send(draft);
              }}
            >
              <textarea
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey) {
                    e.preventDefault();
                    void send(draft);
                  }
                }}
                rows={2}
                maxLength={4000}
                placeholder={t("ai.input.placeholder")}
                className="min-w-0 flex-1 resize-none rounded-md border border-border bg-background px-3 py-2 text-sm outline-none focus:border-ring"
              />
              <Button type="submit" variant="primary" disabled={busy || !draft.trim() || !context}>
                <Send className="size-4" />
                {t("ai.send")}
              </Button>
            </form>
            <p className="mt-2 text-[10px] text-muted-foreground">{t("ai.footer")}</p>
          </Card>
        </div>
      </PageBody>
    </>
  );
}

function AnswerExtras({ answer }: { answer: Answer }) {
  const { t } = useI18n();
  const errorText = useErrorText();
  const [done, setDone] = useState<Record<string, string>>({});
  const apply = async (id: string) => {
    try {
      if (id === "visual_fx") await backend.boostVisualFx("balanced");
      else await backend.boostApply(id, true);
      setDone((d) => ({ ...d, [id]: t("ai.action.applied") }));
    } catch (e) {
      setDone((d) => ({ ...d, [id]: errorText(e) }));
    }
  };
  // Startup names from the model are matched against the live list (enabled, not protected);
  // anything else is dropped. Rust validates the item again on click.
  const [startup, setStartup] = useState<StartupItem[]>([]);
  useEffect(() => {
    if (answer.actions.startup.length === 0) return;
    backend.startupList().then((list) => {
      const wanted = new Set(answer.actions.startup);
      setStartup((list ?? []).filter((i) => wanted.has(i.name) && i.enabled && !i.protected));
    });
  }, [answer.actions.startup]);
  const disableStartup = async (it: StartupItem) => {
    const k = `startup:${it.location}:${it.name}`;
    try {
      await backend.startupSet(it.location, it.name, false);
      setDone((d) => ({ ...d, [k]: t("ai.action.startup_done") }));
    } catch (e) {
      setDone((d) => ({ ...d, [k]: errorText(e) }));
    }
  };
  const hasActions =
    answer.actions.tweaks.length > 0 || answer.actions.cleanup.length > 0 || startup.length > 0;
  return (
    <>
      {hasActions ? (
        <div className="mt-3 rounded-md border border-primary/30 bg-primary/5 p-3">
          <p className="mb-2 flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-primary">
            <Wrench className="size-3.5" />
            {t("ai.actions")}
          </p>
          <ul className="space-y-1.5">
            {answer.actions.tweaks.map((id) => {
              const tw = TWEAKS.find((x) => x.id === id);
              return (
                <li key={id} className="flex flex-wrap items-center gap-2 text-sm">
                  <span>{t(tweakName(id))}</span>
                  {tw?.admin ? <Badge tone="neutral">{t("common.admin_required")}</Badge> : null}
                  {done[id] ? (
                    <span className="ml-auto text-xs text-muted-foreground">{done[id]}</span>
                  ) : (
                    <Button
                      size="sm"
                      variant="outline"
                      className="ml-auto"
                      onClick={() => apply(id)}
                    >
                      {t("common.apply")}
                    </Button>
                  )}
                </li>
              );
            })}
            {startup.map((it) => {
              const k = `startup:${it.location}:${it.name}`;
              return (
                <li key={k} className="flex flex-wrap items-center gap-2 text-sm">
                  <span>{t("ai.action.startup", { name: it.name })}</span>
                  {it.needs_admin ? (
                    <Badge tone="neutral">{t("common.admin_required")}</Badge>
                  ) : null}
                  {done[k] ? (
                    <span className="ml-auto text-xs text-muted-foreground">{done[k]}</span>
                  ) : (
                    <Button
                      size="sm"
                      variant="outline"
                      className="ml-auto"
                      onClick={() => disableStartup(it)}
                    >
                      {t("ai.action.startup_off")}
                    </Button>
                  )}
                </li>
              );
            })}
            {answer.actions.cleanup.map((id) => (
              <li key={id} className="flex items-center gap-2 text-sm">
                <span>{t(cleanupName(id))}</span>
                <Button
                  size="sm"
                  variant="outline"
                  className="ml-auto"
                  onClick={() => navigate("cleanup")}
                >
                  {t("ai.action.open_cleanup")}
                </Button>
              </li>
            ))}
          </ul>
          <p className="mt-2 text-[10px] text-muted-foreground">{t("ai.actions.note")}</p>
        </div>
      ) : null}
      {answer.sources.length > 0 ? (
        <div className="mt-3">
          <p className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            {t("ai.sources.title")}
          </p>
          <ul className="space-y-0.5 text-xs">
            {answer.sources.slice(0, 8).map((s) => (
              <li key={s.url}>
                <ExternalLink url={s.url} label={s.title} />
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </>
  );
}

/** How to plug this PC into Claude Desktop / Claude Code: the app binary serves MCP with `--mcp`. */
function McpCard() {
  const { t } = useI18n();
  const [exe, setExe] = useState<string | null>(null);
  useEffect(() => {
    backend.appInfo().then((i) => setExe(i?.exe ?? null));
  }, []);
  const path = exe ?? "C:\\…\\G2 Boost.exe";
  const cli = `claude mcp add g2boost -- "${path}" --mcp`;
  const desktop = JSON.stringify(
    { mcpServers: { g2boost: { command: path, args: ["--mcp"] } } },
    null,
    2,
  );
  return (
    <Card className="animate-rise" style={rise(2)}>
      <CardTitle help={t("ai.mcp.hint")}>
        <Bot className="size-4" />
        {t("ai.mcp.title")}
      </CardTitle>
      <p className="text-xs text-muted-foreground">{t("ai.mcp.body")}</p>
      <p className="mt-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
        Claude Code
      </p>
      <pre className="mt-1 overflow-x-auto rounded-md border border-border bg-background/70 p-2 font-mono text-[10px] text-foreground/90">
        {cli}
      </pre>
      <p className="mt-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
        Claude Desktop · claude_desktop_config.json
      </p>
      <pre className="mt-1 overflow-x-auto rounded-md border border-border bg-background/70 p-2 font-mono text-[10px] text-foreground/90">
        {desktop}
      </pre>
    </Card>
  );
}
