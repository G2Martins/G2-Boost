import { openUrl } from "@tauri-apps/plugin-opener";
import type { ReactNode } from "react";
import { inTauri } from "@/lib/backend";
import { type Inline, type ListBlock, parseBlocks, parseInline } from "@/lib/markdown";

/**
 * Model answers as React text nodes (parser in `lib/markdown.ts`): no HTML from the model ever
 * reaches the DOM. Links open in the system browser, https only.
 */
export function SafeMarkdown({ text }: { text: string }) {
  return (
    <div className="text-sm text-foreground/95">
      {parseBlocks(text).map((b, k) => {
        // Blocks never reorder: the index is a stable key.
        const key = `b${k}`;
        switch (b.kind) {
          case "code":
            return (
              <pre
                key={key}
                className="my-2 overflow-x-auto rounded-md border border-border bg-background/70 p-3 font-mono text-xs"
              >
                {b.text}
              </pre>
            );
          case "table":
            return (
              <div key={key} className="my-2 overflow-x-auto">
                <table className="w-full border-collapse text-xs">
                  {b.head ? (
                    <thead>
                      <tr>
                        {b.head.map((c, ci) => (
                          <th
                            // biome-ignore lint/suspicious/noArrayIndexKey: parsed once per render, cells never reorder
                            key={`h${ci}`}
                            className="border-b border-border px-2 py-1.5 text-left font-semibold"
                          >
                            {inline(c)}
                          </th>
                        ))}
                      </tr>
                    </thead>
                  ) : null}
                  <tbody>
                    {b.rows.map((row, ri) => (
                      // biome-ignore lint/suspicious/noArrayIndexKey: rows never reorder
                      <tr key={`r${ri}`} className="border-b border-border/60">
                        {row.map((c, ci) => (
                          // biome-ignore lint/suspicious/noArrayIndexKey: cells never reorder
                          <td key={`r${ri}c${ci}`} className="num px-2 py-1.5">
                            {inline(c)}
                          </td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            );
          case "heading":
            return (
              <p
                key={key}
                className={
                  b.level <= 2
                    ? "mt-3 font-display text-[15px] font-semibold"
                    : "mt-2 font-display text-[13px] font-semibold"
                }
              >
                {inline(b.text)}
              </p>
            );
          case "list":
            return <List key={key} list={b} />;
          default:
            return (
              <p key={key} className="my-1.5 leading-relaxed">
                {inline(b.text)}
              </p>
            );
        }
      })}
    </div>
  );
}

function List({ list, nested = false }: { list: ListBlock; nested?: boolean }) {
  const items = list.items.map((it, j) => (
    // Items never reorder: the index is a stable key.
    // biome-ignore lint/suspicious/noArrayIndexKey: static parsed list
    <li key={j}>
      {inline(it.text)}
      {it.children ? <List list={it.children} nested /> : null}
    </li>
  ));
  const cls = `${nested ? "mt-1" : "my-1.5"} space-y-1 pl-5`;
  return list.ordered ? (
    <ol start={list.start} className={`${cls} list-decimal`}>
      {items}
    </ol>
  ) : (
    <ul className={`${cls} list-disc`}>{items}</ul>
  );
}

function inline(s: string): ReactNode[] {
  return render(parseInline(s));
}

function render(tokens: Inline[]): ReactNode[] {
  return tokens.map((t, k) => {
    const key = `i${k}`;
    switch (t.kind) {
      case "text":
        return t.text;
      case "bold":
        return <strong key={key}>{render(t.children)}</strong>;
      case "code":
        return (
          <code key={key} className="rounded bg-card-hover px-1 font-mono text-[0.85em]">
            {t.text}
          </code>
        );
      default:
        return <ExternalLink key={key} url={t.url} label={t.label} />;
    }
  });
}

export function ExternalLink({ url, label }: { url: string; label: string }) {
  let host = "";
  try {
    const u = new URL(url);
    if (u.protocol === "https:") host = u.hostname.replace(/^www\./, "");
  } catch {
    host = "";
  }
  if (!host) return <span>{label}</span>;
  return (
    <button
      type="button"
      onClick={() => {
        if (inTauri) void openUrl(url);
      }}
      className="text-primary underline decoration-dotted underline-offset-2 hover:brightness-110"
      title={url}
    >
      {label}
      <span className="ml-1 text-[10px] text-muted-foreground">({host})</span>
    </button>
  );
}
