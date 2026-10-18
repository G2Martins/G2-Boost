import { FolderOpen, Lock, Pause, Play, Search, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { rise } from "@/components/shell/page-header";
import { Badge, Button, Card, HelpHint, Modal, Skeleton } from "@/components/ui/primitives";
import { backend, inTauri, type ProcInfo } from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn, formatBytes } from "@/lib/utils";

const POLL_MS = 2000;
const MAX_ROWS = 250;

/** Task manager: live CPU/RAM per process, open file location, end (confirmed) — never protected ones. */
export function ProcsTab({ elevated }: { elevated: boolean }) {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const [list, setList] = useState<ProcInfo[] | null>(null);
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<"cpu" | "ram">("cpu");
  const [paused, setPaused] = useState(false);
  const [confirm, setConfirm] = useState<ProcInfo | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  useEffect(() => {
    if (!inTauri || paused) return;
    let alive = true;
    const tick = async () => {
      const l = await backend.procsList();
      if (alive && l) setList(l);
    };
    void tick();
    // The first sample has no CPU delta yet; the second (2 s later) does.
    const id = window.setInterval(tick, POLL_MS);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, [paused]);

  const rows = useMemo(() => {
    const q = query.trim().toLowerCase();
    const filtered = (list ?? []).filter((p) => !q || p.name.includes(q) || String(p.pid) === q);
    filtered.sort((a, b) => (sort === "cpu" ? b.cpu_pct - a.cpu_pct : b.ram_bytes - a.ram_bytes));
    return filtered.slice(0, MAX_ROWS);
  }, [list, query, sort]);

  const procError = (e: unknown) => {
    const m = e instanceof Error ? e.message : String(e);
    if (m === "process_gone") return t("procs.error.gone");
    if (m === "protected_process") return t("procs.error.protected");
    return errorText(e);
  };

  const end = async (p: ProcInfo) => {
    setConfirm(null);
    setMsg(null);
    try {
      await backend.procsEnd(p.pid, p.created);
      setMsg(t("procs.ended", { name: p.name }));
      setList((l) => l?.filter((x) => !(x.pid === p.pid && x.created === p.created)) ?? l);
    } catch (e) {
      setMsg(procError(e));
    }
  };
  const reveal = (p: ProcInfo) =>
    backend.procsReveal(p.pid, p.created).catch((e) => setMsg(procError(e)));

  const totalCpu = (list ?? []).reduce((s, p) => s + p.cpu_pct, 0);
  const headerBtn = (key: "cpu" | "ram", label: string) => (
    <button
      type="button"
      onClick={() => setSort(key)}
      className={cn(
        "press text-right transition-colors hover:text-foreground",
        sort === key && "text-foreground",
      )}
      aria-pressed={sort === key}
    >
      {label}
      {sort === key ? " ↓" : ""}
    </button>
  );

  return (
    <>
      <Card className="animate-rise flex flex-wrap items-center gap-3 py-4">
        <span className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
          {t("procs.title")}
          <HelpHint text={t("procs.help")} />
        </span>
        {list ? (
          <Badge tone="neutral">
            {t("procs.count", { n: list.length, cpu: Math.min(100, totalCpu).toFixed(0) })}
          </Badge>
        ) : null}
        {!elevated ? <ElevateButton route="boost" /> : null}
        <label className="ml-auto flex h-9 w-64 items-center gap-2 rounded-md border border-border bg-background px-3 text-muted-foreground focus-within:border-ring">
          <Search className="size-4" />
          <input
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t("procs.search")}
            aria-label={t("procs.search")}
            className="w-full bg-transparent text-[13px] text-foreground outline-none placeholder:text-muted-foreground"
          />
        </label>
        <Button
          variant="outline"
          disabled={!inTauri}
          onClick={() => setPaused((v) => !v)}
          aria-label={paused ? t("procs.resume") : t("procs.pause")}
          title={paused ? t("procs.resume") : t("procs.pause")}
        >
          {paused ? <Play className="size-4" /> : <Pause className="size-4" />}
        </Button>
        <p className="w-full text-xs text-muted-foreground">{t("procs.note")}</p>
        {msg ? <p className="w-full text-xs text-foreground/90">{msg}</p> : null}
      </Card>

      <Card className="animate-rise min-w-0 py-2" style={rise(1)}>
        <div className="grid grid-cols-[1fr_70px_80px_96px_76px] gap-3 border-b border-border px-1 py-2 text-[11px] font-semibold text-muted-foreground">
          <span>{t("procs.col.name")}</span>
          <span className="text-right">PID</span>
          {headerBtn("cpu", "CPU")}
          {headerBtn("ram", t("procs.col.ram"))}
          <span />
        </div>
        {list === null && inTauri
          ? [0, 1, 2, 3, 4, 5, 6, 7].map((k) => <Skeleton key={k} className="my-2.5 h-5 w-full" />)
          : null}
        <ul className="divide-y divide-border/70">
          {rows.map((p) => (
            <li
              key={`${p.pid}:${p.created}`}
              className="grid grid-cols-[1fr_70px_80px_96px_76px] items-center gap-3 px-1 py-2"
            >
              <span className="flex min-w-0 items-center gap-2" title={p.path}>
                <span className="truncate text-sm font-medium">{p.name}</span>
                {p.protected ? (
                  <Badge tone="neutral">{t(`procs.kind.${p.protected}` as I18nKey)}</Badge>
                ) : null}
              </span>
              <span className="num text-right text-xs text-muted-foreground">{p.pid}</span>
              <span className="flex flex-col items-end gap-1">
                <span className="num text-xs">{p.cpu_pct.toFixed(1)}%</span>
                <span className="h-0.5 w-14 overflow-hidden rounded-full bg-foreground/[0.06]">
                  <span
                    className="block h-full rounded-full bg-primary/80 transition-[width] duration-500 ease-out-expo"
                    style={{ width: `${Math.min(100, p.cpu_pct)}%` }}
                  />
                </span>
              </span>
              <span className="num text-right text-xs font-semibold">
                {formatBytes(p.ram_bytes, locale)}
              </span>
              <span className="flex justify-end gap-1">
                <button
                  type="button"
                  onClick={() => reveal(p)}
                  aria-label={t("procs.reveal")}
                  title={t("procs.reveal")}
                  className="press flex size-8 items-center justify-center rounded-md text-muted-foreground hover:bg-foreground/[0.06] hover:text-foreground"
                >
                  <FolderOpen className="size-4" />
                </button>
                {p.protected ? (
                  <span
                    className="flex size-8 items-center justify-center text-muted-foreground/60"
                    title={t("procs.error.protected")}
                  >
                    <Lock className="size-3.5" />
                  </span>
                ) : (
                  <button
                    type="button"
                    onClick={() => setConfirm(p)}
                    aria-label={t("procs.end_named", { name: p.name })}
                    title={t("procs.end")}
                    className="press flex size-8 items-center justify-center rounded-md text-muted-foreground hover:bg-destructive/12 hover:text-destructive"
                  >
                    <X className="size-4" />
                  </button>
                )}
              </span>
            </li>
          ))}
        </ul>
        {list && rows.length === 0 ? (
          <p className="py-8 text-center text-xs text-muted-foreground">{t("procs.empty")}</p>
        ) : null}
      </Card>

      <Modal
        open={confirm !== null}
        onClose={() => setConfirm(null)}
        closeLabel={t("common.cancel")}
        title={t("procs.confirm_title", { name: confirm?.name ?? "" })}
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirm(null)}>
              {t("common.cancel")}
            </Button>
            <Button variant="danger" onClick={() => confirm && end(confirm)}>
              {t("procs.end")}
            </Button>
          </>
        }
      >
        <p className="text-muted-foreground">
          {t("procs.confirm_body", { pid: confirm?.pid ?? 0 })}
        </p>
        <p
          className="mt-2 truncate font-mono text-[11px] text-muted-foreground/80"
          title={confirm?.path}
        >
          {confirm?.path}
        </p>
      </Modal>
    </>
  );
}
