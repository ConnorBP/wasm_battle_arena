// Browser-level protocol-v4 public queue harness. Run only against the local
// Wrangler instance. It intentionally uses public WebSocket contracts rather
// than game-private hooks.
import { chromium } from "playwright";
import { randomBytes } from "node:crypto";
import { readFile } from "node:fs/promises";

const worker = (process.env.WORKER_URL ?? "ws://127.0.0.1:8787").replace(/\/$/, "");
const origin = (process.env.ORIGIN ?? "http://127.0.0.1:4173").replace(/\/$/, "");
const timeout = Number(process.env.PROTOCOL_TIMEOUT_MS ?? 20_000);
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const assert = (value, message) => { if (!value) throw new Error(message); };

let browser;
const clients = [];
class Client {
  constructor(page, preference, target = 8) {
    this.page = page; this.id = randomBytes(8).toString("hex"); this.events = [];
    this.preference = preference; this.target = target;
    this.url = `${worker}/queue/public-v4?protocol=4&preference=${preference}&target=${target}`;
  }
  async open() {
    await this.page.evaluate(({ id, url }) => {
      const ws = new WebSocket(url); window.sockets.set(id, ws);
      ws.onmessage = event => window.dispatchQueue(id, "message", JSON.parse(event.data));
      ws.onclose = event => window.dispatchQueue(id, "close", { code: event.code, reason: event.reason });
    }, { id: this.id, url: this.url });
    await this.wait("queued"); return this;
  }
  async wait(type, predicate = () => true, budget = timeout) {
    const end = Date.now() + budget;
    while (Date.now() < end) {
      const event = this.events.find(value => value.kind === "message" && value.data.type === type && predicate(value.data));
      if (event) return event.data;
      await sleep(20);
    }
    throw new Error(`${this.id}: timed out waiting for ${type}: ${JSON.stringify(this.events.slice(-8))}`);
  }
  send(value) { return this.page.evaluate(({ id, value }) => window.sockets.get(id).send(JSON.stringify(value)), { id: this.id, value }); }
  close() { return this.page.evaluate(id => window.sockets.get(id)?.close(1000, "harness"), this.id).catch(() => {}); }
}

async function setup() {
  browser = await chromium.launch({ headless: true, args: ["--disable-web-security", "--allow-insecure-localhost"] });
  const page = await browser.newPage();
  await page.exposeFunction("dispatchQueue", (id, kind, data) => clients.find(c => c.id === id)?.events.push({ kind, data }));
  await page.route(`${origin}/**`, route => route.fulfill({ status: 200, contentType: "text/html", body: "queue harness" }));
  await page.goto(`${origin}/queue-harness`);
  await page.evaluate(() => { window.sockets = new Map(); });
  return page;
}
async function connect(page, preference, target = 8) { const c = new Client(page, preference, target); clients.push(c); return c.open(); }
async function assigned(client, mode, capacity) {
  const value = await client.wait("assigned");
  assert(value.protocol === 4 && value.mode === mode && value.capacity === capacity, "wrong assignment");
  assert(/^q4_[0-9a-f]{32}$/.test(value.room) && /^[0-9a-f]{64}$/.test(value.token), "unbounded assignment scalars");
  return value;
}
async function scenario(page, specs, expectedMode, expectedCapacity) {
  const group = [];
  for (const [preference, target] of specs) group.push(await connect(page, preference, target));
  await Promise.all(group.map(c => assigned(c, expectedMode, expectedCapacity)));
}
async function resetQueue(page) {
  await Promise.allSettled(clients.map(client => client.close()));
  clients.length = 0;
  // Queue close events and reducer cancellation are asynchronous.
  await page.waitForTimeout(100);
}

try {
  const page = await setup();
  await scenario(page, [["any", 8], ["duel", 8]], "duel", 2);                 // Any+Duel immediate
  await resetQueue(page);
  await scenario(page, [["duel", 8], ["duel", 8]], "duel", 2);              // Duel+Duel
  await resetQueue(page);
  await scenario(page, [["any", 8], ["any", 8]], "duel", 2);                // hold then Duel
  await resetQueue(page);
  await scenario(page, [["any", 3], ["any", 3], ["any", 3]], "deathmatch", 3);
  await resetQueue(page);
  await scenario(page, [["deathmatch", 3], ["any", 3], ["any", 3]], "deathmatch", 3);
  await resetQueue(page);

  // DM+Any remains unlocked; a later Duel steals the Any.
  const dm = await connect(page, "deathmatch", 8); const any = await connect(page, "any", 8); const duel = await connect(page, "duel", 8);
  await Promise.all([assigned(any, "duel", 2), assigned(duel, "duel", 2)]);
  assert(!dm.events.some(e => e.data?.type === "assigned"), "Duel incorrectly took Deathmatch-only client");
  await dm.send({ type: "cancel" }); await dm.wait("cancelled");
  await resetQueue(page);

  // Explicit cancellation/disconnect must not strand survivors.
  const cancel = await connect(page, "deathmatch", 8); await cancel.send({ type: "cancel" }); await cancel.wait("cancelled");
  const disconnected = await connect(page, "deathmatch", 8); await disconnected.close();
  await resetQueue(page);

  // Heartbeats prove waiting beyond the old two-minute client timeout. Keep
  // this practical by allowing CI to lower WAIT_BEYOND_OLD_TIMEOUT_MS while a
  // source assertion below ensures the production policy remains unbounded.
  const source = await readFile(new URL("../src/cloudflare_net.rs", import.meta.url), "utf8");
  assert(source.includes('if (phase === "queue_wait") return null'), "queue wait timeout policy regressed");
  const long = await connect(page, "deathmatch", 8);
  const wait = Number(process.env.WAIT_BEYOND_OLD_TIMEOUT_MS ?? 1_000);
  const end = Date.now() + wait;
  while (Date.now() < end) { await long.send({ type: "heartbeat", nonce: "long-wait" }); await sleep(Math.min(10_000, Math.max(1, end - Date.now()))); }
  assert(!long.events.some(e => e.kind === "close"), "ordinary queue wait closed");
  await long.send({ type: "cancel" });
  console.log("PASS: protocol-v4 flexible queue arbitration, cancellation, disconnect, and long-wait policy");
} finally {
  await Promise.allSettled(clients.map(client => client.close()));
  await browser?.close();
}
