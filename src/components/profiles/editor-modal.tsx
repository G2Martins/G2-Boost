import { Search } from "lucide-react";
import { useMemo, useState } from "react";
import { Badge, Button, Modal, selectClass } from "@/components/ui/primitives";
import { toast } from "@/components/ui/toast";
import { categoryKey, TWEAKS, tweakDesc, tweakName } from "@/data/catalogs";
import { backend, type FxLevel, type TweakProfile } from "@/lib/backend";
import type { StateMap } from "@/lib/boost-hooks";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { currentState, inputClass, profileErrorKey } from "./shared";

export type EditorMode = { kind: "create" } | { kind: "edit"; profile: TweakProfile };

const LEVELS = ["minimal", "balanced", "performance"] as const;
const plain = (s: string) => s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

/** Create or edit a profile: a name and the tweaks, grouped by category, with search. Keyed by the caller per opening. */
export function EditorModal({
  mode,
  open,
  states,
  onClose,
  onSaved,
}: {
  mode: EditorMode;
  open: boolean;
  states: StateMap | null;
  onClose: () => void;
  onSaved: (list: TweakProfile[]) => void;
}) {
  const { t } = useI18n();
  const editing = mode.kind === "edit" ? mode.profile : null;
  const [name, setName] = useState(editing?.name ?? "");
  const [picked, setPicked] = useState<Set<string>>(
    () => new Set(editing ? editing.tweaks : states ? currentState(states).tweaks : []),
  );
  const [fx, setFx] = useState<FxLevel>(
    editing?.fx_level ?? (states ? currentState(states).fx_level : null) ?? "balanced",
  );
  const [start, setStart] = useState<"current" | "empty">(states && !editing ? "current" : "empty");
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const groups = useMemo(() => {
    const q = plain(query.trim());
    const rows = TWEAKS.map((tw) => ({
      tw,
      label: t(tweakName(tw.id)),
      desc: t(tweakDesc(tw.id)),
      cat: states?.[tw.id]?.category ?? null,
    })).filter((r) => !q || plain(r.label).includes(q) || r.tw.id.includes(q));
    const order: (string | null)[] = [];
    for (const r of rows) if (!order.includes(r.cat)) order.push(r.cat);
    return order.map((cat) => ({ cat, rows: rows.filter((r) => r.cat === cat) }));
  }, [query, states, t]);

  const known = new Set<string>(TWEAKS.map((x) => x.id));
  const hidden = [...picked].filter((id) => !known.has(id)).length;

  const toggle = (id: string) =>
    setPicked((s) => {
      const next = new Set(s);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  const begin = (from: "current" | "empty") => {
    setStart(from);
    setPicked(new Set(from === "current" && states ? currentState(states).tweaks : []));
    if (from === "current" && states) setFx(currentState(states).fx_level ?? "balanced");
  };

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      const tweaks = [
        ...TWEAKS.map((x) => x.id).filter((id) => picked.has(id)),
        ...[...picked].filter((id) => !known.has(id)),
      ];
      const list = await backend.profilesSave(
        { name, tweaks, fx_level: picked.has("visual_fx") ? fx : null },
        editing?.name ?? null,
      );
      toast(t("profiles.editor.saved"), { tone: "success" });
      onSaved(list);
    } catch (e) {
      const key = profileErrorKey(e);
      setError(key ? t(key) : e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      open={open}
      onClose={() => !saving && onClose()}
      closeLabel={t("common.close")}
      title={t(editing ? "profiles.editor.edit" : "profiles.editor.create")}
      footer={
        <>
          <span className="mr-auto self-center text-xs text-muted-foreground">
            {t("profiles.editor.selected", { count: picked.size })}
          </span>
          <Button variant="ghost" disabled={saving} onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button variant="primary" disabled={saving || name.trim() === ""} onClick={save}>
            {t("profiles.editor.save")}
          </Button>
        </>
      }
    >
      <div className="flex w-[min(520px,80vw)] flex-col gap-3">
        <div className="flex flex-col gap-1.5">
          <label htmlFor="profile-name" className="text-xs font-semibold text-muted-foreground">
            {t("profiles.editor.name")}
          </label>
          <input
            id="profile-name"
            type="text"
            maxLength={40}
            value={name}
            autoComplete="off"
            placeholder={t("profiles.editor.name.placeholder")}
            onChange={(e) => setName(e.target.value)}
            className={cn(inputClass, "w-full")}
          />
        </div>
        {!editing && states ? (
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-xs font-semibold text-muted-foreground">
              {t("profiles.editor.start")}
            </span>
            {(["current", "empty"] as const).map((from) => (
              <button
                key={from}
                type="button"
                onClick={() => begin(from)}
                className={cn(
                  "press rounded-md border px-2.5 py-1.5 text-xs font-semibold transition-colors duration-200 ease-out-expo",
                  start === from
                    ? "border-border-strong bg-foreground/8 text-foreground"
                    : "border-border text-muted-foreground hover:text-foreground",
                )}
              >
                {t(`profiles.editor.start.${from}` as I18nKey)}
              </button>
            ))}
          </div>
        ) : null}
        <div className="relative">
          <Search className="pointer-events-none absolute top-2.5 left-3 size-4 text-muted-foreground" />
          <input
            type="search"
            value={query}
            placeholder={t("profiles.editor.search")}
            aria-label={t("profiles.editor.search")}
            onChange={(e) => setQuery(e.target.value)}
            className={cn(inputClass, "w-full pl-9")}
          />
        </div>
        <div className="flex max-h-[42vh] flex-col gap-3 overflow-auto pr-1">
          {groups.length === 0 || groups.every((g) => g.rows.length === 0) ? (
            <p className="py-6 text-center text-xs text-muted-foreground">
              {t("profiles.editor.none_found")}
            </p>
          ) : null}
          {groups.map((g) => (
            <section key={g.cat ?? "all"} className="flex flex-col gap-0.5">
              <h4 className="px-2 pb-1 text-[10px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
                {g.cat ? t(categoryKey(g.cat)) : t("profiles.editor.group.all")}
              </h4>
              {g.rows.map(({ tw, label, desc }) => (
                <div key={tw.id} className="rounded-md hover:bg-foreground/5">
                  <label className="flex cursor-pointer items-start gap-3 px-2 py-2">
                    <input
                      type="checkbox"
                      className="mt-0.5 size-4 shrink-0 accent-primary"
                      checked={picked.has(tw.id)}
                      onChange={() => toggle(tw.id)}
                    />
                    <span className="min-w-0 flex-1">
                      <span className="flex flex-wrap items-center gap-1.5 text-[13px] font-semibold">
                        {label}
                        {tw.admin ? (
                          <Badge tone="neutral">{t("common.admin_required")}</Badge>
                        ) : null}
                        {tw.beta ? <Badge tone="accent">{t("common.beta")}</Badge> : null}
                      </span>
                      <span className="line-clamp-1 text-xs text-muted-foreground" title={desc}>
                        {desc}
                      </span>
                    </span>
                  </label>
                  {tw.id === "visual_fx" && picked.has("visual_fx") ? (
                    <div className="flex items-center gap-2 px-9 pb-2">
                      <label htmlFor="profile-fx" className="text-xs text-muted-foreground">
                        {t("profiles.editor.fx")}
                      </label>
                      <select
                        id="profile-fx"
                        value={fx}
                        onChange={(e) => setFx(e.target.value as FxLevel)}
                        className={cn(selectClass, "h-8 text-xs")}
                      >
                        {LEVELS.map((lv) => (
                          <option key={lv} value={lv}>
                            {t(`boost.fx.level.${lv}` as const)}
                          </option>
                        ))}
                      </select>
                    </div>
                  ) : null}
                </div>
              ))}
            </section>
          ))}
        </div>
        {hidden > 0 ? (
          <p className="text-xs text-muted-foreground">
            {t("profiles.card.missing", { count: hidden })}
          </p>
        ) : null}
        {error ? <p className="text-xs text-destructive">{error}</p> : null}
      </div>
    </Modal>
  );
}
