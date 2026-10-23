import { ChevronRight, CircleCheck, ListChecks, Power, TriangleAlert } from "lucide-react";
import { Fragment, useEffect, useState } from "react";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { rise } from "@/components/shell/page-header";
import { Badge, Button, Card, CardTitle, Modal, Skeleton } from "@/components/ui/primitives";
import { type BiosGuide, type BiosItemId, backend, inTauri } from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";

/** Items whose change can go wrong in a known way carry a `.warn` line. */
const WARN: BiosItemId[] = ["csm", "xmp", "secure_boot", "bios_update"];

/** The firmware's menu trail, as it reads on screen: one chip per level. */
function MenuPath({ path }: { path: string }) {
  const parts = path.split(" › ");
  return (
    <p className="mt-2 flex flex-wrap items-center gap-1 rounded-md border border-border bg-background/70 px-2.5 py-2 font-mono text-[11px] leading-none text-foreground/85">
      {parts.map((p, i) => (
        // The trail up to this level: unique and stable.
        <Fragment key={parts.slice(0, i + 1).join("›")}>
          {i > 0 ? <ChevronRight aria-hidden className="size-3 text-muted-foreground" /> : null}
          <span className={i === parts.length - 1 ? "text-primary" : undefined}>{p}</span>
        </Fragment>
      ))}
    </p>
  );
}

/** "O que mudar na BIOS": checklist from the firmware findings + restart into the setup. */
export function BiosGuideCard({ elevated, index }: { elevated: boolean; index: number }) {
  const { t } = useI18n();
  const errorText = useErrorText();
  const [guide, setGuide] = useState<BiosGuide | null>(null);
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    backend.biosGuide().then((g) => alive && setGuide(g));
    return () => {
      alive = false;
    };
  }, []);

  if (!inTauri) return null; // nothing to tell without the core

  const reboot = async () => {
    setBusy(true);
    setMsg(null);
    try {
      await backend.biosReboot();
      setMsg(t("bios.reboot.started"));
    } catch (e) {
      const m = e instanceof Error ? e.message : String(e);
      setMsg(m === "unsupported" ? t("bios.reboot.unsupported") : errorText(e));
    } finally {
      setBusy(false);
      setConfirm(false);
    }
  };

  return (
    <Card className="animate-rise xl:col-span-2" style={rise(index)}>
      <CardTitle
        help={t("bios.help")}
        right={
          guide ? (
            guide.uefi ? (
              elevated ? (
                <Button
                  variant="outline"
                  size="sm"
                  disabled={busy}
                  onClick={() => setConfirm(true)}
                >
                  <Power className="size-4" />
                  {t("bios.reboot")}
                </Button>
              ) : (
                <ElevateButton route="dashboard" tab="mypc" />
              )
            ) : (
              <span className="text-xs text-muted-foreground">{t("bios.reboot.legacy")}</span>
            )
          ) : null
        }
      >
        <ListChecks className="size-4" />
        {t("bios.title")}
        {guide ? (
          <Badge tone="neutral" className="ml-1">
            {t(`bios.vendor.${guide.vendor}` as I18nKey)}
          </Badge>
        ) : null}
      </CardTitle>

      {guide ? (
        <>
          <p className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
            <span>{t("bios.keys", { key: guide.enter_key })}</span>
            {guide.advanced_key ? (
              <span>{t("bios.keys.advanced", { key: guide.advanced_key })}</span>
            ) : null}
          </p>
          {guide.items.length === 0 ? (
            <p className="mt-4 flex items-center gap-2 text-sm text-success">
              <CircleCheck className="size-4" />
              {t("bios.none")}
            </p>
          ) : (
            <ol className="mt-3 grid grid-cols-1 gap-3 lg:grid-cols-2">
              {guide.items.map((it, i) => (
                <li
                  key={it.id}
                  className="flex gap-3 rounded-lg border border-border bg-background/40 p-3"
                >
                  <span className="num flex size-6 shrink-0 items-center justify-center rounded-full border border-border-strong font-display text-xs font-semibold text-muted-foreground">
                    {i + 1}
                  </span>
                  <div className="min-w-0 flex-1">
                    <p className="flex flex-wrap items-center gap-2 text-sm font-semibold">
                      {t(`bios.item.${it.id}` as I18nKey)}
                      <Badge tone={it.optional ? "neutral" : "accent"}>
                        {it.optional ? t("bios.optional") : t("bios.recommended")}
                      </Badge>
                    </p>
                    <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
                      {t(`bios.item.${it.id}.desc` as I18nKey)}
                    </p>
                    <MenuPath path={it.path} />
                    {WARN.includes(it.id) ? (
                      <p className="mt-2 flex gap-1.5 text-[11px] leading-relaxed text-warning">
                        <TriangleAlert className="mt-px size-3.5 shrink-0" />
                        {t(`bios.item.${it.id}.warn` as I18nKey)}
                      </p>
                    ) : null}
                  </div>
                </li>
              ))}
            </ol>
          )}
          <p className="mt-3 text-[11px] text-muted-foreground">{t("bios.names_vary")}</p>
          {msg ? <p className="animate-rise mt-2 text-xs text-foreground/90">{msg}</p> : null}
        </>
      ) : (
        <div className="flex flex-col gap-2">
          <Skeleton className="h-3.5 w-56" />
          <Skeleton className="h-20 w-full" />
        </div>
      )}

      <Modal
        open={confirm}
        onClose={() => !busy && setConfirm(false)}
        title={t("bios.reboot.title")}
        closeLabel={t("bios.reboot.cancel")}
        footer={
          <>
            <Button variant="ghost" disabled={busy} onClick={() => setConfirm(false)}>
              {t("bios.reboot.cancel")}
            </Button>
            <Button variant="primary" disabled={busy} onClick={reboot}>
              <Power className="size-4" />
              {t("bios.reboot.confirm")}
            </Button>
          </>
        }
      >
        <p>{t("bios.reboot.body")}</p>
        <p className="mt-2 text-muted-foreground">{t("bios.reboot.back")}</p>
      </Modal>
    </Card>
  );
}
