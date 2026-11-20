/**
 * A skill package (.zip with `<name>/SKILL.md` at the top) opened in the webview, without a
 * dependency: read the central directory, take only the SKILL.md entries, inflate them with the
 * platform's `DecompressionStream("deflate-raw")`. Scripts and other files of the package are never
 * read, and no name from the zip is ever used as a path (the core stores skills in one JSON file).
 *
 * The zip is hostile: sizes are checked against what was actually inflated (a header can lie),
 * names with `..`, absolute or backslash paths are ignored, encrypted and zip64 archives refused.
 */

export const MAX_ZIP = 25 * 1024 * 1024;
/** Far above what the core accepts (16 KB of instructions): anything bigger is refused here. */
const MAX_ENTRY = 64 * 1024;
/** Same as the core's skill limit. */
const MAX_SKILLS = 30;

export type ZipError =
  | "too_large"
  | "not_zip"
  | "unsupported"
  | "encrypted"
  | "corrupt"
  | "no_skill";

export class SkillZipError extends Error {
  constructor(readonly code: ZipError) {
    super(code);
  }
}

/** True for bytes that start like a zip (local file header). */
export function looksLikeZip(b: ArrayBuffer): boolean {
  return b.byteLength >= 4 && new DataView(b).getUint32(0, true) === 0x04034b50;
}

/** `SKILL.md` alone or `<folder>/SKILL.md`; anything deeper, odd or traversing is ignored. */
export function isSkillPath(name: string): boolean {
  if (
    name.includes("\\") ||
    name.startsWith("/") ||
    /^[a-z]:/i.test(name) ||
    name.startsWith("__MACOSX/") ||
    name.split("/").some((p) => p === ".." || p === ".")
  )
    return false;
  return /^([^/]+\/)?SKILL\.md$/i.test(name);
}

async function inflate(data: Uint8Array): Promise<Uint8Array> {
  const reader = new Blob([data as BlobPart])
    .stream()
    .pipeThrough(new DecompressionStream("deflate-raw"))
    .getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      total += value.length;
      if (total > MAX_ENTRY) {
        await reader.cancel().catch(() => undefined);
        throw new SkillZipError("too_large"); // a zip bomb lies in its header; the stream does not
      }
      chunks.push(value);
    }
  } catch (e) {
    throw e instanceof SkillZipError ? e : new SkillZipError("corrupt");
  }
  const out = new Uint8Array(total);
  let o = 0;
  for (const c of chunks) {
    out.set(c, o);
    o += c.length;
  }
  return out;
}

/** The text of every SKILL.md in the package (≤ 30), in archive order. */
export async function skillsFromZip(buf: ArrayBuffer): Promise<string[]> {
  if (buf.byteLength > MAX_ZIP) throw new SkillZipError("too_large");
  const v = new DataView(buf);
  const len = buf.byteLength;
  // End of central directory: 22 bytes + a comment of up to 64 KB, at the very end.
  let eocd = -1;
  for (let i = len - 22; i >= Math.max(0, len - 22 - 0xffff); i--) {
    if (v.getUint32(i, true) === 0x06054b50) {
      eocd = i;
      break;
    }
  }
  if (eocd < 0) throw new SkillZipError("not_zip");
  const count = v.getUint16(eocd + 10, true);
  const cdSize = v.getUint32(eocd + 12, true);
  let p = v.getUint32(eocd + 16, true);
  if (count === 0xffff || p === 0xffffffff || cdSize === 0xffffffff)
    throw new SkillZipError("unsupported");
  if (p + cdSize > eocd) throw new SkillZipError("not_zip");
  const names = new TextDecoder();
  const out: string[] = [];
  for (let n = 0; n < count && out.length < MAX_SKILLS; n++) {
    if (p + 46 > eocd || v.getUint32(p, true) !== 0x02014b50) throw new SkillZipError("not_zip");
    const flags = v.getUint16(p + 8, true);
    const method = v.getUint16(p + 10, true);
    const csize = v.getUint32(p + 20, true);
    const nlen = v.getUint16(p + 28, true);
    const local = v.getUint32(p + 42, true);
    if (p + 46 + nlen > eocd) throw new SkillZipError("not_zip");
    const name = names.decode(new Uint8Array(buf, p + 46, nlen));
    p += 46 + nlen + v.getUint16(p + 30, true) + v.getUint16(p + 32, true);
    if (!isSkillPath(name)) continue;
    if (flags & 1) throw new SkillZipError("encrypted");
    if (local + 30 > len || v.getUint32(local, true) !== 0x04034b50)
      throw new SkillZipError("not_zip");
    const start = local + 30 + v.getUint16(local + 26, true) + v.getUint16(local + 28, true);
    if (start + csize > len) throw new SkillZipError("not_zip");
    const data = new Uint8Array(buf, start, csize);
    let bytes: Uint8Array;
    if (method === 0) {
      if (csize > MAX_ENTRY) throw new SkillZipError("too_large");
      bytes = data;
    } else if (method === 8) {
      bytes = await inflate(data);
    } else {
      throw new SkillZipError("unsupported");
    }
    out.push(new TextDecoder().decode(bytes));
  }
  if (out.length === 0) throw new SkillZipError("no_skill");
  return out;
}
