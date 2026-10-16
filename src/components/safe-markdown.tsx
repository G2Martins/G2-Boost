import { openUrl } from "@tauri-apps/plugin-opener";
import type { ReactNode } from "react";
import { inTauri } from "@/lib/backend";

/**
 * Tiny markdown subset for model answers: headings, bullets, numbered lists, pipe tables,
 * **bold**, `code`, [links](https://…). Everything is rendered as React text nodes — no
 * HTML from the model ever reaches the DOM. Links open in the system browser, https only.
 */
export function SafeMarkdown({ text }: { text: string }) {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  const out: ReactNode[] = [];
  let i = 0;
  let key = 0;
  while (i < lines.length) {
    const line = lines[i] ?? "";
    const trimmed = line.trim();
    if (trimmed.startsWith("```")) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !(lines[i] ?? "").trim().startsWith("```"))
        body.push(lines[i++] ?? "");
      i++;
      out.push(
        <pre
          key={key++}
          className="my-2 overflow-x-auto rounded-md border border-border bg-background/70 p-3 font-mono text-xs"
        >
          {body.join("\n")}
        </pre>,
      );
      continue;
    }
    if (trimmed.startsWith("|")) {
      const rows: string[][] = [];
      while (i < lines.length && (lines[i] ?? "").trim().startsWith("|")) {
        const cells = (lines[i] ?? "")
          .trim()
          .replace(/^\||\|$/g, "")
          .split("|")
          .map((c) => c.trim());
        if (!cells.every((c) => /^:?-{2,}:?$/.test(c))) rows.push(cells);
        i++;
      }
      // Stable ids computed here (not from map indexes): rows/cells never reorder.
      const table = rows.map((cells, ri) => ({
        id: `r${ri}`,
        cells: cells.map((c, ci) => ({ id: `r${ri}c${ci}`, c })),
      }));
      const [head, ...rest] = table;
      out.push(
        <div key={key++} className="my-2 overflow-x-auto">
          <table className="w-full border-collapse text-xs">
            {head ? (
              <thead>
                <tr>
                  {head.cells.map((cell) => (
                    <th
                      key={cell.id}
                      className="border-b border-border px-2 py-1.5 text-left font-semibold"
                    >
                      {inline(cell.c)}
                    </th>
                  ))}
                </tr>
              </thead>
            ) : null}
            <tbody>
              {rest.map((row) => (
                <tr key={row.id} className="border-b border-border/60">
                  {row.cells.map((cell) => (
                    <td key={cell.id} className="num px-2 py-1.5">
                      {inline(cell.c)}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>,
      );
      continue;
    }
    const h = trimmed.match(/^(#{1,4})\s+(.*)$/);
    if (h) {
      const level = h[1]?.length ?? 1;
      out.push(
        <p
          key={key++}
          className={
            level <= 2
              ? "mt-3 font-display text-[15px] font-semibold"
              : "mt-2 font-display text-[13px] font-semibold"
          }
        >
          {inline(h[2] ?? "")}
        </p>,
      );
      i++;
      continue;
    }
    if (/^([-*•]|\d+[.)])\s+/.test(trimmed)) {
      const items: string[] = [];
      const ordered = /^\d+[.)]/.test(trimmed);
      while (i < lines.length && /^\s*([-*•]|\d+[.)])\s+/.test(lines[i] ?? "")) {
        items.push((lines[i] ?? "").trim().replace(/^([-*•]|\d+[.)])\s+/, ""));
        i++;
      }
      const List = ordered ? "ol" : "ul";
      out.push(
        <List
          key={key++}
          className={`my-1.5 space-y-1 pl-5 ${ordered ? "list-decimal" : "list-disc"}`}
        >
          {items.map((it, j) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: static list items
            <li key={j}>{inline(it)}</li>
          ))}
        </List>,
      );
      continue;
    }
    if (trimmed === "") {
      i++;
      continue;
    }
    out.push(
      <p key={key++} className="my-1.5 leading-relaxed">
        {inline(trimmed)}
      </p>,
    );
    i++;
  }
  return <div className="text-sm text-foreground/95">{out}</div>;
}

/** **bold**, `code`, [text](https://url) → React nodes (plain text otherwise). */
function inline(s: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  const re = /(\*\*[^*]+\*\*|`[^`]+`|\[[^\]]+\]\([^)\s]+\))/g;
  let last = 0;
  let k = 0;
  for (const m of s.matchAll(re)) {
    const idx = m.index ?? 0;
    if (idx > last) nodes.push(s.slice(last, idx));
    const tok = m[0];
    if (tok.startsWith("**")) nodes.push(<strong key={k++}>{tok.slice(2, -2)}</strong>);
    else if (tok.startsWith("`"))
      nodes.push(
        <code key={k++} className="rounded bg-card-hover px-1 font-mono text-[0.85em]">
          {tok.slice(1, -1)}
        </code>,
      );
    else {
      const lm = tok.match(/^\[([^\]]+)\]\(([^)\s]+)\)$/);
      const label = lm?.[1] ?? tok;
      const url = lm?.[2] ?? "";
      nodes.push(<ExternalLink key={k++} url={url} label={label} />);
    }
    last = idx + tok.length;
  }
  if (last < s.length) nodes.push(s.slice(last));
  return nodes;
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
