import { mkdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { chromium } from "playwright";

const url = process.argv[2] ?? "http://127.0.0.1:7300/";
const count = Number(process.argv[3] ?? 20);
const gapMs = Number(process.argv[4] ?? 150);
const folder = join(import.meta.dirname, "..", "build", "look", "burst");
mkdirSync(folder, { recursive: true });

const browser = await chromium.launch({
  channel: "msedge",
  headless: false,
  args: ["--inprivate", "--disable-sync", "--window-size=1620,1100"],
});
const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
await page.goto(url, { waitUntil: "load", timeout: 30000 });
await page.waitForTimeout(6000);
const frame = page.locator("#vnc");
for (let i = 0; i < count; i++) {
  const file = join(folder, `frame-${String(i).padStart(2, "0")}.png`);
  await frame.screenshot({ path: file });
  console.log(`${i} ${Date.now()} ${statSync(file).size}`);
  await page.waitForTimeout(gapMs);
}
await browser.close();
