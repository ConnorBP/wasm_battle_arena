import { chromium } from "playwright";
import { mkdir, writeFile } from "node:fs/promises";
import { randomBytes } from "node:crypto";
import path from "node:path";

const workerUrl = (process.env.WORKER_URL ?? "ws://127.0.0.1:8787").replace(/\/$/, "");
const origin = (process.env.ORIGIN ?? "http://127.0.0.1:4173").replace(/\/$/, "");
const timeoutMs = Number(process.env.PROTOCOL_TIMEOUT_MS ?? 20_000);
const artifactDir = path.resolve(process.env.ARTIFACT_DIR ?? "artifacts/local-multiplayer-smoke", "protocol");
const transcript = [];
const clients = [];
let browser;
let page;

function assert(condition, message) {
  if (!condition) throw new Error(message);
}
function hexNonce() {
  return randomBytes(16).toString("hex");
}
function roomName(label) {
  return `local-${label}-${Date.now().toString(36)}-${randomBytes(3).toString("hex")}`;
}
function sleep(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}
function canonical(value) {
  return JSON.stringify(value);
}

class ProtocolClient {
  constructor(id) {
    this.id = id;
    this.events = [];
    this.playerId = null;
    clients.push(this);
  }
  mark() {
    return this.events.length;
  }
  receive(kind, payload) {
    const event = { at: new Date().toISOString(), client: this.id, kind, payload };
    this.events.push(event);
    transcript.push(event);
    if (kind === "message" && payload.type === "welcome") this.playerId = payload.playerId;
  }
  async waitFor(type, predicate = () => true, since = 0, timeout = timeoutMs) {
    const deadline = Date.now() + timeout;
    for (;;) {
      const event = this.events.slice(since).find(entry =>
        entry.kind === "message" && entry.payload.type === type && predicate(entry.payload));
      if (event) return event.payload;
      const closed = this.events.slice(since).find(entry => entry.kind === "close");
      if (closed) throw new Error(`${this.id}: socket closed while waiting for ${type}: ${canonical(closed.payload)}`);
      if (Date.now() >= deadline) {
        const tail = this.events.slice(-12).map(entry => `${entry.kind} ${canonical(entry.payload)}`).join("\n");
        throw new Error(`${this.id}: timed out waiting for ${type}\n${tail}`);
      }
      await sleep(25);
    }
  }
  async send(message) {
    transcript.push({ at: new Date().toISOString(), client: this.id, kind: "send", payload: message });
    await page.evaluate(({ id, message }) => {
      const socket = window.__protocolSockets.get(id);
      if (!socket || socket.readyState !== WebSocket.OPEN) throw new Error(`socket ${id} is not open`);
      socket.send(JSON.stringify(message));
    }, { id: this.id, message });
  }
  async close() {
    await page.evaluate(id => {
      const socket = window.__protocolSockets?.get(id);
      if (socket && socket.readyState < WebSocket.CLOSING) socket.close(1000, "harness cleanup");
    }, this.id).catch(() => {});
  }
}

async function connect(room, mode, capacity, label) {
  const client = new ProtocolClient(`${label}-${clients.length + 1}`);
  const query = new URLSearchParams({ protocol: "3", mode, capacity: String(capacity) });
  const url = `${workerUrl}/lobby/${room}?${query}`;
  await page.evaluate(({ id, url }) => {
    const socket = new WebSocket(url);
    window.__protocolSockets.set(id, socket);
    socket.addEventListener("open", () => window.__protocolDispatch(id, "open", null));
    socket.addEventListener("message", event => {
      let payload;
      try { payload = JSON.parse(event.data); }
      catch { payload = { type: "invalid_json", raw: String(event.data) }; }
      window.__protocolDispatch(id, "message", payload);
    });
    socket.addEventListener("error", () => window.__protocolDispatch(id, "socket_error", null));
    socket.addEventListener("close", event => window.__protocolDispatch(id, "close", { code: event.code, reason: event.reason }));
  }, { id: client.id, url });
  const welcome = await client.waitFor("welcome");
  assert(welcome.protocol === 3, `${client.id}: wrong protocol welcome`);
  assert(/^[0-9a-f]{32}$/.test(welcome.playerId), `${client.id}: invalid player id`);
  assert(/^[0-9a-f]{32}$/.test(welcome.reconnectToken), `${client.id}: invalid reconnect token`);
  return client;
}

async function formLobby(mode, capacity, label) {
  const room = roomName(label);
  const members = [];
  for (let index = 0; index < capacity; index++) {
    const client = await connect(room, mode, capacity, label);
    members.push(client);
    await client.send({ type: "profile", name: `${label}-${index + 1}`, paletteId: index % 4, cosmeticId: 0 });
    await client.waitFor("profile_accepted");
    await client.send({ type: "ready" });
  }
  const starts = await Promise.all(members.map(client => client.waitFor("start")));
  const expected = starts[0];
  assert(expected.protocol === 3 && expected.mode === mode && expected.capacity === capacity,
    `${label}: malformed start metadata`);
  assert(expected.epoch === 0 && expected.round === 0, `${label}: initial epoch/round was not 0/0`);
  assert(/^[0-9a-f]{32}$/.test(expected.seed), `${label}: invalid seed`);
  assert(expected.roster.length === capacity, `${label}: wrong roster size`);
  const ids = expected.roster.map(entry => entry.playerId);
  assert(canonical(ids) === canonical([...ids].sort()), `${label}: roster is not canonical`);
  assert(new Set(ids).size === capacity, `${label}: duplicate roster identity`);
  expected.roster.forEach((entry, index) => {
    assert(entry.index === index, `${label}: non-canonical roster index`);
    assert(entry.score === 0, `${label}: initial score was not zero`);
    assert(entry.profile?.name === `${label}-${members.findIndex(member => member.playerId === entry.playerId) + 1}`,
      `${label}: profile snapshot did not match identity`);
  });
  for (const start of starts) assert(canonical(start) === canonical(expected), `${label}: players received different immutable starts`);
  console.log(`PASS: ${mode} ${capacity}-player immutable roster`);
  return { room, members, start: expected };
}

function outcomes(start, winnerId = null, winnerDelta = 0) {
  return start.roster.map((entry, index) => ({
    playerId: entry.playerId,
    placement: winnerId === entry.playerId ? 1 : index + 1,
    scoreDelta: winnerId === entry.playerId ? winnerDelta : 0,
  }));
}

async function exerciseRoundIdempotence(lobby) {
  const [first, ...rest] = lobby.members;
  const report = { type: "report", epoch: lobby.start.epoch, round: lobby.start.round, outcomes: outcomes(lobby.start) };
  let mark = first.mark();
  await first.send(report);
  const firstAck = await first.waitFor("report_ack", () => true, mark);
  assert(firstAck.duplicate === false && firstAck.received === 1, "first report was not accepted once");
  mark = first.mark();
  await first.send(report);
  const duplicate = await first.waitFor("report_ack", message => message.duplicate === true, mark);
  assert(duplicate.required === lobby.members.length, "duplicate report changed consensus requirement");

  const commitMarks = new Map(lobby.members.map(client => [client, client.mark()]));
  for (const client of rest) await client.send(report);
  const commits = await Promise.all(lobby.members.map(client => client.waitFor("round_commit", () => true, commitMarks.get(client))));
  commits.forEach(commit => assert(commit.epoch === lobby.start.epoch && commit.round === lobby.start.round,
    "commit addressed the wrong round"));
  const nextStarts = await Promise.all(lobby.members.map(client => client.waitFor("start",
    message => message.epoch === lobby.start.epoch && message.round === lobby.start.round + 1, commitMarks.get(client))));
  lobby.start = nextStarts[0];
  nextStarts.forEach(start => assert(canonical(start.roster.map(entry => entry.playerId)) === canonical(lobby.start.roster.map(entry => entry.playerId)),
    "ordinary next round changed immutable membership"));

  // Exercise another ordinary rollover on the same long-lived control sockets.
  // This catches clients which survive one close/recreate but accidentally
  // close the promoted-new epoch on the following cleanup.
  const secondReport = { type: "report", epoch: lobby.start.epoch, round: lobby.start.round, outcomes: outcomes(lobby.start) };
  const secondMarks = new Map(lobby.members.map(client => [client, client.mark()]));
  for (const client of lobby.members) await client.send(secondReport);
  await Promise.all(lobby.members.map(client => client.waitFor("round_commit", message =>
    message.epoch === secondReport.epoch && message.round === secondReport.round, secondMarks.get(client))));
  const thirdStarts = await Promise.all(lobby.members.map(client => client.waitFor("start", message =>
    message.epoch === secondReport.epoch && message.round === secondReport.round + 1, secondMarks.get(client))));
  lobby.start = thirdStarts[0];
  thirdStarts.forEach(start => assert(canonical(start) === canonical(lobby.start),
    "second consecutive round rollover diverged across clients"));

  mark = first.mark();
  await first.send(report);
  await first.waitFor("report_ack", message => message.duplicate === true, mark);
  const conflicting = structuredClone(report);
  conflicting.outcomes[0].scoreDelta = 1;
  mark = first.mark();
  await first.send(conflicting);
  await first.waitFor("error", message => message.error === "conflicting_terminal_report", mark);
  mark = first.mark();
  await first.send({
    type: "signal", epoch: lobby.start.epoch + 99, to: rest[0].playerId,
    data: { type: "ice", candidate: null },
  });
  await first.waitFor("error", message => message.error === "stale_or_invalid_signal", mark);
  console.log("PASS: duplicate, terminal, stale epoch, and consecutive round rollover behavior");
}

async function finishMatch(lobby) {
  const winner = lobby.members[0].playerId;
  const report = {
    type: "report", epoch: lobby.start.epoch, round: lobby.start.round,
    outcomes: outcomes(lobby.start, winner, 3),
  };
  const marks = new Map(lobby.members.map(client => [client, client.mark()]));
  for (const client of lobby.members) await client.send(report);
  const commits = await Promise.all(lobby.members.map(client => client.waitFor("round_commit", () => true, marks.get(client))));
  commits.forEach(commit => assert(commit.scores.find(score => score.playerId === winner)?.score >= 3,
    "match-point score did not commit"));
  await Promise.all(lobby.members.map(client => client.waitFor("match_over", () => true, marks.get(client))));
}

async function expectNewStart(lobby, marks, generation) {
  const starts = await Promise.all(lobby.members.map(client => client.waitFor("start",
    message => message.matchGeneration === generation, marks.get(client))));
  const expected = starts[0];
  starts.forEach(start => assert(canonical(start) === canonical(expected), "rematch start differed across roster"));
  assert(expected.epoch > lobby.start.epoch && expected.round === 0, "rematch did not install a new epoch");
  assert(expected.seed !== lobby.start.seed, "rematch did not advance the match seed");
  assert(expected.roster.every(entry => entry.score === 0), "rematch did not reset scores");
  assert(canonical(expected.roster.map(entry => entry.playerId)) === canonical(lobby.start.roster.map(entry => entry.playerId)),
    "rematch changed the same-lobby roster");
  lobby.start = expected;
}

async function restartAfterDenial(lobby) {
  const marks = new Map(lobby.members.map(client => [client, client.mark()]));
  for (const client of lobby.members) await client.send({ type: "ready" });
  const starts = await Promise.all(lobby.members.map(client => client.waitFor("start", () => true, marks.get(client))));
  lobby.start = starts[0];
  starts.forEach(start => assert(canonical(start) === canonical(lobby.start), "restart after denial differed across players"));
}

async function exerciseRematches(lobby) {
  const [first, second] = lobby.members;
  await finishMatch(lobby);

  // A request is an acceptance. Repeating it is idempotent, while the other
  // player's simultaneous request accepts the first authoritative nonce.
  let nonce = hexNonce();
  let marks = new Map(lobby.members.map(client => [client, client.mark()]));
  await first.send({ type: "rematch_request", generation: 1, nonce });
  const pending = await first.waitFor("rematch_pending", message => message.nonce === nonce, marks.get(first));
  assert(pending.accepted.length === 1 && pending.required === 2, "rematch request did not count as one acceptance");
  let duplicateMark = first.mark();
  await first.send({ type: "rematch_request", generation: 1, nonce });
  const duplicatePending = await first.waitFor("rematch_pending", message => message.nonce === nonce, duplicateMark);
  assert(duplicatePending.accepted.length === 1 && duplicatePending.deadline === pending.deadline,
    "duplicate rematch request changed vote or deadline");
  await second.send({ type: "rematch_request", generation: 1, nonce: hexNonce() });
  await Promise.all(lobby.members.map(client => client.waitFor("rematch_accepted", message => message.nonce === nonce, marks.get(client))));
  await expectNewStart(lobby, marks, 1);

  duplicateMark = second.mark();
  await second.send({ type: "rematch_response", generation: 1, nonce, accept: true });
  await second.waitFor("rematch_accepted", message => message.nonce === nonce, duplicateMark);
  let staleMark = first.mark();
  await first.send({ type: "rematch_response", generation: 1, nonce: hexNonce(), accept: true });
  await first.waitFor("error", message => message.error === "stale_rematch", staleMark);
  console.log("PASS: rematch request, duplicate, simultaneous request, and stale vote");

  await finishMatch(lobby);
  nonce = hexNonce();
  marks = new Map(lobby.members.map(client => [client, client.mark()]));
  await first.send({ type: "rematch_request", generation: 2, nonce });
  await first.waitFor("rematch_pending", message => message.nonce === nonce, marks.get(first));
  await second.send({ type: "rematch_response", generation: 2, nonce, accept: true });
  await Promise.all(lobby.members.map(client => client.waitFor("rematch_accepted", message => message.nonce === nonce, marks.get(client))));
  await expectNewStart(lobby, marks, 2);
  console.log("PASS: explicit rematch acceptance");

  await finishMatch(lobby);
  nonce = hexNonce();
  marks = new Map(lobby.members.map(client => [client, client.mark()]));
  await first.send({ type: "rematch_request", generation: 3, nonce });
  await first.waitFor("rematch_pending", message => message.nonce === nonce, marks.get(first));
  await second.send({ type: "rematch_response", generation: 3, nonce, accept: false });
  const denied = await Promise.all(lobby.members.map(client => client.waitFor("rematch_denied", message => message.reason === "denied", marks.get(client))));
  denied.forEach(message => assert(message.destination === "main_menu", "denial did not release roster to menu"));
  console.log("PASS: rematch denial");

  await restartAfterDenial(lobby);
  await finishMatch(lobby);
  nonce = hexNonce();
  marks = new Map(lobby.members.map(client => [client, client.mark()]));
  await first.send({ type: "rematch_request", generation: 4, nonce });
  const timeoutPending = await first.waitFor("rematch_pending", message => message.nonce === nonce, marks.get(first));
  const timeoutBudget = Math.max(1_000, timeoutPending.deadline - Date.now() + 5_000);
  await Promise.all(lobby.members.map(client => client.waitFor("rematch_denied",
    message => message.reason === "timeout", marks.get(client), timeoutBudget)));
  console.log("PASS: rematch timeout");

  await restartAfterDenial(lobby);
  await finishMatch(lobby);
  nonce = hexNonce();
  marks = new Map(lobby.members.map(client => [client, client.mark()]));
  await first.send({ type: "rematch_request", generation: 5, nonce });
  await first.waitFor("rematch_pending", message => message.nonce === nonce, marks.get(first));
  await second.close();
  const disconnected = await first.waitFor("rematch_denied",
    message => message.reason === "participant_disconnected", marks.get(first));
  assert(disconnected.destination === "main_menu", "disconnect denial did not return to menu");
  console.log("PASS: disconnect denies rematch");
}

async function main() {
  await mkdir(artifactDir, { recursive: true });
  browser = await chromium.launch({
    headless: true,
    args: ["--enable-unsafe-swiftshader", "--use-angle=swiftshader", "--ignore-gpu-blocklist", "--disable-web-security", "--allow-insecure-localhost"],
  });
  const context = await browser.newContext();
  page = await context.newPage();
  await page.exposeFunction("__protocolDispatch", (id, kind, payload) => {
    const client = clients.find(candidate => candidate.id === id);
    if (client) client.receive(kind, payload);
  });
  await page.route(`${origin}/**`, route => route.fulfill({
    status: 200,
    contentType: "text/html",
    body: "<!doctype html><title>local protocol harness</title>",
  }));
  await page.goto(`${origin}/__protocol_harness__`, { waitUntil: "domcontentloaded" });
  await page.evaluate(() => { window.__protocolSockets = new Map(); });

  const duel = await formLobby("duel", 2, "duel2");
  await formLobby("deathmatch", 3, "death3");
  await formLobby("deathmatch", 4, "death4");
  const eight = await formLobby("deathmatch", 8, "death8");

  const incumbentMarks = new Map(eight.members.map(client => [client, client.mark()]));
  const queued = await connect(eight.room, "deathmatch", 8, "death8-queued");
  const queuedStatusMark = queued.mark();
  await queued.send({ type: "profile", name: "queued-excess", paletteId: 0, cosmeticId: 0 });
  await queued.waitFor("profile_accepted", () => true, queuedStatusMark);
  await queued.send({ type: "ready" });
  const status = await queued.waitFor("status", message => message.ready === true, queuedStatusMark);
  assert(status.status === "active" && status.active?.epoch === eight.start.epoch, "excess player did not remain waiting beside active epoch");
  await sleep(750);
  assert(!queued.events.slice(queuedStatusMark).some(event => event.kind === "message" && event.payload.type === "start"),
    "excess player entered immutable active roster");
  for (const client of eight.members) {
    assert(!client.events.slice(incumbentMarks.get(client)).some(event => event.kind === "message" && event.payload.type === "start"),
      "mid-round join replaced an incumbent start");
  }
  const queuedErrorMark = queued.mark();
  await queued.send({
    type: "signal", epoch: eight.start.epoch, to: eight.members[0].playerId,
    data: { type: "ice", candidate: null },
  });
  await queued.waitFor("error", message => message.error === "stale_or_invalid_signal", queuedErrorMark);
  console.log("PASS: excess player queued without mutating 8-player roster");

  await exerciseRoundIdempotence(duel);
  await exerciseRematches(duel);
  console.log("PASS: bounded local multiplayer protocol suite");
}

let failure;
try {
  await main();
} catch (error) {
  failure = error;
  console.error(error?.stack ?? error);
  if (page) await page.screenshot({ path: path.join(artifactDir, "failure.png"), fullPage: true }).catch(() => {});
  process.exitCode = 1;
} finally {
  await mkdir(artifactDir, { recursive: true });
  await writeFile(path.join(artifactDir, "transcript.json"), JSON.stringify({
    workerUrl, origin, failure: failure ? String(failure.stack ?? failure) : null, transcript,
  }, null, 2));
  if (page) await Promise.allSettled(clients.map(client => client.close()));
  if (browser) await browser.close().catch(() => {});
}
