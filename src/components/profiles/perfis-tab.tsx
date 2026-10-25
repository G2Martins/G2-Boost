import { Download, Layers, Plus } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useErrorText } from "@/components/elevate-button";
import { rise } from "@/components/shell/page-header";
import {
  Badge,
  Button,
  Card,
  EmptyState,
  HelpHint,
  Modal,
  Skeleton,
} from "@/components/ui/primitives";
import { toast } from "@/components/ui/toast";
import { backend, inTauri, type ProfileExport, type TweakProfile } from "@/lib/backend";
import { type StateMap, useLatest } from "@/lib/boost-hooks";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { type Applied, ApplyModal } from "./apply-modal";
import { EditorModal, type EditorMode } from "./editor-modal";
import {
  activeCount,
  builtinEntries,
  type Coverage,
  coverage,
  type Entry,
  reverseSteps,
  tweakLabel,
} from "./shared";
import { ExportModal, ImportModal } from "./transfer-modals";

/** One lit segment per tweak of the profile: on = ember, off = dim. Exact, at a glance. */
function LedStrip({ cov, names }: { cov: Coverage | null; names: Record<string, string> }) {
  if (!cov) return inTauri ? <Skeleton className="h-3.5 w-40" /> : null;
  return (
    <div aria-hidden className="flex flex-wrap gap-[3px]">
      {cov.leds.map((l) => (
        <span
          key={l.id}
          title={names[l.id]}
          className={cn(
            "h-3.5 w-1.5 rounded-[2px]",
            l.on ? "bg-primary shadow-[0_0_6px_hsl(var(--primary)/0.55)]" : "bg-foreground/12",
          )}
        />
      ))}
    </div>
  );
}

function ProfileCard({
  entry,
  index,
  states,
  onApply,
  onExport,
  onEdit,
  onDelete,
}: {
  entry: Entry;
  index: number;
  states: StateMap | null;
  onApply: () => void;
  onExport?: () => void;
  onEdit?: () => void;
  onDelete?: () => void;
}) {
  const { t } = useI18n();
  const isWindows = entry.key === "windows";
  const cov = isWindows ? null : coverage(entry.ids, entry.fx, states);
  const active = cov !== null && cov.total > 0 && cov.on === cov.total;
  const names = useMemo(
    () => Object.fromEntries(entry.ids.map((id) => [id, tweakLabel(t, id)])),
    [entry.ids, t],
  );
  const Icon = entry.icon;
  const toTurnOff = isWindows ? activeCount(states) : 0;
  return (
    <Card
      className="panel-hover halo animate-rise flex flex-col gap-3"
      style={rise(index)}
      data-profile={entry.key}
    >
      <div className="flex items-start gap-3">
        <span className="flex size-9 shrink-0 items-center justify-center rounded-md border border-border-strong bg-card-hover text-foreground/90">
          <Icon className="size-5" strokeWidth={1.8} />
        </span>
        <div className="min-w-0 flex-1">
          <h3
            className="truncate font-display text-[15px] font-semibold tracking-wide"
            title={entry.name}
          >
            {entry.name}
          </h3>
          <div className="mt-1 flex flex-wrap items-center gap-1.5">
            {entry.builtin ? <Badge tone="neutral">{t("profiles.badge.builtin")}</Badge> : null}
            {active ? <Badge tone="success">{t("profiles.badge.active")}</Badge> : null}
          </div>
        </div>
      </div>
      {entry.desc ? (
        <p className="text-xs leading-relaxed text-muted-foreground">{entry.desc}</p>
      ) : null}
      <div className="mt-auto flex flex-col gap-2">
        {isWindows ? null : <LedStrip cov={cov} names={names} />}
        <p className="num text-[11px] text-muted-foreground">
          {isWindows
            ? states
              ? toTurnOff > 0
                ? t("profiles.card.windows", { count: toTurnOff })
                : t("profiles.card.windows_none")
              : null
            : cov
              ? entry.ids.length === 0
                ? t("profiles.card.no_tweaks")
                : cov.total > 0
                  ? t("profiles.card.count", { on: cov.on, total: cov.total })
                  : null
              : null}
          {cov && cov.missing > 0 ? (
            <span className="ml-2 text-warning">
              {t("profiles.card.missing", { count: cov.missing })}
            </span>
          ) : null}
        </p>
        {cov ? (
          <span className="sr-only">
            {t("profiles.card.leds", { on: cov.on, total: cov.total })}
          </span>
        ) : null}
      </div>
      <div className="rule-accent" aria-hidden />
      <div className="flex flex-wrap items-center gap-2">
        <Button size="sm" variant="secondary" onClick={onApply}>
          {t("profiles.apply")}
        </Button>
        {onExport ? (
          <Button size="sm" variant="outline" onClick={onExport}>
            {t("profiles.export")}
          </Button>
        ) : null}
        {onEdit ? (
          <Button size="sm" variant="ghost" onClick={onEdit}>
            {t("profiles.edit")}
          </Button>
        ) : null}
        {onDelete ? (
          <Button size="sm" variant="ghost" onClick={onDelete}>
            {t("profiles.delete")}
          </Button>
        ) : null}
      </div>
    </Card>
  );
}

export function PerfisTab({
  states,
  refresh,
  elevated,
}: {
  states: StateMap | null;
  refresh: () => Promise<void>;
  elevated: boolean;
}) {
  const { t } = useI18n();
  const errorText = useErrorText();
  const [profiles, setProfiles] = useState<TweakProfile[] | null>(null);
  const [restoreBefore, setRestoreBefore] = useState(true);
  const [applying, setApplying] = useState<Entry | null>(null);
  const [editor, setEditor] = useState<EditorMode | null>(null);
  const [exported, setExported] = useState<ProfileExport | null>(null);
  const [importing, setImporting] = useState(false);
  const [deleting, setDeleting] = useState<TweakProfile | null>(null);
  const [err, setErr] = useState<string | null>(null);
  // Every opening gets a fresh dialog (new key): no leftovers from the last time.
  const [seq, setSeq] = useState(0);
  const openApply = (e: Entry) => {
    setSeq((n) => n + 1);
    setApplying(e);
  };
  const openEditor = (m: EditorMode) => {
    setSeq((n) => n + 1);
    setEditor(m);
  };

  useEffect(() => {
    let alive = true;
    backend.profilesList().then((p) => alive && setProfiles(p ?? []));
    backend.settingsGet().then((s) => alive && s && setRestoreBefore(s.restore_before_preset));
    return () => {
      alive = false;
    };
  }, []);

  const builtins = builtinEntries(t);
  const custom: Entry[] = (profiles ?? []).map((p) => ({
    key: `custom:${p.name}`,
    builtin: false,
    name: p.name,
    desc: null,
    icon: Layers,
    ids: p.tweaks,
    fx: p.fx_level,
    off: "ask",
    offDefault: false,
  }));

  // Dialogs keep their last content while they fade out; the key restarts them per opening.
  const shownApply = useLatest(applying);
  const shownEditor = useLatest(editor);
  const shownExport = useLatest(exported);
  const shownDelete = useLatest(deleting);

  const exportEntry = async (e: Entry) => {
    setErr(null);
    try {
      const data = await backend.profilesExport({ name: e.name, tweaks: e.ids, fx_level: e.fx });
      setSeq((n) => n + 1);
      setExported(data);
    } catch (error) {
      setErr(errorText(error));
    }
  };

  const applied = async ({ entry, steps, results }: Applied) => {
    await refresh();
    const ok = results.filter((r) => r.ok).length;
    const back = reverseSteps(steps, results);
    toast(t("profiles.toast.applied", { name: entry.name, ok, total: results.length }), {
      tone: ok === results.length ? "success" : "warning",
      duration: 12000,
      action:
        back.length > 0
          ? {
              label: t("profiles.toast.undo"),
              run: async () => {
                for (const s of back) {
                  await backend.profilesStep(s, "@undo").catch(() => undefined);
                }
                await refresh();
                toast(t("profiles.toast.undone"));
              },
            }
          : undefined,
    });
  };

  const remove = async () => {
    if (!deleting) return;
    try {
      setProfiles(await backend.profilesDelete(deleting.name));
      toast(t("profiles.deleted"));
    } catch (e) {
      setErr(errorText(e));
    }
    setDeleting(null);
  };

  return (
    <>
      <Card className="animate-rise flex flex-wrap items-center gap-3 py-4">
        <span className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
          {t("profiles.tab.profiles")}
          <HelpHint text={t("profiles.help")} />
        </span>
        <div className="ml-auto flex flex-wrap items-center gap-2">
          <Button variant="outline" onClick={() => setImporting(true)}>
            <Download className="size-4" />
            {t("profiles.import")}
          </Button>
          <Button variant="primary" tactile onClick={() => openEditor({ kind: "create" })}>
            <Plus className="size-4" />
            {t("profiles.create")}
          </Button>
        </div>
      </Card>
      {err ? <p className="text-xs text-destructive">{err}</p> : null}
      {!inTauri ? (
        <p className="text-xs text-muted-foreground">{t("profiles.error.app_only")}</p>
      ) : null}

      <section className="flex flex-col gap-3" aria-labelledby="profiles-builtin">
        <h2 id="profiles-builtin" className="px-1 text-xs font-semibold text-muted-foreground">
          {t("profiles.builtin.title")}
        </h2>
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2 2xl:grid-cols-4">
          {builtins.map((e, i) => (
            <ProfileCard
              key={e.key}
              entry={e}
              index={i}
              states={states}
              onApply={() => openApply(e)}
              onExport={e.key === "windows" ? undefined : () => exportEntry(e)}
            />
          ))}
        </div>
      </section>

      <section className="flex flex-col gap-3" aria-labelledby="profiles-custom">
        <h2 id="profiles-custom" className="px-1 text-xs font-semibold text-muted-foreground">
          {t("profiles.custom.title")}
        </h2>
        {profiles === null ? (
          <Skeleton className="h-32 w-full" />
        ) : custom.length === 0 ? (
          <Card>
            <EmptyState
              icon={<Layers className="size-5" />}
              title={t("profiles.custom.empty.title")}
              body={t("profiles.custom.empty.body")}
            />
          </Card>
        ) : (
          <div className="grid grid-cols-1 gap-3 md:grid-cols-2 2xl:grid-cols-4">
            {custom.map((e, i) => (
              <ProfileCard
                key={e.key}
                entry={e}
                index={i}
                states={states}
                onApply={() => openApply(e)}
                onExport={() => exportEntry(e)}
                onEdit={() => {
                  const p = profiles.find((x) => x.name === e.name);
                  if (p) openEditor({ kind: "edit", profile: p });
                }}
                onDelete={() => setDeleting(profiles.find((x) => x.name === e.name) ?? null)}
              />
            ))}
          </div>
        )}
      </section>

      {shownApply ? (
        <ApplyModal
          key={`apply-${seq}`}
          entry={shownApply}
          open={applying !== null}
          states={states}
          elevated={elevated}
          restoreBefore={restoreBefore}
          onClose={() => setApplying(null)}
          onDone={applied}
        />
      ) : null}
      {shownEditor ? (
        <EditorModal
          key={`editor-${seq}`}
          mode={shownEditor}
          open={editor !== null}
          states={states}
          onClose={() => setEditor(null)}
          onSaved={(list) => {
            setProfiles(list);
            setEditor(null);
          }}
        />
      ) : null}
      {shownExport ? (
        <ExportModal
          key={`export-${seq}`}
          data={shownExport}
          open={exported !== null}
          onClose={() => setExported(null)}
        />
      ) : null}
      <ImportModal
        open={importing}
        onClose={() => setImporting(false)}
        onImported={(list, name) => {
          setProfiles(list);
          setImporting(false);
          toast(t("profiles.import.done", { name }), { tone: "success" });
        }}
      />
      <Modal
        open={deleting !== null}
        onClose={() => setDeleting(null)}
        closeLabel={t("common.close")}
        title={t("profiles.delete.title", { name: shownDelete?.name ?? "" })}
        footer={
          <>
            <Button variant="ghost" onClick={() => setDeleting(null)}>
              {t("common.cancel")}
            </Button>
            <Button variant="danger" onClick={remove}>
              {t("profiles.delete")}
            </Button>
          </>
        }
      >
        <p className="w-[min(420px,80vw)] text-muted-foreground">{t("profiles.delete.body")}</p>
      </Modal>
    </>
  );
}
