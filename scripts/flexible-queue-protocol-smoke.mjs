// Browser-level protocol-v4 dynamic public queue harness. Run only against a
// local Wrangler instance. It exercises the public WebSocket contract rather
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
  constructor(page, preference) {
    this.page = page; this.id = randomBytes(8).toString("hex"); this.events = [];
    this.preference = preference;
    this.url = `${worker}/queue/devbattle-0-6-0?protocol=4&preference=${preference}`;
  }
  async open() {
    await this.page.evaluate(({ id, url }) => {
      const ws = new WebSocket(url); window.sockets.set(id, ws);
      ws.onmessage = event => window.dispatchQueue(id, "message", JSON.parse(event.data));
      ws.onclose = event => window.dispatchQueue(id, "close", { code: event.code, reason: event.reason });
    }, { id: this.id, url: this.url });
    const queued = await this.wait("queued");
    assert(queued.protocol === 4 && queued.preference === this.preference && !("target" in queued), "invalid target-free acknowledgement");
    return this;
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
async function connect(page, preference) { const client = new Client(page, preference); clients.push(client); return client.open(); }
async function assigned(client, mode, capacity) {
  const value = await client.wait("assigned");
  assert(value.protocol === 4 && value.mode === mode && value.capacity === capacity, "wrong assignment");
  assert(/^q4_[0-9a-f]{32}$/.test(value.room) && /^[0-9a-f]{64}$/.test(value.token), "unbounded assignment scalars");
  return value;
}
async function staging(client, count) {
  return client.wait("status", value => {
    const valid = value.status === "staging" && value.count === count && Number.isInteger(value.votes) &&
      value.votes >= 0 && value.votes <= count && value.votesRequired === Math.floor(count / 2) + 1 &&
      Number.isSafeInteger(value.deadline) && value.deadline > Date.now() && typeof value.voted === "boolean";
    return valid;
  });
}
async function resetQueue(page) {
  await Promise.allSettled(clients.map(client => client.close()));
  clients.length = 0;
  await page.waitForTimeout(100);
}

try {
  const page = await setup();

  // Duel remains immediate and has no public roster-size target.
  const duelA = await connect(page, "duel"); const duelB = await connect(page, "duel");
  await Promise.all([assigned(duelA, "duel", 2), assigned(duelB, "duel", 2)]);
  await resetQueue(page);

  // Three compatible players establish a dynamic LGS stage. Deadline is fixed,
  // votes are recipient-specific, and joins increase the strict majority.
  const group = [await connect(page, "deathmatch"), await connect(page, "any"), await connect(page, "deathmatch")];
  const initial = await Promise.all(group.map(client => staging(client, 3)));
  const deadline = initial[0].deadline;
  assert(initial.every(value => value.deadline === deadline && value.votesRequired === 2 && value.voted === false), "inconsistent initial stage");
  await group[0].send({ type: "vote_start" });
  const voted = await group[0].wait("status", value => value.status === "staging" && value.votes === 1 && value.voted === true);
  assert(voted.deadline === deadline, "vote reset fixed deadline");
  const fourth = await connect(page, "any"); group.push(fourth);
  const joined = await staging(fourth, 4);
  assert(joined.deadline === deadline && joined.votes === 1 && joined.votesRequired === 3 && !joined.voted, "dynamic join/vote status wrong");
  await group[0].send({ type: "withdraw_start_vote" });
  const withdrawn = await group[0].wait("status", value => value.status === "staging" && value.count === 4 && value.votes === 0 && value.voted === false);
  assert(withdrawn.deadline === deadline, "withdrawal reset fixed deadline");
  await Promise.all(group.slice(0, 3).map(client => client.send({ type: "vote_start" })));
  await Promise.all(group.map(client => assigned(client, "deathmatch", 4)));
  await resetQueue(page);

  // Fill-to-eight auto-starts and proves every dynamic capacity stays bounded.
  const eight = [];
  for (let index = 0; index < 8; index++) eight.push(await connect(page, "deathmatch"));
  await Promise.all(eight.map(client => assigned(client, "deathmatch", 8)));
  await resetQueue(page);

  // Cancellation removes a staged voter and publishes recomputed status.
  const cancelGroup = [await connect(page, "deathmatch"), await connect(page, "deathmatch"), await connect(page, "deathmatch"), await connect(page, "deathmatch")];
  await Promise.all(cancelGroup.map(client => staging(client, 4)));
  // Discard the transient three-player snapshots emitted while the fourth
  // socket was still opening; the next count-three status must be the shrink.
  for (const client of cancelGroup) client.events.length = 0;
  await cancelGroup[0].send({ type: "vote_start" });
  await cancelGroup[0].wait("status", value => value.status === "staging" && value.voted && value.votes === 1);
  await cancelGroup[0].send({ type: "cancel" }); await cancelGroup[0].wait("cancelled");
  const shrunk = await staging(cancelGroup[1], 3);
  assert(shrunk.votes === 0 && shrunk.votesRequired === 2, "staged cancellation did not remove vote");
  await resetQueue(page);

  const source = await readFile(new URL("../src/cloudflare_net.rs", import.meta.url), "utf8");
  assert(source.includes('if (phase === "queue_wait") return null'), "queue wait timeout policy regressed");
  assert(!source.includes(`&tar${"get"}=\${tar${"get"}}`), "obsolete public roster-size URL remains");
  assert(!source.includes(`message.tar${"get"} !== tar${"get"}`), "obsolete roster-size acknowledgement validation remains");
  console.log("PASS: protocol-v4 dynamic staging, voting, fixed deadlines, cancellation, and target-free queue");
} finally {
  await Promise.allSettled(clients.map(client => client.close()));
  await browser?.close();
}
