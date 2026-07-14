import { chromium } from "playwright";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";

const gameUrl = process.env.GAME_URL ?? "http://127.0.0.1:4173";
const out = path.resolve(process.env.ARTIFACT_DIR ?? "artifacts/network-transition");
const fatal = /panicked at|RuntimeError|unreachable|peer disconnected|lobby service disconnected|assertion failed|NetworkInterrupted/i;
await mkdir(out, { recursive: true });
const browser = await chromium.launch({ headless: true, args: ["--enable-unsafe-swiftshader", "--use-angle=swiftshader", "--ignore-gpu-blocklist"] });
const contexts = await Promise.all([browser.newContext(), browser.newContext()]);
const pages = await Promise.all(contexts.map(context => context.newPage()));
const logs = [[], []];
let failure;
let passed = false;
try {
  pages.forEach((page, index) => {
    page.on("console", message => { const text = message.text(); logs[index].push(text); if (fatal.test(text)) failure ??= new Error(`peer ${index + 1}: ${text}`); });
    page.on("pageerror", error => { if (!String(error).includes("Using exceptions for control flow")) failure ??= error; });
  });
  await Promise.all(pages.map(page => page.addInitScript(() => { window.__ghostTransitionEvents = []; })));
  await Promise.all(pages.map(page => page.goto(gameUrl, { waitUntil: "domcontentloaded", timeout: 120_000 })));
  const deadline = Date.now() + 240_000;
  while (Date.now() < deadline) {
    if (failure) throw failure;
    const events = await Promise.all(pages.map((page, index) => page.evaluate(() => window.__ghostTransitionEvents ?? []).then(list => {
      if (list.length) return list;
      return logs[index].filter(line => line.includes("GHOST_TRANSITION")).map(line => {
        const match = /GHOST_TRANSITION (\w+) (\d+):(\d+) frame=(\d+)/.exec(line);
        return match ? { kind: match[1], epoch: Number(match[2]), round: Number(match[3]), frame: Number(match[4]) } : null;
      }).filter(Boolean);
    })));
    const sessions = events.map(list => list.filter(event => event.kind === "session"));
    const rounds = sessions.map(list => [...new Set(list.map(event => `${event.epoch}:${event.round}`))]);
    if (rounds.every(list => list.length >= 4)) {
      if (sessions.some(list => list.some(event => event.frame !== 0))) throw new Error("replacement session did not start at frame zero");
      const expected = JSON.stringify(rounds[0].slice(0, 4));
      if (rounds.some(list => JSON.stringify(list.slice(0, 4)) !== expected)) throw new Error("peers observed different rollover sequence");
      await Promise.all(pages.map(page => page.waitForTimeout(10_000)));
      if (failure) throw failure;
      console.log(`PASS: two real WASM/GGRS peers completed ${rounds[0].length - 1} rollovers at frame zero`);
      passed = true;
      break;
    }
    await pages[0].waitForTimeout(250);
  }
  if (!passed) throw new Error("timed out waiting for four real GGRS sessions");
} catch (error) {
  failure = error; process.exitCode = 1; console.error(error.stack ?? error);
  await Promise.all(pages.map((page, index) => page.screenshot({ path: path.join(out, `failure-${index + 1}.png`), fullPage: true }).catch(() => {})));
} finally {
  await writeFile(path.join(out, "logs.json"), JSON.stringify({ failure: failure ? String(failure.stack ?? failure) : null, logs }, null, 2));
  await browser.close();
}
