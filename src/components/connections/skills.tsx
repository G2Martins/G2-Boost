import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  BookOpen,
  ClipboardPaste,
  Eye,
  FileText,
  FolderOpen,
  ShieldAlert,
  Trash2,
  Upload,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import {
  looksLikeZip,
  MAX_ZIP,
  SkillZipError,
  skillsFromZip,
} from "@/components/connections/skill-zip";
import { useErrorText } from "@/components/elevate-button";
import { rise } from "@/components/shell/page-header";
import {
  Button,
  Card,
  CardTitle,
  EmptyState,
  Meter,
  Modal,
  Switch,
} from "@/components/ui/primitives";
import { SKILLS_BUDGET, skillsBlock } from "@/lib/ai";
import { backend, inTauri, type LuliSkill } from "@/lib/backend";
import { useI18n } from "@/lib/i18n";
import { useLuli } from "@/lib/luli";
import { cn } from "@/lib/utils";

/** Error codes from the core (`skills.rs`) and the zip reader that have their own text. */
const CODES = [
  "no_frontmatter",
  "bad_frontmatter",
  "missing_name",
  "missing_description",
  "invalid_name",
  "reserved_name",
  "invalid_description",
  "empty_body",
  "too_large",
  "control_chars",
  "duplicate",
  "too_many",
  "not_zip",
  "unsupported",
  "encrypted",
  "corrupt",
  "no_skill",
] as const;
type Code = (typeof CODES)[number];

/**
 * The obvious mistakes, caught before asking the core (which still decides): a pasted text with
 * no frontmatter, or one without `name:`/`description:`.
 */
export function quickCheck(text: string): Code | null {
  if (text.length > 24 * 1024) return "too_large";
  const m = text
    .replace(/^﻿/, "")
    .replace(/\r\n?/g, "\n")
    .match(/^\s*---\n([\s\S]*?)\n---[ \t]*(\n|$)/);
  if (!m) return "no_frontmatter";
  if (!/^name:/m.test(m[1] ?? "")) return "missing_name";
  if (!/^description:/m.test(m[1] ?? "")) return "missing_description";
  return null;
}

type Result = { added: number; errors: string[] };

/** Conexões › Skills: the user's SKILL.md files, off until read and turned on. */
export function SkillsTab({
  dropped,
  onDropHandled,
}: {
  dropped: ArrayBuffer | null;
  onDropHandled: () => void;
}) {
  const { t } = useI18n();
  const errorText = useErrorText();
  const luli = useLuli();
  const skills = luli.skills;
  const [result, setResult] = useState<Result | null>(null);
  const [paste, setPaste] = useState(false);
  const [view, setView] = useState<{ skill: LuliSkill; enabling: boolean } | null>(null);
  const [removing, setRemoving] = useState<LuliSkill | null>(null);
  const [dragging, setDragging] = useState(false);
  const fileRef = useRef<HTMLInputElement>(null);

  const message = (e: unknown): string => {
    const code = e instanceof SkillZipError ? e.code : e instanceof Error ? e.message : String(e);
    return (CODES as readonly string[]).includes(code)
      ? t(`conn.skills.err.${code as Code}` as const)
      : errorText(e);
  };

  /** Add each SKILL.md; one bad skill does not stop the others. */
  const addTexts = async (texts: string[]): Promise<Result> => {
    const r: Result = { added: 0, errors: [] };
    for (const text of texts) {
      const quick = quickCheck(text);
      if (quick) {
        r.errors.push(t(`conn.skills.err.${quick}` as const));
        continue;
      }
      try {
        luli.setSkills(await backend.skillsAdd(text));
        r.added++;
      } catch (e) {
        r.errors.push(message(e));
      }
    }
    setResult(r);
    return r;
  };

  const importBytes = async (buf: ArrayBuffer) => {
    try {
      const texts = looksLikeZip(buf) ? await skillsFromZip(buf) : [new TextDecoder().decode(buf)];
      await addTexts(texts);
    } catch (e) {
      setResult({ added: 0, errors: [message(e)] });
    }
  };

  // A file dropped on the window (the page took its bytes from the core).
  // biome-ignore lint/correctness/useExhaustiveDependencies: run once per dropped file
  useEffect(() => {
    if (!dropped) return;
    onDropHandled();
    void importBytes(dropped);
  }, [dropped]);

  // Light the drop zone while a file is dragged over the window.
  useEffect(() => {
    if (!inTauri) return;
    const off = getCurrentWebview().onDragDropEvent((e) =>
      setDragging(e.payload.type === "enter" || e.payload.type === "over"),
    );
    return () => {
      void off.then((f) => f());
    };
  }, []);

  const toggle = async (s: LuliSkill, on: boolean) => {
    try {
      luli.setSkills(await backend.skillsSet(s.name, on));
    } catch (e) {
      setResult({ added: 0, errors: [message(e)] });
    }
  };

  const remove = async (s: LuliSkill) => {
    setRemoving(null);
    try {
      luli.setSkills(await backend.skillsRemove(s.name));
    } catch (e) {
      setResult({ added: 0, errors: [message(e)] });
    }
  };

  const enabled = skills.filter((s) => s.enabled);
  const block = skillsBlock(enabled);
  const used = new TextEncoder().encode(block).length;
  const listed = (block.match(/<skill name="/g) ?? []).length;
  const partial =
    (block.match(/\(Instructions left out/g) ?? []).length + (enabled.length - listed);

  const actions = (
    <div className="flex flex-wrap gap-2">
      <Button size="sm" variant="primary" onClick={() => setPaste(true)}>
        <ClipboardPaste className="size-4" />
        {t("conn.skills.paste")}
      </Button>
      <Button size="sm" variant="outline" onClick={() => fileRef.current?.click()}>
        <FolderOpen className="size-4" />
        {t("conn.skills.pick")}
      </Button>
    </div>
  );

  return (
    <>
      <input
        ref={fileRef}
        type="file"
        accept=".md,.zip,text/markdown,application/zip"
        className="hidden"
        onChange={async (e) => {
          const f = e.target.files?.[0];
          e.target.value = "";
          if (!f) return;
          if (f.size > MAX_ZIP) setResult({ added: 0, errors: [t("conn.skills.err.too_large")] });
          else await importBytes(await f.arrayBuffer());
        }}
      />

      <Card className="animate-rise">
        <CardTitle help={t("conn.skills.hint")} right={skills.length > 0 ? actions : null}>
          <BookOpen className="size-4" />
          {t("conn.skills.title")}
        </CardTitle>
        <p className="max-w-3xl text-xs leading-relaxed text-muted-foreground">
          {t("conn.skills.body")}
        </p>

        {/* drop zone: the window takes the drop, this only says so (and lights up) */}
        <div
          className={cn(
            "mt-4 flex items-center gap-3 rounded-md border border-dashed px-4 py-3 text-xs transition-colors duration-200",
            dragging
              ? "border-primary/70 bg-primary/8 text-foreground"
              : "border-border-strong text-muted-foreground",
          )}
        >
          <Upload className={cn("size-4 shrink-0", dragging && "text-primary")} />
          <span>{t("conn.skills.drop")}</span>
        </div>

        {result ? (
          <div className="mt-3 space-y-1 text-xs" aria-live="polite">
            {result.added > 0 ? (
              <p className="text-success">
                {t("conn.skills.added", { count: String(result.added) })}
              </p>
            ) : null}
            {result.errors.map((e, i) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: one-off list of messages
              <p key={i} className="text-destructive" data-testid="skill-error">
                {e}
              </p>
            ))}
          </div>
        ) : null}

        {skills.length === 0 ? (
          <EmptyState
            icon={<FileText className="size-5" />}
            title={t("conn.skills.empty.title")}
            body={inTauri ? t("conn.skills.empty.body") : t("conn.app_only")}
            action={actions}
          />
        ) : (
          <>
            <div className="mt-4 flex flex-wrap items-center gap-3 text-[11px] text-muted-foreground">
              <span>
                {t("conn.skills.budget", {
                  on: String(enabled.length),
                  kb: (used / 1024).toFixed(1),
                  max: String(SKILLS_BUDGET / 1024),
                })}
              </span>
              <Meter
                className="w-40"
                value={used / SKILLS_BUDGET}
                tone={partial > 0 ? "warning" : "accent"}
                label={t("conn.skills.budget_label")}
              />
              {partial > 0 ? (
                <span className="text-warning">{t("conn.skills.partial")}</span>
              ) : null}
            </div>
            <ul className="mt-3 grid gap-2 lg:grid-cols-2">
              {skills.map((s, i) => (
                <li
                  key={s.name}
                  className={cn(
                    "animate-rise flex items-start gap-3 rounded-md border bg-background/50 p-3 transition-colors",
                    s.enabled ? "border-border-strong" : "border-border",
                  )}
                  style={rise(i)}
                >
                  <FileText
                    className={cn(
                      "mt-0.5 size-4 shrink-0",
                      s.enabled ? "text-primary" : "text-muted-foreground",
                    )}
                  />
                  <div className="min-w-0 flex-1">
                    <p className="flex flex-wrap items-baseline gap-2">
                      <span className="font-mono text-[13px] font-semibold">{s.name}</span>
                      <span className="num text-[10px] text-muted-foreground">
                        {(new TextEncoder().encode(s.body).length / 1024).toFixed(1)} KB
                      </span>
                    </p>
                    <p className="mt-0.5 line-clamp-2 text-xs text-muted-foreground">
                      {s.description}
                    </p>
                    <div className="mt-1.5 -ml-3 flex gap-1">
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => setView({ skill: s, enabling: false })}
                      >
                        <Eye className="size-3.5" />
                        {t("conn.skills.view")}
                      </Button>
                      <Button size="sm" variant="ghost" onClick={() => setRemoving(s)}>
                        <Trash2 className="size-3.5" />
                        {t("conn.skills.remove")}
                      </Button>
                    </div>
                  </div>
                  <Switch
                    checked={s.enabled}
                    aria-label={t("conn.skills.toggle", { name: s.name })}
                    onCheckedChange={(on) =>
                      on ? setView({ skill: s, enabling: true }) : void toggle(s, false)
                    }
                  />
                </li>
              ))}
            </ul>
          </>
        )}
        <p className="mt-4 flex items-start gap-2 text-[11px] leading-relaxed text-muted-foreground">
          <ShieldAlert className="mt-0.5 size-3.5 shrink-0" />
          {t("conn.skills.safety")}
        </p>
      </Card>

      <PasteModal open={paste} onClose={() => setPaste(false)} add={addTexts} />

      <Modal
        open={view !== null}
        onClose={() => setView(null)}
        closeLabel={t("common.cancel")}
        title={
          view?.enabling
            ? t("conn.skills.enable_title", { name: view.skill.name })
            : (view?.skill.name ?? "")
        }
        footer={
          view?.enabling ? (
            <>
              <Button variant="ghost" onClick={() => setView(null)}>
                {t("common.cancel")}
              </Button>
              <Button
                variant="primary"
                onClick={() => {
                  if (view) void toggle(view.skill, true);
                  setView(null);
                }}
              >
                {t("conn.skills.enable")}
              </Button>
            </>
          ) : (
            <Button variant="ghost" onClick={() => setView(null)}>
              {t("conn.skills.close")}
            </Button>
          )
        }
      >
        {view?.enabling ? (
          <p className="mb-3 flex items-start gap-2 rounded-md border border-warning/35 bg-warning/8 p-3 text-xs">
            <ShieldAlert className="mt-0.5 size-4 shrink-0 text-warning" />
            {t("conn.skills.enable_warning")}
          </p>
        ) : null}
        <p className="text-xs text-muted-foreground">{view?.skill.description}</p>
        <pre className="mt-3 max-h-80 overflow-auto whitespace-pre-wrap wrap-break-word rounded-md border border-border bg-background/70 p-3 font-mono text-[11px] leading-relaxed text-foreground/90">
          {view?.skill.body}
        </pre>
      </Modal>

      <Modal
        open={removing !== null}
        onClose={() => setRemoving(null)}
        closeLabel={t("common.cancel")}
        title={t("conn.skills.remove_title", { name: removing?.name ?? "" })}
        footer={
          <>
            <Button variant="ghost" onClick={() => setRemoving(null)}>
              {t("common.cancel")}
            </Button>
            <Button variant="danger" onClick={() => removing && void remove(removing)}>
              {t("conn.skills.remove")}
            </Button>
          </>
        }
      >
        <p>{t("conn.skills.remove_body")}</p>
      </Modal>
    </>
  );
}

function PasteModal({
  open,
  onClose,
  add,
}: {
  open: boolean;
  onClose: () => void;
  add: (texts: string[]) => Promise<Result>;
}) {
  const { t } = useI18n();
  const [text, setText] = useState("");
  const [errors, setErrors] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const close = () => {
    setErrors([]);
    onClose();
  };
  return (
    <Modal
      open={open}
      onClose={close}
      closeLabel={t("common.cancel")}
      title={t("conn.skills.paste_title")}
      footer={
        <>
          <Button variant="ghost" onClick={close}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            disabled={!text.trim() || busy}
            onClick={async () => {
              setBusy(true);
              const r = await add([text]);
              setBusy(false);
              setErrors(r.errors);
              if (r.added > 0) {
                setText("");
                close();
              }
            }}
          >
            {t("conn.skills.add")}
          </Button>
        </>
      }
    >
      <p className="text-xs text-muted-foreground">{t("conn.skills.paste_body")}</p>
      <textarea
        value={text}
        onChange={(e) => setText(e.target.value)}
        rows={14}
        spellCheck={false}
        aria-label={t("conn.skills.paste_title")}
        placeholder={t("conn.skills.paste_placeholder")}
        className="mt-3 w-full min-w-[min(36rem,80vw)] resize-y rounded-md border border-border bg-background px-3 py-2 font-mono text-[11px] leading-relaxed outline-none focus:border-ring"
      />
      {errors.map((e, i) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: one-off list of messages
        <p key={i} className="mt-2 text-xs text-destructive" role="alert">
          {e}
        </p>
      ))}
    </Modal>
  );
}
