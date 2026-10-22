// Renders an SVG to PNG with the installed Edge: node tools/render-svg.mjs in.svg out.png width [height]
import { chromium } from "playwright";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const [, , input, output, wArg, hArg] = process.argv;
const w = Number(wArg ?? 512);
const h = Number(hArg ?? w);
const svg = readFileSync(resolve(input), "utf8").replace(/width="\d+" height="\d+"/, `width="${w}" height="${h}"`);
const browser = await chromium.launch({ channel: "msedge" });
const page = await browser.newPage({ viewport: { width: w, height: h } });
await page.setContent(`<html><body style="margin:0;background:transparent">${svg}</body></html>`);
await page.screenshot({ path: resolve(output), omitBackground: true });
await browser.close();
