import test from "node:test";
import assert from "node:assert/strict";
import {
  createEpochState, startNextEpoch, submitReport, selectNextRoster,
  markActiveReconnect, rolloverActiveReconnect,
} from "../src/epoch-state.js";
import { rotateReconnectIdentity } from "../vendor/cloudflare-game-common/lifecycle.js";

function player(id, joinedAt) {
  return { playerId: id, joinedAt, connected: true, ready: true, expired: false, profile: { name: id, paletteId: 0, cosmeticId: 0 }, score: 0, reconnectUntil: null };
}
function report(active, winner = active.roster[0].playerId) {
  return active.roster.map((entry, index) => ({ playerId: entry.playerId, placement: index + 1, scoreDelta: entry.playerId === winner ? 1 : 0 }));
}
function complete(state, seed = "1".repeat(32)) {
  const outcomes = report(state.active);
  let result;
  for (const entry of [...state.active.roster]) result = submitReport(state, entry.playerId, state.epoch, state.round, outcomes, seed);
  return result;
}

test("start requires profiles and ready regardless of arrival order", () => {
  const state = createEpochState("duel", 2, 0);
  state.players.a = player("a", 1);
  state.players.b = { ...player("b", 2), profile: null };
  assert.equal(startNextEpoch(state, "0".repeat(32), "ready_before_profile"), null);
  state.players.b.profile = { name: "b", paletteId: 0, cosmeticId: 0 };
  assert.ok(startNextEpoch(state, "0".repeat(32), "profile_after_ready"));
});

test("ready never replaces an active epoch and join waits for the next epoch", () => {
  const state = createEpochState("deathmatch", 3, 0);
  state.players.a = player("a", 1); state.players.b = player("b", 2); state.players.c = player("c", 3);
  const first = startNextEpoch(state, "0".repeat(32), "initial");
  const immutable = structuredClone(first);
  state.players.d = player("d", 4);
  assert.equal(startNextEpoch(state, "f".repeat(32), "late_ready"), null);
  assert.deepEqual(state.active, immutable);
  state.players.c.ready = false;
  const committed = complete(state);
  assert.equal(committed.type, "commit");
  assert.deepEqual(committed.next.roster.map((entry) => entry.playerId), ["a", "b", "d"]);
  assert.equal(committed.next.epoch, 1);
  assert.equal(committed.next.round, 0);
});

test("unchanged roster advances round while changed roster advances epoch", () => {
  const state = createEpochState("duel", 2, 0);
  state.players.a = player("a", 1); state.players.b = player("b", 2);
  startNextEpoch(state, "0".repeat(32), "initial");
  const second = complete(state).next;
  assert.deepEqual([second.epoch, second.round], [0, 1]);
  state.players.b.ready = false; state.players.c = player("c", 3);
  const third = complete(state, "2".repeat(32)).next;
  assert.deepEqual(third.roster.map((entry) => entry.playerId), ["a", "c"]);
  assert.deepEqual([third.epoch, third.round], [1, 0]);
});

test("duplicate reports are idempotent and conflicting reports abort without scores", () => {
  const state = createEpochState("duel", 2, 0);
  state.players.a = player("a", 1); state.players.b = player("b", 2);
  startNextEpoch(state, "0".repeat(32), "initial");
  const one = report(state.active, "a");
  const two = report(state.active, "b");
  assert.deepEqual(submitReport(state, "a", 0, 0, one, "1".repeat(32)), { type: "ack", epoch: 0, round: 0, duplicate: false, received: 1, required: 2 });
  assert.deepEqual(submitReport(state, "a", 0, 0, [...one].reverse(), "1".repeat(32)), { type: "ack", epoch: 0, round: 0, duplicate: true, received: 1, required: 2 });
  assert.equal(submitReport(state, "b", 0, 0, two, "1".repeat(32)).type, "abort");
  assert.equal(state.players.a.score, 0); assert.equal(state.players.b.score, 0);
});

test("stale epoch reports are rejected without mutating active state", () => {
  const state = createEpochState("duel", 2, 0);
  state.players.a = player("a", 1); state.players.b = player("b", 2);
  startNextEpoch(state, "0".repeat(32), "initial");
  const before = structuredClone(state.active);
  assert.equal(submitReport(state, "a", 9, 0, report(state.active), "1".repeat(32)).code, "stale_report");
  assert.deepEqual(state.active, before);
});

test("terminal decisions acknowledge identical late reports and reject conflicts", () => {
  const state = createEpochState("duel", 2, 0);
  state.players.a = player("a", 1); state.players.b = player("b", 2);
  startNextEpoch(state, "0".repeat(32), "initial");
  const outcomes = report(state.active);
  assert.equal(submitReport(state, "a", 0, 0, outcomes, "1".repeat(32)).type, "ack");
  assert.equal(submitReport(state, "b", 0, 0, outcomes, "1".repeat(32)).type, "commit");
  assert.equal(state.players.a.score, 1);
  assert.equal(submitReport(state, "a", 0, 0, outcomes, "2".repeat(32)).duplicate, true);
  assert.equal(submitReport(state, "a", 0, 0, report({ roster: [{ playerId: "b" }, { playerId: "a" }] }, "b"), "2".repeat(32)).code, "conflicting_terminal_report");
  assert.equal(state.players.a.score, 1);
});

test("reconnect accepts only the current token and rotates it", () => {
  const identity = { tokenHash: "old", connected: false, expired: false, reconnectUntil: 100 };
  assert.equal(rotateReconnectIdentity(identity, "wrong", "next", 50).code, "invalid_reconnect");
  assert.equal(rotateReconnectIdentity(identity, "old", "next", 50).ok, true);
  assert.equal(identity.tokenHash, "next");
  assert.equal(identity.connected, true);
  assert.equal(rotateReconnectIdentity(identity, "old", "again", 51).code, "invalid_reconnect");
});

test("one active reload rolls to a fresh epoch without replaying a round", () => {
  const state = createEpochState("duel", 2, 0);
  state.players.a = player("a", 1); state.players.b = player("b", 2);
  const first = startNextEpoch(state, "0".repeat(32), "initial");
  const pending = markActiveReconnect(state, "a", 100);
  assert.equal(pending.deadline, 400);
  assert.equal(rolloverActiveReconnect(state, 399, "1".repeat(32)), null);
  const result = rolloverActiveReconnect(state, 400, "1".repeat(32));
  assert.equal(result.type, "rollover");
  assert.deepEqual([result.next.epoch, result.next.round], [first.epoch + 1, 0]);
  assert.equal(result.next.seed, "1".repeat(32));
  assert.equal(state.reconnectBatchDeadline, null);
});

test("simultaneous and staggered reloads batch into one epoch increment", () => {
  const state = createEpochState("deathmatch", 3, 0);
  state.players.a = player("a", 1); state.players.b = player("b", 2); state.players.c = player("c", 3);
  startNextEpoch(state, "0".repeat(32), "initial");
  assert.equal(markActiveReconnect(state, "a", 100).deadline, 400);
  assert.equal(markActiveReconnect(state, "b", 100).deadline, 400);
  assert.equal(markActiveReconnect(state, "c", 250).deadline, 400);
  assert.equal(rolloverActiveReconnect(state, 400, "2".repeat(32)).type, "rollover");
  assert.equal(state.epoch, 1);
  assert.equal(rolloverActiveReconnect(state, 900, "3".repeat(32)), null);
  assert.equal(state.epoch, 1);
});

test("reconnect rollover preserves ids, profiles, scores, and match generation", () => {
  const state = createEpochState("duel", 2, 0);
  state.players.a = player("a", 1); state.players.b = player("b", 2);
  state.players.a.score = 2;
  state.players.b.score = 1;
  state.players.a.profile = { name: "Alpha", paletteId: 2, cosmeticId: 3 };
  state.matchGeneration = 7;
  startNextEpoch(state, "0".repeat(32), "initial");
  markActiveReconnect(state, "a", 10);
  const next = rolloverActiveReconnect(state, 310, "f".repeat(32)).next;
  assert.deepEqual(next.roster.map((entry) => [entry.playerId, entry.score]), [["a", 2], ["b", 1]]);
  assert.deepEqual(next.roster[0].profile, { name: "Alpha", paletteId: 2, cosmeticId: 3 });
  assert.equal(state.matchGeneration, 7);
});

test("absent active member clears only batch and retains reconnect grace", () => {
  const state = createEpochState("duel", 2, 0);
  state.players.a = player("a", 1); state.players.b = player("b", 2);
  startNextEpoch(state, "0".repeat(32), "initial");
  state.players.b.connected = false;
  state.players.b.reconnectUntil = 30_000;
  const before = structuredClone(state.active);
  markActiveReconnect(state, "a", 100);
  const result = rolloverActiveReconnect(state, 400, "1".repeat(32));
  assert.deepEqual(result, { type: "waiting", missing: ["b"], next: null });
  assert.deepEqual(state.active, before);
  assert.equal(state.players.b.reconnectUntil, 30_000);
  assert.equal(state.reconnectBatchDeadline, null);
  assert.equal(state.epoch, 0);
});

test("roster selection prefers incumbents then waiters", () => {
  const state = createEpochState("deathmatch", 3, 0);
  state.players.a = player("a", 1); state.players.b = player("b", 2); state.players.c = player("c", 3); state.players.d = player("d", 4);
  state.lastRoster = ["c", "a", "b"];
  state.players.b.ready = false;
  assert.deepEqual(selectNextRoster(state), ["a", "c", "d"]);
});
