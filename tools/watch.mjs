import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { chromium } from "playwright";

const url = process.argv[2] ?? "http://127.0.0.1:7300/";
const everySeconds = Number(process.argv[3] ?? 15);
const longestMinutes = Number(process.argv[4] ?? 60);
const folder = join(import.meta.dirname, "..", "build", "look");
mkdirSync(folder, { recursive: true });

const browser = await chromium.launch({
  channel: "msedge",
  headless: false,
  args: ["--inprivate", "--disable-sync", "--window-size=1620,1100"],
});
const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
page.on("console", (message) => {
  if (message.type() === "error") {
    console.log(`console error: ${message.text()}`);
  }
});
page.on("close", () => process.exit(0));

const started = Date.now();
let loaded = false;
while (!loaded && Date.now() - started < 120000) {
  try {
    await page.goto(url, { waitUntil: "load", timeout: 10000 });
    loaded = true;
  } catch {
    await page.waitForTimeout(2000);
  }
}
if (!loaded) {
  console.log(`could not open ${url}`);
  await browser.close();
  process.exit(1);
}
console.log(`watching ${url}`);

let shot = 0;
while (Date.now() - started < longestMinutes * 60000) {
  await page.waitForTimeout(everySeconds * 1000);
  const seconds = Math.round((Date.now() - started) / 1000);
  const file = join(folder, `watch-${String(seconds).padStart(4, "0")}.png`);
  try {
    await page.screenshot({ path: file });
    await page.screenshot({ path: join(folder, "watch-latest.png") });
  } catch {
    break;
  }
  shot += 1;
  const over = await page
    .locator("text=/Прогон закончен|The run is over/")
    .count()
    .catch(() => 0);
  if (over > 0) {
    console.log(`the run is over after ${seconds} s, ${shot} screenshots`);
    await page.waitForTimeout(10000);
    break;
  }
}
await browser.close();
