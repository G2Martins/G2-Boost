import { Gamepad2, X } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/primitives";
import { useI18n } from "@/lib/i18n";

/** Editable list of game executables (`name.exe`). The core sanitizes again on save. */
export function GameExesEditor({
  exes,
  disabled,
  onChange,
}: {
  exes: string[];
  disabled?: boolean;
  onChange: (exes: string[]) => void;
}) {
  const { t } = useI18n();
  const [draft, setDraft] = useState("");
  const add = () => {
    const v = draft.trim().toLowerCase();
    if (!v) return;
    onChange([...exes.filter((e) => e !== v), v]);
    setDraft("");
  };
  return (
    <div className="flex flex-col gap-3">
      <form
        className="flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          add();
        }}
      >
        <input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder={t("settings.game.placeholder")}
          disabled={disabled}
          maxLength={64}
          className="h-9 min-w-0 flex-1 rounded-md border border-border bg-background px-3 font-mono text-sm outline-none placeholder:text-muted-foreground/70 focus:border-ring disabled:opacity-50"
        />
        <Button type="submit" variant="outline" size="sm" disabled={disabled || !draft.trim()}>
          + {t("settings.game.add")}
        </Button>
      </form>
      {exes.length === 0 ? (
        <p className="text-xs text-muted-foreground">{t("settings.game.exes.empty")}</p>
      ) : (
        <ul className="flex flex-wrap gap-2">
          {exes.map((e) => (
            <li
              key={e}
              className="inline-flex items-center gap-2 rounded-md border border-border bg-background px-2.5 py-1 font-mono text-xs"
            >
              <Gamepad2 className="size-3.5 text-muted-foreground" />
              {e}
              <button
                type="button"
                aria-label={t("settings.game.remove", { name: e })}
                disabled={disabled}
                onClick={() => onChange(exes.filter((x) => x !== e))}
                className="text-muted-foreground hover:text-destructive"
              >
                <X className="size-3.5" />
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
