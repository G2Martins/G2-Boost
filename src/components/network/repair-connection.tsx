import { CircleCheck, Eraser, ExternalLink, Loader2, RefreshCw, Wrench } from "lucide-react";
import { useEffect, useState } from "react";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { rise } from "@/components/shell/page-header";
import { Badge, Button, Card, CardTitle } from "@/components/ui/primitives";
import { backend, type DhcpAdapter, inTauri } from "@/lib/backend";
import { useI18n } from "@/lib/i18n";

type Msg = { at: string; text: string; ok: boolean };

/** "Reparar conexão": DNS cache flush, DHCP renew per adapter, Windows' own Network reset. */
export function RepairConnectionCard({ index }: { index: number }) {
  const { t } = useI18n();
  const errorText = useErrorText();
  const [elevated, setElevated] = useState(false);
  const [adapters, setAdapters] = useState<DhcpAdapter[] | null>(null);
  const [confirm, setConfirm] = useState<number | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [msg, setMsg] = useState<Msg | null>(null);

  useEffect(() => {
    backend.appInfo().then((i) => i && setElevated(i.elevated));
    backend.netDhcpAdapters().then((a) => setAdapters(a ?? []));
  }, []);

  const act = async (at: string, run: () => Promise<string>) => {
    setBusy(at);
    setMsg(null);
    try {
      setMsg({ at, text: await run(), ok: true });
    } catch (e) {
      const m = e instanceof Error ? e.message : String(e);
      setMsg({
        at,
        text: m === "unknown_adapter" ? t("maint2.net.unknown_adapter") : errorText(e),
        ok: false,
      });
    } finally {
      setBusy(null);
    }
  };

  const renew = (a: DhcpAdapter) => {
    setConfirm(null);
    void act(`renew:${a.index}`, async () => {
      const ip = await backend.netRenew(a.index);
      setAdapters((await backend.netDhcpAdapters()) ?? []);
      return ip ? t("maint2.net.renewed", { ip }) : t("maint2.net.renewed_noip");
    });
  };

  const line = (at: string) =>
    msg?.at === at ? (
      <p
        className={
          msg.ok
            ? "mt-1.5 flex items-center gap-1.5 text-xs text-success"
            : "mt-1.5 text-xs text-destructive"
        }
      >
        {msg.ok ? <CircleCheck className="size-3.5" /> : null}
        {msg.text}
      </p>
    ) : null;

  return (
    <Card className="animate-rise" style={rise(index)}>
      <CardTitle
        help={t("maint2.net.help")}
        right={elevated ? null : <ElevateButton route="network" />}
      >
        <Wrench className="size-4" />
        {t("maint2.net.title")}
      </CardTitle>
      <div className="grid grid-cols-1 gap-3 lg:grid-cols-3">
        <section className="rounded-lg border border-border bg-background/40 p-3">
          <p className="flex items-center gap-2 text-sm font-semibold">
            <Eraser className="size-4 text-muted-foreground" />
            {t("maint2.net.dns.title")}
          </p>
          <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
            {t("maint2.net.dns.body")}
          </p>
          <Button
            className="mt-3"
            size="sm"
            variant="primary"
            disabled={!inTauri || busy !== null}
            onClick={() =>
              void act("dns", async () => {
                await backend.netFlushDns();
                return t("maint2.net.dns.done");
              })
            }
          >
            {busy === "dns" ? (
              <Loader2 className="size-4 animate-spin" />
            ) : (
              <Eraser className="size-4" />
            )}
            {t("maint2.net.dns.button")}
          </Button>
          {line("dns")}
        </section>

        <section className="rounded-lg border border-border bg-background/40 p-3">
          <p className="flex items-center gap-2 text-sm font-semibold">
            <RefreshCw className="size-4 text-muted-foreground" />
            {t("maint2.net.renew.title")}
            <Badge tone={elevated ? "neutral" : "warning"}>{t("common.admin_required")}</Badge>
          </p>
          <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
            {t("maint2.net.renew.body")}
          </p>
          {adapters !== null && adapters.length === 0 ? (
            <p className="mt-3 text-xs text-muted-foreground">{t("maint2.net.renew.none")}</p>
          ) : (
            <ul className="mt-2 flex flex-col divide-y divide-border">
              {(adapters ?? []).map((a) => {
                const at = `renew:${a.index}`;
                return (
                  <li key={a.index} className="py-2">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="min-w-0 flex-1">
                        <span
                          className="block truncate text-[13px] font-medium"
                          title={a.description}
                        >
                          {a.name}
                          {a.primary ? (
                            <Badge tone="accent" className="ml-1.5">
                              {t("maint2.net.primary")}
                            </Badge>
                          ) : null}
                        </span>
                        <span className="num block text-[11px] text-muted-foreground">
                          {a.ipv4 ?? "—"}
                        </span>
                      </span>
                      {confirm === a.index ? null : (
                        <Button
                          size="sm"
                          variant="outline"
                          disabled={!inTauri || !elevated || busy !== null}
                          onClick={() => setConfirm(a.index)}
                        >
                          {busy === at ? <Loader2 className="size-4 animate-spin" /> : null}
                          {t("maint2.net.renew.button")}
                        </Button>
                      )}
                    </div>
                    {confirm === a.index ? (
                      <div className="mt-2 flex flex-wrap items-center gap-2 rounded-md bg-warning/8 p-2.5 ring-1 ring-warning/25 ring-inset">
                        <p className="flex-1 text-xs text-foreground/90">
                          {t("maint2.net.renew.confirm")}
                        </p>
                        <div className="ml-auto flex gap-2">
                          <Button variant="ghost" size="sm" onClick={() => setConfirm(null)}>
                            {t("common.cancel")}
                          </Button>
                          <Button variant="primary" size="sm" onClick={() => renew(a)}>
                            {t("maint2.net.renew.button")}
                          </Button>
                        </div>
                      </div>
                    ) : null}
                    {line(at)}
                  </li>
                );
              })}
            </ul>
          )}
        </section>

        <section className="rounded-lg border border-border bg-background/40 p-3">
          <p className="flex items-center gap-2 text-sm font-semibold">
            <ExternalLink className="size-4 text-muted-foreground" />
            {t("maint2.net.reset.title")}
          </p>
          <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
            {t("maint2.net.reset.body")}
          </p>
          <Button
            className="mt-3"
            size="sm"
            variant="outline"
            disabled={!inTauri || busy !== null}
            onClick={() =>
              void act("reset", async () => {
                await backend.netOpenReset();
                return t("maint2.net.reset.opened");
              })
            }
          >
            <ExternalLink className="size-4" />
            {t("maint2.net.reset.button")}
          </Button>
          {line("reset")}
        </section>
      </div>
    </Card>
  );
}
