// Skill packages are hostile zips: only top-level SKILL.md entries are read, sizes come from what
// was inflated (not the header), traversal names are ignored.
import { expect, test } from "bun:test";
import { isSkillPath, looksLikeZip, skillsFromZip } from "../src/components/connections/skill-zip";

const enc = new TextEncoder();

async function deflate(b: Uint8Array): Promise<Uint8Array> {
  const s = new Blob([b as BlobPart]).stream().pipeThrough(new CompressionStream("deflate-raw"));
  return new Uint8Array(await new Response(s).arrayBuffer());
}

type Entry = { name: string; text: string; method?: 0 | 8; lieSize?: number; flags?: number };

/** A minimal zip (no CRC: the reader does not check it). */
async function zip(entries: Entry[]): Promise<ArrayBuffer> {
  const locals: Uint8Array[] = [];
  const centrals: Uint8Array[] = [];
  let offset = 0;
  for (const e of entries) {
    const name = enc.encode(e.name);
    const raw = enc.encode(e.text);
    const method = e.method ?? 8;
    const data = method === 8 ? await deflate(raw) : raw;
    const usize = e.lieSize ?? raw.length;
    const lh = new DataView(new ArrayBuffer(30));
    lh.setUint32(0, 0x04034b50, true);
    lh.setUint16(6, e.flags ?? 0, true);
    lh.setUint16(8, method, true);
    lh.setUint32(18, data.length, true);
    lh.setUint32(22, usize, true);
    lh.setUint16(26, name.length, true);
    const local = new Uint8Array([...new Uint8Array(lh.buffer), ...name, ...data]);
    const ch = new DataView(new ArrayBuffer(46));
    ch.setUint32(0, 0x02014b50, true);
    ch.setUint16(8, e.flags ?? 0, true);
    ch.setUint16(10, method, true);
    ch.setUint32(20, data.length, true);
    ch.setUint32(24, usize, true);
    ch.setUint16(28, name.length, true);
    ch.setUint32(42, offset, true);
    centrals.push(new Uint8Array([...new Uint8Array(ch.buffer), ...name]));
    locals.push(local);
    offset += local.length;
  }
  const cd = centrals.flatMap((c) => [...c]);
  const end = new DataView(new ArrayBuffer(22));
  end.setUint32(0, 0x06054b50, true);
  end.setUint16(8, entries.length, true);
  end.setUint16(10, entries.length, true);
  end.setUint32(12, cd.length, true);
  end.setUint32(16, offset, true);
  return new Uint8Array([...locals.flatMap((l) => [...l]), ...cd, ...new Uint8Array(end.buffer)])
    .buffer;
}

const SKILL = "---\nname: notes\ndescription: d\n---\nbody";

test("reads <folder>/SKILL.md, stored or deflated, and nothing else", async () => {
  const z = await zip([
    { name: "notes/SKILL.md", text: SKILL },
    { name: "notes/scripts/run.py", text: "import os; os.system('x')" },
    { name: "other/SKILL.md", text: SKILL.replace("notes", "other"), method: 0 },
    { name: "deep/a/SKILL.md", text: "nested too deep" },
  ]);
  expect(looksLikeZip(z)).toBe(true);
  const got = await skillsFromZip(z);
  expect(got).toEqual([SKILL, SKILL.replace("notes", "other")]);
});

test("traversal and odd names are ignored, never followed", async () => {
  for (const name of [
    "../SKILL.md",
    "../../x/SKILL.md",
    "a/../SKILL.md",
    "/abs/SKILL.md",
    "C:/x/SKILL.md",
    "a\\SKILL.md",
    "__MACOSX/SKILL.md",
    "./SKILL.md",
  ])
    expect(isSkillPath(name)).toBe(false);
  expect(isSkillPath("SKILL.md")).toBe(true);
  expect(isSkillPath("my-skill/skill.md")).toBe(true);
  const z = await zip([{ name: "../evil/SKILL.md", text: SKILL }]);
  await expect(skillsFromZip(z)).rejects.toThrow("no_skill");
});

test("a header that lies about the size does not get past the inflate limit", async () => {
  const bomb = "A".repeat(2 * 1024 * 1024); // compresses to a few KB
  const z = await zip([{ name: "b/SKILL.md", text: bomb, lieSize: 100 }]);
  await expect(skillsFromZip(z)).rejects.toThrow("too_large");
  const stored = await zip([{ name: "s/SKILL.md", text: "x".repeat(70_000), method: 0 }]);
  await expect(skillsFromZip(stored)).rejects.toThrow("too_large");
});

test("garbage, encrypted and truncated archives are refused", async () => {
  await expect(skillsFromZip(enc.encode("not a zip at all").buffer as ArrayBuffer)).rejects.toThrow(
    "not_zip",
  );
  const enc1 = await zip([{ name: "e/SKILL.md", text: SKILL, flags: 1 }]);
  await expect(skillsFromZip(enc1)).rejects.toThrow("encrypted");
  const good = new Uint8Array(await zip([{ name: "t/SKILL.md", text: SKILL }]));
  // Central directory points past the end once the archive is cut in the middle.
  const cut = new Uint8Array([...good.slice(0, 20), ...good.slice(good.length - 22)]);
  await expect(skillsFromZip(cut.buffer)).rejects.toThrow("not_zip");
  // Corrupt deflate data.
  const bad = new Uint8Array(await zip([{ name: "c/SKILL.md", text: SKILL, method: 0 }]));
  const v = new DataView(bad.buffer);
  v.setUint16(8, 8, true); // local header now claims deflate
  const cdAt = v.getUint32(bad.length - 6, true);
  v.setUint16(cdAt + 10, 8, true);
  bad[30 + "c/SKILL.md".length] = 0xff;
  await expect(skillsFromZip(bad.buffer)).rejects.toThrow(/corrupt|too_large/);
});
