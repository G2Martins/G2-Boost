import { Copy, FileJson } from "lucide-react";
import { useEffect, useState } from "react";
import { Button, Modal, Skeleton } from "@/components/ui/primitives";
import { toast } from "@/components/ui/toast";
import { backend, type ProfileExport, type ProfileFile, type TweakProfile } from "@/lib/backend";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { inputClass, profileErrorKey } from "./shared";

/** The exported text, where the file went, and a copy button (the clipboard is a click away). */
export function ExportModal({
  data,
  open,
  onClose,
}: {
  data: ProfileExport;
  open: boolean;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(data.json);
      toast(t("profiles.export.copied"), { tone: "success" });
    } catch {
      toast(t("profiles.export.copy_failed"), { tone: "warning" });
    }
  };
  return (
    <Modal
      open={open}
      onClose={onClose}
      closeLabel={t("common.close")}
      title={t("profiles.export.title")}
      footer={
        <>
          <Button variant="outline" onClick={copy}>
            <Copy className="size-4" />
            {t("profiles.export.copy")}
          </Button>
          <Button variant="secondary" onClick={onClose}>
            {t("common.close")}
          </Button>
        </>
      }
    >
      <div className="flex w-[min(520px,80vw)] flex-col gap-3">
        {data.path ? (
          <p className="text-xs text-muted-foreground">
            {t("profiles.export.saved")}{" "}
            <span className="num break-all text-foreground">{data.path}</span>
          </p>
        ) : (
          <p className="text-xs text-warning">{t("profiles.export.unsaved")}</p>
        )}
        <textarea
          readOnly
          rows={8}
          value={data.json}
          aria-label={t("profiles.export.title")}
          className={cn(
            inputClass,
            "num h-auto w-full resize-none py-2 text-[11px] leading-relaxed",
          )}
        />
        <p className="text-xs text-muted-foreground">{t("profiles.export.hint")}</p>
      </div>
    </Modal>
  );
}

/** Pick a file from the Profiles folder, or paste the text. Everything is validated in the core. */
export function ImportModal({
  open,
  onClose,
  onImported,
}: {
  open: boolean;
  onClose: () => void;
  onImported: (list: TweakProfile[], name: string) => void;
}) {
  const { t } = useI18n();
  const [files, setFiles] = useState<ProfileFile[] | null>(null);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    let alive = true;
    setFiles(null);
    setError(null);
    backend.profilesFiles().then((f) => alive && setFiles(f ?? []));
    return () => {
      alive = false;
    };
  }, [open]);

  const run = async (
    source: { kind: "file"; file: string } | { kind: "text"; text: string },
    label?: string,
  ) => {
    setBusy(true);
    setError(null);
    try {
      const before = new Set((await backend.profilesList())?.map((p) => p.name));
      const list = await backend.profilesImport(source);
      const added = list.find((p) => !before.has(p.name))?.name ?? label ?? "";
      setText("");
      onImported(list, added);
    } catch (e) {
      const key = profileErrorKey(e);
      setError(key ? t(key) : e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={open}
      onClose={() => !busy && onClose()}
      closeLabel={t("common.close")}
      title={t("profiles.import.title")}
      footer={
        <Button variant="ghost" disabled={busy} onClick={onClose}>
          {t("common.cancel")}
        </Button>
      }
    >
      <div className="flex w-[min(520px,80vw)] flex-col gap-4">
        <section className="flex flex-col gap-2">
          <h3 className="text-xs font-semibold text-muted-foreground">
            {t("profiles.import.files")}
          </h3>
          {files === null ? (
            <Skeleton className="h-9 w-full" />
          ) : files.length === 0 ? (
            <p className="text-xs text-muted-foreground">{t("profiles.import.files.empty")}</p>
          ) : (
            <ul className="flex max-h-40 flex-col gap-1 overflow-auto">
              {files.map((f) => (
                <li
                  key={f.file}
                  className="flex items-center gap-3 rounded-md border border-border px-3 py-2"
                >
                  <FileJson className="size-4 shrink-0 text-muted-foreground" />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[13px] font-semibold">{f.name}</span>
                    <span className="num block text-[11px] text-muted-foreground">
                      {t("profiles.import.tweaks", { count: f.tweaks })}
                    </span>
                  </span>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={busy}
                    onClick={() => run({ kind: "file", file: f.file }, f.name)}
                  >
                    {t("profiles.import.go")}
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </section>
        <section className="flex flex-col gap-2">
          <label htmlFor="profile-paste" className="text-xs font-semibold text-muted-foreground">
            {t("profiles.import.paste")}
          </label>
          <textarea
            id="profile-paste"
            rows={5}
            value={text}
            spellCheck={false}
            placeholder={t("profiles.import.paste.placeholder")}
            onChange={(e) => setText(e.target.value)}
            className={cn(
              inputClass,
              "num h-auto w-full resize-none py-2 text-[11px] leading-relaxed",
            )}
          />
          <div className="flex justify-end">
            <Button
              size="sm"
              variant="secondary"
              disabled={busy || text.trim() === ""}
              onClick={() => run({ kind: "text", text })}
            >
              {t("profiles.import.go")}
            </Button>
          </div>
        </section>
        {error ? <p className="text-xs text-destructive">{error}</p> : null}
      </div>
    </Modal>
  );
}
