import { chromium, devices } from "playwright";

const url = process.env.SMOKE_URL ?? "http://127.0.0.1:4173";
const browser = await chromium.launch({ headless: true });
try {
  for (const [deviceName, landscape] of [["iPhone 13", false], ["Pixel 5", true]]) {
    const base = devices[deviceName];
    const viewport = landscape
      ? { width: base.viewport.height, height: base.viewport.width }
      : base.viewport;
    const context = await browser.newContext({ ...base, viewport, screen: viewport });
    const page = await context.newPage();
    const fatal = [];
    page.on("pageerror", error => {
      if (!String(error).includes("Using exceptions for control flow")) fatal.push(String(error));
    });
    await page.goto(url, { waitUntil: "networkidle", timeout: 120_000 });
    const input = page.locator("#ghost-mobile-input");
    await input.waitFor({ state: "visible", timeout: 60_000 });
    await input.tap();
    await input.fill("Room-42!");
    if (!(await input.evaluate(element => element === document.activeElement))) {
      throw new Error(`${deviceName}: native input did not receive focus`);
    }
    if ((await input.inputValue()) !== "Room-42!") throw new Error(`${deviceName}: input value failed`);

    const canvas = page.locator("canvas");
    await canvas.waitFor({ state: "visible", timeout: 60_000 });
    const bounds = await canvas.boundingBox();
    if (!bounds) throw new Error(`${deviceName}: canvas has no bounds`);
    const windowSize = await page.evaluate(() => ({ width: innerWidth, height: innerHeight }));
    const epsilon = 1;
    if (bounds.x < -epsilon || bounds.y < -epsilon ||
        bounds.x + bounds.width > windowSize.width + epsilon ||
        bounds.y + bounds.height > windowSize.height + epsilon) {
      throw new Error(`${deviceName}: canvas escaped viewport: ${JSON.stringify({ bounds, windowSize })}`);
    }
    if (Math.min(bounds.x, bounds.y, windowSize.width - bounds.x - bounds.width,
      windowSize.height - bounds.y - bounds.height) < -epsilon) {
      throw new Error(`${deviceName}: unsafe viewport edge`);
    }
    if (fatal.length) throw new Error(fatal.join("\n"));
    console.log(`PASS: ${deviceName} ${landscape ? "landscape" : "portrait"} bridge and bounds`);
    await context.close();
  }
} finally {
  await browser.close();
}
