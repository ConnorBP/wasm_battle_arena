import test from "node:test";
import assert from "node:assert/strict";
import {
  abortLifecycleRound,
  canonicalOutcomes,
  createLifecycleState,
  rotateReconnectIdentity,
  sameRoster,
  selectLifecycleRoster,
  startLifecycleRound,
  submitLifecycleReport,
  validateEpochSignal,
} from "../vendor/cloudflare-game-common/lifecycle.js";

const DUEL = 2;
const NOW = 1_000_000;

// Build a minimal player record matching the shape the Worker persists.
function makePlayer(playerId, { joinedAt = NOW, connected = true, ready = true, expired = false, profile = { name: "Ghost", paletteId: 0, cosmeticId: 0 }, tokenHash = "h:" + playerId, reconnectUntil = null, score = 0 } = {}) {
  return { playerId, joinedAt, connected, ready, expired, profile, tokenHash, reconnectUntil, score };
}

// Wire up a state with the given player ids (all ready+profile+connected).
function seededState(ids, { mode = "duel", capacity = DUEL } = {}) {
  const state = createLifecycleState(mode, capacity, NOW);
  ids.forEach((id, index) => { state.players[id] = makePlayer(id, { joinedAt: NOW + index }); });
  return state;
}

const A = "0".repeat(31) + "1";
const B = "0".repeat(31) + "2";
const C = "0".repeat(31) + "3";

// Canonical outcome helpers: placement 1 = win (scoreDelta 1), else 0.
function outcomesFor(roster, winnerId) {
  return roster.map((entry) => ({
    playerId: entry.playerId,
    placement: entry.playerId === winnerId ? 1 : 2,
    scoreDelta: entry.playerId === winnerId ? 1 : 0,
  }));
}

// ---------------------------------------------------------------------------
// Active-ready immutability
// ---------------------------------------------------------------------------

test("startLifecycleRound never replaces an active epoch", () => {
  const state = seededState([A, B]);
  const first = startLifecycleRound(state, "seed-0", "roster_ready");
  assert.equal(first.epoch, 0);
  assert.equal(first.round, 0);
  assert.deepEqual(first.roster.map((r) => r.playerId), [A, B]);

  // ready / join / profile events all funnel through startNextEpoch, which must
  // no-op while an active bootstrap exists.
  state.players[C] = makePlayer(C, { joinedAt: NOW + 5 });
  const second = startLifecycleRound(state, "seed-1", "roster_ready");
  assert.equal(second, null);
  assert.equal(state.active.epoch, 0);
  assert.equal(state.active.round, 0);
  assert.deepEqual(state.active.roster.map((r) => r.playerId), [A, B]);
});

test("a partial roster never starts a GGRS session", () => {
  const state = seededState([A]);
  assert.equal(startLifecycleRound(state, "seed", "roster_ready"), null);
  assert.equal(state.active, null);
});

// ---------------------------------------------------------------------------
// start / ready / profile ordering
// ---------------------------------------------------------------------------

test("selection requires both profile and ready", () => {
  const state = seededState([A, B]);
  state.players[A].profile = null;
  assert.deepEqual(selectLifecycleRoster(state), [B]);
  state.players[A].profile = { name: "A", paletteId: 1, cosmeticId: 0 };
  state.players[A].ready = false;
  assert.deepEqual(selectLifecycleRoster(state), [B]);
  state.players[A].ready = true;
  assert.deepEqual(selectLifecycleRoster(state), [A, B].sort());
});

test("expired and disconnected players are never selected", () => {
  const state = seededState([A, B]);
  state.players[A].expired = true;
  assert.deepEqual(selectLifecycleRoster(state), [B]);
  state.players[A].expired = false;
  state.players[A].connected = false;
  assert.deepEqual(selectLifecycleRoster(state), [B]);
});

// ---------------------------------------------------------------------------
// Unchanged-roster round continuity
// ---------------------------------------------------------------------------

test("commit with unchanged roster increments round only", () => {
  const state = seededState([A, B]);
  const active = startLifecycleRound(state, "s0", "roster_ready");
  const outcomes = outcomesFor(active.roster, A);
  assert.equal(submitLifecycleReport(state, A, 0, 0, outcomes, "s1").type, "ack");
  const commit = submitLifecycleReport(state, B, 0, 0, outcomes, "s2");
  assert.equal(commit.type, "commit");
  assert.equal(commit.epoch, 0);
  assert.equal(commit.round, 0);
  assert.equal(commit.next.epoch, 0);
  assert.equal(commit.next.round, 1);
  assert.deepEqual(commit.next.roster.map((r) => r.playerId), [A, B].sort());
});

// ---------------------------------------------------------------------------
// Changed-roster epoch continuity
// ---------------------------------------------------------------------------

test("commit after a roster member leaves increments epoch and resets round", () => {
  const state = seededState([A, B]);
  // A queued mid-round joiner (later joinedAt so incumbents keep their seat).
  state.players[C] = makePlayer(C, { joinedAt: NOW + 10 });
  const active = startLifecycleRound(state, "s0", "roster_ready");
  assert.deepEqual(active.roster.map((r) => r.playerId), [A, B].sort());
  const outcomes = outcomesFor(active.roster, A);

  // A reports while still connected, then disconnects before B's report lands.
  assert.equal(submitLifecycleReport(state, A, 0, 0, outcomes, "s1").type, "ack");
  state.players[A].connected = false;

  const commit = submitLifecycleReport(state, B, 0, 0, outcomes, "s2");
  assert.equal(commit.type, "commit");
  assert.equal(commit.next.epoch, 1);
  assert.equal(commit.next.round, 0);
  assert.deepEqual(commit.next.roster.map((r) => r.playerId), [B, C].sort());
});

// ---------------------------------------------------------------------------
// Mid-round joins are queued for the next selection
// ---------------------------------------------------------------------------

test("mid-round joiner is queued while the roster is unchanged", () => {
  const state = seededState([A, B]);
  const active = startLifecycleRound(state, "s0", "roster_ready");
  state.players[C] = makePlayer(C, { joinedAt: NOW + 10 });
  // A ready event from the joiner must not replace the active epoch.
  assert.equal(startLifecycleRound(state, "s1", "roster_ready"), null);
  const outcomes = outcomesFor(active.roster, A);
  submitLifecycleReport(state, A, 0, 0, outcomes, "s1");
  const commit = submitLifecycleReport(state, B, 0, 0, outcomes, "s2");
  // Incumbents keep their seats: C is still queued for a later slot.
  assert.deepEqual(commit.next.roster.map((r) => r.playerId), [A, B].sort());
  assert.equal(commit.next.round, 1);
});

test("a queued joiner is selected once an incumbent leaves", () => {
  const state = seededState([A, B]);
  state.players[C] = makePlayer(C, { joinedAt: NOW + 10 });
  const active = startLifecycleRound(state, "s0", "roster_ready");
  const outcomes = outcomesFor(active.roster, A);
  submitLifecycleReport(state, A, 0, 0, outcomes, "s1");
  state.players[A].connected = false;
  const commit = submitLifecycleReport(state, B, 0, 0, outcomes, "s2");
  assert.deepEqual(commit.next.roster.map((r) => r.playerId), [B, C].sort());
  assert.equal(commit.next.epoch, 1);
});

// ---------------------------------------------------------------------------
// Stale / wrong-epoch signaling rejection
// ---------------------------------------------------------------------------

test("validateEpochSignal rejects stale, wrong-epoch, and non-roster signaling", () => {
  const state = seededState([A, B]);
  state.players[C] = makePlayer(C, { joinedAt: NOW + 10 });
  const active = startLifecycleRound(state, "s0", "roster_ready");
  const epoch = active.epoch;

  assert.deepEqual(validateEpochSignal(state, A, B, epoch), { ok: true, epoch });
  assert.equal(validateEpochSignal(state, A, B, epoch + 1).code, "stale_epoch");
  assert.equal(validateEpochSignal(state, A, A, epoch).code, "invalid_target");
  assert.equal(validateEpochSignal(state, A, C, epoch).code, "not_in_roster");
  assert.equal(validateEpochSignal(state, C, B, epoch).code, "not_in_roster");

  // Advance to a new epoch (A leaves, the queued C is promoted); signaling for
  // the old epoch is now stale while the new epoch is valid.
  const outcomes = outcomesFor(active.roster, A);
  submitLifecycleReport(state, A, 0, 0, outcomes, "s1");
  state.players[A].connected = false;
  submitLifecycleReport(state, B, 0, 0, outcomes, "s2");
  assert.equal(state.active.epoch, 1);
  assert.equal(validateEpochSignal(state, B, C, 0).code, "stale_epoch");
  assert.deepEqual(validateEpochSignal(state, B, C, 1), { ok: true, epoch: 1 });
});

test("validateEpochSignal rejects signaling when no round is active", () => {
  const state = seededState([A, B]);
  assert.equal(validateEpochSignal(state, A, B, 0).code, "no_active_round");
});

// ---------------------------------------------------------------------------
// Duplicate and conflicting reports
// ---------------------------------------------------------------------------

test("duplicate live reports are acknowledged without re-counting", () => {
  const state = seededState([A, B]);
  const active = startLifecycleRound(state, "s0", "roster_ready");
  const outcomes = outcomesFor(active.roster, A);

  const first = submitLifecycleReport(state, A, 0, 0, outcomes, "s1");
  assert.equal(first.type, "ack");
  assert.equal(first.duplicate, false);
  assert.equal(first.received, 1);
  assert.equal(first.required, 2);

  const dup = submitLifecycleReport(state, A, 0, 0, outcomes, "s1");
  assert.equal(dup.type, "ack");
  assert.equal(dup.duplicate, true);
  assert.equal(dup.received, 1);
  assert.equal(state.players[A].score, 0);
});

test("conflicting live reports abort without score mutation", () => {
  const state = seededState([A, B]);
  const active = startLifecycleRound(state, "s0", "roster_ready");
  const winA = outcomesFor(active.roster, A);
  const winB = outcomesFor(active.roster, B);

  submitLifecycleReport(state, A, 0, 0, winA, "s1");
  const result = submitLifecycleReport(state, B, 0, 0, winB, "s2");
  assert.equal(result.type, "abort");
  assert.equal(result.epoch, 0);
  assert.equal(result.round, 0);
  assert.equal(result.reason, "conflicting_reports");
  assert.equal(state.players[A].score, 0);
  assert.equal(state.players[B].score, 0);
  // The aborted (0,0) round is gone; with both still ready the reducer retries
  // as the next round of the same epoch (unchanged roster => round++).
  assert.equal(result.next.epoch, 0);
  assert.equal(result.next.round, 1);
  assert.equal(state.active, result.next);
});

test("a reporter flipping their own outcome aborts", () => {
  const state = seededState([A, B]);
  const active = startLifecycleRound(state, "s0", "roster_ready");
  const winA = outcomesFor(active.roster, A);
  const winB = outcomesFor(active.roster, B);
  submitLifecycleReport(state, A, 0, 0, winA, "s1");
  const result = submitLifecycleReport(state, A, 0, 0, winB, "s2");
  assert.equal(result.type, "abort");
  assert.equal(state.players[A].score, 0);
});

test("stale reports are rejected", () => {
  const state = seededState([A, B]);
  const active = startLifecycleRound(state, "s0", "roster_ready");
  const outcomes = outcomesFor(active.roster, A);
  // Wrong epoch.
  assert.equal(submitLifecycleReport(state, A, 5, 0, outcomes, "s").code, "stale_report");
  // Wrong round.
  assert.equal(submitLifecycleReport(state, A, 0, 5, outcomes, "s").code, "stale_report");
  // Non-roster reporter.
  state.players[C] = makePlayer(C);
  assert.equal(submitLifecycleReport(state, C, 0, 0, outcomes, "s").code, "stale_report");
  // No active round at all (fresh state, nothing decided).
  const idle = seededState([A, B]);
  assert.equal(submitLifecycleReport(idle, A, 0, 0, outcomes, "s").code, "stale_report");
});

test("malformed reports are rejected as invalid", () => {
  const state = seededState([A, B]);
  const active = startLifecycleRound(state, "s0", "roster_ready");
  // Wrong roster size.
  assert.equal(submitLifecycleReport(state, A, 0, 0, [outcomesFor(active.roster, A)[0]], "s").code, "invalid_report");
  // Unknown roster member.
  assert.equal(submitLifecycleReport(state, A, 0, 0, [
    { playerId: A, placement: 1, scoreDelta: 1 },
    { playerId: C, placement: 2, scoreDelta: 0 },
  ], "s").code, "invalid_report");
  // Out-of-range score.
  assert.equal(submitLifecycleReport(state, A, 0, 0, [
    { playerId: A, placement: 1, scoreDelta: -1 },
    { playerId: B, placement: 2, scoreDelta: 0 },
  ], "s").code, "invalid_report");
});

// ---------------------------------------------------------------------------
// Terminal idempotence
// ---------------------------------------------------------------------------

test("scores apply exactly once and terminal commits are idempotent", () => {
  const state = seededState([A, B]);
  const active = startLifecycleRound(state, "s0", "roster_ready");
  const outcomes = outcomesFor(active.roster, A);

  submitLifecycleReport(state, A, 0, 0, outcomes, "s1");
  const commit = submitLifecycleReport(state, B, 0, 0, outcomes, "s2");
  assert.equal(commit.type, "commit");
  assert.equal(state.players[A].score, 1);
  assert.equal(state.players[B].score, 0);

  // Late identical duplicate from a roster member is acknowledged, not scored.
  const late = submitLifecycleReport(state, A, 0, 0, outcomes, "s3");
  assert.equal(late.type, "ack");
  assert.equal(late.duplicate, true);
  assert.equal(late.received, 2);
  assert.equal(late.required, 2);
  assert.equal(state.players[A].score, 1);
});

test("late conflicting reports against a terminal commit are rejected", () => {
  const state = seededState([A, B]);
  const active = startLifecycleRound(state, "s0", "roster_ready");
  const winA = outcomesFor(active.roster, A);
  const winB = outcomesFor(active.roster, B);
  submitLifecycleReport(state, A, 0, 0, winA, "s1");
  submitLifecycleReport(state, B, 0, 0, winA, "s2"); // commit A wins

  const conflict = submitLifecycleReport(state, B, 0, 0, winB, "s3");
  assert.equal(conflict.type, "error");
  assert.equal(conflict.code, "conflicting_terminal_report");
  assert.equal(state.players[A].score, 1);
  assert.equal(state.players[B].score, 0);
});

test("late reports for an aborted round are stale and never scored", () => {
  const state = seededState([A, B]);
  const active = startLifecycleRound(state, "s0", "roster_ready");
  const winA = outcomesFor(active.roster, A);
  const winB = outcomesFor(active.roster, B);
  submitLifecycleReport(state, A, 0, 0, winA, "s1");
  const aborted = submitLifecycleReport(state, B, 0, 0, winB, "s2");
  assert.equal(aborted.type, "abort");

  // A roster member re-sending after the abort is stale, not a conflict.
  assert.equal(submitLifecycleReport(state, A, 0, 0, winA, "s3").code, "stale_report");
  // Even an identical-to-original report is stale (no consensus was cached).
  assert.equal(submitLifecycleReport(state, A, 0, 0, winA, "s3").code, "stale_report");
  assert.equal(state.players[A].score, 0);
  assert.equal(state.players[B].score, 0);
});

test("a non-roster late report for a terminal commit is stale", () => {
  const state = seededState([A, B]);
  const active = startLifecycleRound(state, "s0", "roster_ready");
  const outcomes = outcomesFor(active.roster, A);
  submitLifecycleReport(state, A, 0, 0, outcomes, "s1");
  submitLifecycleReport(state, B, 0, 0, outcomes, "s2");
  state.players[C] = makePlayer(C);
  assert.equal(submitLifecycleReport(state, C, 0, 0, outcomes, "s3").code, "stale_report");
});

test("abortLifecycleRound is terminal and refuses a missing active round", () => {
  const state = seededState([A, B]);
  assert.equal(abortLifecycleRound(state, "none", "s").code, "no_active_round");
  const active = startLifecycleRound(state, "s0", "roster_ready");
  const aborted = abortLifecycleRound(state, "manual", "s1");
  assert.equal(aborted.type, "abort");
  assert.equal(aborted.epoch, active.epoch);
  assert.equal(aborted.round, active.round);
  // The aborted round is gone; both still ready so a retry round begins.
  assert.equal(state.active, aborted.next);
  assert.equal(aborted.next.round, 1);
  // The aborted (epoch, round) is now terminal.
  assert.equal(submitLifecycleReport(state, A, 0, 0, outcomesFor(active.roster, A), "s").code, "stale_report");
});

// ---------------------------------------------------------------------------
// Token rotation / expiry / supersession
// ---------------------------------------------------------------------------

function hashed(token) { return "h:" + token; }

test("rotateReconnectIdentity rotates the token on a valid reconnect", () => {
  const player = makePlayer(A, { tokenHash: hashed("old"), connected: false, reconnectUntil: NOW + 5_000 });
  const result = rotateReconnectIdentity(player, hashed("old"), hashed("new"), NOW);
  assert.equal(result.ok, true);
  assert.equal(player.tokenHash, hashed("new"));
  assert.equal(player.connected, true);
  assert.equal(player.reconnectUntil, null);
});

test("rotateReconnectIdentity rejects a wrong token", () => {
  const player = makePlayer(A, { tokenHash: hashed("old") });
  assert.equal(rotateReconnectIdentity(player, hashed("wrong"), hashed("new"), NOW).code, "invalid_reconnect");
  assert.equal(player.tokenHash, hashed("old"));
});

test("rotateReconnectIdentity rejects an unknown identity", () => {
  assert.equal(rotateReconnectIdentity(undefined, hashed("old"), hashed("new"), NOW).code, "invalid_reconnect");
});

test("rotateReconnectIdentity rejects an expired identity and after grace elapses", () => {
  const expired = makePlayer(A, { tokenHash: hashed("old"), expired: true });
  assert.equal(rotateReconnectIdentity(expired, hashed("old"), hashed("new"), NOW).code, "reconnect_expired");

  const graceLapsed = makePlayer(A, { tokenHash: hashed("old"), connected: false, reconnectUntil: NOW });
  assert.equal(rotateReconnectIdentity(graceLapsed, hashed("old"), hashed("new"), NOW).code, "reconnect_expired");
  // A reconnect strictly before the grace boundary still succeeds.
  const within = makePlayer(B, { tokenHash: hashed("old"), connected: false, reconnectUntil: NOW + 1 });
  assert.equal(rotateReconnectIdentity(within, hashed("old"), hashed("new"), NOW).ok, true);
});

test("a superseded token can no longer reconnect", () => {
  const player = makePlayer(A, { tokenHash: hashed("v1"), connected: false, reconnectUntil: NOW + 5_000 });
  // First reconnect rotates v1 -> v2.
  assert.equal(rotateReconnectIdentity(player, hashed("v1"), hashed("v2"), NOW).ok, true);
  // The old token (v1) is now invalid: a second tab presenting it is rejected.
  assert.equal(rotateReconnectIdentity(player, hashed("v1"), hashed("v3"), NOW).code, "invalid_reconnect");
  // The latest token (v2) still works (same-tab reload preserves identity).
  assert.equal(rotateReconnectIdentity(player, hashed("v2"), hashed("v3"), NOW).ok, true);
  assert.equal(player.tokenHash, hashed("v3"));
});

test("a still-connected identity can rotate on a same-tab reload", () => {
  const player = makePlayer(A, { tokenHash: hashed("v1"), connected: true, reconnectUntil: null });
  assert.equal(rotateReconnectIdentity(player, hashed("v1"), hashed("v2"), NOW).ok, true);
  assert.equal(player.connected, true);
});

// ---------------------------------------------------------------------------
// Canonicalization helpers
// ---------------------------------------------------------------------------

test("canonicalOutcomes validates roster alignment and bounds", () => {
  const roster = [{ playerId: A }, { playerId: B }];
  assert.equal(canonicalOutcomes({ roster }, [
    { playerId: A, placement: 1, scoreDelta: 1 },
    { playerId: B, placement: 2, scoreDelta: 0 },
  ]).length, 2);
  assert.equal(canonicalOutcomes({ roster }, [{ playerId: A, placement: 1, scoreDelta: 1 }]), null);
  assert.equal(canonicalOutcomes({ roster }, [
    { playerId: A, placement: 1, scoreDelta: 1 },
    { playerId: A, placement: 2, scoreDelta: 0 },
  ]), null);
  assert.equal(canonicalOutcomes({ roster }, [
    { playerId: A, placement: 1, scoreDelta: 1, extra: 1 },
    { playerId: B, placement: 2, scoreDelta: 0 },
  ]), null);
});

test("sameRoster compares ordered ids", () => {
  assert.equal(sameRoster([A, B], [A, B]), true);
  assert.equal(sameRoster([A, B], [B, A]), false);
  assert.equal(sameRoster([A, B], [A]), false);
  assert.equal(sameRoster([], []), true);
});
