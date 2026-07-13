import test from "node:test";
import assert from "node:assert/strict";
import {
  advanceMatchSeed, createLifecycleState, denyLifecycleRematch,
  expireLifecycleRematch, leaveLifecycleMatch, requeueLifecyclePlayer,
  requestLifecycleRematch, respondLifecycleRematch, startLifecycleRound,
} from "../vendor/cloudflare-game-common/lifecycle.js";

const ids = Array.from({ length: 10 }, (_, i) => i.toString(16).padStart(32, "0"));
function ended(capacity = 2) {
  const state = createLifecycleState(capacity === 2 ? "duel" : "deathmatch", capacity, 1);
  for (let i = 0; i < capacity + 2; i += 1) state.players[ids[i]] = {
    playerId: ids[i], joinedAt: i, connected: true, ready: true, expired: false,
    profile: { name: `G${i}`, paletteId: i % 4, cosmeticId: 0 }, tokenHash: "h", reconnectUntil: null, score: i === 0 ? 3 : 1,
  };
  const active = startLifecycleRound(state, "1".repeat(32), "test");
  state.active = null; state.matchOver = true; state.lastRoster = active.roster.map((r) => r.playerId);
  return state;
}

test("all supported capacities form immutable rosters and queue excess players", () => {
  for (let capacity = 2; capacity <= 8; capacity += 1) {
    const state = ended(capacity);
    assert.equal(state.lastRoster.length, capacity);
    assert.equal(state.lastRoster.includes(ids[capacity]), false);
  }
});

test("simultaneous rematch requests are mutual acceptance and recreate epoch", () => {
  const state = ended(2); const nonce = "a".repeat(32);
  const first = requestLifecycleRematch(state, ids[0], 1, nonce, 100);
  assert.equal(first.type, "pending");
  // A concurrent request may carry another nonce; generation makes it an
  // acceptance of the already-authoritative proposal.
  const second = requestLifecycleRematch(state, ids[1], 1, "b".repeat(32), 101);
  assert.equal(second.type, "accepted");
  assert.equal(second.nonce, nonce);
  assert.equal(second.next.epoch, 1);
  assert.deepEqual(second.next.roster.map((r) => r.playerId), state.lastRoster);
  assert.ok(second.next.roster.every((r) => r.score === 0));
});

test("duplicate accept is idempotent and stale nonce/generation are rejected", () => {
  const state = ended(3); const nonce = "c".repeat(32);
  requestLifecycleRematch(state, ids[0], 1, nonce, 10);
  const first = respondLifecycleRematch(state, ids[1], 1, nonce, true);
  assert.equal(first.duplicate, false);
  const duplicate = respondLifecycleRematch(state, ids[1], 1, nonce, true);
  assert.equal(duplicate.duplicate, true);
  assert.equal(respondLifecycleRematch(state, ids[2], 1, "d".repeat(32), true).code, "stale_rematch");
  assert.equal(requestLifecycleRematch(state, ids[2], 3, nonce, 12).code, "stale_rematch");
});

test("deny timeout disconnect policy releases whole roster to main menu", () => {
  for (const reason of ["denied", "disconnect"]) {
    const state = ended(2); const nonce = "e".repeat(32);
    requestLifecycleRematch(state, ids[0], 1, nonce, 100);
    const result = denyLifecycleRematch(state, reason);
    assert.equal(result.type, "denied"); assert.equal(result.reason, reason);
    assert.ok(result.roster.every((id) => state.players[id].ready === false));
  }
  const timed = ended(2); requestLifecycleRematch(timed, ids[0], 1, "f".repeat(32), 100);
  assert.equal(expireLifecycleRematch(timed, 10_099), null);
  assert.equal(expireLifecycleRematch(timed, 10_100).reason, "timeout");
});

test("exit affects current immutable roster and seed advance is deterministic", () => {
  const state = ended(4); const roster = [...state.lastRoster];
  const result = leaveLifecycleMatch(state, roster[0]);
  assert.deepEqual(result.roster, roster); assert.equal(result.destination, "main_menu");
  assert.equal(state.active, null);
  const queued = ended(2); const requeued = requeueLifecyclePlayer(queued, ids[0]);
  assert.equal(requeued.destination, "requeue");
  assert.equal(queued.players[ids[0]].ready, true);
  assert.equal(queued.players[ids[1]].ready, false);
  const seed = "0123456789abcdef0123456789abcdef";
  assert.equal(advanceMatchSeed(seed, 1), advanceMatchSeed(seed, 1));
  assert.notEqual(advanceMatchSeed(seed, 1), advanceMatchSeed(seed, 2));
});
