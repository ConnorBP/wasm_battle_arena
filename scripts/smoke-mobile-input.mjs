import { chromium, devices } from "playwright";

const url = process.env.SMOKE_URL ?? "http://127.0.0.1:4173";
const browser = await chromium.launch({ headless: true });
try {
  for (const deviceName of ["iPhone 13", "Pixel 5"]) {
    const context = await browser.newContext({ ...devices[deviceName] });
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
    if (fatal.length) throw new Error(fatal.join("\n"));
    console.log(`PASS: ${deviceName} loaded mobile bridge-capable build`);
    await context.close();
  }
} finally {
  await browser.close();
}
