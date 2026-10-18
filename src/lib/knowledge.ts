/**
 * Luli's knowledge base: every `knowledge/*.md` except the README, in file-name order, embedded
 * at build time. Edit the Markdown files, not this module (rules in knowledge/README.md).
 */
const files = import.meta.glob("../../knowledge/*.md", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

export const KNOWLEDGE = Object.keys(files)
  .filter((p) => !p.toLowerCase().endsWith("/readme.md"))
  .sort()
  .map((p) => `<doc name="${p.split("/").pop()}">\n${(files[p] ?? "").trim()}\n</doc>`)
  .join("\n\n");
