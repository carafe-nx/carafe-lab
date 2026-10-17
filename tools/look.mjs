import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { chromium } from "playwright";

const url = process.argv[2] ?? "http://127.0.0.1:7300/";
const waitSeconds = Number(process.argv[3] ?? 8);
const name = process.argv[4] ?? new Date().toISOString().replace(/[:.]/g, "-");
const folder = join(import.meta.dirname, "..", "build", "look");
mkdirSync(folder, { recursive: true });

const browser = await chromium.launch({
  channel: "msedge",
  headless: false,
  args: ["--inprivate", "--disable-sync", "--window-size=1620,1100"],
});
const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
const notes = [];
page.on("console", (message) => {
  if (message.type() === "error" || message.type() === "warning") {
    notes.push(`console ${message.type()}: ${message.text()}`);
  }
});
page.on("requestfailed", (request) => {
  notes.push(`request failed: ${request.url()} ${request.failure()?.errorText ?? ""}`);
});
page.on("websocket", (socket) => {
  notes.push(`websocket opened: ${socket.url()}`);
  socket.on("close", () => notes.push(`websocket closed: ${socket.url()}`));
});

await page.goto(url, { waitUntil: "load", timeout: 30000 });
await page.waitForTimeout(waitSeconds * 1000);
const file = join(folder, `${name}.png`);
await page.screenshot({ path: file, fullPage: true });

const frames = page.frames().map((frame) => frame.url()).filter((frameUrl) => frameUrl !== "about:blank");
console.log(`screenshot: ${file}`);
console.log(`frames: ${frames.join(" | ")}`);
for (const note of notes) {
  console.log(note);
}
await browser.close();
