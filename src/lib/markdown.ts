/**
 * Parser for the markdown subset model answers use: code fences, pipe tables, headings, nested
 * bullet/numbered lists, paragraphs; inline **bold** (may contain `code`), `code`, [links](url).
 * Output is plain data — the renderer turns it into React text nodes, never HTML.
 */

export type Inline =
  | { kind: "text"; text: string }
  | { kind: "bold"; children: Inline[] }
  | { kind: "code"; text: string }
  | { kind: "link"; label: string; url: string };

export type ListItem = { text: string; children: ListBlock | null };
export type ListBlock = { kind: "list"; ordered: boolean; start: number; items: ListItem[] };
export type Block =
  | { kind: "code"; text: string }
  | { kind: "table"; head: string[] | null; rows: string[][] }
  | { kind: "heading"; level: number; text: string }
  | { kind: "para"; text: string }
  | ListBlock;

const LIST_RE = /^(\s*)([-*•]|(\d+)[.)])\s+(.*)$/;

const indentOf = (ws: string) => ws.replace(/\t/g, "    ").length;

/** Inline tokens; unmatched markers stay literal text. */
export function parseInline(s: string): Inline[] {
  const out: Inline[] = [];
  let buf = "";
  const flush = () => {
    if (buf) out.push({ kind: "text", text: buf });
    buf = "";
  };
  let i = 0;
  while (i < s.length) {
    if (s.startsWith("**", i)) {
      const j = s.indexOf("**", i + 2);
      if (j > i + 2) {
        flush();
        out.push({ kind: "bold", children: parseInline(s.slice(i + 2, j)) });
        i = j + 2;
        continue;
      }
    }
    if (s[i] === "`") {
      const j = s.indexOf("`", i + 1);
      if (j > i + 1) {
        flush();
        out.push({ kind: "code", text: s.slice(i + 1, j) });
        i = j + 1;
        continue;
      }
    }
    if (s[i] === "[") {
      const close = s.indexOf("](", i + 1);
      if (close > i + 1) {
        // URL with balanced parentheses (`…/Foo_(bar)`); whitespace ends it.
        let depth = 1;
        let k = close + 2;
        while (k < s.length && depth > 0 && !/\s/.test(s[k] ?? "")) {
          if (s[k] === "(") depth++;
          else if (s[k] === ")") depth--;
          k++;
        }
        if (depth === 0) {
          flush();
          out.push({ kind: "link", label: s.slice(i + 1, close), url: s.slice(close + 2, k - 1) });
          i = k;
          continue;
        }
      }
    }
    buf += s[i];
    i++;
  }
  flush();
  return out;
}

/** A list starting at `lines[i]` (nesting by indentation); returns the block and the next index. */
export function parseList(lines: string[], i: number): [ListBlock, number] {
  let root: ListBlock | null = null;
  const stack: { indent: number; list: ListBlock }[] = [];
  let last: ListItem | null = null;
  while (i < lines.length) {
    const line = lines[i] ?? "";
    if (line.trim() === "") {
      // A blank line inside a list (loose list) continues it when the next text is another item.
      let j = i + 1;
      while (j < lines.length && (lines[j] ?? "").trim() === "") j++;
      if (j < lines.length && LIST_RE.test(lines[j] ?? "")) {
        i = j;
        continue;
      }
      break;
    }
    const m = line.match(LIST_RE);
    if (!m) {
      // Indented continuation of the previous item.
      if (last && /^\s+\S/.test(line)) {
        last.text += ` ${line.trim()}`;
        i++;
        continue;
      }
      break;
    }
    const indent = indentOf(m[1] ?? "");
    const ordered = m[3] !== undefined;
    const item: ListItem = { text: m[4] ?? "", children: null };
    const fresh = (): ListBlock => ({
      kind: "list",
      ordered,
      start: ordered ? Number(m[3]) : 1,
      items: [],
    });
    while (stack.length > 0 && indent < (stack[stack.length - 1]?.indent ?? 0)) stack.pop();
    const top = stack[stack.length - 1];
    if (!root) {
      root = fresh();
      stack.push({ indent, list: root });
    } else if (!top) {
      stack.push({ indent, list: root }); // dedented below the first item: back to the root level
    } else if (indent > top.indent && last) {
      last.children ??= fresh();
      stack.push({ indent, list: last.children });
    }
    stack[stack.length - 1]?.list.items.push(item);
    last = item;
    i++;
  }
  return [root ?? { kind: "list", ordered: false, start: 1, items: [] }, i];
}

export function parseBlocks(text: string): Block[] {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  const out: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const trimmed = (lines[i] ?? "").trim();
    if (trimmed.startsWith("```")) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !(lines[i] ?? "").trim().startsWith("```"))
        body.push(lines[i++] ?? "");
      i++;
      out.push({ kind: "code", text: body.join("\n") });
      continue;
    }
    if (trimmed.startsWith("|")) {
      const rows: string[][] = [];
      let sawRule = false;
      while (i < lines.length && (lines[i] ?? "").trim().startsWith("|")) {
        const cells = (lines[i] ?? "")
          .trim()
          .replace(/^\||\|$/g, "")
          .split("|")
          .map((c) => c.trim());
        if (cells.every((c) => /^:?-{2,}:?$/.test(c))) sawRule = true;
        else rows.push(cells);
        i++;
      }
      const head = sawRule && rows.length > 0 ? (rows.shift() ?? null) : null;
      out.push({ kind: "table", head, rows });
      continue;
    }
    const h = trimmed.match(/^(#{1,4})\s+(.*)$/);
    if (h) {
      out.push({ kind: "heading", level: h[1]?.length ?? 1, text: h[2] ?? "" });
      i++;
      continue;
    }
    if (LIST_RE.test(lines[i] ?? "")) {
      const [list, next] = parseList(lines, i);
      out.push(list);
      i = next;
      continue;
    }
    if (trimmed === "") {
      i++;
      continue;
    }
    out.push({ kind: "para", text: trimmed });
    i++;
  }
  return out;
}
