/*
 * Vendored, dependency-free lifecycle reducer shared with car_game_ai.
 * Keep this module limited to Web/ECMAScript APIs supported by Workers and Node.
 *
 * Invariants enforced here:
 *  - An immutable `active` epoch is never replaced by ready/profile/join events.
 *  - Roster continuity: unchanged membership increments `round`; changed
 *    membership increments `epoch` and resets `round` to zero.
 *  - Outcome consensus applies scores exactly once. Canonical identical
 *    duplicates are idempotently acknowledged; conflicting live reports abort
 *    without score mutation; stale reports are rejected; terminal decisions
 *    cannot be changed by late conflicts.
 *  - Reconnect tokens are rotated on every successful reconnect and supersede
 *    the previous token hash. Only hashes are ever stored by the caller.
 *  - An active-roster reconnect is collected into one short persisted batch.
 *    Once due, a fully connected immutable roster is moved to exactly one new
 *    epoch so every peer rebuilds its transport from a server bootstrap.
 */

export function sameRoster(left = [], right = []) {
  return left.length === right.length && left.every((id, index) => id === right[index]);
}

/**
 * Pure reconnect transition: a same-runtime socket or page reload presents the
 * latest token hash, receives a rotation, and invalidates the old token.
 * `presentedHash` is the SHA-256 hash of the token the client stored; the
 * caller computes both hashes so this module stays crypto-free.
 */
export function rotateReconnectIdentity(player, presentedHash, rotatedHash, now) {
  if (!player || player.tokenHash !== presentedHash) return { ok: false, code: "invalid_reconnect" };
  if (player.expired || (player.reconnectUntil != null && player.reconnectUntil <= now)) {
    return { ok: false, code: "reconnect_expired" };
  }
  player.tokenHash = rotatedHash;
  player.connected = true;
  player.reconnectUntil = null;
  return { ok: true, player };
}

export function createLifecycleState(mode, capacity, now) {
  return {
    version: 3,
    mode,
    capacity,
    createdAt: now,
    matchId: null,
    epoch: -1,
    round: -1,
    players: {},
    active: null,
    lastRoster: [],
    decisions: {},
    matchGeneration: 0,
    matchSeed: null,
    matchOver: false,
    rematch: null,
    lastRematchDecision: null,
    reconnectBatchDeadline: null,
    reconnectBatchEpoch: null,
    reconnectBatchRound: null,
    // Active members may opt out only at the report boundary. Entries are
    // persisted with the immutable round identity so duplicate requests are
    // harmless and can never affect a later round accidentally.
    boundaryDepartures: {},
  };
}

export const ACTIVE_RECONNECT_BATCH_MS = 300;

function clearReconnectBatch(state) {
  state.reconnectBatchDeadline = null;
  state.reconnectBatchEpoch = null;
  state.reconnectBatchRound = null;
}

/**
 * Marks the first reconnect of an immutable active roster. Further reconnects
 * for that same round join the existing fixed window instead of extending it,
 * which makes simultaneous reloads converge on one authoritative restart.
 */
export function markLifecycleActiveReconnect(state, playerId, now, delay = ACTIVE_RECONNECT_BATCH_MS) {
  const active = state.active;
  if (!active?.roster.some((entry) => entry.playerId === playerId)) return null;
  const sameBatch = state.reconnectBatchDeadline != null &&
    state.reconnectBatchEpoch === active.epoch && state.reconnectBatchRound === active.round;
  if (!sameBatch) {
    state.reconnectBatchDeadline = now + delay;
    state.reconnectBatchEpoch = active.epoch;
    state.reconnectBatchRound = active.round;
  }
  return {
    type: "pending", deadline: state.reconnectBatchDeadline,
    epoch: active.epoch, round: active.round, required: active.roster.length,
  };
}

/**
 * Resolves a due reconnect batch. The old active round remains immutable while
 * the batch is pending. If every member is connected, this records the old
 * round as aborted and installs one changed epoch using the same player IDs,
 * current profiles/scores and match generation. An absent member leaves the
 * round in place and keeps their caller-owned reconnect grace untouched.
 */
export function rolloverLifecycleActiveReconnect(state, now, seed) {
  const deadline = state.reconnectBatchDeadline;
  if (deadline == null || deadline > now) return null;
  const expectedEpoch = state.reconnectBatchEpoch;
  const expectedRound = state.reconnectBatchRound;
  clearReconnectBatch(state);

  const previous = state.active;
  if (!previous || previous.epoch !== expectedEpoch || previous.round !== expectedRound) {
    return { type: "stale", next: null };
  }
  const missing = previous.roster.map((entry) => entry.playerId).filter((playerId) => {
    const player = state.players[playerId];
    return !player || !player.connected || player.expired;
  });
  if (missing.length) return { type: "waiting", missing, next: null };

  if (!state.decisions) state.decisions = {};
  const key = decisionKey(previous.epoch, previous.round);
  state.decisions[key] ??= {
    type: "abort", roster: cloneRoster(previous.roster), outcomes: null,
    reason: "active_reconnect_rollover",
  };
  state.epoch = previous.epoch + 1;
  state.round = 0;
  const roster = previous.roster.map((entry, index) => {
    const player = state.players[entry.playerId];
    return {
      playerId: entry.playerId,
      index,
      profile: cloneProfile(entry.profile),
      score: player.score,
    };
  });
  state.lastRoster = roster.map((entry) => entry.playerId);
  state.active = {
    epoch: state.epoch, round: 0, seed, reason: "active_reconnect_rollover",
    roster, proposal: null, reports: {},
  };
  return {
    type: "rollover", previousEpoch: previous.epoch, previousRound: previous.round,
    next: state.active,
  };
}

/**
 * Deterministic, defense-in-depth canonicalization of an outcome list against
 * an active (or terminal) roster. Returns a sorted, minimal array or null when
 * the payload is malformed or does not line up with the roster exactly.
 */
export function canonicalOutcomes(active, outcomes) {
  if (!active || !Array.isArray(outcomes) || outcomes.length !== active.roster.length) return null;
  const roster = new Set(active.roster.map((entry) => entry.playerId));
  const seen = new Set();
  const normalized = [];
  for (const value of outcomes) {
    if (!value || typeof value !== "object" || Array.isArray(value) ||
        Object.keys(value).sort().join(",") !== "placement,playerId,scoreDelta" ||
        typeof value.playerId !== "string" || !roster.has(value.playerId) || seen.has(value.playerId) ||
        !Number.isInteger(value.placement) || value.placement < 1 || value.placement > active.roster.length ||
        !Number.isSafeInteger(value.scoreDelta) || value.scoreDelta < 0 || value.scoreDelta > 1_000_000) return null;
    seen.add(value.playerId);
    normalized.push({ playerId: value.playerId, placement: value.placement, scoreDelta: value.scoreDelta });
  }
  normalized.sort((a, b) => a.playerId.localeCompare(b.playerId));
  return normalized;
}

/**
 * Selects the next canonical roster. Incumbents (members of `lastRoster`) keep
 * their seat ahead of newer joiners, which stabilizes membership across rounds
 * and queues mid-round joiners until a slot opens.
 */
export function selectLifecycleRoster(state) {
  const incumbentOrder = new Map((state.lastRoster ?? []).map((id, index) => [id, index]));
  const eligible = Object.values(state.players).filter((player) =>
    player.connected && player.ready && !player.expired && player.profile
  );
  eligible.sort((a, b) => {
    const ai = incumbentOrder.has(a.playerId) ? incumbentOrder.get(a.playerId) : Number.MAX_SAFE_INTEGER;
    const bi = incumbentOrder.has(b.playerId) ? incumbentOrder.get(b.playerId) : Number.MAX_SAFE_INTEGER;
    return ai - bi || a.joinedAt - b.joinedAt || a.playerId.localeCompare(b.playerId);
  });
  return eligible.slice(0, state.capacity).map((player) => player.playerId).sort();
}

/**
 * Validates an epoch-scoped signaling relay request against the immutable
 * active epoch. Returns `{ ok: true, epoch }` or `{ ok: false, code }`. Used by
 * the control WebSocket so stale/wrong-epoch signaling is rejected before any
 * relay, and unit-testable without a Workers runtime.
 */
export function validateEpochSignal(state, fromPlayerId, toPlayerId, epoch, round) {
  const active = state.active;
  if (!active) return { ok: false, code: "no_active_round" };
  if (epoch !== active.epoch || round !== active.round) return { ok: false, code: "stale_epoch" };
  if (fromPlayerId === toPlayerId) return { ok: false, code: "invalid_target" };
  const fromActive = active.roster.some((entry) => entry.playerId === fromPlayerId);
  const toActive = active.roster.some((entry) => entry.playerId === toPlayerId);
  if (!fromActive || !toActive) return { ok: false, code: "not_in_roster" };
  return { ok: true, epoch: active.epoch, round: active.round };
}

/**
 * Installs one immutable epoch bootstrap. An active bootstrap is never replaced:
 * ready/profile/join events during a round only affect the next selection, so a
 * call while `state.active` is set is a no-op (returns null). Ordinary assembly
 * requires exact capacity; a boundary-departure transition can explicitly allow
 * LGS to continue down to three players, while Duel still requires both seats.
 */
export function startLifecycleRound(state, seed, reason, allowReduced = false) {
  if (state.active) return null;
  const ids = selectLifecycleRoster(state);
  // Ordinary assembly remains exact-capacity. A boundary departure may
  // explicitly continue LGS with fewer seats, but never below mode minimum.
  const minimum = state.mode === "duel" ? 2 : 3;
  const required = allowReduced ? minimum : state.capacity;
  if (ids.length < required) return null;

  if (state.epoch < 0) {
    state.epoch = 0;
    state.round = 0;
  } else if (sameRoster(state.lastRoster, ids)) {
    state.round += 1;
  } else {
    state.epoch += 1;
    state.round = 0;
  }

  const roster = ids.map((playerId, index) => {
    const player = state.players[playerId];
    return {
      playerId,
      index,
      profile: cloneProfile(player.profile),
      score: player.score,
    };
  });
  state.lastRoster = [...ids];
  state.matchSeed ??= seed.padStart(32, "0").slice(-32);
  state.active = { epoch: state.epoch, round: state.round, seed, reason, roster, proposal: null, reports: {} };
  return state.active;
}

function decisionKey(epoch, round) { return `${epoch}:${round}`; }

/**
 * Queues an active member's departure without changing the current immutable
 * round. The request itself is durable and idempotent; readiness changes only
 * when that exact round commits or aborts.
 */
export function requestLifecycleBoundaryLeave(state, playerId) {
  const active = state.active;
  if (!active || !active.roster.some((entry) => entry.playerId === playerId)) {
    return { type: "error", code: "not_in_active_roster" };
  }
  state.boundaryDepartures ??= {};
  const existing = state.boundaryDepartures[playerId];
  const duplicate = existing?.epoch === active.epoch && existing?.round === active.round;
  if (!duplicate) state.boundaryDepartures[playerId] = { epoch: active.epoch, round: active.round };
  return {
    type: "pending", duplicate, playerId,
    epoch: active.epoch, round: active.round,
  };
}

function applyBoundaryDepartures(state, previous) {
  const pending = state.boundaryDepartures ?? {};
  const departing = previous.roster
    .map((entry) => entry.playerId)
    .filter((playerId) => pending[playerId]?.epoch === previous.epoch && pending[playerId]?.round === previous.round)
    .sort();
  // The terminal boundary consumes all requests for that immutable round.
  state.boundaryDepartures = {};
  for (const playerId of departing) {
    if (state.players[playerId]) state.players[playerId].ready = false;
  }
  return departing;
}

function finishBoundarySelection(state, completed, previous, seedForNext, reason) {
  const departing = applyBoundaryDepartures(state, previous);
  completed.next = startLifecycleRound(state, seedForNext, reason, departing.length > 0);
  if (!departing.length) return;

  const minimum = state.mode === "duel" ? 2 : 3;
  const eligible = selectLifecycleRoster(state);
  const terminated = !completed.next && eligible.length < minimum
    ? previous.roster.map((entry) => entry.playerId).filter((id) => !departing.includes(id)).sort()
    : [];
  if (terminated.length) {
    for (const playerId of terminated) if (state.players[playerId]) state.players[playerId].ready = false;
    state.lastRoster = [];
  }
  completed.boundary = { departing, terminated };
}

/**
 * Consensus is idempotent. Scores are applied once, only after every member
 * reports the same canonical outcome. A conflicting live proposal aborts the
 * round without mutating scores; a late conflict against a terminal decision is
 * rejected without mutating that decision or scores.
 *
 * Return shapes (consumed by the control WebSocket boundary):
 *  - { type: "ack",   epoch, round, duplicate, received, required }
 *  - { type: "commit", epoch, round, outcomes, scores, next }
 *  - { type: "abort",  epoch, round, reason, next }
 *  - { type: "error",  code }
 */
export function submitLifecycleReport(state, playerId, epoch, round, outcomes, seedForNext) {
  if (!state.decisions) state.decisions = {};
  const key = decisionKey(epoch, round);
  const decided = state.decisions[key];
  if (decided) {
    // Terminal decisions are immutable. An aborted round cached no outcome, so
    // any late report for it is stale (there is no consensus to acknowledge).
    if (decided.type === "abort") return { type: "error", code: "stale_report" };
    const inRoster = decided.roster.some((entry) => entry.playerId === playerId);
    if (!inRoster) return { type: "error", code: "stale_report" };
    const normalized = canonicalOutcomes({ roster: decided.roster }, outcomes);
    if (!normalized) return { type: "error", code: "stale_report" };
    if (JSON.stringify(normalized) !== decided.outcomes) {
      return { type: "error", code: "conflicting_terminal_report" };
    }
    return {
      type: "ack", epoch, round, duplicate: true,
      received: decided.roster.length, required: decided.roster.length,
    };
  }

  const active = state.active;
  if (!active || active.epoch !== epoch || active.round !== round ||
      !active.roster.some((entry) => entry.playerId === playerId)) return { type: "error", code: "stale_report" };
  const normalized = canonicalOutcomes(active, outcomes);
  if (!normalized) return { type: "error", code: "invalid_report" };
  const encoded = JSON.stringify(normalized);
  if ((active.reports[playerId] && active.reports[playerId] !== encoded) ||
      (active.proposal && active.proposal !== encoded)) {
    return abortLifecycleRound(state, "conflicting_reports", seedForNext);
  }
  active.proposal ??= encoded;
  const duplicate = active.reports[playerId] === encoded;
  active.reports[playerId] = encoded;
  const received = Object.keys(active.reports).length;
  const required = active.roster.length;
  if (received < required) {
    return { type: "ack", epoch: active.epoch, round: active.round, duplicate, received, required };
  }

  for (const outcome of normalized) state.players[outcome.playerId].score += outcome.scoreDelta;
  const roster = cloneRoster(active.roster);
  const completed = {
    type: "commit", epoch, round, outcomes: normalized,
    scores: roster.map((entry) => ({ playerId: entry.playerId, score: state.players[entry.playerId].score })),
  };
  state.decisions[key] = { type: "commit", roster, outcomes: encoded };
  state.active = null;
  clearReconnectBatch(state);
  const reachedMatchPoint = completed.scores.some((entry) => entry.score >= 3);
  const hasBoundaryDeparture = active.roster.some((entry) => {
    const pending = state.boundaryDepartures?.[entry.playerId];
    return pending?.epoch === active.epoch && pending?.round === active.round;
  });
  if (reachedMatchPoint && !hasBoundaryDeparture) {
    state.matchOver = true;
    completed.next = null;
    completed.matchOver = true;
    completed.matchGeneration = state.matchGeneration;
  } else {
    // An explicit boundary departure takes precedence over match-point UI: it
    // must produce the promised replacement/cleanup transition exactly once.
    state.matchOver = false;
    finishBoundarySelection(state, completed, active, seedForNext, "round_complete");
  }
  return completed;
}

/**
 * Terminates the active round without applying scores. The abort is terminal:
 * it is cached in `decisions` so late reports for this (epoch, round) are
 * rejected as stale. The returned `next` is the following bootstrap (or null if
 * a full roster is not yet eligible).
 */
export function abortLifecycleRound(state, reason, seedForNext) {
  const previous = state.active;
  if (!previous) return { type: "error", code: "no_active_round" };
  if (!state.decisions) state.decisions = {};
  const key = decisionKey(previous.epoch, previous.round);
  state.decisions[key] = {
    type: "abort",
    roster: cloneRoster(previous.roster),
    outcomes: null,
    reason,
  };
  state.active = null;
  clearReconnectBatch(state);
  const completed = {
    type: "abort", epoch: previous.epoch, round: previous.round, reason,
    next: null,
  };
  finishBoundarySelection(state, completed, previous, seedForNext, "epoch_abort");
  return completed;
}

export const REMATCH_DEADLINE_MS = 10_000;

/** Deterministically advances the 128-bit match seed without platform RNG. */
export function advanceMatchSeed(seed, generation) {
  let value = BigInt(`0x${seed}`) ^ (BigInt(generation) << 64n) ^ 0x9e3779b97f4a7c15n;
  const mask = (1n << 128n) - 1n;
  value = ((value ^ (value >> 30n)) * 0xbf58476d1ce4e5b9n) & mask;
  value = ((value ^ (value >> 27n)) * 0x94d049bb133111ebn) & mask;
  value ^= value >> 31n;
  return (value & mask).toString(16).padStart(32, "0");
}

function rematchRoster(state) {
  return state.lastRoster.filter((id) => {
    const player = state.players[id];
    return player && player.connected && !player.expired;
  });
}

function finishRematch(state) {
  const proposal = state.rematch;
  const ids = rematchRoster(state);
  if (!proposal || ids.length !== state.capacity || proposal.accepted.length !== ids.length) return null;
  for (const id of ids) state.players[id].score = 0;
  state.matchGeneration = proposal.generation;
  state.matchSeed = advanceMatchSeed(state.matchSeed, state.matchGeneration);
  state.epoch += 1;
  state.round = 0;
  const roster = [...ids].sort().map((playerId, index) => {
    const player = state.players[playerId];
    return { playerId, index, profile: cloneProfile(player.profile), score: 0 };
  });
  state.lastRoster = roster.map((entry) => entry.playerId);
  state.active = { epoch: state.epoch, round: 0, seed: state.matchSeed, reason: "rematch_accepted", roster, proposal: null, reports: {} };
  state.matchOver = false;
  clearReconnectBatch(state);
  const result = { type: "accepted", generation: proposal.generation, nonce: proposal.nonce, roster: [...state.lastRoster], next: state.active };
  state.lastRematchDecision = { type: "accepted", generation: result.generation, nonce: result.nonce };
  state.rematch = null;
  return result;
}

/**
 * Opens (or idempotently joins) a server-authoritative rematch vote. A request
 * is also an acceptance. Concurrent requests for the same next generation are
 * therefore mutual acceptances, regardless of which request reaches the DO
 * first; the first nonce becomes authoritative.
 */
export function requestLifecycleRematch(state, playerId, generation, nonce, now) {
  const terminal = state.lastRematchDecision ?? state.rematchDecision;
  if (terminal?.generation === generation && terminal.nonce === nonce) return { ...terminal, duplicate: true };
  if (generation <= state.matchGeneration) return { type: "error", code: "stale_rematch" };
  const ids = rematchRoster(state);
  if (!state.matchOver || generation !== state.matchGeneration + 1 || !state.lastRoster.includes(playerId)
      || ids.length !== state.lastRoster.length || ids.length !== state.capacity) return { type: "error", code: "stale_rematch" };
  if (state.rematch?.deadline <= now) return denyLifecycleRematch(state, "timeout");
  if (!state.rematch) state.rematch = { generation, nonce, deadline: now + REMATCH_DEADLINE_MS, requestedBy: playerId, accepted: [] };
  if (state.rematch.generation !== generation) return { type: "error", code: "stale_rematch" };
  const duplicate = state.rematch.accepted.includes(playerId);
  if (!duplicate) state.rematch.accepted.push(playerId);
  state.rematch.accepted.sort();
  return finishRematch(state) ?? { type: "pending", ...state.rematch, duplicate, required: state.capacity };
}

export function respondLifecycleRematch(state, playerId, generation, nonce, accept) {
  const terminal = state.lastRematchDecision ?? state.rematchDecision;
  if (terminal?.generation === generation && terminal.nonce === nonce) return { ...terminal, duplicate: true };
  if (generation <= state.matchGeneration) return { type: "error", code: "stale_rematch" };
  const proposal = state.rematch;
  if (!proposal || proposal.generation !== generation || proposal.nonce !== nonce || !state.lastRoster.includes(playerId)) return { type: "error", code: "stale_rematch" };
  if (!accept) return denyLifecycleRematch(state, "denied");
  const duplicate = proposal.accepted.includes(playerId);
  if (!duplicate) proposal.accepted.push(playerId);
  proposal.accepted.sort();
  return finishRematch(state) ?? { type: "pending", ...proposal, duplicate, required: state.capacity };
}

/** A denial/timeout/disconnect releases the entire immutable roster to menu. */
export function denyLifecycleRematch(state, reason) {
  const proposal = state.rematch;
  if (!proposal) return { type: "error", code: "no_rematch" };
  const result = { type: "denied", generation: proposal.generation, nonce: proposal.nonce, reason, roster: [...state.lastRoster] };
  state.matchGeneration = proposal.generation;
  state.lastRematchDecision = result;
  state.rematch = null;
  state.matchOver = false;
  state.active = null;
  clearReconnectBatch(state);
  for (const id of state.lastRoster) if (state.players[id]) state.players[id].ready = false;
  state.lastRoster = [];
  return result;
}

export function expireLifecycleRematch(state, now) {
  return state.rematch && state.rematch.deadline <= now ? denyLifecycleRematch(state, "timeout") : null;
}

/** Explicit exit never promotes a waiter into the current epoch. */
export function leaveLifecycleMatch(state, playerId, reason = "exit") {
  if (!state.lastRoster.includes(playerId)) return { type: "left", destination: "main_menu", roster: [playerId], reason };
  const roster = [...state.lastRoster];
  state.active = null;
  clearReconnectBatch(state);
  state.matchOver = false;
  state.rematch = null;
  for (const id of roster) if (state.players[id]) state.players[id].ready = false;
  return { type: "left", destination: "main_menu", roster, reason };
}

/** Explicit requeue releases the old roster and readies only the requester. */
export function requeueLifecyclePlayer(state, playerId) {
  const result = leaveLifecycleMatch(state, playerId, "requeue");
  if (state.players[playerId]) state.players[playerId].ready = true;
  state.lastRoster = [];
  return { ...result, destination: "requeue", requester: playerId };
}

function cloneProfile(profile) {
  return profile ? { name: profile.name, paletteId: profile.paletteId, cosmeticId: profile.cosmeticId } : null;
}

function cloneRoster(roster) {
  return roster.map((entry) => ({
    playerId: entry.playerId,
    index: entry.index,
    profile: cloneProfile(entry.profile),
    score: entry.score,
  }));
}
