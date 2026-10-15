import { ShieldCheck } from "lucide-react";
import { useCallback, useState } from "react";
import { Button } from "@/components/ui/primitives";
import { backend } from "@/lib/backend";
import { useI18n } from "@/lib/i18n";

/** Error codes from the Rust core → i18n. */
export function useErrorText() {
  const { t } = useI18n();
  return useCallback(
    (e: unknown) => {
      const msg = e instanceof Error ? e.message : String(e);
      if (msg === "needs_admin") return t("boost.error.needs_admin");
      if (msg === "cancelled") return t("boost.error.cancelled");
      if (msg === "bad_token") return t("cleanup.error.bad_token");
      return t("boost.error.generic", { error: msg });
    },
    [t],
  );
}

/** Relaunch the app elevated (UAC) on `route`; this instance exits on success. */
export function ElevateButton({ route, size = "sm" }: { route: string; size?: "sm" | "md" }) {
  const { t } = useI18n();
  const errorText = useErrorText();
  const [err, setErr] = useState<string | null>(null);
  return (
    <span className="inline-flex items-center gap-2">
      <Button
        variant="outline"
        size={size}
        onClick={() => backend.relaunchElevated(route).catch((e) => setErr(errorText(e)))}
      >
        <ShieldCheck className="size-4" />
        {t("boost.elevate")}
      </Button>
      {err ? <span className="text-xs text-destructive">{err}</span> : null}
    </span>
  );
}
