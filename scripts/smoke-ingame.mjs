import { chromium } from "playwright";

const url = process.env.SMOKE_URL ?? "http://127.0.0.1:4173";
const fatal = /panicked at|RuntimeError|unreachable|schedule ambiguity|conflicting access|wasm trap|assertion failed/i;
const knownControlFlow = "Using exceptions for control flow, don't mind me. This isn't actually an error!";
const messages = [];
let fatalError = null;

const browser = await chromium.launch({
  headless: true,
  args: ["--enable-unsafe-swiftshader", "--use-angle=swiftshader", "--ignore-gpu-blocklist"],
});
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
page.on("console", (message) => {
  const text = message.text();
  messages.push(text);
  if (fatal.test(text)) fatalError ??= new Error(text);
});
page.on("pageerror", (error) => {
  if (!String(error).includes(knownControlFlow)) fatalError ??= error;
});

try {
  await page.goto(url, { waitUntil: "networkidle", timeout: 120_000 });
  const deadline = Date.now() + 60_000;
  while (!messages.some((message) => message.includes("local sync-test session ready"))) {
    if (Date.now() >= deadline) {
      throw new Error(`sync-test did not become ready\n${messages.slice(-30).join("\n")}`);
    }
    await page.waitForTimeout(250);
  }
  await page.waitForTimeout(15_000);
  if (fatalError) throw fatalError;
  if (!messages.some((message) => message.includes("spawning players"))) {
    throw new Error("InGame did not spawn players");
  }
  console.log("PASS: local sync-test entered InGame and ran for 15 seconds");
} finally {
  await browser.close();
}
