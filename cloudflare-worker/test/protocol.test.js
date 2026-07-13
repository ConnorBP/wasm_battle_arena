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
} from "../src/protocol.js";

test("routePath preserves the legacy /match route and adds /lobby", () => {
  assert.deepEqual(routePath("/match/old_room-1"), { kind: "match", room: "old_room-1" });
  assert.deepEqual(routePath("/lobby/new_room-1"), { kind: "lobby", room: "new_room-1" });
  assert.equal(routePath("/match"), null);
  assert.equal(routePath("/lobby/a/b"), null);
  assert.equal(routePath(`/lobby/${"a".repeat(65)}`), null);
});

test("lobby query validates mode, capacity, and reconnect pair", () => {
  assert.deepEqual(parseLobbyQuery(new URLSearchParams("mode=duel&capacity=2")), {
    ok: true,
    value: { mode: "duel", capacity: 2, playerId: null, reconnectToken: null },
  });
  assert.equal(parseLobbyQuery(new URLSearchParams("mode=duel&capacity=3")).ok, false);
  assert.equal(parseLobbyQuery(new URLSearchParams("mode=deathmatch&capacity=1")).ok, false);
  assert.equal(parseLobbyQuery(new URLSearchParams("mode=deathmatch&capacity=4")).ok, true);
  assert.equal(parseLobbyQuery(new URLSearchParams("mode=deathmatch&capacity=5")).ok, false);
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

test("epoch protocol validates profiles and reports", () => {
  assert.equal(parseEpochLobbyQuery(new URLSearchParams("protocol=3&mode=deathmatch&capacity=4")).ok, true);
  assert.equal(parseEpochClientMessage(JSON.stringify({ type:"profile", name:"Ghost", paletteId:1, cosmeticId:2 })).ok, true);
  assert.equal(parseEpochClientMessage(JSON.stringify({ type:"report", epoch:0, round:0, outcomes:[] })).ok, true);
  assert.equal(parseEpochClientMessage(JSON.stringify({ type:"profile", name:"", paletteId:1, cosmeticId:2 })).ok, false);
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
