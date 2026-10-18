import {
  Bot,
  Brain,
  ChevronDown,
  CircleCheck,
  Globe,
  KeyRound,
  ListTodo,
  Lock,
  RefreshCw,
  Send,
  ShieldCheck,
  SlidersHorizontal,
  Trash2,
  Wrench,
} from "lucide-react";
import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { ClaudeConnectCard } from "@/components/claude-connect";
import { useErrorText } from "@/components/elevate-button";
import { LuliMark } from "@/components/luli-card";
import { LuliInstructionsCard, LuliMemoryCard, LuliSidebar } from "@/components/luli-sidebar";
import { LuliThinking, TypeText } from "@/components/luli-typing";
import { ExternalLink } from "@/components/safe-markdown";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import { Badge, Button, Card, CardTitle, HelpHint } from "@/components/ui/primitives";
import { cleanupName, TWEAKS, tweakName } from "@/data/catalogs";
import { type Answer, isTrustedSource, PROVIDERS } from "@/lib/ai";
import { backend, inTauri, type StartupItem } from "@/lib/backend";
import { useI18n } from "@/lib/i18n";
import { type LuliMessage, useLuli } from "@/lib/luli";
import { navigate } from "@/lib/nav";
import { cn } from "@/lib/utils";

export function AiPage() {
  const { t } = useI18n();
  const luli = useLuli();
  const {
    provider,
    models,
    keys,
    context,
    contextBusy,
    messages,
    busy,
    streaming,
    note,
    draft,
    setDraft,
    ask,
    web,
    webAlways,
    webHere,
    setWebHere,
    fetchContext,
  } = luli;
  const [keyDraft, setKeyDraft] = useState("");
  const [showContext, setShowContext] = useState(false);
  // The settings column (provider, context, personalization, memory) can be folded away.
  const [panel, setPanel] = useState(() => {
    try {
      return localStorage.getItem(PANEL_KEY) !== "0";
    } catch {
      return true;
    }
  });
  const togglePanel = () =>
    setPanel((v) => {
      try {
        localStorage.setItem(PANEL_KEY, v ? "0" : "1");
      } catch {
        /* per-viewer convenience only */
      }
      return !v;
    });
  // Without a key the provider card is the only way forward: never hide it then.
  const showPanel = panel || !keys[provider];
  const title = luli.history.conversations.find((c) => c.id === luli.convId)?.title;
  const endRef = useRef<HTMLDivElement | null>(null);
  const listRef = useRef<HTMLDivElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);

  // The answer in progress is a row of the transcript, at the index the final answer takes, so
  // the typewriter keeps its place when the streamed text becomes the saved answer.
  const rows: LuliMessage[] = busy
    ? [...messages, { role: "assistant", text: streaming }]
    : messages;
  // A new transcript (other conversation, new chat) remounts the rows: only answers born while
  // Luli was answering type; reopened ones show at once.
  const listKey = transcriptKey(messages[0]);
  const live = useRef({ key: listKey, index: -1 });
  if (live.current.key !== listKey) live.current = { key: listKey, index: -1 };
  if (busy) live.current.index = messages.length;

  useEffect(() => {
    if (!context) void fetchContext();
  }, [context, fetchContext]);

  // Follow the text while it is written, unless the user scrolled up to read.
  useEffect(() => {
    const list = listRef.current;
    const content = contentRef.current;
    if (!list || !content) return;
    let atBottom = true;
    const onScroll = () => {
      atBottom = list.scrollHeight - list.scrollTop - list.clientHeight < 80;
    };
    const ro = new ResizeObserver(() => {
      if (atBottom) list.scrollTop = list.scrollHeight;
    });
    list.addEventListener("scroll", onScroll, { passive: true });
    ro.observe(content);
    return () => {
      list.removeEventListener("scroll", onScroll);
      ro.disconnect();
    };
  }, []);

  useEffect(() => {
    // An empty chat keeps the page at the top (header and keys visible).
    if (messages.length > 0 || streaming)
      endRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [messages, streaming]);

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
    t("ai.quick.errors"),
    ...games.map((g) => t("ai.quick.fps", { game: g })),
  ];

  const contextKb = context ? (new TextEncoder().encode(context).length / 1024).toFixed(1) : "—";
  const last = messages[messages.length - 1];
  const askWeb =
    luli.webAvailable &&
    !busy &&
    !web &&
    last?.role === "assistant" &&
    last.answer?.actions.needWeb;

  return (
    <>
      <PageHeader title={t("ai.title")} subtitle={t("ai.subtitle")} />
      <PageBody>
        <div
          className={cn(
            "grid grid-cols-1 gap-4 lg:grid-cols-[256px_minmax(0,1fr)]",
            showPanel && "2xl:grid-cols-[256px_minmax(0,1fr)_320px]",
          )}
        >
          {/* ---------------- history ---------------- */}
          <LuliSidebar />

          {/* ---------------- conversation ---------------- */}
          <Card
            className={cn(
              "animate-rise flex min-h-140 flex-col lg:h-[calc(100vh-13rem)] lg:self-start",
              // Pinned only beside the settings column: below 2xl the settings sit under the chat
              // and a pinned chat would cover them while scrolling.
              "2xl:sticky 2xl:top-4",
            )}
            style={rise(2)}
          >
            <CardTitle
              right={
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={togglePanel}
                  aria-pressed={showPanel}
                  disabled={!keys[provider]}
                >
                  <SlidersHorizontal className="size-4" />
                  {showPanel ? t("ai.panel.hide") : t("ai.panel.show")}
                </Button>
              }
            >
              <LuliMark className="size-6" />
              <span className="min-w-0 truncate">{title ?? t("ai.console")}</span>
              <span className="ml-2 font-mono text-[11px] font-normal text-muted-foreground">
                {models[provider]}
              </span>
            </CardTitle>

            <div ref={listRef} className="flex-1 overflow-y-auto pr-1">
              <div ref={contentRef} className="flex min-h-full flex-col space-y-4">
                {messages.length === 0 && !busy ? (
                  <div className="flex flex-1 flex-col items-start justify-center gap-3 py-10">
                    <p className="font-display text-lg font-semibold">{t("ai.empty.title")}</p>
                    <p className="max-w-xl text-sm text-muted-foreground">{t("ai.empty.body")}</p>
                  </div>
                ) : null}
                <Fragment key={listKey}>
                  {rows.map((m, i) =>
                    m.role === "user" ? (
                      // biome-ignore lint/suspicious/noArrayIndexKey: append-only transcript
                      <div key={i} className="flex flex-col items-end gap-1">
                        <p className="max-w-[80%] rounded-lg rounded-br-sm bg-card-hover px-3.5 py-2 text-sm">
                          {m.text}
                        </p>
                        {m.sent ? (
                          <span className="text-[10px] text-muted-foreground">
                            {t("ai.page_data_sent")}
                          </span>
                        ) : null}
                      </div>
                    ) : (
                      // biome-ignore lint/suspicious/noArrayIndexKey: append-only transcript
                      <div key={i} className="border-l-2 border-primary/60 pl-4">
                        {m.error ? (
                          <p className="text-sm text-destructive">{m.error}</p>
                        ) : !m.text && !m.answer ? (
                          <LuliThinking web={web} />
                        ) : (
                          <TypeText
                            text={m.text}
                            live={i === live.current.index}
                            pending={busy && i === rows.length - 1}
                            after={
                              m.answer ? (
                                <AnswerExtras answer={m.answer} latest={i === rows.length - 1} />
                              ) : null
                            }
                          />
                        )}
                      </div>
                    ),
                  )}
                </Fragment>
                {askWeb ? (
                  <div className="animate-rise rounded-md border border-primary/35 bg-primary/5 p-3">
                    <p className="flex items-center gap-2 text-sm">
                      <Globe className="size-4 text-primary" />
                      {t("ai.web.ask")}
                    </p>
                    <div className="mt-2 flex flex-wrap gap-2">
                      <Button size="sm" variant="primary" onClick={() => luli.allowWeb("here")}>
                        {t("ai.web.allow_here")}
                      </Button>
                      <Button size="sm" variant="outline" onClick={() => luli.allowWeb("always")}>
                        {t("ai.web.allow_always")}
                      </Button>
                    </div>
                  </div>
                ) : null}
                <div ref={endRef} />
              </div>
            </div>

            {messages.length === 0 ? (
              <div className="mt-4 flex flex-wrap gap-1.5">
                {quick.map((q) => (
                  <Chip key={q} disabled={busy} onClick={() => ask(q)}>
                    {q}
                  </Chip>
                ))}
              </div>
            ) : null}
            <form
              className="mt-3 flex gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                void ask(draft);
              }}
            >
              <textarea
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey) {
                    e.preventDefault();
                    void ask(draft);
                  }
                }}
                rows={2}
                maxLength={4000}
                placeholder={t("ai.input.placeholder")}
                className="min-w-0 flex-1 resize-none rounded-md border border-border bg-background px-3 py-2 text-sm outline-none focus:border-ring"
              />
              <Button type="submit" variant="primary" disabled={busy || !draft.trim()}>
                <Send className="size-4" />
                {t("ai.send")}
              </Button>
            </form>
            <div className="mt-2 flex flex-wrap items-center gap-3">
              <button
                type="button"
                disabled={webAlways || !luli.webAvailable}
                onClick={() => setWebHere(!webHere)}
                aria-pressed={web}
                className={cn(
                  "press flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] transition-colors",
                  web
                    ? "border-primary/45 bg-primary/10 text-primary"
                    : "border-border text-muted-foreground hover:text-foreground",
                )}
              >
                <Globe className="size-3.5" />
                {!luli.webAvailable
                  ? t("ai.web.unavailable")
                  : webAlways
                    ? t("ai.web.always")
                    : web
                      ? t("ai.web.on")
                      : t("ai.web.off")}
              </button>
              <p className="text-[10px] text-muted-foreground">{t("ai.footer")}</p>
            </div>
          </Card>
          {/* ---------------- settings ---------------- */}
          {showPanel ? (
            <div className="grid content-start gap-4 lg:col-start-2 lg:grid-cols-2 2xl:col-start-auto 2xl:grid-cols-1">
              <Card className="animate-rise">
                <CardTitle help={t("ai.provider.hint")}>
                  <Bot className="size-4" />
                  {t("ai.provider")}
                </CardTitle>
                <div className="grid grid-cols-2 gap-1.5 rounded-lg border border-border bg-background/60 p-1">
                  {PROVIDERS.map((p) => (
                    <button
                      key={p}
                      type="button"
                      onClick={() => {
                        setKeyDraft("");
                        luli.switchProvider(p);
                      }}
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
                {provider === "gemini" || provider === "nvidia" ? (
                  <p className="mt-2 text-[11px] leading-relaxed text-muted-foreground">
                    {t(`ai.free.${provider}` as const)}{" "}
                    <ExternalLink
                      url={
                        provider === "gemini"
                          ? "https://aistudio.google.com/apikey"
                          : "https://build.nvidia.com/"
                      }
                      label={provider === "gemini" ? "aistudio.google.com" : "build.nvidia.com"}
                    />
                  </p>
                ) : null}
                <label
                  className="mt-4 block text-[10px] font-semibold uppercase tracking-wider text-muted-foreground"
                  htmlFor="ai-model"
                >
                  {t("ai.model")}
                </label>
                <input
                  id="ai-model"
                  value={models[provider]}
                  onChange={(e) => luli.setModel(provider, e.target.value.trim())}
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
                      onClick={async () => {
                        if (await luli.saveKey(keyDraft)) setKeyDraft("");
                      }}
                    >
                      {t("ai.key.save")}
                    </Button>
                  </div>
                  {keys[provider] ? (
                    <button
                      type="button"
                      onClick={luli.deleteKey}
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
                      onClick={luli.reloadContext}
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
                <p className="mt-3 text-[11px] text-muted-foreground">{t("ai.knowledge")}</p>
              </Card>

              <LuliInstructionsCard />
              <LuliMemoryCard />
              <ClaudeConnectCard />
            </div>
          ) : null}
        </div>
      </PageBody>
    </>
  );
}

const PANEL_KEY = "g2boost.luli.settings";

/** Stable id per transcript, from its first message object (appending keeps it). */
const transcriptIds = new WeakMap<object, number>();
let nextTranscript = 0;
function transcriptKey(first: LuliMessage | undefined): string {
  if (!first) return "empty";
  let id = transcriptIds.get(first);
  if (id === undefined) {
    id = ++nextTranscript;
    transcriptIds.set(first, id);
  }
  return `t${id}`;
}

function Chip({
  children,
  disabled,
  onClick,
}: {
  children: React.ReactNode;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className="press rounded-full border border-border px-3 py-1 text-[11px] text-muted-foreground transition-colors hover:border-primary/50 hover:text-foreground disabled:opacity-40"
    >
      {children}
    </button>
  );
}

function AnswerExtras({ answer, latest }: { answer: Answer; latest: boolean }) {
  const { t } = useI18n();
  const errorText = useErrorText();
  const luli = useLuli();
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
  const [checked, setChecked] = useState<Set<number>>(new Set());
  const hasActions =
    answer.actions.tweaks.length > 0 || answer.actions.cleanup.length > 0 || startup.length > 0;
  // Benchmark/vendor sources first.
  const sources = [...answer.sources].sort(
    (a, b) => Number(isTrustedSource(b)) - Number(isTrustedSource(a)),
  );
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
      {answer.actions.remember.length > 0 ? (
        <div className="mt-3 rounded-md border border-border bg-background/50 p-3">
          <p className="mb-2 flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            <Brain className="size-3.5" />
            {t("ai.remember.title")}
          </p>
          <ul className="space-y-1.5">
            {answer.actions.remember.map((r) => {
              const saved = luli.memory.some((m) => m.text.toLowerCase() === r.toLowerCase());
              return (
                <li key={r} className="flex items-center gap-2 text-sm">
                  <span className="min-w-0 flex-1">{r}</span>
                  {saved ? (
                    <span className="text-xs text-success">{t("ai.remember.saved")}</span>
                  ) : (
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={!inTauri}
                      onClick={() => luli.remember(r).catch(() => undefined)}
                    >
                      {t("ai.remember.save")}
                    </Button>
                  )}
                </li>
              );
            })}
          </ul>
          <p className="mt-2 text-[10px] text-muted-foreground">{t("ai.remember.note")}</p>
        </div>
      ) : null}
      {answer.model ? (
        <p className="mt-2 text-[11px] text-muted-foreground">
          {t("ai.fallback_model", { model: answer.model })}
        </p>
      ) : null}
      {answer.actions.todo.length > 0 ? (
        <div className="mt-3 rounded-md border border-border bg-background/50 p-3">
          <p className="mb-2 flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            <ListTodo className="size-3.5" />
            {t("ai.todo")}
          </p>
          <ul className="space-y-1">
            {answer.actions.todo.map((item, i) => (
              <li key={item}>
                <button
                  type="button"
                  onClick={() =>
                    setChecked((s) => {
                      const n = new Set(s);
                      if (n.has(i)) n.delete(i);
                      else n.add(i);
                      return n;
                    })
                  }
                  aria-pressed={checked.has(i)}
                  className="flex w-full items-start gap-2 text-left text-sm"
                >
                  <CircleCheck
                    className={cn(
                      "mt-0.5 size-4 shrink-0 transition-colors",
                      checked.has(i) ? "text-success" : "text-muted-foreground/50",
                    )}
                  />
                  <span className={cn(checked.has(i) && "text-muted-foreground line-through")}>
                    {item}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
      {sources.length > 0 ? (
        <div className="mt-3">
          <p className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            {t("ai.sources.title")}
          </p>
          <ul className="space-y-0.5 text-xs">
            {sources.slice(0, 8).map((s) => (
              <li key={s.url} className="flex items-center gap-2">
                <ExternalLink url={s.url} label={s.title} />
                {isTrustedSource(s) ? (
                  <Badge tone="neutral">{t("ai.sources.trusted")}</Badge>
                ) : null}
              </li>
            ))}
          </ul>
        </div>
      ) : null}
      {latest && answer.actions.followups.length > 0 ? (
        <div className="mt-3 flex flex-wrap gap-1.5">
          {answer.actions.followups.map((q) => (
            <Chip key={q} disabled={luli.busy} onClick={() => luli.ask(q)}>
              {q}
            </Chip>
          ))}
        </div>
      ) : null}
    </>
  );
}
