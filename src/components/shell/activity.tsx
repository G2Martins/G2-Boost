// The shell's view of what is happening: the bell (header), the floating job pill, and the effects
// that turn a new bell line into a toast or a Windows notification. Stores live in lib/jobs.ts.
import { Bell, BellOff, ChevronRight } from "lucide-react";
import { useEffect, useId, useRef, useState, useSyncExternalStore } from "react";
import { NAV, type Route } from "@/components/shell/app-shell";
import { Badge, DoneMark, ProgressRing, type Tone } from "@/components/ui/primitives";
import { toast } from "@/components/ui/toast";
import { type ActivityEntry, type ActivityTone, backend } from "@/lib/backend";
import { useI18n } from "@/lib/i18n";
import {
  attachNote,
  getFeed,
  getJobs,
  goTo,
  type Job,
  markRead,
  onJobEnded,
  onLine,
  subscribe,
} from "@/lib/jobs";
import { formatAgo, lineText } from "@/lib/runs";
import { cn, formatBytes } from "@/lib/utils";

export const useJobs = () => useSyncExternalStore(subscribe, getJobs);
export const useFeed = () => useSyncExternalStore(subscribe, getFeed);

// Lazy: app-shell imports this file, so NAV is read at call time, never at module load.
const item = (r: string | null | undefined) =>
  NAV.flatMap((g) => g.items).find((i) => i.route === r);
export const isRoute = (r: string | null | undefined): r is Route => !!item(r);
/** The module's sidebar icon (a line or a run is drawn with the page it opens). */
export const routeIcon = (r: string | null | undefined) => item(r)?.icon ?? Bell;
export const routeLabel = (r: string | null | undefined) => item(r)?.label;

export const LINE_TONE: Record<ActivityTone, Tone> = {
  info: "neutral",
  ok: "success",
  warn: "warning",
  err: "danger",
};

/** One bell line: module icon in a well, the sentence, when. Clicking opens the module. */
export function LineRow({
  entry,
  now,
  onOpen,
}: {
  entry: ActivityEntry;
  now: number;
  onOpen?: () => void;
}) {
  const { t, locale } = useI18n();
  const Icon = routeIcon(entry.route);
  const route = isRoute(entry.route) ? entry.route : null;
  return (
    <button
      type="button"
      disabled={!route}
      data-unread={entry.read ? undefined : ""}
      onClick={() => {
        if (!route) return;
        onOpen?.();
        goTo(route);
      }}
      className={cn(
        "line-row press flex w-full items-start gap-3 px-3.5 py-2.5 text-left transition-colors duration-200 ease-out-expo hover:bg-foreground/5 disabled:cursor-default disabled:hover:bg-transparent",
        `tone-${LINE_TONE[entry.tone]}`,
      )}
    >
      <span className="mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-md border border-border bg-card text-muted-foreground">
        <Icon className="size-3.5" />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block text-[13px] leading-snug">{lineText(t, locale, entry)}</span>
        <span className="mt-0.5 block text-[11px] text-muted-foreground">
          {formatAgo(entry.at, now, locale)}
        </span>
      </span>
      {entry.read ? null : (
        <span
          role="img"
          aria-label={t("activity.unread")}
          className="mt-2 size-1.5 shrink-0 rounded-full bg-primary shadow-[0_0_6px_hsl(var(--primary)/0.7)]"
        />
      )}
    </button>
  );
}

/** Header bell: unread count, the last ten lines in a native popover, "Ver tudo". */
export function ActivityBell() {
  const { t } = useI18n();
  const feed = useFeed();
  const id = useId();
  const pop = useRef<HTMLDivElement>(null);
  const [now, setNow] = useState(0);
  const unread = feed.filter((e) => !e.read).length;
  const recent = feed.slice(0, 10);
  return (
    <>
      <button
        type="button"
        popoverTarget={id}
        aria-label={t("activity.bell.label", { count: unread })}
        title={t("activity.bell")}
        className="press relative flex size-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-foreground/6 hover:text-foreground"
      >
        {/* Re-keyed by the newest unread line: the bell swings once when one arrives. */}
        <Bell
          key={unread ? `ring-${feed[0]?.id}` : "quiet"}
          className={cn("size-4", unread > 0 && "bell-ring text-foreground")}
        />
        {unread ? (
          <span key={`n-${unread}`} className="bell-count num" aria-hidden>
            {unread > 9 ? "9+" : unread}
          </span>
        ) : null}
      </button>
      <div
        ref={pop}
        id={id}
        popover="auto"
        role="dialog"
        aria-label={t("activity.bell")}
        className="bell-pop"
        onToggle={(e) => {
          if (e.newState === "open") setNow(Date.now());
          // Seen once the popover closes (the dots stay while it is open).
          else markRead(recent.filter((x) => !x.read).map((x) => x.id));
        }}
      >
        <header className="flex items-center justify-between gap-3 border-b border-border px-3.5 py-2.5">
          <span className="font-display text-[14px] font-semibold tracking-wide">
            {t("activity.bell")}
          </span>
          {unread ? (
            <button
              type="button"
              onClick={() => markRead()}
              className="press rounded-md px-2 py-1 text-[11px] font-semibold text-muted-foreground hover:bg-foreground/6 hover:text-foreground"
            >
              {t("activity.read_all")}
            </button>
          ) : null}
        </header>
        {recent.length ? (
          <div className="min-h-0 flex-1 divide-y divide-border overflow-y-auto">
            {recent.map((e) => (
              <LineRow key={e.id} entry={e} now={now} onOpen={() => pop.current?.hidePopover()} />
            ))}
          </div>
        ) : (
          <div className="flex flex-col items-center gap-2 px-6 py-8 text-center">
            <BellOff className="size-5 text-muted-foreground" />
            <p className="text-xs text-muted-foreground">{t("activity.bell.empty")}</p>
          </div>
        )}
        <footer className="border-t border-border p-1.5">
          <button
            type="button"
            onClick={() => {
              pop.current?.hidePopover();
              goTo("activity");
            }}
            className="press flex w-full items-center justify-center gap-1.5 rounded-md py-2 text-xs font-semibold text-primary hover:bg-primary/10"
          >
            {t("activity.bell.all")}
            <ChevronRight className="size-3.5" />
          </button>
        </footer>
      </div>
    </>
  );
}

const JOB_TONE: Record<Job["state"], string> = {
  run: "tone-accent",
  ok: "tone-success",
  err: "tone-danger",
};

/** Ring (or the done mark) + label: shared by the pill and the page's "Em andamento". */
export function JobLine({ job }: { job: Job }) {
  const { t, locale } = useI18n();
  const running = job.state === "run";
  const meta = [
    running && job.percent !== null ? `${Math.round(job.percent * 100)}%` : null,
    job.bytes ? formatBytes(job.bytes, locale) : null,
  ]
    .filter(Boolean)
    .join(" · ");
  return (
    <>
      {running ? (
        <ProgressRing value={job.percent} />
      ) : (
        <DoneMark
          ok={job.state === "ok"}
          className={job.state === "ok" ? "text-success" : "text-destructive"}
        />
      )}
      <span className="flex min-w-0 flex-col items-start leading-tight">
        <span className="max-w-72 truncate text-[12.5px] font-semibold">
          {running
            ? t(job.label)
            : (job.note ?? t(job.state === "ok" ? "jobs.done" : "jobs.failed"))}
        </span>
        <span className="num text-[10.5px] text-muted-foreground">
          {running ? meta || " " : t(job.label)}
        </span>
      </span>
    </>
  );
}

/** Floating readout of the job running on another page; click goes back to it. */
export function JobPill({ route }: { route: Route }) {
  const { t } = useI18n();
  const away = useJobs().filter((j) => j.route !== route);
  const job = away.find((j) => j.state === "run") ?? away[away.length - 1];
  if (!job) return null;
  const more = away.length - 1;
  return (
    <div className="pointer-events-none absolute inset-x-0 bottom-5 z-40 flex justify-center px-4">
      <button
        type="button"
        data-state={job.leaving ? "out" : "in"}
        onClick={() => goTo(job.route, job.tab)}
        aria-label={t("jobs.open", { label: t(job.label) })}
        className={cn("job-pill press pointer-events-auto", JOB_TONE[job.state])}
      >
        <JobLine job={job} />
        {more > 0 ? <Badge tone="neutral">{t("jobs.more", { count: more })}</Badge> : null}
        <ChevronRight className="size-3.5 shrink-0 text-muted-foreground" />
      </button>
    </div>
  );
}

/** New bell line → a toast when its page is not on screen (or the pill's note when that page has
 *  a job), and a Windows notification when the window is hidden (the core decides). */
export function ActivityEffects({ route }: { route: Route }) {
  const { t, locale } = useI18n();
  const here = useRef(route);
  useEffect(() => {
    here.current = route;
  }, [route]);
  useEffect(() => {
    // route → when a Windows notification last went out (one per job, not one per line + end)
    const sent = new Map<string, number>();
    const native = async (title: string, body: string, r: string | null) => {
      const res = await backend.activityOsNotify(title, body, r);
      if (res === "toast" || res === "flash") sent.set(r ?? "", Date.now());
    };
    const recently = (r: string | null) => Date.now() - (sent.get(r ?? "") ?? 0) < 5000;
    const offLine = onLine((e) => {
      const text = lineText(t, locale, e);
      const label = routeLabel(e.route);
      // A game just started: the person is playing, the bell is enough.
      if (e.key !== "activity.ram.game" && !recently(e.route))
        void native(label ? t(label) : t("app.name"), text, e.route);
      if (e.route === here.current) return;
      if (attachNote(e.route, text)) return;
      const r = isRoute(e.route) ? e.route : null;
      toast(text, {
        tone: LINE_TONE[e.tone],
        duration: 7000,
        action: r ? { label: t("activity.open"), run: () => goTo(r) } : undefined,
      });
    });
    const offEnd = onJobEnded((j) => {
      // A bell line for the same job usually lands first and carries more detail.
      window.setTimeout(() => {
        if (recently(j.route)) return;
        void native(
          t(j.label),
          t(j.state === "ok" ? "jobs.native.done" : "jobs.native.failed"),
          j.route,
        );
      }, 1500);
    });
    const offOpen = backend.on<string>("activity:open", (r) => {
      if (isRoute(r)) goTo(r);
    });
    return () => {
      offLine();
      offEnd();
      void offOpen.then((f) => f());
    };
  }, [t, locale]);
  return null;
}
