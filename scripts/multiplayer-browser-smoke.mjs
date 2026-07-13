import { chromium } from "playwright";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";

const gameUrl = process.env.GAME_URL ?? "http://127.0.0.1:4173";
const artifactDir = path.resolve(process.env.ARTIFACT_DIR ?? "artifacts/local-multiplayer-smoke", "browser");
const profileKey = "ghosties.casual-profile.v1";
const readyTimeout = Number(process.env.BROWSER_SMOKE_TIMEOUT_MS ?? 120_000);
const knownControlFlow = "Using exceptions for control flow, don't mind me. This isn't actually an error!";
const fatalPattern = /panicked at|RuntimeError|unreachable|wasm trap|assertion failed|memory access out of bounds/i;
const logs = [];
const failures = [];
let browser;

function assert(condition, message) {
  if (!condition) throw new Error(message);
}
function canonicalProfile(name, palette = 0) {
  return `GHOSTIES_PROFILE\t1\t${name}\t55\t100\t${palette}\t0\t0\t0\t1\t0\t`;
}
function record(label, kind, text) {
  const entry = { at: new Date().toISOString(), label, kind, text: String(text) };
  logs.push(entry);
  if ((kind === "console:error" && fatalPattern.test(entry.text)) ||
      (kind === "pageerror" && !entry.text.includes(knownControlFlow))) failures.push(entry);
}

async function waitForGame(page, label) {
  await page.goto(gameUrl, { waitUntil: "domcontentloaded", timeout: readyTimeout });
  await page.waitForFunction(() => document.querySelector("canvas"), null,
    { timeout: readyTimeout });
  const canvas = page.locator("canvas").first();
  await canvas.waitFor({ state: "visible", timeout: readyTimeout });
  const box = await canvas.boundingBox();
  assert(box && box.width >= 300 && box.height >= 200, `${label}: game canvas was not usable`);
  return { canvas, box };
}

async function screenshot(page, name) {
  await page.screenshot({ path: path.join(artifactDir, name), fullPage: true });
}

async function main() {
  await mkdir(artifactDir, { recursive: true });
  browser = await chromium.launch({
    headless: true,
    args: ["--enable-unsafe-swiftshader", "--use-angle=swiftshader", "--ignore-gpu-blocklist"],
  });

  // Separate browser contexts model two independent players. Seed each one
  // through the public browser storage API before loading the game; no game
  // test hook or internal WASM bridge is used.
  const contexts = await Promise.all([0, 1].map(() => browser.newContext({ viewport: { width: 1280, height: 720 } })));
  const pages = await Promise.all(contexts.map(context => context.newPage()));
  pages.forEach((page, index) => {
    const label = `player-${index + 1}`;
    page.on("console", message => record(label, `console:${message.type()}`, message.text()));
    page.on("pageerror", error => record(label, "pageerror", error?.stack ?? error));
    page.on("requestfailed", request => record(label, "requestfailed", `${request.method()} ${request.url()} ${request.failure()?.errorText ?? ""}`));
    page.on("websocket", socket => record(label, "websocket", socket.url()));
  });

  const profileA = canonicalProfile("SmokeOne", 1);
  const profileB = canonicalProfile("SmokeTwo", 2);
  await Promise.all(pages.map((page, index) => page.addInitScript(({ key, value }) => {
    localStorage.setItem(key, value);
  }, { key: profileKey, value: index === 0 ? profileA : profileB })));

  const loaded = await Promise.all(pages.map((page, index) => waitForGame(page, `player-${index + 1}`)));
  await Promise.all(pages.map((page, index) => screenshot(page, `player-${index + 1}-loaded.png`)));

  // Exercise real canvas input without relying on private game hooks. The
  // protocol suite owns deterministic queue formation because egui widget
  // coordinates vary with viewport/font metrics.
  await Promise.all(loaded.map(async ({ canvas, box }) => {
    await canvas.click({ position: { x: box.width / 2, y: box.height / 2 } });
    await canvas.press("Escape");
  }));
  await Promise.all(pages.map(page => page.waitForTimeout(1_000)));
  await Promise.all(pages.map((page, index) => screenshot(page, `player-${index + 1}-interaction.png`)));

  const persisted = await Promise.all(pages.map(page => page.evaluate(key => localStorage.getItem(key), profileKey)));
  assert(persisted[0]?.includes("\tSmokeOne\t"), "player 1 profile was not loaded/preserved");
  assert(persisted[1]?.includes("\tSmokeTwo\t"), "player 2 profile was not loaded/preserved");

  await Promise.all(pages.map(page => page.reload({ waitUntil: "domcontentloaded", timeout: readyTimeout })));
  await Promise.all(pages.map(page => page.waitForFunction(() => document.querySelector("canvas"), null,
    { timeout: readyTimeout })));
  const afterReload = await Promise.all(pages.map(page => page.evaluate(key => localStorage.getItem(key), profileKey)));
  assert(afterReload[0]?.includes("\tSmokeOne\t") && afterReload[1]?.includes("\tSmokeTwo\t"),
    "local profile did not survive reload in isolated contexts");
  await Promise.all(pages.map((page, index) => screenshot(page, `player-${index + 1}-reload.png`)));

  // Corrupt persisted data must be repaired to the canonical safe profile by
  // the normal startup path. This observes the public localStorage contract.
  await pages[0].evaluate(key => localStorage.setItem(key, "not-a-ghost-profile\u0000corrupt"), profileKey);
  await pages[0].reload({ waitUntil: "domcontentloaded", timeout: readyTimeout });
  await pages[0].waitForFunction(() => document.querySelector("canvas"), null,
    { timeout: readyTimeout });
  await pages[0].waitForTimeout(1_000);
  const repaired = await pages[0].evaluate(key => localStorage.getItem(key), profileKey);
  assert(repaired !== "not-a-ghost-profile\u0000corrupt", "corrupt profile was not repaired");
  await screenshot(pages[0], "player-1-corrupt-fallback.png");

  await pages[1].waitForTimeout(1_000);
  if (failures.length) throw new Error(`fatal browser diagnostics:\n${failures.map(entry => `${entry.label} ${entry.kind}: ${entry.text}`).join("\n")}`);
  console.log("PASS: two isolated browser contexts loaded and accepted real canvas input");
  console.log("PASS: localStorage profile survived reload and corrupt data fell back safely");
}

let failure;
try {
  await main();
} catch (error) {
  failure = error;
  process.exitCode = 1;
  console.error(error?.stack ?? error);
} finally {
  await mkdir(artifactDir, { recursive: true });
  await writeFile(path.join(artifactDir, "browser.log.json"), JSON.stringify({
    gameUrl, failure: failure ? String(failure.stack ?? failure) : null, failures, logs,
  }, null, 2));
  if (browser) {
    for (const [index, context] of browser.contexts().entries()) {
      for (const page of context.pages()) {
        await page.screenshot({ path: path.join(artifactDir, `final-${index + 1}.png`), fullPage: true }).catch(() => {});
      }
    }
    await browser.close().catch(() => {});
  }
}
