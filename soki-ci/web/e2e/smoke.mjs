// Minimal browser smoke test. Requires a running soki-ci service and a Chrome
// binary. Set SOKI_CI_URL (default http://127.0.0.1:9847) and CHROME_PATH.
import { chromium } from "playwright-core";

const url = process.env.SOKI_CI_URL ?? "http://127.0.0.1:9847";
const executablePath = process.env.CHROME_PATH;
const browser = await chromium.launch({
  headless: true,
  executablePath,
  args: ["--no-sandbox", "--disable-dev-shm-usage", "--disable-gpu"],
});
const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
const errors = [];
page.on("console", (message) => {
  if (message.type() === "error") errors.push(`console: ${message.text()}`);
});
page.on("pageerror", (error) => errors.push(`pageerror: ${error.message}`));

await page.goto(url, { waitUntil: "domcontentloaded", timeout: 20_000 });
await page.waitForTimeout(1_500);
const text = await page.locator("body").innerText();
if (!text.includes("soki-ci")) throw new Error("web UI did not render");
if (errors.length) throw new Error(`browser errors:\n${errors.join("\n")}`);
console.log(`ok: ${url} rendered (${text.length} chars)`);
await browser.close();
