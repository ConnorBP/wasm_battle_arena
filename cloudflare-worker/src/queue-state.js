/*
 * Pure protocol-v4 flexible matchmaking reducer. It deliberately contains no
 * clock, random, crypto, socket, or storage access. Callers provide an absolute
 * time and stable ticket; every ordering uses (sequence, ticket).
 */
export const ANY_HOLD_MS = 3_000;
export const EXPANSION_MS = 2_000;
export const QUEUE_WATCHDOG_MS = 45_000;
export const ASSIGNMENT_TTL_MS = 30_000;
export const MAX_QUEUE_ENTRIES = 256;

export function createQueueState(now = 0) {
  return { version: 4, createdAt: now, nextSequence: 0, entries: {}, lock: null };
}

export function queueEntry(state, value, now) {
  if (state.entries[value.ticket]) return { type: "error", code: "duplicate_ticket" };
  if (Object.keys(state.entries).length >= MAX_QUEUE_ENTRIES) return { type: "error", code: "queue_full" };
  state.entries[value.ticket] = {
    ticket: value.ticket,
    preference: value.preference,
    target: value.target,
    sequence: state.nextSequence++,
    joinedAt: now,
    heartbeatAt: now,
    anyHoldDeadline: null,
    locked: false,
  };
  return arbitrateQueue(state, now);
}

export function heartbeatQueue(state, ticket, now) {
  const entry = state.entries[ticket];
  if (!entry) return { type: "error", code: "not_queued" };
  entry.heartbeatAt = now;
  return { type: "heartbeat", ticket };
}

export function cancelQueue(state, ticket, now, reason = "cancelled") {
  if (!state.entries[ticket]) return { type: "noop", groups: [] };
  removeEntry(state, ticket);
  const result = arbitrateQueue(state, now);
  return { ...result, removed: [{ ticket, reason }] };
}

export function advanceQueue(state, now) {
  const removed = [];
  for (const entry of orderedEntries(state)) {
    if (entry.heartbeatAt + QUEUE_WATCHDOG_MS <= now) {
      removed.push({ ticket: entry.ticket, reason: "heartbeat_timeout" });
      removeEntry(state, entry.ticket);
    }
  }
  const result = arbitrateQueue(state, now);
  return { ...result, removed };
}

/**
 * Run all decisions possible at `now`. Returned groups are deterministic and
 * already removed from state. A group has an exact final mode/capacity.
 */
export function arbitrateQueue(state, now) {
  const groups = [];

  // A locked deathmatch is immutable against duel stealing. It only expands
  // with compatible DM/Any entries until its common target or deadline.
  if (state.lock) {
    fillLock(state);
    if (state.lock.tickets.length >= state.lock.target || state.lock.deadline <= now) {
      groups.push(finalizeLock(state));
    }
  }

  let changed = true;
  while (changed) {
    changed = false;

    // Specific duel is strongest: pair the oldest duel with another duel;
    // only when none exists may it take the oldest unlocked Any.
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

    // Three unlocked DM-compatible players establish an immutable expansion
    // lock. Any target is a maximum acceptable roster size, so the common
    // expansion target is the minimum requested target.
    const compatible = unlocked(state).filter((entry) => entry.preference !== "duel");
    if (!state.lock && compatible.length >= 3) {
      const members = compatible.slice(0, 3);
      state.lock = {
        tickets: members.map((entry) => entry.ticket),
        target: Math.min(...members.map((entry) => entry.target)),
        deadline: now + EXPANSION_MS,
      };
      for (const member of members) {
        member.locked = true;
        member.anyHoldDeadline = null;
      }
      fillLock(state);
      if (state.lock.tickets.length >= state.lock.target) groups.push(finalizeLock(state));
      changed = true;
      // Only one expansion timer is needed per public-pool DO. Do not build a
      // second lock until the first atomically finalizes.
      if (state.lock) break;
      continue;
    }

    // Two Any wait three seconds for a compatible third. A duel may steal one
    // during this interval (handled above). At expiry they become a duel.
    const any = compatible.filter((entry) => entry.preference === "any");
    if (any.length >= 2) {
      const pair = any.slice(0, 2);
      const deadline = pair[0].anyHoldDeadline ?? pair[1].anyHoldDeadline ?? now + ANY_HOLD_MS;
      pair[0].anyHoldDeadline = deadline;
      pair[1].anyHoldDeadline = deadline;
      if (deadline <= now) {
        groups.push(finalize(state, "duel", pair));
        changed = true;
        continue;
      }
    }
  }
  return { type: groups.length ? "assigned" : "waiting", groups };
}

export function nextQueueDeadline(state) {
  let deadline = state.lock?.deadline ?? Infinity;
  for (const entry of Object.values(state.entries)) {
    deadline = Math.min(deadline, entry.heartbeatAt + QUEUE_WATCHDOG_MS);
    if (entry.anyHoldDeadline !== null) deadline = Math.min(deadline, entry.anyHoldDeadline);
  }
  return Number.isFinite(deadline) ? deadline : null;
}

function fillLock(state) {
  const lock = state.lock;
  if (!lock) return;
  while (lock.tickets.length < lock.target) {
    const size = lock.tickets.length + 1;
    const candidate = unlocked(state).find((entry) =>
      entry.preference !== "duel" && entry.target >= size
    );
    if (!candidate) break;
    candidate.locked = true;
    candidate.anyHoldDeadline = null;
    lock.tickets.push(candidate.ticket);
    lock.target = Math.min(lock.target, candidate.target);
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
  delete state.entries[ticket];
  clearUnlockedAnyHolds(state);
  if (!state.lock?.tickets.includes(ticket)) return;
  // A disconnect/cancel cannot leave a partial lock. Survivors are unlocked
  // with their original sequence and may be deterministically reconsidered.
  for (const memberTicket of state.lock.tickets) {
    const member = state.entries[memberTicket];
    if (member) member.locked = false;
  }
  state.lock = null;
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
