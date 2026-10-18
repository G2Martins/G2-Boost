/**
 * Luli's session, shared by every screen: provider/model/keys, the context pack, the transcript
 * and the in-flight request live here (mounted once in App), so a question asked from the disk
 * or network page keeps going while the user switches screens and the chat survives navigation.
 *
 * Pages publish a text snapshot of what they show (`usePageData`) and the Luli card on that page
 * attaches it to the question as `<page_data>`. Paths under a user profile are redacted first.
 */
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import { useErrorText } from "@/components/elevate-button";
import {
  type Answer,
  askStateless,
  ClaudeChat,
  DEFAULT_MODELS,
  isBusy,
  isTimeout,
  normalizeAnswer,
  PROVIDER_WEB,
  PROVIDERS,
  type Provider,
  systemPrompt,
  type Turn,
} from "@/lib/ai";
import {
  type AdvisorReport,
  backend,
  inTauri,
  type LuliHistory,
  type LuliMemoryItem,
} from "@/lib/backend";
import { useI18n } from "@/lib/i18n";
import { KNOWLEDGE } from "@/lib/knowledge";
import { redactPaths } from "@/lib/utils";

export type LuliMessage = {
  role: "user" | "assistant";
  /** What the transcript shows. */
  text: string;
  /** What went to the model (the question plus `<page_data>`), when different. */
  sent?: string;
  answer?: Answer;
  error?: string;
};

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

/* ---------------- page snapshots ---------------- */

const pageData = new Map<string, string>();

/**
 * A page publishes what it shows (null = nothing yet). Removed when the page unmounts, unless
 * `sticky` (a finished measurement, like a speed test, stays valid after leaving its tab).
 */
export function usePageData(key: string, value: string | null, sticky = false) {
  useEffect(() => {
    if (value) pageData.set(key, value);
    else if (!sticky) pageData.delete(key);
    return () => {
      if (!sticky) pageData.delete(key);
    };
  }, [key, value, sticky]);
}

const GB = 1024 ** 3;
/** Sizes for the model: plain, locale-free numbers. */
export function sizeText(bytes: number): string {
  return bytes >= GB ? `${(bytes / GB).toFixed(1)} GB` : `${(bytes / 1024 ** 2).toFixed(0)} MB`;
}

const MAX_PAGE_DATA = 12_000;

/** The question plus the published snapshots it refers to, as the model receives it. */
export function withPageData(question: string, keys: string[]): string | undefined {
  const parts = keys.map((k) => pageData.get(k)).filter((v): v is string => !!v);
  if (parts.length === 0) return undefined;
  const data = redactPaths(parts.join("\n\n")).slice(0, MAX_PAGE_DATA).replace(/</g, "\\u003c");
  return `${question}\n\n<page_data>\n${data}\n</page_data>`;
}

/* ---------------- saved history ---------------- */

const newId = (prefix: string) =>
  `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;

/** A transcript read back from disk is hostile input: keep only well-formed turns, re-validated. */
function sanitizeMessages(raw: unknown[]): LuliMessage[] {
  const out: LuliMessage[] = [];
  for (const m of raw) {
    if (!m || typeof m !== "object") continue;
    const x = m as Record<string, unknown>;
    if ((x.role !== "user" && x.role !== "assistant") || typeof x.text !== "string") continue;
    out.push({
      role: x.role,
      text: x.text.slice(0, 20000),
      sent: typeof x.sent === "string" ? x.sent.slice(0, 20000) : undefined,
      answer: x.role === "assistant" ? normalizeAnswer(x.answer) : undefined,
      error: typeof x.error === "string" ? x.error.slice(0, 500) : undefined,
    });
  }
  return out;
}

/* ---------------- session ---------------- */

function useLuliState() {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const [provider, setProvider] = useState<Provider>(() => {
    const v = readLs(LS_PROVIDER);
    return v && (PROVIDERS as string[]).includes(v) ? (v as Provider) : "anthropic";
  });
  const [models, setModels] = useState<Record<Provider, string>>(() => ({
    anthropic: readLs(lsModel("anthropic")) || DEFAULT_MODELS.anthropic,
    openai: readLs(lsModel("openai")) || DEFAULT_MODELS.openai,
    // gemini-2.5-* are closed to new API users: an old saved default falls back to the new one.
    gemini:
      (readLs(lsModel("gemini")) ?? "").replace(/^gemini-2\.5-pro$/, "") || DEFAULT_MODELS.gemini,
    nvidia: readLs(lsModel("nvidia")) || DEFAULT_MODELS.nvidia,
  }));
  const [keys, setKeys] = useState<Record<string, boolean>>({});
  const [context, setContext] = useState<string | null>(null);
  const [contextBusy, setContextBusy] = useState(false);
  const [messages, setMessages] = useState<LuliMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [streaming, setStreaming] = useState("");
  const [note, setNote] = useState<string | null>(null);
  /** Settings › "always allow"; `webHere` = allowed in this conversation only. */
  const [webAlways, setWebAlways] = useState(false);
  const [webHere, setWebHere] = useState(false);
  const [report, setReport] = useState<AdvisorReport | null>(null);
  /** Saved conversations (sidebar), what Luli remembers, the user's own instructions. */
  const [history, setHistory] = useState<LuliHistory>({ groups: [], conversations: [] });
  const [memory, setMemory] = useState<LuliMemoryItem[]>([]);
  const [instructions, setInstructions] = useState("");
  const [convId, setConvId] = useState<string | null>(null);
  const conv = useRef<{ id: string | null; group: string | null }>({ id: null, group: null });
  const reportBusy = useRef(false);
  const claude = useRef<ClaudeChat | null>(null);
  // The request in flight: aborted on provider/model switch and on a new conversation, so a late
  // answer never lands in a transcript it no longer belongs to.
  const inflight = useRef<AbortController | null>(null);

  const cancel = useCallback(() => {
    inflight.current?.abort();
    inflight.current = null;
  }, []);

  const refreshHistory = useCallback(async () => {
    const h = await backend.luliHistory();
    if (h) setHistory(h);
  }, []);

  useEffect(() => {
    backend.aiKeyStatus().then((k) => k && setKeys(k));
    backend.settingsGet().then((s) => {
      if (!s) return;
      setWebAlways(!!s.ai_web_always);
      setInstructions(s.luli_instructions ?? "");
    });
    backend.luliMemory().then((m) => m && setMemory(m));
    void refreshHistory();
  }, [refreshHistory]);

  const newConversation = useCallback(() => {
    cancel();
    claude.current = null;
    conv.current = { id: null, group: null };
    setConvId(null);
    setMessages([]);
    setWebHere(false);
    setNote(null);
  }, [cancel]);

  /** Save the transcript under the current conversation (a new one gets an id and a title). */
  const persist = useCallback(
    async (transcript: LuliMessage[]) => {
      const first = transcript.find((m) => m.role === "user");
      if (!first || !inTauri) return;
      const id = conv.current.id ?? newId("c");
      conv.current.id = id;
      setConvId(id);
      try {
        await backend.luliSave({
          id,
          title: first.text.slice(0, 80),
          group: conv.current.group,
          messages: transcript,
        });
        await refreshHistory();
      } catch {
        /* history is a convenience: a failed save never breaks the chat */
      }
    },
    [refreshHistory],
  );

  /** Reopen a saved conversation; the next question continues it. */
  const openConversation = useCallback(
    async (id: string) => {
      const c = await backend.luliGet(id);
      if (!c) return;
      cancel();
      claude.current = null; // rebuilt from the transcript on the next question
      conv.current = { id: c.id, group: c.group };
      setConvId(c.id);
      setMessages(sanitizeMessages(c.messages));
      setWebHere(false);
      setNote(null);
    },
    [cancel],
  );

  const deleteConversation = useCallback(
    async (id: string) => {
      await backend.luliDelete(id).catch(() => undefined);
      if (conv.current.id === id) newConversation();
      await refreshHistory();
    },
    [newConversation, refreshHistory],
  );

  const renameConversation = useCallback(
    async (id: string, title: string) => {
      await backend.luliRename(id, title).catch(() => undefined);
      await refreshHistory();
    },
    [refreshHistory],
  );

  const moveConversation = useCallback(
    async (id: string, group: string | null) => {
      await backend.luliMove(id, group).catch(() => undefined);
      if (conv.current.id === id) conv.current.group = group;
      await refreshHistory();
    },
    [refreshHistory],
  );

  const createGroup = useCallback(
    async (name: string) => {
      const id = newId("g");
      await backend.luliGroupPut(id, name).catch(() => undefined);
      await refreshHistory();
      return id;
    },
    [refreshHistory],
  );

  const renameGroup = useCallback(
    async (id: string, name: string) => {
      await backend.luliGroupPut(id, name).catch(() => undefined);
      await refreshHistory();
    },
    [refreshHistory],
  );

  const deleteGroup = useCallback(
    async (id: string) => {
      await backend.luliGroupDelete(id).catch(() => undefined);
      if (conv.current.group === id) conv.current.group = null;
      await refreshHistory();
    },
    [refreshHistory],
  );

  const clearHistory = useCallback(async () => {
    await backend.luliClear().catch(() => undefined);
    newConversation();
    await refreshHistory();
  }, [newConversation, refreshHistory]);

  const remember = useCallback(async (text: string) => {
    const m = await backend.luliRemember(text);
    setMemory(m);
  }, []);

  const forget = useCallback(async (id: string) => {
    const m = await backend.luliForget(id).catch(() => null);
    if (m) setMemory(m);
  }, []);

  const saveInstructions = useCallback(async (text: string) => {
    const s = await backend.settingsGet();
    if (!s) return;
    const saved = await backend.settingsSet({ ...s, luli_instructions: text });
    setInstructions(saved.luli_instructions);
  }, []);

  const fetchContext = useCallback(async (): Promise<string | null> => {
    if (!inTauri) return null;
    setContextBusy(true);
    try {
      const c = await backend.aiContext();
      const s = c ? JSON.stringify(c) : null;
      setContext(s);
      return s;
    } catch (e) {
      setNote(errorText(e));
      return null;
    } finally {
      setContextBusy(false);
    }
  }, [errorText]);

  /** Fresh context = fresh conversation (the old answers were about the old numbers). */
  const reloadContext = useCallback(async () => {
    newConversation();
    await fetchContext();
  }, [newConversation, fetchContext]);

  const loadReport = useCallback(
    async (force = false) => {
      if (reportBusy.current || (!force && report)) return;
      reportBusy.current = true;
      try {
        const r = await backend.advisorReport();
        if (r) setReport(r);
      } finally {
        reportBusy.current = false;
      }
    },
    [report],
  );

  const switchProvider = useCallback(
    (p: Provider) => {
      newConversation();
      setProvider(p);
      writeLs(LS_PROVIDER, p);
    },
    [newConversation],
  );

  const setModel = useCallback(
    (p: Provider, v: string) => {
      cancel();
      claude.current = null; // the next Claude turn uses the new model
      setModels((m) => ({ ...m, [p]: v }));
      writeLs(lsModel(p), v);
    },
    [cancel],
  );

  const saveKey = useCallback(
    async (value: string) => {
      setNote(null);
      try {
        await backend.aiKeySet(provider, value);
        setKeys((k) => ({ ...k, [provider]: true }));
        setNote(t("ai.key.saved"));
        return true;
      } catch (e) {
        setNote(
          e instanceof Error && e.message === "invalid_key" ? t("ai.key.invalid") : errorText(e),
        );
        return false;
      }
    },
    [provider, t, errorText],
  );

  const deleteKey = useCallback(async () => {
    await backend.aiKeyDelete(provider).catch(() => undefined);
    setKeys((k) => ({ ...k, [provider]: false }));
  }, [provider]);

  const ask = useCallback(
    async (
      question: string,
      opts: { sent?: string; web?: boolean; prior?: LuliMessage[] } = {},
    ) => {
      const q = question.trim().slice(0, 4000);
      if (!q || inflight.current) return;
      if (!keys[provider]) {
        setDraft(q);
        setNote(t("ai.key.missing"));
        return;
      }
      const ctx = context ?? (await fetchContext());
      if (!ctx) return;
      const noWeb = !PROVIDER_WEB[provider];
      const web = !noWeb && (opts.web ?? (webAlways || webHere));
      const sent = opts.sent ?? q;
      const prior = opts.prior ?? messages;
      setNote(null);
      setDraft("");
      setBusy(true);
      setStreaming("");
      const userMsg: LuliMessage = { role: "user", text: q, sent: sent === q ? undefined : sent };
      setMessages([...prior, userMsg]);
      const system = systemPrompt(locale, ctx, {
        knowledge: KNOWLEDGE,
        web,
        noWeb,
        memory: memory.map((m) => m.text),
        instructions,
      });
      const ctrl = new AbortController();
      inflight.current = ctrl;
      try {
        let answer: Answer;
        if (provider === "anthropic") {
          if (!claude.current) {
            claude.current = new ClaudeChat(models.anthropic);
            // A reopened (or failed and restarted) conversation continues from its transcript.
            claude.current.seed(
              prior.filter((m) => !m.error).map((m) => ({ role: m.role, text: m.sent ?? m.text })),
            );
          }
          answer = await claude.current.ask(sent, system, web, setStreaming, ctrl.signal);
        } else {
          const turns: Turn[] = [
            ...prior.filter((m) => !m.error).map((m) => ({ role: m.role, text: m.sent ?? m.text })),
            { role: "user", text: sent },
          ];
          answer = await askStateless(
            provider,
            models[provider],
            system,
            turns,
            web,
            setStreaming,
            ctrl.signal,
          );
        }
        if (!ctrl.signal.aborted) {
          const reply: LuliMessage = { role: "assistant", text: answer.text, answer };
          setMessages((m) => [...m, reply]);
          void persist([...prior.filter((m) => !m.error), userMsg, reply]);
        }
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
                  : msg === "busy" || isBusy(e)
                    ? t("ai.error.busy")
                    : t("ai.error.generic", { error: msg });
          setMessages((m) => [...m, { role: "assistant", text: "", error: text }]);
        }
        claude.current = null; // start clean after a failed Claude turn (history stays append-only)
      } finally {
        if (inflight.current === ctrl) inflight.current = null;
        setBusy(false);
        setStreaming("");
      }
    },
    [
      keys,
      provider,
      context,
      fetchContext,
      webAlways,
      webHere,
      messages,
      locale,
      models,
      t,
      memory,
      instructions,
      persist,
    ],
  );

  /**
   * One question outside the conversation (the Console G2 asks this way): same provider, key,
   * context and knowledge, web per the global setting, the answer streams through `onText`.
   */
  const askOnce = useCallback(
    async (
      question: string,
      opts: {
        sent?: string;
        console?: string[];
        onText?: (s: string) => void;
        signal?: AbortSignal;
      } = {},
    ): Promise<Answer> => {
      if (!keys[provider]) throw new Error("no_key");
      const ctx = context ?? (await fetchContext());
      if (!ctx) throw new Error("no_context");
      const sent = opts.sent ?? question.trim().slice(0, 4000);
      const noWeb = !PROVIDER_WEB[provider];
      const web = !noWeb && webAlways;
      const system = systemPrompt(locale, ctx, {
        knowledge: KNOWLEDGE,
        web,
        noWeb,
        console: opts.console,
        memory: memory.map((m) => m.text),
        instructions,
      });
      const onText = opts.onText ?? (() => undefined);
      return provider === "anthropic"
        ? new ClaudeChat(models.anthropic).ask(sent, system, web, onText, opts.signal)
        : askStateless(
            provider,
            models[provider],
            system,
            [{ role: "user", text: sent }],
            web,
            onText,
            opts.signal,
          );
    },
    [keys, provider, context, fetchContext, webAlways, locale, models, memory, instructions],
  );

  /** The last answer asked for the web: allow it (here or always) and ask the same question again. */
  const allowWeb = useCallback(
    async (scope: "here" | "always") => {
      if (scope === "always") {
        setWebAlways(true);
        const s = await backend.settingsGet();
        if (s) await backend.settingsSet({ ...s, ai_web_always: true }).catch(() => undefined);
      }
      setWebHere(true);
      const i = messages.length - 2;
      const user = messages[i];
      if (user?.role !== "user" || inflight.current) return;
      claude.current?.rollback();
      await ask(user.text, { sent: user.sent, web: true, prior: messages.slice(0, i) });
    },
    [messages, ask],
  );

  return {
    provider,
    switchProvider,
    models,
    setModel,
    keys,
    saveKey,
    deleteKey,
    context,
    contextBusy,
    fetchContext,
    reloadContext,
    messages,
    busy,
    streaming,
    note,
    draft,
    setDraft,
    ask,
    askOnce,
    newConversation,
    web: PROVIDER_WEB[provider] && (webAlways || webHere),
    webAvailable: PROVIDER_WEB[provider],
    webAlways,
    setWebAlways,
    webHere,
    setWebHere,
    allowWeb,
    report,
    loadReport,
    history,
    convId,
    openConversation,
    deleteConversation,
    renameConversation,
    moveConversation,
    createGroup,
    renameGroup,
    deleteGroup,
    clearHistory,
    memory,
    remember,
    forget,
    instructions,
    saveInstructions,
  };
}

export type Luli = ReturnType<typeof useLuliState>;
const LuliContext = createContext<Luli | null>(null);

export function LuliProvider({ children }: { children: ReactNode }) {
  const value = useLuliState();
  return <LuliContext.Provider value={value}>{children}</LuliContext.Provider>;
}

export function useLuli(): Luli {
  const c = useContext(LuliContext);
  if (!c) throw new Error("LuliProvider missing");
  return c;
}
