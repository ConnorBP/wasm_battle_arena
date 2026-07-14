import { chromium } from "playwright";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";

const scenario = process.env.TRANSITION_SCENARIO ?? "rollover";
const supported = new Set(["rollover", "active_disconnect", "rollover_disconnect", "reconnect", "rematch", "requeue", "changed_roster"]);
if (!supported.has(scenario)) throw new Error(`unknown TRANSITION_SCENARIO: ${scenario}`);

const gameUrl = process.env.GAME_URL ?? "http://127.0.0.1:4173";
const room = (process.env.TRANSITION_ROOM ?? "GTTRANSITION").replace(/[^a-z0-9]/gi, "").slice(0, 16) || "GTTRANSITION";
const out = path.resolve(process.env.ARTIFACT_DIR ?? "artifacts/network-transition", scenario);
const timeout = Number(process.env.TRANSITION_TIMEOUT_MS ?? 240_000);
const playerCount = scenario === "changed_roster" ? 4 : 2;
const knownControlFlow = "Using exceptions for control flow, don't mind me. This isn't actually an error!";
const fatal = /panicked at|RuntimeError|unreachable|wasm trap|assertion failed|memory access out of bounds/i;
const expectedDisconnect = /peer disconnected|connection interrupted|lobby service disconnected|lobby peer disconnected/i;

await mkdir(out, { recursive: true });
const browser = await chromium.launch({
  headless: true,
  args: ["--enable-unsafe-swiftshader", "--use-angle=swiftshader", "--ignore-gpu-blocklist"],
});
const contexts = await Promise.all(Array.from({ length: playerCount }, () => browser.newContext({ viewport: { width: 1280, height: 720 } })));
const pages = await Promise.all(contexts.map(context => context.newPage()));
const records = pages.map(() => ({ logs: [], events: [], errors: [], closedIntentionally: false }));
let failure;
let note = null;

function assert(condition, message) {
  if (!condition) throw new Error(message);
}
function eventKey(event) { return `${event.epoch}:${event.round}`; }
function scoreMap(event) {
  return new Map((event.scores ?? []).map(entry => [entry.playerId, entry.score]));
}
function validateEvent(value, index) {
  assert(value && typeof value === "object" && value.schema === 1, `peer ${index + 1}: invalid transition event schema`);
  assert(value.scenario === scenario, `peer ${index + 1}: event scenario mismatch`);
  assert(typeof value.kind === "string" && value.kind.length <= 512, `peer ${index + 1}: unsafe event kind`);
  assert(typeof value.detail === "string" && value.detail.length <= 512, `peer ${index + 1}: unsafe event detail`);
  assert(value.identity === "" || /^[0-9a-f]{32}$/.test(value.identity), `peer ${index + 1}: invalid event identity`);
  assert(value.seed === "" || /^[0-9a-f]{32}$/.test(value.seed), `peer ${index + 1}: invalid event seed`);
  assert(Array.isArray(value.roster) && value.roster.length <= 8 && value.roster.every(id => /^[0-9a-f]{32}$/.test(id)), `peer ${index + 1}: invalid event roster`);
  assert(Array.isArray(value.scores) && value.scores.length <= 8 && value.scores.every(entry => /^[0-9a-f]{32}$/.test(entry.playerId) && Number.isInteger(entry.score) && entry.score >= 0), `peer ${index + 1}: invalid event scores`);
  for (const field of ["epoch", "round", "frame", "generation"])  assert(Number.isInteger(value[field]) && value[field] >= 0, `peer ${index + 1}: invalid ${field}`);
  return value;
}
function refreshEvents(index, list) {
  const known = records[index].events.length;
  if (list.length < known) records[index].events = [];
  for (const value of list.slice(records[index].events.length)) records[index].events.push(validateEvent(value, index));
}
function unexpectedDiagnostics() {
  return records.flatMap((record, index) => record.errors
    .filter(text => !(record.closedIntentionally && expectedDisconnect.test(text)))
    .map(text => `peer ${index + 1}: ${text}`));
}
async function snapshotEvents(activePages = pages) {
  await Promise.all(activePages.map(async page => {
    const index = pages.indexOf(page);
    if (page.isClosed()) return;
    const list = await page.evaluate(() => window.__ghostTransitionEvents ?? []);
    refreshEvents(index, list);
  }));
  const diagnostics = unexpectedDiagnostics();
  if (diagnostics.length) throw new Error(`fatal browser diagnostics:\n${diagnostics.join("\n")}`);
}
async function waitUntil(description, predicate, activePages = pages, limit = timeout) {
  const deadline = Date.now() + limit;
  while (Date.now() < deadline) {
    await snapshotEvents(activePages);
    if (predicate()) return;
    const live = activePages.find(page => !page.isClosed());
    if (!live) throw new Error(`${description}: no live browser page`);
    await live.waitForTimeout(100);
  }
  throw new Error(`timed out waiting for ${description}`);
}
function sessions(index) { return records[index].events.filter(event => event.kind === "session"); }
function returnedToMenu(index) { return records[index].events.some(event => event.kind === "menu" && event.detail === "returned"); }
async function command(page, name) {
  const accepted = await page.evaluate(commandName => window.__ghostTransitionApi?.[commandName]?.() === true, name);
  assert(accepted, `${name} command was not accepted by the feature-only browser bridge`);
}
async function closePeer(index) {
  records[index].closedIntentionally = true;
  await contexts[index].close();
}

pages.forEach((page, index) => {
  page.on("console", message => {
    const text = message.text();
    records[index].logs.push({ type: message.type(), text });
    if (fatal.test(text) || (expectedDisconnect.test(text) && !records[index].closedIntentionally && !["active_disconnect", "rollover_disconnect", "reconnect"].includes(scenario))) {
      records[index].errors.push(text);
    }
  });
  page.on("pageerror", error => {
    const text = String(error?.stack ?? error);
    if (!text.includes(knownControlFlow)) records[index].errors.push(text);
  });
});

try {
  const url = new URL(gameUrl);
  url.searchParams.set("ghost_transition", scenario);
  url.searchParams.set("ghost_room", room);
  const initialPages = scenario === "changed_roster" ? pages.slice(0, 3) : pages;
  await Promise.all(initialPages.map(page => page.goto(url.href, { waitUntil: "domcontentloaded", timeout: 120_000 })));
  await waitUntil("feature-only harness startup", () => initialPages.every(page => records[pages.indexOf(page)].events.some(event => event.kind === "harness_ready")), initialPages);

  if (scenario === "rollover") {
    await waitUntil("four real GGRS sessions on both peers", () => sessions(0).length >= 4 && sessions(1).length >= 4);
    const sequences = [0, 1].map(index => sessions(index).slice(0, 4).map(eventKey));
    assert(JSON.stringify(sequences[0]) === JSON.stringify(sequences[1]), "peers observed different rollover sequence");
    assert(sessions(0).slice(0, 4).every(event => event.frame === 0) && sessions(1).slice(0, 4).every(event => event.frame === 0), "replacement session did not start at frame zero");
    console.log(`PASS rollover: ${sequences[0].join(" -> ")}`);
  }

  if (scenario === "active_disconnect") {
    await waitUntil("both peers in an active round", () => records.slice(0, 2).every(record => record.events.some(event => event.kind === "checkpoint")));
    await closePeer(1);
    await waitUntil("survivor clean return to main menu", () => returnedToMenu(0), [pages[0]]);
    assert(sessions(0).length === 1, "active disconnect unexpectedly installed a replacement session");
    console.log("PASS active_disconnect: one browser closed mid-round and survivor returned to menu");
  }

  if (scenario === "rollover_disconnect") {
    // RAF polling reacts inside the >=75ms reset barrier; the general harness
    // polling interval is intentionally not used for this injection point.
    await pages[1].waitForFunction(() => (window.__ghostTransitionEvents ?? []).some(event => event.kind === "reset_barrier"), null, { timeout });
    await snapshotEvents();
    const barrier = records[1].events.find(event => event.kind === "reset_barrier");
    await closePeer(1);
    await waitUntil("survivor clean return after rollover disconnect", () => returnedToMenu(0), [pages[0]], timeout);
    assert(barrier.state === "matchmaking", "disconnect was not injected during the reset barrier");
    console.log(`PASS rollover_disconnect: browser closed during reset barrier for ${eventKey(barrier)}`);
  }

  if (scenario === "reconnect") {
    await waitUntil("a committed score before reconnect", () => sessions(0).some(event => [...scoreMap(event).values()].some(score => score > 0)) && sessions(1).some(event => [...scoreMap(event).values()].some(score => score > 0)));
    const before = [0, 1].map(index => sessions(index).at(-1));
    const identities = before.map(event => event.identity);
    const scores = before.map(event => JSON.stringify([...scoreMap(event)]));
    assert(identities.every(Boolean) && identities[0] !== identities[1], "pre-reload identities were not distinct and observable");
    // Reload both members together so the immutable active roster can rebuild
    // its full WebRTC mesh; sessionStorage supplies each rotating credential.
    await Promise.all(pages.map(page => page.reload({ waitUntil: "domcontentloaded", timeout: 120_000 })));
    records.forEach(record => { record.events = []; });
    await waitUntil("reconnected sessions and continued round", () => sessions(0).some(event => event.kind === "session") && sessions(1).some(event => event.kind === "session") && records.slice(0, 2).every(record => record.events.some(event => event.kind === "checkpoint")));
    const after = [0, 1].map(index => sessions(index)[0]);
    after.forEach((event, index) => {
      assert(event.identity === identities[index], `peer ${index + 1} identity changed across grace reconnect`);
      assert(JSON.stringify([...scoreMap(event)]) === scores[index], `peer ${index + 1} score snapshot changed across reconnect`);
      assert(eventKey(event) === eventKey(before[index]), `peer ${index + 1} reconnect created a new round`);
    });
    console.log("PASS reconnect: page reloads preserved both identities, score snapshot, active round, and continued GGRS");
  }

  if (scenario === "rematch") {
    await waitUntil("first-to-three match endpoint", () => records.slice(0, 2).every(record => record.events.some(event => event.kind === "match_over")));
    const endpoints = [0, 1].map(index => records[index].events.find(event => event.kind === "match_over"));
    assert([...scoreMap(endpoints[0]).values()].some(score => score === 3), "endpoint did not reach first-to-three");
    await Promise.all(pages.map(page => command(page, "rematch")));
    await waitUntil("accepted rematch replacement sessions", () => [0, 1].every(index => sessions(index).some(event => event.generation > endpoints[index].generation)));
    [0, 1].forEach(index => {
      const next = sessions(index).find(event => event.generation > endpoints[index].generation);
      assert(next.epoch > endpoints[index].epoch && next.round === 0, `peer ${index + 1}: rematch did not start a new generation epoch`);
      assert([...scoreMap(next).values()].every(score => score === 0), `peer ${index + 1}: rematch scores were not reset`);
      assert(next.seed && next.seed !== endpoints[index].seed, `peer ${index + 1}: rematch seed was not fresh`);
      assert(records[index].events.some(event => event.kind === "rematch_api" && event.detail === "sent"), `peer ${index + 1}: real rematch API was not invoked`);
    });
    console.log("PASS rematch: both real client APIs produced a new generation, zero scores, and fresh seed");
  }

  if (scenario === "requeue") {
    await waitUntil("match endpoint before requeue", () => records.slice(0, 2).every(record => record.events.some(event => event.kind === "match_over")));
    await command(pages[0], "requeue");
    await waitUntil("requester fresh protocol-v4 queue connection", () => records[0].events.some(event => event.kind === "fresh_queue"), [pages[0]]);
    assert(records[0].events.some(event => event.kind === "requeue_api" && event.detail === "sent"), "real requeue API was not invoked");
    console.log("PASS requeue: real client API left the match and opened a fresh queue connection");
  }

  if (scenario === "changed_roster") {
    // Establish the immutable capacity-three roster before admitting the waiter;
    // otherwise a simultaneous fourth join can receive a start broadcast while
    // its identity is not in that immutable roster.
    await waitUntil("initial three-player real LGS", () => [0, 1, 2].every(index => sessions(index).some(event => event.roster.length === 3)), pages.slice(0, 3));
    await pages[3].goto(url.href, { waitUntil: "domcontentloaded", timeout: 120_000 });
    await waitUntil("waiting fourth admission", () => records[3].events.some(event => event.kind === "matchmaking") && sessions(3).length === 0);
    const activeRosters = [0, 1, 2].map(index => sessions(index)[0].roster.join(","));
    assert(activeRosters.every(value => value === activeRosters[0]), "three incumbents did not share one immutable LGS roster");
    const capability = await pages[0].evaluate(() => window.__ghostTransitionApi?.capabilities?.());
    assert(capability?.changedRosterBoundaryDeparture === false, "unexpected changed-roster hook capability value");
    note = "Worker exposes no boundary-only incumbent-departure client API: leave aborts/releases the whole active roster. Verified real 3-player LGS plus waiting fourth; boundary roster replacement is explicitly skipped.";
    console.log(`PASS changed_roster: real three-player LGS formed with one waiting player. SKIP boundary departure: ${note}`);
  }

  await snapshotEvents(pages.filter(page => !page.isClosed()));
} catch (error) {
  failure = error;
  process.exitCode = 1;
  console.error(error?.stack ?? error);
  await Promise.all(pages.map((page, index) => page.isClosed() ? null : page.screenshot({ path: path.join(out, `failure-${index + 1}.png`), fullPage: true }).catch(() => {})));
} finally {
  await writeFile(path.join(out, "result.json"), JSON.stringify({
    scenario, room, failure: failure ? String(failure.stack ?? failure) : null, note, records,
  }, null, 2));
  await browser.close().catch(() => {});
}
