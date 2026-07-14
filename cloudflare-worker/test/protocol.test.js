import test from "node:test";
import assert from "node:assert/strict";
import {
  MAX_MESSAGES_PER_SECOND,
  applyMessageRateLimit,
  canonicalRoster,
  parseClientMessage,
  parseLobbyQuery,
  randomHex,
  routePath,
  validateSignalData,
  parseEpochClientMessage,
  parseEpochLobbyQuery,
  parseQueueQuery,
} from "../src/protocol.js";

test("routePath preserves the legacy /match route and adds /lobby", () => {
  assert.deepEqual(routePath("/match/old_room-1"), { kind: "match", room: "old_room-1" });
  assert.deepEqual(routePath("/lobby/new_room-1"), { kind: "lobby", room: "new_room-1" });
  assert.equal(routePath("/match"), null);
  assert.equal(routePath("/lobby/a/b"), null);
  assert.equal(routePath(`/lobby/${"a".repeat(65)}`), null);
  assert.deepEqual(routePath("/queue/battle-0-7-0"), { kind: "queue", room: "battle-0-7-0" });
  assert.deepEqual(routePath("/queue/devbattle-0-7-0"), { kind: "queue", room: "devbattle-0-7-0" });
  assert.equal(routePath("/queue/public-v4"), null);
  assert.equal(routePath("/queue/battle-latest"), null);
  assert.equal(routePath("/queue/private-room"), null);
});

test("queue query is protocol 4 public preference-only matchmaking", () => {
  for (const preference of ["any", "duel", "deathmatch"]) {
    assert.deepEqual(parseQueueQuery(new URLSearchParams(`protocol=4&preference=${preference}`)), {
      ok: true, value: { preference, legacyTarget: null },
    });
  }
  assert.equal(parseQueueQuery(new URLSearchParams("protocol=3&preference=any")).ok, false);
  assert.equal(parseQueueQuery(new URLSearchParams("protocol=4&preference=unknown")).ok, false);
  // One-release bridge: valid legacy targets are echoed but never enter reducer policy.
  assert.deepEqual(parseQueueQuery(new URLSearchParams("protocol=4&preference=any&target=3")), {
    ok: true, value: { preference: "any", legacyTarget: 3 },
  });
  assert.equal(parseQueueQuery(new URLSearchParams("protocol=4&preference=any&target=2")).ok, false);
  assert.equal(parseQueueQuery(new URLSearchParams("protocol=4&preference=any&target=9")).ok, false);
  assert.equal(parseQueueQuery(new URLSearchParams("protocol=4&preference=any&extra=x")).ok, false);
  assert.equal(parseQueueQuery(new URLSearchParams("protocol=4&preference=any&preference=any")).ok, false);
});

test("lobby query validates mode, capacity, and reconnect pair", () => {
  assert.deepEqual(parseLobbyQuery(new URLSearchParams("mode=duel&capacity=2")), {
    ok: true,
    value: { mode: "duel", capacity: 2, playerId: null, reconnectToken: null },
  });
  assert.equal(parseLobbyQuery(new URLSearchParams("mode=duel&capacity=3")).ok, false);
  assert.equal(parseLobbyQuery(new URLSearchParams("mode=deathmatch&capacity=1")).ok, false);
  assert.equal(parseLobbyQuery(new URLSearchParams("mode=deathmatch&capacity=2")).ok, false);
  assert.equal(parseLobbyQuery(new URLSearchParams("mode=deathmatch&capacity=3")).ok, true);
  assert.equal(parseLobbyQuery(new URLSearchParams("mode=deathmatch&capacity=4")).ok, true);
  for (let capacity = 3; capacity <= 8; capacity += 1) {
    assert.equal(parseLobbyQuery(new URLSearchParams(`mode=deathmatch&capacity=${capacity}`)).ok, true);
  }
  assert.equal(parseLobbyQuery(new URLSearchParams("mode=deathmatch&capacity=9")).ok, false);
  assert.equal(parseLobbyQuery(new URLSearchParams("mode=duel&capacity=2&playerId=abc")).ok, false);
  assert.equal(parseLobbyQuery(new URLSearchParams(
    `mode=duel&capacity=2&playerId=${"a".repeat(32)}&reconnectToken=${"B".repeat(32)}`,
  )).value.reconnectToken, "b".repeat(32));
  assert.equal(parseLobbyQuery(new URLSearchParams("mode=duel&capacity=2&extra=yes")).ok, false);
  assert.equal(parseLobbyQuery(new URLSearchParams("mode=duel&mode=duel&capacity=2")).ok, false);
});

test("directed signal schema accepts bounded SDP and ICE only", () => {
  const target = "1".repeat(32);
  assert.equal(parseClientMessage(JSON.stringify({
    type: "signal", to: target, data: { type: "offer", sdp: "v=0" },
  })).ok, true);
  assert.equal(parseClientMessage(JSON.stringify({
    type: "signal", to: target, data: { type: "ice", candidate: null },
  })).ok, true);
  assert.equal(validateSignalData({ type: "answer", sdp: "x".repeat(12 * 1024 + 1) }).ok, false);
  assert.equal(validateSignalData({ type: "answer", sdp: "é".repeat(6 * 1024 + 1) }).ok, false);
  assert.equal(validateSignalData({
    type: "ice",
    candidate: { candidate: "é".repeat(1025), sdpMid: null, sdpMLineIndex: null, usernameFragment: null },
  }).ok, false);
  assert.equal(parseClientMessage(JSON.stringify({ type: "signal", to: target, data: {}, extra: 1 })).ok, false);
  assert.equal(parseClientMessage("not-json").ok, false);
});

test("epoch query accepts bounded assignment handoff only as an all-or-none set", () => {
  const ticket = "a".repeat(32);
  const token = "b".repeat(64);
  const parsed = parseEpochLobbyQuery(new URLSearchParams(
    `protocol=3&mode=deathmatch&capacity=4&queueTicket=${ticket}&queueExpires=2000000000000&queueToken=${token}`,
  ));
  assert.equal(parsed.ok, true);
  assert.deepEqual(parsed.value.assignment, { ticket, expiresAt: 2_000_000_000_000, token });
  assert.equal(parseEpochLobbyQuery(new URLSearchParams(
    `protocol=3&mode=deathmatch&capacity=4&queueTicket=${ticket}`,
  )).ok, false);
  assert.equal(parseEpochLobbyQuery(new URLSearchParams(
    `protocol=3&mode=deathmatch&capacity=4&queueTicket=${ticket}&queueExpires=bad&queueToken=${token}`,
  )).ok, false);
});

test("epoch protocol validates profiles and reports", () => {
  assert.equal(parseEpochLobbyQuery(new URLSearchParams("protocol=3&mode=deathmatch&capacity=4")).ok, true);
  assert.equal(parseEpochClientMessage(JSON.stringify({ type:"profile", name:"Ghost", paletteId:1, cosmeticId:2 })).ok, true);
  assert.equal(parseEpochClientMessage(JSON.stringify({ type:"report", epoch:0, round:0, outcomes:[
    { playerId:"a".repeat(32), placement:1, scoreDelta:1 },
    { playerId:"b".repeat(32), placement:2, scoreDelta:0 },
  ] })).ok, true);
  assert.equal(parseEpochClientMessage(JSON.stringify({ type:"report", epoch:0, round:0, outcomes:[] })).ok, false);
  assert.equal(parseEpochClientMessage(JSON.stringify({ type:"profile", name:"", paletteId:1, cosmeticId:2 })).ok, false);
  const nonce = "f".repeat(32);
  assert.equal(parseEpochClientMessage(JSON.stringify({ type:"rematch_request", generation:1, nonce })).ok, true);
  assert.equal(parseEpochClientMessage(JSON.stringify({ type:"rematch_response", generation:1, nonce, accept:false })).ok, true);
  assert.equal(parseEpochClientMessage(JSON.stringify({ type:"leave" })).ok, true);
  assert.equal(parseEpochClientMessage(JSON.stringify({ type:"requeue" })).ok, true);
  assert.equal(parseEpochClientMessage(JSON.stringify({ type:"rematch_request", generation:0, nonce })).ok, false);
});

test("epoch signals are always epoch-scoped and never downgrade to v2", () => {
  const target = "1".repeat(32);
  const valid = parseEpochClientMessage(JSON.stringify({
    type: "signal", epoch: 0, to: target, data: { type: "offer", sdp: "v=0" },
  }));
  assert.equal(valid.ok, true);
  assert.equal(valid.value.epoch, 0);
  assert.equal(valid.value.to, target);
  // A signal missing `epoch` is rejected rather than accepted as a v2 signal.
  assert.equal(parseEpochClientMessage(JSON.stringify({
    type: "signal", to: target, data: { type: "offer", sdp: "v=0" },
  })).ok, false);
  // A negative or non-integer epoch is rejected.
  assert.equal(parseEpochClientMessage(JSON.stringify({
    type: "signal", epoch: -1, to: target, data: { type: "offer", sdp: "v=0" },
  })).ok, false);
  assert.equal(parseEpochClientMessage(JSON.stringify({
    type: "signal", epoch: "0", to: target, data: { type: "offer", sdp: "v=0" },
  })).ok, false);
  // Extra keys on a signal are rejected.
  assert.equal(parseEpochClientMessage(JSON.stringify({
    type: "signal", epoch: 0, to: target, data: { type: "offer", sdp: "v=0" }, extra: 1 },
  )).ok, false);
});

test("epoch protocol requires exactly one protocol=3 parameter", () => {
  assert.equal(parseEpochLobbyQuery(new URLSearchParams("mode=duel&capacity=2")).ok, false);
  assert.equal(parseEpochLobbyQuery(new URLSearchParams("protocol=2&mode=duel&capacity=2")).ok, false);
  assert.equal(parseEpochLobbyQuery(new URLSearchParams("protocol=3&protocol=3&mode=duel&capacity=2")).ok, false);
  assert.equal(parseEpochLobbyQuery(new URLSearchParams("protocol=3&mode=duel&capacity=2")).value.mode, "duel");
});

test("rate limiter resets after a one-second window", () => {
  let rate;
  for (let count = 0; count < MAX_MESSAGES_PER_SECOND; count += 1) {
    const result = applyMessageRateLimit(rate, 10);
    assert.equal(result.allowed, true);
    rate = result.rate;
  }
  assert.equal(applyMessageRateLimit(rate, 10).allowed, false);
  assert.equal(applyMessageRateLimit(rate, 1010).allowed, true);
});

test("canonical roster sorts IDs and assigns stable indices", () => {
  const roster = canonicalRoster([
    { playerId: "f".repeat(32), joinedAt: 1 },
    { playerId: "0".repeat(32), joinedAt: 2 },
  ]);
  assert.deepEqual(roster.map(({ playerId, index }) => ({ playerId, index })), [
    { playerId: "0".repeat(32), index: 0 },
    { playerId: "f".repeat(32), index: 1 },
  ]);
});

test("randomHex emits server-style 32 hex characters", () => {
  const fakeCrypto = { getRandomValues: (bytes) => bytes.fill(0xab) };
  assert.equal(randomHex(16, fakeCrypto), "ab".repeat(16));
});
