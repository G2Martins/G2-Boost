// Release packaging (`bun run release` runs `tauri build` first): collects the NSIS and MSI
// installers Tauri built, makes the portable zip (exe + portable.txt, data stays next to the exe)
// and writes SHA256SUMS.txt. Output: release/ (gitignored). Run with `node`, not bun (dev tool).
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const { version } = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
const bundle = "target/release/bundle";
const out = "release";
rmSync(out, { recursive: true, force: true });
mkdirSync(out);

const assets = [];
for (const [dir, ext] of [
  ["nsis", ".exe"],
  ["msi", ".msi"],
]) {
  for (const f of readdirSync(join(bundle, dir))) {
    if (!f.endsWith(ext) || !f.includes(`_${version}_`)) continue;
    const name = f.replaceAll(" ", "-"); // GitHub turns spaces in asset names into dots
    copyFileSync(join(bundle, dir, f), join(out, name));
    assets.push(name);
  }
}
if (!assets.some((a) => a.endsWith(".exe")) || !assets.some((a) => a.endsWith(".msi"))) {
  throw new Error(`installers for ${version} not found in ${bundle}: run \`bun run tauri build\` first`);
}

const PORTABLE_TXT = `G2 Boost ${version}: modo portátil

Com este arquivo ao lado do G2 Boost.exe, configurações, backups dos ajustes e históricos ficam na
pasta "data" aqui mesmo (nada vai para %LOCALAPPDATA%). Apague este arquivo para usar a pasta do
usuário. Desfaça os ajustes pelo app (Perfis e restauração) antes de apagar a pasta: os backups
moram nela.

Requer Windows 10 ou 11 (64 bits) e o WebView2, que já vem no Windows 11 e chega pelo Windows
Update no Windows 10.

---

Portable mode: with this file next to G2 Boost.exe, settings, tweak backups and history stay in the
"data" folder right here. Delete this file to use the user profile folder instead. Undo tweaks in
the app (Profiles and restore) before deleting the folder: the backups live in it.
`;

// Portable zip: the app exe already embeds the whole interface.
const stage = join(out, "portable");
mkdirSync(stage);
// `mainBinaryName` makes tauri build rename the cargo binary (g2-boost.exe) to "G2 Boost.exe".
const exe = ["target/release/G2 Boost.exe", "target/release/g2-boost.exe"].find((p) => existsSync(p));
if (!exe) throw new Error("app exe not found in target/release");
copyFileSync(exe, join(stage, "G2 Boost.exe"));
writeFileSync(join(stage, "portable.txt"), PORTABLE_TXT.replaceAll("\n", "\r\n"));
const zip = `G2-Boost_${version}_x64-portable.zip`;
// Windows' own bsdtar writes zip (-a picks the format from the extension); Git's GNU tar can't.
const tar = join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe");
execFileSync(tar, ["-a", "-c", "-f", join("..", zip), "G2 Boost.exe", "portable.txt"], { cwd: stage });
rmSync(stage, { recursive: true });
assets.push(zip);

const sums = assets
  .map((a) => `${createHash("sha256").update(readFileSync(join(out, a))).digest("hex")}  ${a}`)
  .join("\n");
writeFileSync(join(out, "SHA256SUMS.txt"), `${sums}\n`);
console.log(`release/ ready:\n${assets.map((a) => `  ${a}`).join("\n")}\n  SHA256SUMS.txt`);
