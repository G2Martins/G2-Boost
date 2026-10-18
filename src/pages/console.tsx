import { Copy, CornerDownLeft, Play, RotateCcw, SquareTerminal, Trash2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { LuliMark } from "@/components/luli-card";
import { SafeMarkdown } from "@/components/safe-markdown";
import type { Route } from "@/components/shell/app-shell";
import { PageBody, PageHeader } from "@/components/shell/page-header";
import { Card } from "@/components/ui/primitives";
import { type Answer, isTimeout } from "@/lib/ai";
import { backend, type ConsoleCommand, type ConsoleReply, inTauri } from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { useLuli } from "@/lib/luli";
import { navigate } from "@/lib/nav";
import { cn } from "@/lib/utils";

/* ---------------- session store (survives leaving the page) ---------------- */

type Entry = {
  id: number;
  line: string;
  kind: "cmd" | "luli" | "help";
  status: "running" | "ok" | "error";
  ms?: number;
  reply?: ConsoleReply;
  answer?: Answer;
  streaming?: string;
  error?: string;
};

let entries: Entry[] = [];
let nextId = 1;
const listeners = new Set<() => void>();
function update(f: (e: Entry[]) => Entry[]) {
  entries = f(entries);
  for (const l of listeners) l();
}
function patchEntry(id: number, p: Partial<Entry>) {
  update((list) => list.map((e) => (e.id === id ? { ...e, ...p } : e)));
}
function useEntries() {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => entries,
  );
}

const LS_HISTORY = "g2boost.console.history";
function loadHistory(): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(LS_HISTORY) ?? "[]");
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string").slice(-100) : [];
  } catch {
    return [];
  }
}
function saveHistory(h: string[]) {
  try {
    localStorage.setItem(LS_HISTORY, JSON.stringify(h.slice(-100)));
  } catch {
    /* storage unavailable */
  }
}

/** A table as tab-separated text (copy, and the snapshot Luli reads). */
function tableText(r: ConsoleReply): string {
  return r.blocks
    .map((b) =>
      b.kind === "table"
        ? [b.columns.join("\t"), ...b.rows.map((row) => row.join("\t"))].join("\n")
        : `${b.key} ${JSON.stringify(b.params)}`,
    )
    .join("\n\n");
}

export function ConsolePage() {
  const { t } = useI18n();
  const luli = useLuli();
  const list = useEntries();
  const [commands, setCommands] = useState<ConsoleCommand[]>([]);
  const [draft, setDraft] = useState("");
  const [history, setHistory] = useState<string[]>(loadHistory);
  const [cursor, setCursor] = useState<number | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const endRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    backend.consoleCommands().then((c) => c && setCommands(c));
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    if (list.length > 0) endRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [list]);

  const names = useMemo(() => {
    const s = new Set<string>();
    for (const c of commands) {
      s.add(c.name);
      for (const a of c.aliases) s.add(a);
    }
    return s;
  }, [commands]);

  const word = draft.trimStart().split(/\s+/)[0]?.toLowerCase() ?? "";
  const matches =
    draft.includes(" ") || word === "" || word.startsWith("?")
      ? []
      : commands.filter(
          (c) => c.name.startsWith(word) || c.aliases.some((a) => a.startsWith(word)),
        );

  const remember = (line: string) => {
    const h = [...history.filter((x) => x !== line), line];
    setHistory(h);
    saveHistory(h);
    setCursor(null);
  };

  const runLine = async (raw: string) => {
    const line = raw.trim();
    if (!line) return;
    setDraft("");
    remember(line);
    if (line.startsWith("?")) {
      void askLuli(line.slice(1).trim(), line);
      return;
    }
    const id = nextId++;
    const t0 = performance.now();
    update((l) => [...l, { id, line, kind: "cmd", status: "running" }]);
    try {
      const reply = await backend.consoleRun(line);
      const ms = Math.round(performance.now() - t0);
      if (reply.effect?.kind === "clear") {
        update(() => []);
        return;
      }
      if (reply.effect?.kind === "help") {
        patchEntry(id, { kind: "help", status: "ok", ms });
        return;
      }
      if (reply.effect?.kind === "navigate") {
        patchEntry(id, { status: "ok", ms, reply });
        navigate(reply.effect.route as Route);
        return;
      }
      patchEntry(id, { status: reply.ok ? "ok" : "error", ms, reply });
    } catch (e) {
      patchEntry(id, {
        status: "error",
        error: inTauri ? String(e instanceof Error ? e.message : e) : t("console.browser"),
      });
    }
  };

  /** `? question` or "explain this block": Luli answers inline, suggesting runnable lines. */
  const askLuli = async (question: string, line: string, about?: Entry) => {
    if (!question && !about) return;
    const id = nextId++;
    update((l) => [...l, { id, line, kind: "luli", status: "running" }]);
    const data = about?.reply
      ? `\n\n<page_data>\nConsole: ${about.line}\n${tableText(about.reply).replace(/</g, "\\u003c").slice(0, 12000)}\n</page_data>`
      : "";
    const q = question || t("console.explain.question");
    try {
      const answer = await luli.askOnce(q, {
        sent: `${q}${data}`,
        console: commands.map((c) => c.usage),
        onText: (s) => patchEntry(id, { streaming: s }),
      });
      patchEntry(id, { status: "ok", answer, streaming: undefined });
    } catch (e) {
      const m = e instanceof Error ? e.message : String(e);
      patchEntry(id, {
        status: "error",
        streaming: undefined,
        error:
          m === "no_key"
            ? t("ai.key.missing")
            : isTimeout(e)
              ? t("ai.error.timeout")
              : t("ai.error.generic", { error: m }),
      });
    }
  };

  const onKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      void runLine(draft);
    } else if (e.key === "Tab" && matches[0]) {
      e.preventDefault();
      setDraft(`${matches[0].name} `);
    } else if (e.key === "ArrowUp" && history.length > 0) {
      e.preventDefault();
      const i = cursor === null ? history.length - 1 : Math.max(0, cursor - 1);
      setCursor(i);
      setDraft(history[i] ?? "");
    } else if (e.key === "ArrowDown" && cursor !== null) {
      e.preventDefault();
      const i = cursor + 1;
      if (i >= history.length) {
        setCursor(null);
        setDraft("");
      } else {
        setCursor(i);
        setDraft(history[i] ?? "");
      }
    } else if (e.key === "l" && e.ctrlKey) {
      e.preventDefault();
      update(() => []);
    }
  };

  return (
    <>
      <PageHeader title={t("console.title")} subtitle={t("console.subtitle")} />
      <PageBody className="pt-0">
        <Card className="animate-rise flex min-h-[calc(100vh-220px)] flex-col gap-0 p-0">
          <div className="flex items-center gap-2 border-b border-border px-4 py-2.5 text-[11px] text-muted-foreground">
            <SquareTerminal className="size-4 text-primary" />
            <span className="font-mono">g2</span>
            <span className="ml-auto hidden sm:inline">{t("console.tips")}</span>
            <button
              type="button"
              onClick={() => update(() => [])}
              disabled={list.length === 0}
              aria-label={t("console.clear")}
              className="press rounded p-1 hover:bg-foreground/[0.06] hover:text-foreground disabled:opacity-40"
            >
              <Trash2 className="size-3.5" />
            </button>
          </div>

          <div className="flex-1 space-y-3 overflow-y-auto px-4 py-4 font-mono text-[12px]">
            {list.length === 0 ? <Welcome commands={commands} run={runLine} /> : null}
            {list.map((e) => (
              <EntryView
                key={e.id}
                entry={e}
                commands={commands}
                names={names}
                run={runLine}
                fill={(l) => {
                  setDraft(l);
                  inputRef.current?.focus();
                }}
                explain={() => askLuli("", `? ${t("console.explain.question")}`, e)}
              />
            ))}
            <div ref={endRef} />
          </div>

          <div className="relative border-t border-border">
            {matches.length > 0 ? (
              <ul className="absolute bottom-full left-3 mb-1 max-h-56 w-[min(28rem,90%)] overflow-y-auto rounded-md border border-border-strong bg-surface p-1 shadow-lg">
                {matches.slice(0, 8).map((c, i) => (
                  <li key={c.name}>
                    <button
                      type="button"
                      onMouseDown={(ev) => {
                        ev.preventDefault();
                        setDraft(`${c.name} `);
                        inputRef.current?.focus();
                      }}
                      className={cn(
                        "flex w-full items-baseline gap-3 rounded px-2 py-1.5 text-left hover:bg-foreground/[0.06]",
                        i === 0 && "bg-foreground/[0.04]",
                      )}
                    >
                      <span className="font-mono text-[12px] text-foreground">{c.usage}</span>
                      <span className="truncate text-[11px] text-muted-foreground">
                        {t(`console.cmd.${c.name}` as I18nKey)}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            ) : null}
            <label className="flex items-center gap-2 px-4 py-3">
              <span className="font-mono text-[13px] font-semibold text-primary" aria-hidden>
                ›
              </span>
              <input
                ref={inputRef}
                value={draft}
                onChange={(e) => {
                  setDraft(e.target.value);
                  setCursor(null);
                }}
                onKeyDown={onKey}
                maxLength={512}
                spellCheck={false}
                autoComplete="off"
                aria-label={t("console.input")}
                placeholder={t("console.placeholder")}
                className="min-w-0 flex-1 bg-transparent font-mono text-[13px] outline-none placeholder:text-muted-foreground/60"
              />
              <button
                type="button"
                onClick={() => runLine(draft)}
                disabled={!draft.trim()}
                aria-label={t("console.run")}
                className="press rounded p-1.5 text-muted-foreground hover:bg-foreground/[0.06] hover:text-foreground disabled:opacity-40"
              >
                <CornerDownLeft className="size-4" />
              </button>
            </label>
          </div>
        </Card>
      </PageBody>
    </>
  );
}

function Welcome({ commands, run }: { commands: ConsoleCommand[]; run: (l: string) => void }) {
  const { t } = useI18n();
  const starters = [
    "pc",
    "erros 7",
    "disco",
    "ping 1.1.1.1",
    "processos",
    `? ${t("console.welcome.ask")}`,
  ];
  return (
    <div className="max-w-2xl space-y-3 font-sans">
      <p className="font-display text-[15px] font-semibold tracking-wide">
        {t("console.welcome.title")}
      </p>
      <p className="text-[13px] text-muted-foreground">{t("console.welcome.body")}</p>
      <div className="flex flex-wrap gap-1.5">
        {(commands.length ? starters : []).map((s) => (
          <button
            key={s}
            type="button"
            onClick={() => run(s)}
            className="press rounded-full border border-border px-3 py-1 font-mono text-[11px] text-muted-foreground hover:border-primary/50 hover:text-foreground"
          >
            {s}
          </button>
        ))}
      </div>
    </div>
  );
}

function EntryView({
  entry: e,
  commands,
  names,
  run,
  fill,
  explain,
}: {
  entry: Entry;
  commands: ConsoleCommand[];
  names: Set<string>;
  run: (l: string) => void;
  fill: (l: string) => void;
  explain: () => void;
}) {
  const { t } = useI18n();
  const copy = () => {
    if (e.reply) void navigator.clipboard.writeText(tableText(e.reply)).catch(() => undefined);
  };
  return (
    <section
      className={cn(
        "animate-rise border-l-2 pl-3",
        e.status === "running" && "border-primary/70",
        e.status === "ok" && "border-border-strong",
        e.status === "error" && "border-destructive/70",
      )}
    >
      <header className="group flex items-center gap-2">
        {e.kind === "luli" ? (
          <LuliMark className="size-5" />
        ) : (
          <span className="text-primary">›</span>
        )}
        <span className="truncate text-foreground">{e.line}</span>
        {e.status === "running" ? (
          <span className="size-1.5 animate-pulse rounded-full bg-primary" aria-hidden />
        ) : null}
        <span className="ml-auto flex items-center gap-1 text-[10px] text-muted-foreground">
          {e.ms !== undefined ? <span>{(e.ms / 1000).toFixed(1)} s</span> : null}
          {e.kind === "cmd" && e.reply?.blocks.length ? (
            <>
              <IconButton label={t("console.explain")} onClick={explain}>
                <LuliMark className="size-3.5 bg-transparent shadow-none" />
              </IconButton>
              <IconButton label={t("console.rerun")} onClick={() => run(e.line)}>
                <RotateCcw className="size-3.5" />
              </IconButton>
              <IconButton label={t("console.copy")} onClick={copy}>
                <Copy className="size-3.5" />
              </IconButton>
            </>
          ) : null}
        </span>
      </header>

      <div className="mt-2 space-y-2">
        {e.kind === "help" ? <Help commands={commands} fill={fill} /> : null}
        {e.error ? <p className="text-destructive">{e.error}</p> : null}
        {e.reply?.error ? (
          <p className="text-destructive">
            {t(`console.${e.reply.error.key}` as I18nKey, e.reply.error.params)}
          </p>
        ) : null}
        {e.reply?.blocks.map((b, i) =>
          b.kind === "table" ? (
            // biome-ignore lint/suspicious/noArrayIndexKey: blocks never reorder
            <ConsoleTable key={i} columns={b.columns} rows={b.rows} />
          ) : (
            // biome-ignore lint/suspicious/noArrayIndexKey: blocks never reorder
            <p key={i} className="text-muted-foreground">
              {t(`console.${b.key}` as I18nKey, b.params)}
            </p>
          ),
        )}
        {e.kind === "luli" && (e.streaming || e.answer) ? (
          <div className="font-sans text-[13px]">
            <SafeMarkdown text={e.answer?.text ?? e.streaming ?? ""} />
          </div>
        ) : null}
        {e.kind === "luli" && e.status === "running" && !e.streaming ? (
          <p className="font-sans text-muted-foreground">{t("ai.thinking")}</p>
        ) : null}
        {e.answer?.actions.console.length ? (
          <div className="flex flex-wrap gap-1.5">
            {e.answer.actions.console
              .filter((l) => names.has(l.split(/\s+/)[0]?.toLowerCase() ?? ""))
              .map((l) => (
                <button
                  key={l}
                  type="button"
                  onClick={() => run(l)}
                  className="press flex items-center gap-1.5 rounded-md border border-primary/35 bg-primary/5 px-2.5 py-1 text-[11px] text-foreground hover:bg-primary/10"
                >
                  <Play className="size-3 text-primary" />
                  {l}
                </button>
              ))}
          </div>
        ) : null}
        {e.answer?.actions.todo.length ? (
          <ul className="list-disc space-y-0.5 pl-5 font-sans text-[12px] text-muted-foreground">
            {e.answer.actions.todo.map((x) => (
              <li key={x}>{x}</li>
            ))}
          </ul>
        ) : null}
      </div>
    </section>
  );
}

function IconButton({
  label,
  onClick,
  children,
}: {
  label: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      className="press rounded p-1 opacity-60 hover:bg-foreground/[0.06] hover:text-foreground hover:opacity-100 focus-visible:opacity-100"
    >
      {children}
    </button>
  );
}

/** Column ids and a few id-valued columns (`part`, `hint`) are translated; other cells are data. */
function ConsoleTable({ columns, rows }: { columns: string[]; rows: string[][] }) {
  const { t } = useI18n();
  const tr = (key: string, raw: string) => {
    const v = t(key as I18nKey);
    return v === key ? raw : v;
  };
  if (rows.length === 0) return <p className="text-muted-foreground">{t("console.empty")}</p>;
  return (
    <div className="overflow-x-auto">
      <table className="w-full border-collapse text-left">
        <thead>
          <tr className="text-[10px] uppercase tracking-wider text-muted-foreground">
            {columns.map((c) => (
              <th key={c} className="whitespace-nowrap py-1 pr-4 font-semibold">
                {tr(`console.col.${c}`, c)}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: static rows
            <tr key={i} className="border-t border-border/60 align-top">
              {r.map((cell, j) => {
                const col = columns[j] ?? "";
                const shown =
                  col === "part"
                    ? tr(`console.part.${cell}`, cell)
                    : col === "hint" && cell
                      ? tr(`console.hint.${cell}`, cell)
                      : cell;
                return (
                  <td
                    // biome-ignore lint/suspicious/noArrayIndexKey: static cells
                    key={j}
                    className={cn(
                      "py-1 pr-4",
                      col === "message"
                        ? "min-w-[18rem] whitespace-normal text-muted-foreground"
                        : "whitespace-nowrap",
                    )}
                  >
                    {shown}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Help({ commands, fill }: { commands: ConsoleCommand[]; fill: (l: string) => void }) {
  const { t } = useI18n();
  return (
    <div className="grid gap-x-6 gap-y-1 sm:grid-cols-[auto_1fr]">
      {commands.map((c) => (
        <div key={c.name} className="contents">
          <button
            type="button"
            onClick={() => fill(`${c.name} `)}
            className="text-left text-foreground hover:text-primary"
          >
            {c.usage}
          </button>
          <span className="font-sans text-[12px] text-muted-foreground">
            {t(`console.cmd.${c.name}` as I18nKey)}
            {c.aliases.length ? (
              <span className="ml-2 font-mono text-[10px]">({c.aliases.join(", ")})</span>
            ) : null}
          </span>
        </div>
      ))}
      <span className="text-foreground">? …</span>
      <span className="font-sans text-[12px] text-muted-foreground">{t("console.cmd.luli")}</span>
    </div>
  );
}
