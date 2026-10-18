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
  isTimeout,
  PROVIDER_WEB,
  PROVIDERS,
  type Provider,
  systemPrompt,
  type Turn,
} from "@/lib/ai";
import { type AdvisorReport, backend, inTauri } from "@/lib/backend";
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
  const reportBusy = useRef(false);
  const claude = useRef<ClaudeChat | null>(null);
  // The request in flight: aborted on provider/model switch and on a new conversation, so a late
  // answer never lands in a transcript it no longer belongs to.
  const inflight = useRef<AbortController | null>(null);

  const cancel = useCallback(() => {
    inflight.current?.abort();
    inflight.current = null;
  }, []);

  useEffect(() => {
    backend.aiKeyStatus().then((k) => k && setKeys(k));
    backend.settingsGet().then((s) => s && setWebAlways(!!s.ai_web_always));
  }, []);

  const newConversation = useCallback(() => {
    cancel();
    claude.current = null;
    setMessages([]);
    setWebHere(false);
    setNote(null);
  }, [cancel]);

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
      setMessages([...prior, { role: "user", text: q, sent: sent === q ? undefined : sent }]);
      const system = systemPrompt(locale, ctx, { knowledge: KNOWLEDGE, web, noWeb });
      const ctrl = new AbortController();
      inflight.current = ctrl;
      try {
        let answer: Answer;
        if (provider === "anthropic") {
          if (!claude.current) claude.current = new ClaudeChat(models.anthropic);
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
    },
    [keys, provider, context, fetchContext, webAlways, webHere, messages, locale, models, t],
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
    [keys, provider, context, fetchContext, webAlways, locale, models],
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
