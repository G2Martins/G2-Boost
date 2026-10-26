import {
  Check,
  Copy,
  FolderOpen,
  RefreshCw,
  Shield,
  ShieldAlert,
  ShieldCheck,
  ShieldOff,
  ShieldQuestionMark,
  Undo2,
  X,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { rise } from "@/components/shell/page-header";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  EmptyState,
  Modal,
  Skeleton,
  selectClass,
} from "@/components/ui/primitives";
import { toast } from "@/components/ui/toast";
import { type AvProduct, type AvStatus, type AvVendor, backend, inTauri } from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

type GuideKey = AvVendor | "other";

/** Step keys per guide (`av.guide.<keys>.<n>`); AVG shares Avast's menus. */
const GUIDE: Record<GuideKey, { keys: string; steps: number }> = {
  kaspersky: { keys: "kaspersky", steps: 4 },
  avast: { keys: "avast", steps: 4 },
  avg: { keys: "avast", steps: 4 },
  eset: { keys: "eset", steps: 4 },
  norton: { keys: "norton", steps: 4 },
  bitdefender: { keys: "bitdefender", steps: 4 },
  mcafee: { keys: "mcafee", steps: 4 },
  malwarebytes: { keys: "malwarebytes", steps: 4 },
  trend: { keys: "trend", steps: 4 },
  panda: { keys: "panda", steps: 3 },
  other: { keys: "other", steps: 3 },
};
const GUIDE_KEYS = Object.keys(GUIDE) as GuideKey[];

const SCANNER_TONE = {
  on: "success",
  off: "neutral",
  snoozed: "warning",
  expired: "danger",
} as const;

/** Segurança › Antivírus: what is installed, the one Defender exclusion we offer, vendor guides. */
export function AntivirusTab() {
  const { t } = useI18n();
  const errorText = useErrorText();
  const [s, setS] = useState<AvStatus | null>(null);
  const [loading, setLoading] = useState(inTauri);
  const [confirm, setConfirm] = useState(false);
  const [working, setWorking] = useState(false);
  const [note, setNote] = useState<{ ok: boolean; text: string } | null>(null);
  const [picked, setPicked] = useState<GuideKey | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    const r = await backend.avStatus();
    if (r) setS(r);
    setLoading(false);
  }, []);
  useEffect(() => {
    if (inTauri) void load();
  }, [load]);

  const act = async (action: () => Promise<AvStatus>, ok: I18nKey) => {
    setConfirm(false);
    setWorking(true);
    setNote(null);
    try {
      setS(await action());
      setNote({ ok: true, text: t(ok) });
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      const own = msg === "not_applied" ? "av.error.not_applied" : `av.blocked.${msg}`;
      setNote({ ok: false, text: t(own as I18nKey) !== own ? t(own as I18nKey) : errorText(e) });
    }
    setWorking(false);
  };

  const thirdParty = s?.products.find((p) => !p.defender && p.scanner === "on") ?? null;
  const guide: GuideKey = picked ?? thirdParty?.vendor ?? "other";
  const showDefender = s !== null && (s.defender_active || s.excluded === true);

  return (
    <>
      <p className="animate-rise flex items-center gap-2 text-xs text-muted-foreground">
        <Shield className="size-3.5 shrink-0" />
        {t("av.intro")}
      </p>
      {note ? (
        <p className={cn("text-xs", note.ok ? "text-success" : "text-destructive")}>{note.text}</p>
      ) : null}

      <Card className="animate-rise" style={rise(1)}>
        <CardTitle
          help={t("av.products.help")}
          right={
            <Button variant="ghost" size="sm" disabled={loading || !inTauri} onClick={load}>
              <RefreshCw className={cn("size-4", loading && "motion-safe:animate-spin")} />
              {t("av.refresh")}
            </Button>
          }
        >
          {t("av.products.title")}
        </CardTitle>
        {!inTauri ? (
          <EmptyState
            icon={<ShieldQuestionMark className="size-5" />}
            title={t("common.app_only")}
          />
        ) : !s ? (
          <div className="space-y-2">
            <Skeleton className="h-9 w-full" />
            <Skeleton className="h-9 w-full" />
          </div>
        ) : s.products.length === 0 ? (
          <p className="text-sm text-muted-foreground">{t("av.products.none")}</p>
        ) : (
          <ul className="divide-y divide-border">
            {s.products.map((p) => (
              <ProductRow key={p.name} p={p} reserve={p.defender && thirdParty !== null} />
            ))}
          </ul>
        )}
        {s && !s.products.some((p) => p.scanner === "on") ? (
          <p className="mt-3 text-xs text-warning">{t("av.products.none_on")}</p>
        ) : null}
      </Card>

      {showDefender && s ? (
        <Card className="animate-rise" style={rise(2)}>
          <CardTitle>
            <ShieldCheck className="size-4" />
            {t("av.defender.title")}
          </CardTitle>
          <p className="max-w-3xl text-xs leading-relaxed text-muted-foreground">
            {t("av.defender.body")}
          </p>
          <ExePath exe={s.exe} />
          <ul className="mt-3 space-y-1.5 text-xs">
            <CheckLine ok={s.defender_active} text={t("av.check.active")} />
            <CheckLine
              ok={s.blocked !== "not_protected_folder" && s.blocked !== "unknown_exe"}
              text={t("av.check.folder")}
            />
            <CheckLine
              ok={s.blocked === null || s.blocked === "defender_inactive"}
              text={t("av.check.acl")}
            />
          </ul>
          <div className="mt-4 flex flex-wrap items-center gap-3">
            {s.excluded === true ? (
              <>
                <span className="text-xs text-success">{t("av.defender.excluded")}</span>
                {s.elevated ? (
                  <Button
                    variant="ghost"
                    size="sm"
                    disabled={working}
                    onClick={() => void act(backend.avUnexclude, "av.defender.undone")}
                  >
                    <Undo2 className="size-4" />
                    {t("av.defender.undo")}
                  </Button>
                ) : (
                  <ElevateButton route="security" tab="antivirus" />
                )}
              </>
            ) : s.blocked ? (
              <p className="max-w-3xl text-xs leading-relaxed text-warning">
                {t(`av.blocked.${s.blocked}` as I18nKey)}
              </p>
            ) : s.elevated ? (
              <Button
                variant="primary"
                size="sm"
                disabled={working}
                onClick={() => setConfirm(true)}
              >
                <ShieldCheck className="size-4" />
                {t("av.defender.button")}
              </Button>
            ) : (
              <>
                <ElevateButton route="security" tab="antivirus" />
                <span className="text-xs text-muted-foreground">
                  {t("av.defender.check_admin")}
                </span>
              </>
            )}
          </div>
        </Card>
      ) : null}

      <Card className="animate-rise" style={rise(3)}>
        <CardTitle
          right={
            <label className="flex items-center gap-2 text-xs text-muted-foreground">
              {t("av.guide.pick")}
              <select
                className={selectClass}
                value={guide}
                onChange={(e) => setPicked(e.target.value as GuideKey)}
              >
                {GUIDE_KEYS.map((k) => (
                  <option key={k} value={k}>
                    {t(`av.vendor.${k}` as I18nKey)}
                  </option>
                ))}
              </select>
            </label>
          }
        >
          {guide === "other"
            ? t("av.guide.title_generic")
            : t("av.guide.title", { name: t(`av.vendor.${guide}` as I18nKey) })}
        </CardTitle>
        {s?.blocked === "defender_inactive" && thirdParty ? (
          <p className="mb-2 text-xs text-muted-foreground">{t("av.blocked.defender_inactive")}</p>
        ) : null}
        <p className="text-xs text-muted-foreground">{t("av.guide.body")}</p>
        <ol className="mt-3 space-y-2">
          {Array.from({ length: GUIDE[guide].steps }, (_, i) => i + 1).map((n) => (
            <li key={n} className="flex gap-3 text-[13px] leading-relaxed">
              <span className="num flex size-5.5 shrink-0 items-center justify-center rounded-full text-[11px] font-semibold ring-1 ring-border-strong ring-inset">
                {n}
              </span>
              <span>
                {t(`av.guide.${GUIDE[guide].keys}.${n}` as I18nKey, {
                  name: t(`av.vendor.${guide}` as I18nKey),
                })}
              </span>
            </li>
          ))}
        </ol>
        {s?.exe ? (
          <ExePath exe={s.exe} />
        ) : (
          <p className="mt-3 text-xs text-muted-foreground">{t("av.guide.app_only")}</p>
        )}
        <div className="mt-3 flex flex-wrap gap-2">
          <Button
            variant="secondary"
            size="sm"
            disabled={!s?.exe}
            onClick={() => {
              if (!s?.exe) return;
              navigator.clipboard
                .writeText(s.exe)
                .then(() => toast(t("av.guide.copied"), { tone: "success" }))
                .catch(() => undefined);
            }}
          >
            <Copy className="size-4" />
            {t("av.guide.copy")}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            disabled={!s?.exe}
            onClick={() =>
              backend.avRevealExe().catch((e) => setNote({ ok: false, text: errorText(e) }))
            }
          >
            <FolderOpen className="size-4" />
            {t("av.guide.reveal")}
          </Button>
        </div>
        <p className="mt-3 text-[11px] text-muted-foreground">{t("av.guide.note")}</p>
      </Card>

      <Card className="animate-rise" style={rise(4)}>
        <CardTitle>
          <ShieldQuestionMark className="size-4" />
          {t("av.smartscreen.title")}
        </CardTitle>
        <p className="max-w-3xl text-xs leading-relaxed text-muted-foreground">
          {t("av.smartscreen.body")}
        </p>
      </Card>

      <Modal
        open={confirm}
        onClose={() => setConfirm(false)}
        title={t("av.confirm.title")}
        closeLabel={t("common.cancel")}
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirm(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="primary"
              disabled={working}
              onClick={() => void act(backend.avExclude, "av.defender.done")}
            >
              {t("av.confirm.go")}
            </Button>
          </>
        }
      >
        <div className="space-y-2">
          <p>{t("av.confirm.body")}</p>
          <p className="num break-all rounded-md border border-border bg-background/60 px-2.5 py-1.5 text-[11px]">
            {s?.exe}
          </p>
          <p className="text-xs text-muted-foreground">{t("av.confirm.scope")}</p>
        </div>
      </Modal>
    </>
  );
}

function ProductRow({ p, reserve }: { p: AvProduct; reserve: boolean }) {
  const { t } = useI18n();
  const Icon = p.scanner === "on" ? (p.up_to_date ? ShieldCheck : ShieldAlert) : ShieldOff;
  return (
    <li className="flex items-center gap-3 py-2.5">
      <span
        className={cn(
          "flex size-8 shrink-0 items-center justify-center rounded-md",
          p.scanner === "on"
            ? p.up_to_date
              ? "bg-success/10 text-success"
              : "bg-warning/10 text-warning"
            : "bg-foreground/5 text-muted-foreground",
        )}
      >
        <Icon className="size-4" />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-sm font-medium">{p.name}</span>
        {reserve ? (
          <span className="block text-[11px] text-muted-foreground">
            {t("av.defender.reserve")}
          </span>
        ) : null}
      </span>
      <Badge tone={SCANNER_TONE[p.scanner]}>{t(`av.scanner.${p.scanner}` as const)}</Badge>
      {p.scanner === "on" ? (
        <Badge tone={p.up_to_date ? "neutral" : "warning"}>
          {t(p.up_to_date ? "av.defs.fresh" : "av.defs.old")}
        </Badge>
      ) : null}
    </li>
  );
}

function ExePath({ exe }: { exe: string }) {
  const { t } = useI18n();
  return (
    <p className="mt-3 rounded-md border border-border bg-background/50 px-2.5 py-1.5 text-[11px]">
      <span className="font-semibold text-foreground/70">{t("av.defender.file")}: </span>
      <span className="num break-all text-muted-foreground">{exe}</span>
    </p>
  );
}

function CheckLine({ ok, text }: { ok: boolean; text: string }) {
  return (
    <li className="flex items-center gap-2">
      <span
        aria-hidden
        className={cn(
          "inline-flex size-4 items-center justify-center rounded-full",
          ok ? "bg-success text-background" : "bg-destructive text-background",
        )}
      >
        {ok ? (
          <Check className="size-3" strokeWidth={3} />
        ) : (
          <X className="size-3" strokeWidth={3} />
        )}
      </span>
      <span className={ok ? "text-foreground/85" : "text-muted-foreground"}>{text}</span>
    </li>
  );
}
