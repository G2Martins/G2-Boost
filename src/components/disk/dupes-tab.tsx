import { Copy, Loader2, Square, Trash2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useErrorText } from "@/components/elevate-button";
import { Badge, Button, Card, CardTitle, EmptyState, Modal } from "@/components/ui/primitives";
import { backend, type DupGroup, type DupProgress, inTauri } from "@/lib/backend";
import { boot } from "@/lib/boot";
import { useI18n } from "@/lib/i18n";
import { formatBytes } from "@/lib/utils";

/** Duplicate files (≥ 1 MB) in the last scan; the user picks copies → Recycle Bin, one always stays. */
export function DupesTab({ hasScan }: { hasScan: boolean }) {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const [groups, setGroups] = useState<DupGroup[] | null>(null);
  const [progress, setProgress] = useState<DupProgress | null>(null);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [confirm, setConfirm] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const running = progress !== null;
  const autoStarted = useRef(false);

  useEffect(() => {
    const offs = [
      backend.on<DupProgress>("dupes:progress", setProgress),
      backend.on<DupGroup[]>("dupes:done", (g) => {
        setGroups(g);
        setProgress(null);
        setPicked(new Set());
      }),
    ];
    return () => {
      for (const o of offs) o.then((f) => f());
    };
  }, []);

  const start = async () => {
    setMsg(null);
    setGroups(null);
    setProgress({ candidates: 0, hashed_bytes: 0, errors: 0 });
    try {
      await backend.dupesStart();
    } catch (e) {
      setProgress(null);
      setMsg(errorText(e));
    }
  };
  // Dev aid (screenshots): G2BOOST_START_TAB=dupes + autorun searches right after the scan.
  // biome-ignore lint/correctness/useExhaustiveDependencies: fires once when the scan is ready
  useEffect(() => {
    if (boot.autorun && boot.startTab === "dupes" && hasScan && !autoStarted.current) {
      autoStarted.current = true;
      void start();
    }
  }, [hasScan]);
  const wasted = useMemo(() => (groups ?? []).reduce((a, g) => a + g.wasted, 0), [groups]);
  const pickedBytes = useMemo(
    () =>
      (groups ?? []).reduce((a, g) => a + g.paths.filter((p) => picked.has(p)).length * g.size, 0),
    [groups, picked],
  );
  const toggle = (p: string) =>
    setPicked((cur) => {
      const n = new Set(cur);
      if (n.has(p)) n.delete(p);
      else n.add(p);
      return n;
    });
  /** Every copy except the first of each group (the user can still change it). */
  const pickCopies = () => setPicked(new Set((groups ?? []).flatMap((g) => g.paths.slice(1))));
  const keepsOne = (groups ?? []).every((g) => g.paths.some((p) => !picked.has(p)));

  const recycle = async () => {
    setConfirm(false);
    try {
      const o = await backend.dupesRecycle([...picked]);
      setMsg(t("disk.recycle.done", { count: o.recycled, size: formatBytes(o.bytes, locale) }));
      setGroups(
        (gs) =>
          gs
            ?.map((g) => ({ ...g, paths: g.paths.filter((p) => !picked.has(p)) }))
            .filter((g) => g.paths.length > 1) ?? gs,
      );
      setPicked(new Set());
    } catch (e) {
      setMsg(
        e instanceof Error && e.message === "keeps_none" ? t("dupes.keeps_none") : errorText(e),
      );
    }
  };

  if (!hasScan) {
    return (
      <Card className="animate-rise">
        <EmptyState
          icon={<Copy className="size-5" />}
          title={t("disk.empty.title")}
          body={t("dupes.need_scan")}
        />
      </Card>
    );
  }
  return (
    <Card className="animate-rise">
      <CardTitle
        help={t("dupes.help")}
        right={
          running ? (
            <Button size="sm" variant="outline" onClick={() => backend.dupesCancel()}>
              <Square className="size-3.5" />
              {t("common.stop")}
            </Button>
          ) : (
            <Button size="sm" variant="primary" disabled={!inTauri} onClick={start}>
              <Copy className="size-3.5" />
              {t("dupes.find")}
            </Button>
          )
        }
      >
        {t("dupes.title")}
      </CardTitle>
      {running ? (
        <p className="num flex items-center gap-2 text-xs text-muted-foreground">
          <Loader2 className="size-3.5 animate-spin text-primary" />
          {t("dupes.progress", {
            n: progress.candidates.toLocaleString(locale),
            size: formatBytes(progress.hashed_bytes, locale),
          })}
        </p>
      ) : null}
      {msg ? <p className="mt-2 text-xs text-foreground/90">{msg}</p> : null}
      {groups && groups.length === 0 ? (
        <p className="py-6 text-center text-xs text-muted-foreground">{t("dupes.none")}</p>
      ) : null}
      {groups && groups.length > 0 ? (
        <>
          <div className="mb-3 flex flex-wrap items-center gap-3 text-xs">
            <span className="num text-muted-foreground">
              {t("dupes.summary", { n: groups.length, size: formatBytes(wasted, locale) })}
            </span>
            <Button size="sm" variant="ghost" onClick={pickCopies}>
              {t("dupes.pick_copies")}
            </Button>
            {picked.size > 0 ? (
              <Button
                size="sm"
                variant="danger"
                className="ml-auto"
                disabled={!keepsOne}
                onClick={() => setConfirm(true)}
              >
                <Trash2 className="size-3.5" />
                {t("disk.recycle.selected", {
                  count: picked.size,
                  size: formatBytes(pickedBytes, locale),
                })}
              </Button>
            ) : null}
          </div>
          {!keepsOne ? <p className="mb-2 text-xs text-warning">{t("dupes.keeps_none")}</p> : null}
          <ul className="max-h-130 space-y-3 overflow-y-auto pr-1">
            {groups.slice(0, 200).map((g) => (
              <li key={g.paths[0]} className="rounded-md border border-border p-2.5">
                <p className="mb-1.5 flex items-center gap-2 text-xs">
                  <Badge tone="neutral">{t("dupes.copies", { n: g.paths.length })}</Badge>
                  <span className="num text-muted-foreground">{formatBytes(g.size, locale)}</span>
                  <span className="num ml-auto text-muted-foreground">
                    {t("dupes.wasted", { size: formatBytes(g.wasted, locale) })}
                  </span>
                </p>
                <ul className="space-y-0.5">
                  {g.paths.map((p) => (
                    <li key={p} className="flex items-center gap-2">
                      <input
                        type="checkbox"
                        checked={picked.has(p)}
                        onChange={() => toggle(p)}
                        aria-label={p}
                        className="accent-primary"
                      />
                      <span className="num min-w-0 flex-1 truncate text-[11px]" title={p}>
                        {p}
                      </span>
                    </li>
                  ))}
                </ul>
              </li>
            ))}
          </ul>
        </>
      ) : null}
      <Modal
        open={confirm}
        onClose={() => setConfirm(false)}
        closeLabel={t("common.cancel")}
        title={t("disk.recycle.confirm_title", {
          count: picked.size,
          size: formatBytes(pickedBytes, locale),
        })}
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirm(false)}>
              {t("common.cancel")}
            </Button>
            <Button variant="danger" onClick={recycle}>
              {t("disk.recycle.confirm")}
            </Button>
          </>
        }
      >
        <p className="text-muted-foreground">{t("dupes.confirm_body")}</p>
      </Modal>
    </Card>
  );
}
