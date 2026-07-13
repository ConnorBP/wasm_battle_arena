import { chromium } from "playwright";
const url = process.env.GAME_URL ?? "http://127.0.0.1:4173";
const fatal = /Cannot read properties|panicked at|RuntimeError|unreachable|wasm trap|conflicting access/i;
const messages = [];
let failure = null;
const browser = await chromium.launch({ headless: true, args: ["--enable-unsafe-swiftshader", "--use-angle=swiftshader", "--ignore-gpu-blocklist"] });
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
  page.on("console", message => { messages.push(message.text()); if (fatal.test(message.text())) failure ??= new Error(message.text()); });
  page.on("pageerror", error => { if (!String(error).includes("Using exceptions for control flow")) failure ??= error; });
  await page.goto(url, { waitUntil: "domcontentloaded", timeout: 120_000 });
  const canvas = page.locator("canvas").first();
  await canvas.waitFor({ state: "visible", timeout: 120_000 });
  await page.waitForTimeout(2500);
  const box = await canvas.boundingBox();
  if (!box) throw new Error("canvas bounds unavailable");
  // Default Any Mode public-match button at the fixed desktop smoke viewport.
  await canvas.click({ position: { x: box.width / 2, y: box.height * 0.61 } });
  await page.waitForTimeout(7000);
  if (failure) throw failure;
  if (!messages.some(message => message.includes("connecting to Cloudflare matchmaking"))) {
    throw new Error(`Any Mode click did not enter matchmaking\n${messages.slice(-30).join("\n")}`);
  }
  console.log("PASS: Any Mode entered matchmaking and remained crash-free for 7 seconds");
} finally { await browser.close(); }
