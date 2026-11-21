import { BellOff, CheckCheck, ChevronRight, History, Trash2 } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import {
  isRoute,
  JobLine,
  LineRow,
  routeIcon,
  routeLabel,
  useFeed,
  useJobs,
} from "@/components/shell/activity";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  EmptyState,
  Skeleton,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/primitives";
import { backend, inTauri } from "@/lib/backend";
import { takeStartTab } from "@/lib/boot";
import { useI18n } from "@/lib/i18n";
import { clearFeed, goTo, markRead, onJobEnded } from "@/lib/jobs";
import { dayKey, formatDuration, formatParams, normalizeRuns, type Run } from "@/lib/runs";
import { cn } from "@/lib/utils";

/** Notification center + the unified run history of every module (r11:activity). */
export function ActivityPage() {
  const { t } = useI18n();
  const [arrival] = useState(takeStartTab);
  return (
    <PageHeader
      title={t("activity.title")}
      subtitle={t("activity.subtitle")}
      tabs={
        <Tabs defaultValue={arrival === "runs" ? "runs" : "notifications"}>
          <TabsList>
            <TabsTrigger value="notifications">{t("activity.tab.notifications")}</TabsTrigger>
            <TabsTrigger value="runs">{t("activity.tab.runs")}</TabsTrigger>
          </TabsList>
          <TabsContent value="notifications">
            <PageBody className="px-0 pb-0">
              <Notifications />
            </PageBody>
          </TabsContent>
          <TabsContent value="runs">
            <PageBody className="px-0 pb-0">
              <Runs />
            </PageBody>
          </TabsContent>
        </Tabs>
      }
    />
  );
}

/** "Hoje", "Ontem" or the date: the eyebrow over each day of a timeline. */
function useDayLabel() {
  const { t, locale } = useI18n();
  return (key: number) =>
    key === 0
      ? t("activity.day.today")
      : key === 1
        ? t("activity.day.yesterday")
        : new Date(key).toLocaleDateString(locale, {
            weekday: "long",
            day: "numeric",
            month: "long",
          });
}

function byDay<T>(items: T[], at: (x: T) => number, now: number): [number, T[]][] {
  const days = new Map<number, T[]>();
  for (const x of items) {
    const k = dayKey(at(x), now);
    const list = days.get(k);
    if (list) list.push(x);
    else days.set(k, [x]);
  }
  return [...days];
}

function Eyebrow({ children }: { children: string }) {
  return (
    <p className="px-1 pb-2 text-[10px] font-semibold uppercase tracking-[0.16em] text-muted-foreground/80">
      {children}
    </p>
  );
}

function Notifications() {
  const { t } = useI18n();
  const feed = useFeed();
  const dayLabel = useDayLabel();
  const [now] = useState(() => Date.now());
  // Seen on the way out (the unread marks stay while the person reads). The time check skips
  // StrictMode's instant mount/unmount in dev.
  useEffect(() => {
    const since = Date.now();
    return () => {
      if (Date.now() - since > 300) markRead();
    };
  }, []);
  const unread = feed.some((e) => !e.read);
  if (!feed.length)
    return (
      <Card className="animate-rise">
        <EmptyState
          icon={<BellOff className="size-5" />}
          title={t("activity.notifications.empty.title")}
          body={t("activity.notifications.empty.body")}
        />
      </Card>
    );
  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-end gap-2">
        <Button size="sm" variant="ghost" disabled={!unread} onClick={() => markRead()}>
          <CheckCheck className="size-3.5" />
          {t("activity.read_all")}
        </Button>
        <Button size="sm" variant="ghost" onClick={clearFeed}>
          <Trash2 className="size-3.5" />
          {t("activity.clear")}
        </Button>
      </div>
      {byDay(feed, (e) => e.at, now).map(([day, entries], i) => (
        <section key={day} className="animate-rise" style={rise(i)}>
          <Eyebrow>{dayLabel(day)}</Eyebrow>
          <div className="panel divide-y divide-border overflow-hidden">
            {entries.map((e) => (
              <LineRow key={e.id} entry={e} now={now} />
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}

function Runs() {
  const { t, locale } = useI18n();
  const jobs = useJobs();
  const dayLabel = useDayLabel();
  const [runs, setRuns] = useState<Run[] | null>(null);
  const [filter, setFilter] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());

  const load = useCallback(async () => {
    const [cleanup, repair, volumes, plans, fixes, updates, installs, profiles] = await Promise.all(
      [
        backend.cleanupHistory(),
        backend.repairHistory(),
        backend.volumesHistory(),
        backend.maintPlanHistory(),
        backend.maintFixHistory(),
        backend.updatesHistory(),
        backend.installHistory(),
        backend.profilesHistory(),
      ],
    );
    setNow(Date.now());
    setRuns(normalizeRuns({ cleanup, repair, volumes, plans, fixes, updates, installs, profiles }));
  }, []);
  useEffect(() => {
    void load();
    // A job that ends while the page is open lands in its history: read them again.
    return onJobEnded(() => window.setTimeout(load, 600));
  }, [load]);

  const modules = useMemo(() => {
    const count = new Map<string, number>();
    for (const r of runs ?? []) count.set(r.route, (count.get(r.route) ?? 0) + 1);
    return [...count];
  }, [runs]);
  const shown = (runs ?? []).filter((r) => !filter || r.route === filter);
  const live = jobs.filter((j) => j.state === "run");

  return (
    <div className="flex flex-col gap-4">
      {live.length ? (
        <Card className="animate-rise">
          <CardTitle>{t("activity.runs.live")}</CardTitle>
          <div className="flex flex-col gap-1">
            {live.map((j) => (
              <button
                key={j.kind}
                type="button"
                onClick={() => goTo(j.route, j.tab)}
                className="press tone-accent flex items-center gap-3 rounded-md px-2 py-2 text-left hover:bg-foreground/5"
              >
                <JobLine job={j} />
                <ChevronRight className="ml-auto size-3.5 text-muted-foreground" />
              </button>
            ))}
          </div>
        </Card>
      ) : null}

      {modules.length > 1 ? (
        <fieldset className="flex flex-wrap gap-1.5" aria-label={t("activity.runs.filter")}>
          <Chip on={filter === null} onClick={() => setFilter(null)}>
            {t("activity.runs.all")}
            <span className="num text-muted-foreground">{runs?.length ?? 0}</span>
          </Chip>
          {modules.map(([route, n]) => {
            const Icon = routeIcon(route);
            const label = routeLabel(route);
            return (
              <Chip key={route} on={filter === route} onClick={() => setFilter(route)}>
                <Icon className="size-3.5" />
                {label ? t(label) : route}
                <span className="num text-muted-foreground">{n}</span>
              </Chip>
            );
          })}
        </fieldset>
      ) : null}

      {runs === null && inTauri ? (
        <Card>
          <div className="flex flex-col gap-4">
            {[0, 1, 2, 3].map((i) => (
              <div key={i} className="flex items-center gap-3">
                <Skeleton className="size-9 rounded-full" />
                <div className="flex flex-1 flex-col gap-1.5">
                  <Skeleton className="h-3.5 w-48" />
                  <Skeleton className="h-3 w-28" />
                </div>
              </div>
            ))}
          </div>
        </Card>
      ) : shown.length === 0 ? (
        <Card className="animate-rise">
          <EmptyState
            icon={<History className="size-5" />}
            title={t("activity.runs.empty.title")}
            body={inTauri ? t("activity.runs.empty.body") : t("common.app_only")}
          />
        </Card>
      ) : (
        byDay(shown, (r) => r.at, now).map(([day, list], gi) => (
          <section key={day} className="animate-rise" style={rise(gi)}>
            <Eyebrow>{dayLabel(day)}</Eyebrow>
            <ol className="run-rail panel flex flex-col gap-0.5 p-2">
              {list.map((r) => (
                <li key={r.id}>
                  <RunRow run={r} locale={locale} />
                </li>
              ))}
            </ol>
          </section>
        ))
      )}
    </div>
  );
}

function Chip({
  on,
  onClick,
  children,
}: {
  on: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-pressed={on}
      onClick={onClick}
      className={cn(
        "press inline-flex h-8 items-center gap-1.5 rounded-md border px-2.5 text-xs font-semibold transition-colors duration-200 ease-out-expo",
        on
          ? "border-border-strong bg-foreground/9 text-foreground"
          : "border-border text-muted-foreground hover:bg-foreground/5 hover:text-foreground",
      )}
    >
      {children}
    </button>
  );
}

function RunRow({ run, locale }: { run: Run; locale: string }) {
  const { t } = useI18n();
  const Icon = routeIcon(run.route);
  const [titleKey, titleParams] = run.title;
  const [resultKey, resultParams] = run.result;
  const when = new Date(run.at * 1000).toLocaleTimeString(locale, {
    hour: "2-digit",
    minute: "2-digit",
  });
  return (
    <button
      type="button"
      disabled={!isRoute(run.route)}
      onClick={() => goTo(run.route, run.tab)}
      className="press group flex w-full items-center gap-3 rounded-md px-1 py-1.5 text-left transition-colors duration-200 ease-out-expo hover:bg-foreground/4"
    >
      <span className={cn("run-node", `tone-${run.tone}`)}>
        <Icon className="size-4" />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[13px] font-semibold">
          {t(titleKey, formatParams(titleParams, locale))}
        </span>
        <span className="num mt-0.5 block text-[11px] text-muted-foreground">
          {run.ms ? `${when} · ${formatDuration(run.ms)}` : when}
        </span>
      </span>
      <Badge tone={run.tone} className="normal-case tracking-normal">
        {t(resultKey, formatParams(resultParams, locale))}
      </Badge>
      <ChevronRight className="size-3.5 shrink-0 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100" />
    </button>
  );
}
