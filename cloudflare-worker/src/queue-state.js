/*
 * Pure protocol-v4 flexible matchmaking reducer. It deliberately contains no
 * clock, random, crypto, socket, or storage access. Callers provide an absolute
 * time and stable ticket; every ordering uses (sequence, ticket).
 */
export const ANY_HOLD_MS = 3_000;
export const STAGING_MS = 30_000;
// Backward-compatible source export; expansion is now the 30s staging phase.
export const EXPANSION_MS = STAGING_MS;
export const QUEUE_WATCHDOG_MS = 45_000;
export const ASSIGNMENT_TTL_MS = 30_000;
export const MAX_QUEUE_ENTRIES = 256;
export const MAX_DEATHMATCH_PLAYERS = 8;

export function createQueueState(now = 0) {
  return { version: 5, createdAt: now, nextSequence: 0, entries: {}, lock: null };
}

// Upgrade the earlier target-based, two-second expansion record in place. Its
// creation time is recoverable from the old deadline, so the Wave1 staging
// deadline remains absolute rather than being reset by Worker activation.
export function migrateQueueState(state, now = 0) {
  if (!state || typeof state !== "object") return createQueueState(now);
  state.entries ??= {};
  state.assignments ??= {};
  state.nextSequence ??= Object.keys(state.entries).length;
  for (const entry of Object.values(state.entries)) delete entry.target;
  if ((state.version ?? 4) < 5 && state.lock) {
    state.lock.deadline = (state.lock.deadline ?? now) + (STAGING_MS - 2_000);
    state.lock.votes = [];
    delete state.lock.target;
  }
  state.version = 5;
  return state;
}

export function queueEntry(state, value, now) {
  if (state.entries[value.ticket]) return { type: "error", code: "duplicate_ticket", groups: [] };

  // A deadline wins at its exact millisecond. This makes an alarm racing a
  // join deterministic regardless of which Durable Object event is delivered
  // first: the expired roster is decided before the newcomer is considered.
  const before = arbitrateQueue(state, now);
  if (Object.keys(state.entries).length >= MAX_QUEUE_ENTRIES) {
    return { type: "error", code: "queue_full", groups: before.groups };
  }
  state.entries[value.ticket] = {
    ticket: value.ticket,
    preference: value.preference,
    sequence: state.nextSequence++,
    joinedAt: now,
    heartbeatAt: now,
    anyHoldDeadline: null,
    locked: false,
  };
  return combineResults(before, arbitrateQueue(state, now));
}

export function heartbeatQueue(state, ticket, now) {
  if (!state.entries[ticket]) return { type: "error", code: "not_queued" };
  const result = arbitrateQueue(state, now);
  const entry = state.entries[ticket];
  // A heartbeat at an exact gameplay deadline cannot postpone assignment.
  if (!entry) return result;
  entry.heartbeatAt = now;
  return { ...result, type: result.groups.length ? "assigned" : "heartbeat", ticket };
}

export function voteStartQueue(state, ticket, now) {
  if (!state.entries[ticket]) return { type: "error", code: "not_queued", groups: [] };
  if (!state.lock?.tickets.includes(ticket)) return { type: "error", code: "not_staging", groups: [] };
  const before = arbitrateQueue(state, now);
  // Exact-time expiry may have consumed this otherwise valid staged ticket.
  if (!state.entries[ticket]) return before;
  if (!state.lock.votes.includes(ticket)) state.lock.votes.push(ticket);
  return combineResults(before, arbitrateQueue(state, now));
}

export function withdrawStartVoteQueue(state, ticket, now) {
  if (!state.entries[ticket]) return { type: "error", code: "not_queued", groups: [] };
  if (!state.lock?.tickets.includes(ticket)) return { type: "error", code: "not_staging", groups: [] };
  const before = arbitrateQueue(state, now);
  // Exact-time expiry may have consumed this otherwise valid staged ticket.
  if (!state.entries[ticket]) return before;
  state.lock.votes = state.lock.votes.filter((voter) => voter !== ticket);
  return combineResults(before, arbitrateQueue(state, now));
}

export function cancelQueue(state, ticket, now, reason = "cancelled") {
  // As with joins and votes, an assignment whose deadline is exactly `now`
  // wins over a simultaneous cancellation/disconnect.
  const before = arbitrateQueue(state, now);
  if (!state.entries[ticket]) return before;
  removeEntry(state, ticket);
  const after = arbitrateQueue(state, now);
  const result = combineResults(before, after);
  result.removed = [{ ticket, reason }];
  return result;
}

export function advanceQueue(state, now) {
  // Resolve gameplay deadlines first at an equal timestamp. In particular, a
  // staged roster reaching its deadline is assigned rather than being changed
  // by a watchdog expiration delivered by the same alarm.
  const before = arbitrateQueue(state, now);
  const removed = [];
  for (const entry of orderedEntries(state)) {
    if (entry.heartbeatAt + QUEUE_WATCHDOG_MS <= now) {
      removed.push({ ticket: entry.ticket, reason: "heartbeat_timeout" });
      removeEntry(state, entry.ticket);
    }
  }
  const result = combineResults(before, arbitrateQueue(state, now));
  return { ...result, removed };
}

/**
 * Run all decisions possible at `now`. Returned groups are deterministic and
 * already removed from state. A group has an exact final mode/capacity.
 */
export function arbitrateQueue(state, now) {
  const groups = [];

  // A staged deathmatch is immutable against duel stealing. Its absolute
  // deadline is fixed when the first three members stage and never changes.
  if (state.lock) {
    normalizeLock(state.lock);
    fillLock(state);
    if (shouldStartLock(state.lock, now)) groups.push(finalizeLock(state));
  }

  let changed = true;
  while (changed) {
    changed = false;

    // Specific Duel is strongest before staging: pair the oldest Duel with
    // another Duel, or immediately with the oldest unlocked Any.
    const duel = unlocked(state).filter((entry) => entry.preference === "duel");
    if (duel.length) {
      const first = duel[0];
      const second = duel[1] ?? unlocked(state).find((entry) => entry.preference === "any");
      if (second) {
        groups.push(finalize(state, "duel", [first, second]));
        changed = true;
        continue;
      }
    }

    // Three compatible tickets establish staging. Any and Deathmatch are
    // mutually compatible; there is no client-selected target/capacity.
    const compatible = unlocked(state).filter((entry) => entry.preference !== "duel");
    if (!state.lock && compatible.length >= 3) {
      const members = compatible.slice(0, 3);
      state.lock = {
        tickets: members.map((entry) => entry.ticket),
        votes: [],
        deadline: now + STAGING_MS,
      };
      for (const member of members) {
        member.locked = true;
        member.anyHoldDeadline = null;
      }
      fillLock(state);
      if (shouldStartLock(state.lock, now)) {
        groups.push(finalizeLock(state));
        changed = true;
        continue;
      }
      // At most one staging group exists in a public-pool Durable Object.
      break;
    }

    // Two Any wait three seconds for a compatible third. A Duel may take one
    // during this hold (handled above). At expiry they become a Duel.
    const any = compatible.filter((entry) => entry.preference === "any");
    if (any.length >= 2) {
      const pair = any.slice(0, 2);
      const deadline = pair[0].anyHoldDeadline ?? pair[1].anyHoldDeadline ?? now + ANY_HOLD_MS;
      pair[0].anyHoldDeadline = deadline;
      pair[1].anyHoldDeadline = deadline;
      if (deadline <= now) {
        groups.push(finalize(state, "duel", pair));
        changed = true;
      }
    }
  }
  return { type: groups.length ? "assigned" : "waiting", groups };
}

export function startVotesRequired(count) {
  return Math.floor(count / 2) + 1;
}

export function nextQueueDeadline(state) {
  let deadline = state.lock?.deadline ?? Infinity;
  for (const entry of Object.values(state.entries)) {
    deadline = Math.min(deadline, entry.heartbeatAt + QUEUE_WATCHDOG_MS);
    if (entry.anyHoldDeadline !== null) deadline = Math.min(deadline, entry.anyHoldDeadline);
  }
  return Number.isFinite(deadline) ? deadline : null;
}

function shouldStartLock(lock, now) {
  return lock.tickets.length >= MAX_DEATHMATCH_PLAYERS ||
    lock.deadline <= now ||
    lock.votes.length >= startVotesRequired(lock.tickets.length);
}

function fillLock(state) {
  const lock = state.lock;
  if (!lock) return;
  normalizeLock(lock);
  while (lock.tickets.length < MAX_DEATHMATCH_PLAYERS) {
    const candidate = unlocked(state).find((entry) => entry.preference !== "duel");
    if (!candidate) break;
    candidate.locked = true;
    candidate.anyHoldDeadline = null;
    lock.tickets.push(candidate.ticket);
  }
}

function finalizeLock(state) {
  const members = state.lock.tickets.map((ticket) => state.entries[ticket]).filter(Boolean);
  state.lock = null;
  return finalize(state, "deathmatch", members);
}

function finalize(state, mode, members) {
  const ordered = [...members].sort(compareEntries);
  for (const entry of ordered) delete state.entries[entry.ticket];
  // Any-pair holds are relationships, not individual age limits. Taking one
  // member by Duel dissolves that hold; a future pair receives a fresh 3s.
  if (mode === "duel") clearUnlockedAnyHolds(state);
  return { mode, capacity: ordered.length, tickets: ordered.map((entry) => entry.ticket) };
}

function removeEntry(state, ticket) {
  const removed = state.entries[ticket];
  delete state.entries[ticket];
  if (!removed) return;

  if (!state.lock?.tickets.includes(ticket)) {
    if (removed.preference === "any" && removed.anyHoldDeadline !== null) clearUnlockedAnyHolds(state);
    return;
  }

  state.lock.tickets = state.lock.tickets.filter((member) => member !== ticket);
  state.lock.votes = (state.lock.votes ?? []).filter((voter) => voter !== ticket);
  if (state.lock.tickets.length >= 3) return;

  // Below three, staging dissolves. Survivors retain their original sequence
  // and are immediately reconsidered by pre-stage arbitration.
  for (const memberTicket of state.lock.tickets) {
    const member = state.entries[memberTicket];
    if (member) {
      member.locked = false;
      member.anyHoldDeadline = null;
    }
  }
  state.lock = null;
}

function normalizeLock(lock) {
  if (!Array.isArray(lock.votes)) lock.votes = [];
  lock.votes = [...new Set(lock.votes)].filter((ticket) => lock.tickets.includes(ticket));
}
function combineResults(left, right) {
  const groups = [...(left.groups ?? []), ...(right.groups ?? [])];
  return { type: groups.length ? "assigned" : "waiting", groups };
}
function clearUnlockedAnyHolds(state) {
  for (const entry of Object.values(state.entries)) {
    if (!entry.locked && entry.preference === "any") entry.anyHoldDeadline = null;
  }
}
function unlocked(state) {
  return orderedEntries(state).filter((entry) => !entry.locked);
}
function orderedEntries(state) {
  return Object.values(state.entries).sort(compareEntries);
}
function compareEntries(a, b) {
  return a.sequence - b.sequence || a.ticket.localeCompare(b.ticket);
}
