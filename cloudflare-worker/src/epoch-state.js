export function createEpochState(mode, capacity, now) {
  return { version: 3, mode, capacity, createdAt: now, matchId: null, epoch: -1, round: -1, players: {}, active: null };
}

export function canonicalReport(active, outcomes) {
  if (!active || !Array.isArray(outcomes) || outcomes.length !== active.roster.length) return null;
  const roster = new Set(active.roster.map(entry => entry.playerId));
  const seen = new Set();
  const normalized = [];
  for (const value of outcomes) {
    if (!value || typeof value !== "object" || Object.keys(value).sort().join(",") !== "placement,playerId,scoreDelta") return null;
    if (!roster.has(value.playerId) || seen.has(value.playerId) || !Number.isInteger(value.placement) || value.placement < 1 || value.placement > 4 || !Number.isSafeInteger(value.scoreDelta) || value.scoreDelta < 0 || value.scoreDelta > 1_000_000) return null;
    seen.add(value.playerId);
    normalized.push({ playerId: value.playerId, placement: value.placement, scoreDelta: value.scoreDelta });
  }
  normalized.sort((a,b) => a.playerId.localeCompare(b.playerId));
  return normalized;
}

export function selectNextRoster(state) {
  const oldOrder = new Map((state.active?.roster ?? []).map((entry,index) => [entry.playerId,index]));
  const eligible = Object.values(state.players).filter(player => player.connected && player.ready && !player.expired && player.profile);
  eligible.sort((a,b) => {
    const ai = oldOrder.has(a.playerId) ? oldOrder.get(a.playerId) : Number.MAX_SAFE_INTEGER;
    const bi = oldOrder.has(b.playerId) ? oldOrder.get(b.playerId) : Number.MAX_SAFE_INTEGER;
    return ai - bi || a.joinedAt - b.joinedAt || a.playerId.localeCompare(b.playerId);
  });
  return eligible.slice(0, state.capacity).map(player => player.playerId).sort();
}

export function startNextEpoch(state, seed, reason) {
  const ids = selectNextRoster(state);
  if (ids.length < 2 || (state.epoch < 0 && ids.length < state.capacity)) return null;
  const previousIds = state.active?.roster.map(entry => entry.playerId) ?? [];
  if (JSON.stringify(previousIds) !== JSON.stringify(ids)) state.epoch += 1;
  state.round += 1;
  const roster = ids.map((playerId,index) => {
    const player = state.players[playerId];
    return { playerId, index, profile: structuredClone(player.profile), score: player.score };
  });
  state.active = { epoch: state.epoch, round: state.round, seed, reason, roster, proposal: null, reports: {} };
  return state.active;
}

export function submitReport(state, playerId, epoch, round, outcomes, seedForNext) {
  const active = state.active;
  if (!active || active.epoch !== epoch || active.round !== round || !active.roster.some(entry => entry.playerId === playerId)) return { type: "error", code: "stale_report" };
  const normalized = canonicalReport(active, outcomes);
  if (!normalized) return { type: "error", code: "invalid_report" };
  const encoded = JSON.stringify(normalized);
  if (active.reports[playerId] && active.reports[playerId] !== encoded) return abort(state, "conflicting_reports", seedForNext);
  if (active.proposal && active.proposal !== encoded) return abort(state, "conflicting_reports", seedForNext);
  active.proposal ??= encoded;
  active.reports[playerId] = encoded;
  if (Object.keys(active.reports).length < active.roster.length) return { type: "ack", received: Object.keys(active.reports).length, required: active.roster.length };
  for (const outcome of normalized) state.players[outcome.playerId].score += outcome.scoreDelta;
  const completed = { type: "commit", epoch, round, outcomes: normalized, scores: active.roster.map(entry => ({ playerId: entry.playerId, score: state.players[entry.playerId].score })) };
  state.active = null;
  completed.next = startNextEpoch(state, seedForNext, "round_complete");
  return completed;
}

export function abort(state, reason, seedForNext) {
  const previous = state.active;
  state.active = null;
  return { type: "abort", epoch: previous?.epoch, round: previous?.round, reason, next: startNextEpoch(state, seedForNext, "epoch_abort") };
}
