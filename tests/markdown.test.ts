// Model markdown → data. The renderer only ever emits text nodes; here we pin the structure.
import { expect, test } from "bun:test";
import { isTrustedSource } from "../src/lib/ai";
import { type ListBlock, parseBlocks, parseInline } from "../src/lib/markdown";

test("nested lists keep their depth and ordered lists keep their numbers", () => {
  const [list] = parseBlocks("1. one\n   - sub a\n   - sub b\n\n2. two\n3. three") as [ListBlock];
  expect(list.kind).toBe("list");
  expect(list.ordered).toBe(true);
  expect(list.items.map((i) => i.text)).toEqual(["one", "two", "three"]);
  expect(list.items[0]?.children?.items.map((i) => i.text)).toEqual(["sub a", "sub b"]);
  expect(list.items[0]?.children?.ordered).toBe(false);
  // A list that resumes after a paragraph starts at its own number, not at 1.
  const blocks = parseBlocks("1. a\n\ntext\n\n2. b");
  expect(blocks.map((b) => b.kind)).toEqual(["list", "para", "list"]);
  expect((blocks[2] as ListBlock).start).toBe(2);
});

test("inline: bold may hold code, links keep balanced parentheses", () => {
  expect(parseInline("**`hags`** on")).toEqual([
    { kind: "bold", children: [{ kind: "code", text: "hags" }] },
    { kind: "text", text: " on" },
  ]);
  const [link, rest] = parseInline("[wiki](https://en.wikipedia.org/wiki/Foo_(bar)) end");
  expect(link).toEqual({ kind: "link", label: "wiki", url: "https://en.wikipedia.org/wiki/Foo_(bar)" });
  expect(rest).toEqual({ kind: "text", text: " end" });
  expect(parseInline("a ** b ` c [d](e")).toEqual([{ kind: "text", text: "a ** b ` c [d](e" }]);
  expect(parseInline("<script>x</script>")).toEqual([{ kind: "text", text: "<script>x</script>" }]);
});

test("tables: header only when there is a rule row", () => {
  const [t] = parseBlocks("| GPU | FPS |\n|---|---|\n| 3050 | 90 |");
  expect(t).toEqual({ kind: "table", head: ["GPU", "FPS"], rows: [["3050", "90"]] });
});

test("Gemini sources are post-filtered to the trusted domains", () => {
  expect(isTrustedSource({ url: "https://www.techpowerup.com/review/x", title: "x" })).toBe(true);
  // Grounding redirect: the host is Google's, the title carries the site's domain.
  const redirect = "https://vertexaisearch.cloud.google.com/grounding-api-redirect/abc";
  expect(isTrustedSource({ url: redirect, title: "tomshardware.com" })).toBe(true);
  expect(isTrustedSource({ url: redirect, title: "random-blog.net" })).toBe(false);
  expect(isTrustedSource({ url: "https://evil-techpowerup.com/", title: "evil" })).toBe(false);
});
