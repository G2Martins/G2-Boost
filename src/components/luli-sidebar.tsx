import { FolderPlus, MessageSquare, MoreHorizontal, Plus, Search } from "lucide-react";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { Card } from "@/components/ui/primitives";
import type { LuliSummary } from "@/lib/backend";
import { useI18n } from "@/lib/i18n";
import { useLuli } from "@/lib/luli";
import { cn } from "@/lib/utils";

/** A small menu anchored to its trigger; closes on outside click or Escape. */
function Menu({
  label,
  children,
  onClose,
}: {
  label: string;
  children: (close: () => void) => ReactNode;
  onClose?: () => void;
}) {
  const [open, setOpen] = useState(false);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    if (!open) closeRef.current?.();
  }, [open]);
  const ref = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (!open) return;
    const down = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    const key = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", down);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("mousedown", down);
      document.removeEventListener("keydown", key);
    };
  }, [open]);
  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        aria-label={label}
        aria-expanded={open}
        onClick={(e) => {
          e.stopPropagation();
          setOpen((v) => !v);
        }}
        className="press flex size-6 items-center justify-center rounded text-muted-foreground opacity-0 transition-opacity hover:bg-foreground/8 hover:text-foreground focus-visible:opacity-100 group-hover:opacity-100 aria-expanded:opacity-100"
      >
        <MoreHorizontal className="size-4" />
      </button>
      {open ? (
        <div className="absolute right-0 top-full z-30 mt-1 min-w-44 rounded-md border border-border-strong bg-surface p-1 shadow-lg">
          {children(() => setOpen(false))}
        </div>
      ) : null}
    </div>
  );
}

function MenuItem({
  children,
  onClick,
  danger,
}: {
  children: ReactNode;
  onClick: () => void;
  danger?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "block w-full rounded px-2.5 py-1.5 text-left text-[12px] hover:bg-foreground/6",
        danger ? "text-destructive" : "text-foreground",
      )}
    >
      {children}
    </button>
  );
}

/** Inline text edit (rename): Enter saves, Escape cancels, leaving the field saves. */
function InlineEdit({
  value,
  onDone,
  placeholder,
}: {
  value: string;
  onDone: (v: string | null) => void;
  placeholder?: string;
}) {
  const [v, setV] = useState(value);
  const done = useRef(false);
  const finish = (save: boolean) => {
    if (done.current) return;
    done.current = true;
    onDone(save && v.trim() ? v.trim() : null);
  };
  return (
    <input
      // biome-ignore lint/a11y/noAutofocus: the user just asked to rename this item
      autoFocus
      value={v}
      maxLength={120}
      placeholder={placeholder}
      onChange={(e) => setV(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter") finish(true);
        if (e.key === "Escape") finish(false);
      }}
      onBlur={() => finish(true)}
      className="h-7 w-full min-w-0 rounded border border-ring bg-background px-2 text-[12px] outline-none"
    />
  );
}

function ConversationRow({ c, active }: { c: LuliSummary; active: boolean }) {
  const { t } = useI18n();
  const luli = useLuli();
  const [editing, setEditing] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const targets = luli.history.groups.filter((g) => g.id !== c.group);
  if (editing) {
    return (
      <li className="px-1 py-0.5">
        <InlineEdit
          value={c.title}
          onDone={(v) => {
            setEditing(false);
            if (v && v !== c.title) void luli.renameConversation(c.id, v);
          }}
        />
      </li>
    );
  }
  return (
    <li
      className={cn(
        "group flex items-center gap-2 rounded-md pr-1 transition-colors",
        active
          ? "bg-foreground/8 text-foreground"
          : "text-muted-foreground hover:bg-foreground/4 hover:text-foreground",
      )}
    >
      <button
        type="button"
        onClick={() => luli.openConversation(c.id)}
        aria-current={active ? "true" : undefined}
        className="flex min-w-0 flex-1 items-center gap-2 py-1.5 pl-2 text-left text-[13px]"
        title={c.title}
      >
        <MessageSquare
          className={cn("size-3.5 shrink-0", active ? "text-primary" : "text-muted-foreground/60")}
        />
        <span className="truncate">{c.title}</span>
      </button>
      <Menu label={t("luli.menu.options")} onClose={() => setConfirmDelete(false)}>
        {(close) => (
          <>
            <MenuItem
              onClick={() => {
                close();
                setEditing(true);
              }}
            >
              {t("luli.menu.rename")}
            </MenuItem>
            {targets.length > 0 || c.group ? (
              <p className="px-2.5 pt-1.5 pb-0.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                {t("luli.menu.move")}
              </p>
            ) : null}
            {targets.map((g) => (
              <MenuItem
                key={g.id}
                onClick={() => {
                  close();
                  void luli.moveConversation(c.id, g.id);
                }}
              >
                {g.name}
              </MenuItem>
            ))}
            {c.group ? (
              <MenuItem
                onClick={() => {
                  close();
                  void luli.moveConversation(c.id, null);
                }}
              >
                {t("luli.history.ungrouped")}
              </MenuItem>
            ) : null}
            <div className="my-1 border-t border-border" />
            <MenuItem
              danger
              onClick={() => {
                if (!confirmDelete) {
                  setConfirmDelete(true);
                  return;
                }
                close();
                void luli.deleteConversation(c.id);
              }}
            >
              {confirmDelete ? t("luli.menu.delete_confirm") : t("luli.menu.delete")}
            </MenuItem>
          </>
        )}
      </Menu>
    </li>
  );
}

/** Luli's conversations, like Claude's sidebar: new chat, search, named groups, "no group". */
export function LuliSidebar() {
  const { t } = useI18n();
  const luli = useLuli();
  const [query, setQuery] = useState("");
  const [newGroup, setNewGroup] = useState(false);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [confirmClear, setConfirmClear] = useState(false);
  const { groups, conversations } = luli.history;

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? conversations.filter((c) => c.title.toLowerCase().includes(q)) : conversations;
  }, [conversations, query]);
  const ungrouped = visible.filter((c) => !c.group || !groups.some((g) => g.id === c.group));

  return (
    <Card className="animate-rise flex flex-col gap-3 p-3 lg:sticky lg:top-4 lg:max-h-[calc(100vh-13rem)] lg:self-start">
      <button
        type="button"
        onClick={luli.newConversation}
        className="press flex items-center gap-2 rounded-md bg-foreground/6 px-2.5 py-2 text-[13px] font-semibold hover:bg-foreground/10"
      >
        <Plus className="size-4" />
        {t("ai.new_chat")}
      </button>
      <label className="flex items-center gap-2 rounded-md border border-border bg-background/60 px-2.5">
        <Search className="size-3.5 text-muted-foreground" />
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("luli.history.search")}
          aria-label={t("luli.history.search")}
          className="h-8 min-w-0 flex-1 bg-transparent text-[12px] outline-none placeholder:text-muted-foreground/70"
        />
      </label>

      <div className="-mx-1 min-h-0 flex-1 space-y-3 overflow-y-auto px-1">
        <div className="flex items-center justify-between px-1">
          <span className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
            {t("luli.history.groups")}
          </span>
          <button
            type="button"
            onClick={() => setNewGroup(true)}
            aria-label={t("luli.history.new_group")}
            title={t("luli.history.new_group")}
            className="press rounded p-1 text-muted-foreground hover:bg-foreground/8 hover:text-foreground"
          >
            <FolderPlus className="size-3.5" />
          </button>
        </div>
        {newGroup ? (
          <InlineEdit
            value=""
            placeholder={t("luli.group.name_placeholder")}
            onDone={(v) => {
              setNewGroup(false);
              if (v) void luli.createGroup(v);
            }}
          />
        ) : null}
        {groups.map((g) => {
          const items = visible.filter((c) => c.group === g.id);
          if (query && items.length === 0) return null;
          return (
            <section key={g.id}>
              <div className="group flex items-center gap-1 px-1">
                {renaming === g.id ? (
                  <InlineEdit
                    value={g.name}
                    onDone={(v) => {
                      setRenaming(null);
                      if (v && v !== g.name) void luli.renameGroup(g.id, v);
                    }}
                  />
                ) : (
                  <span className="min-w-0 flex-1 truncate text-[12px] font-semibold text-foreground/90">
                    {g.name}
                  </span>
                )}
                <Menu label={t("luli.menu.options")}>
                  {(close) => (
                    <>
                      <MenuItem
                        onClick={() => {
                          close();
                          setRenaming(g.id);
                        }}
                      >
                        {t("luli.menu.rename")}
                      </MenuItem>
                      <MenuItem
                        danger
                        onClick={() => {
                          close();
                          void luli.deleteGroup(g.id);
                        }}
                      >
                        {t("luli.group.delete")}
                      </MenuItem>
                    </>
                  )}
                </Menu>
              </div>
              <ul className="mt-0.5 space-y-0.5">
                {items.map((c) => (
                  <ConversationRow key={c.id} c={c} active={c.id === luli.convId} />
                ))}
                {items.length === 0 ? (
                  <li className="px-2 py-1 text-[11px] text-muted-foreground/70">
                    {t("luli.group.empty")}
                  </li>
                ) : null}
              </ul>
            </section>
          );
        })}
        <section>
          <p className="px-1 text-[12px] font-semibold text-foreground/90">
            {t("luli.history.ungrouped")}
          </p>
          <ul className="mt-0.5 space-y-0.5">
            {ungrouped.map((c) => (
              <ConversationRow key={c.id} c={c} active={c.id === luli.convId} />
            ))}
          </ul>
          {conversations.length === 0 ? (
            <p className="px-2 py-2 text-[11px] leading-relaxed text-muted-foreground">
              {t("luli.history.empty")}
            </p>
          ) : visible.length === 0 ? (
            <p className="px-2 py-2 text-[11px] text-muted-foreground">
              {t("luli.history.no_match")}
            </p>
          ) : null}
        </section>
      </div>

      <div className="flex items-center justify-between border-t border-border pt-2 text-[10px] text-muted-foreground">
        <span>{t("luli.history.local")}</span>
        {conversations.length > 0 ? (
          <button
            type="button"
            onClick={() => {
              if (!confirmClear) {
                setConfirmClear(true);
                return;
              }
              setConfirmClear(false);
              void luli.clearHistory();
            }}
            onBlur={() => setConfirmClear(false)}
            className={cn("hover:text-destructive", confirmClear && "text-destructive")}
          >
            {confirmClear ? t("luli.history.clear_confirm") : t("luli.history.clear")}
          </button>
        ) : null}
      </div>
    </Card>
  );
}

/** What Luli remembers about the user and this PC: view, add, forget. */
export function LuliMemoryCard() {
  const { t } = useI18n();
  const luli = useLuli();
  const [text, setText] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const add = async () => {
    const v = text.trim();
    if (!v) return;
    try {
      await luli.remember(v);
      setText("");
      setErr(null);
    } catch {
      setErr(t("luli.memory.full"));
    }
  };
  return (
    <Card className="animate-rise">
      <p className="mb-1 flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
        {t("luli.memory.title")}
      </p>
      <p className="mb-3 text-[11px] leading-relaxed text-muted-foreground">
        {t("luli.memory.help")}
      </p>
      {luli.memory.length === 0 ? (
        <p className="mb-3 text-[12px] text-muted-foreground">{t("luli.memory.empty")}</p>
      ) : (
        <ul className="mb-3 space-y-1">
          {luli.memory.map((m) => (
            <li key={m.id} className="group flex items-start gap-2 text-[12px]">
              <span className="mt-1.5 size-1.5 shrink-0 rounded-full bg-primary/70" aria-hidden />
              <span className="min-w-0 flex-1">{m.text}</span>
              <button
                type="button"
                onClick={() => luli.forget(m.id)}
                className="text-[11px] text-muted-foreground opacity-0 hover:text-destructive focus-visible:opacity-100 group-hover:opacity-100"
              >
                {t("luli.memory.remove")}
              </button>
            </li>
          ))}
        </ul>
      )}
      <form
        className="flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          void add();
        }}
      >
        <input
          value={text}
          onChange={(e) => setText(e.target.value)}
          maxLength={300}
          placeholder={t("luli.memory.add_placeholder")}
          className="h-8 min-w-0 flex-1 rounded-md border border-border bg-background px-2.5 text-[12px] outline-none focus:border-ring"
        />
        <button
          type="submit"
          disabled={!text.trim()}
          className="press rounded-md border border-border px-3 text-[12px] hover:border-primary/50 disabled:opacity-40"
        >
          {t("luli.memory.add")}
        </button>
      </form>
      {err ? <p className="mt-2 text-[11px] text-destructive">{err}</p> : null}
    </Card>
  );
}

/** The user's own instructions, sent with every question (tone, depth, format). */
export function LuliInstructionsCard() {
  const { t } = useI18n();
  const luli = useLuli();
  const [text, setText] = useState(luli.instructions);
  const [saved, setSaved] = useState(false);
  useEffect(() => setText(luli.instructions), [luli.instructions]);
  const dirty = text.trim() !== luli.instructions.trim();
  return (
    <Card className="animate-rise">
      <p className="mb-1 font-display text-[15px] font-semibold tracking-wide">
        {t("luli.instructions.title")}
      </p>
      <p className="mb-3 text-[11px] leading-relaxed text-muted-foreground">
        {t("luli.instructions.help")}
      </p>
      <textarea
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          setSaved(false);
        }}
        rows={4}
        maxLength={1500}
        placeholder={t("luli.instructions.placeholder")}
        className="w-full resize-y rounded-md border border-border bg-background px-2.5 py-2 text-[12px] outline-none focus:border-ring"
      />
      <div className="mt-2 flex items-center justify-end gap-3">
        {saved ? (
          <span className="text-[11px] text-success">{t("luli.instructions.saved")}</span>
        ) : null}
        <button
          type="button"
          disabled={!dirty}
          onClick={async () => {
            await luli.saveInstructions(text);
            setSaved(true);
          }}
          className="press rounded-md border border-border px-3 py-1.5 text-[12px] hover:border-primary/50 disabled:opacity-40"
        >
          {t("luli.instructions.save")}
        </button>
      </div>
    </Card>
  );
}
