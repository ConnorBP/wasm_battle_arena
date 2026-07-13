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
  if (player.expired || (player.reconnectUntil !== null && player.reconnectUntil <= now)) {
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
export function validateEpochSignal(state, fromPlayerId, toPlayerId, epoch) {
  const active = state.active;
  if (!active) return { ok: false, code: "no_active_round" };
  if (epoch !== active.epoch) return { ok: false, code: "stale_epoch" };
  if (fromPlayerId === toPlayerId) return { ok: false, code: "invalid_target" };
  const fromActive = active.roster.some((entry) => entry.playerId === fromPlayerId);
  const toActive = active.roster.some((entry) => entry.playerId === toPlayerId);
  if (!fromActive || !toActive) return { ok: false, code: "not_in_roster" };
  return { ok: true, epoch: active.epoch };
}

/**
 * Installs one immutable epoch bootstrap. An active bootstrap is never replaced:
 * ready/profile/join events during a round only affect the next selection, so a
 * call while `state.active` is set is a no-op (returns null). A fixed-capacity
 * game never constructs a partial GGRS session, so a short roster also no-ops.
 */
export function startLifecycleRound(state, seed, reason) {
  if (state.active) return null;
  const ids = selectLifecycleRoster(state);
  if (ids.length !== state.capacity) return null;

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
  state.active = { epoch: state.epoch, round: state.round, seed, reason, roster, proposal: null, reports: {} };
  return state.active;
}

function decisionKey(epoch, round) { return `${epoch}:${round}`; }

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
  completed.next = startLifecycleRound(state, seedForNext, "round_complete");
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
  return {
    type: "abort", epoch: previous.epoch, round: previous.round, reason,
    next: startLifecycleRound(state, seedForNext, "epoch_abort"),
  };
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
