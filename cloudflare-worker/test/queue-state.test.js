import test from "node:test";
import assert from "node:assert/strict";
import {
  ANY_HOLD_MS, EXPANSION_MS, QUEUE_WATCHDOG_MS, advanceQueue,
  cancelQueue, createQueueState, heartbeatQueue, nextQueueDeadline, queueEntry,
} from "../src/queue-state.js";

const NOW = 1_000_000;
const ticket = (n) => n.toString(16).padStart(32, "0");
function add(state, n, preference, target = 8, now = NOW) {
  return queueEntry(state, { ticket: ticket(n), preference, target }, now);
}
function group(result) { return result.groups[0]; }

test("all two-player preference combinations obey duel and non-downgrade rules", () => {
  const preferences = ["duel", "any", "deathmatch"];
  for (const left of preferences) for (const right of preferences) {
    const state = createQueueState(NOW);
    add(state, 1, left);
    const result = add(state, 2, right);
    const shouldDuel = (left === "duel" && right !== "deathmatch") ||
      (right === "duel" && left !== "deathmatch");
    if (shouldDuel) {
      assert.equal(result.groups.length, 1, `${left}+${right}`);
      assert.equal(group(result).mode, "duel");
    } else {
      assert.equal(result.groups.length, 0, `${left}+${right}`);
    }
  }

  for (const left of preferences) for (const middle of preferences) for (const right of preferences) {
    const state = createQueueState(NOW);
    add(state, 1, left);
    add(state, 2, middle);
    const result = add(state, 3, right);
    // Every triple must make a bounded deterministic decision or remain in a
    // valid waiting/expansion state; the detailed policy cases follow below.
    assert.ok(result.groups.every((value) =>
      (value.mode === "duel" && value.capacity === 2) ||
      (value.mode === "deathmatch" && value.capacity >= 3 && value.capacity <= 8)
    ));
    assert.ok(state.lock === null || state.lock.tickets.length >= 3);
  }
});

test("Duel pairs oldest Duel, otherwise immediately takes oldest Any", () => {
  const state = createQueueState(NOW);
  add(state, 1, "any");
  add(state, 2, "any");
  const result = add(state, 3, "duel");
  assert.deepEqual(group(result), { mode: "duel", capacity: 2, tickets: [ticket(1), ticket(3)] });

  const duelState = createQueueState(NOW);
  add(duelState, 4, "duel");
  const paired = add(duelState, 5, "duel");
  assert.deepEqual(group(paired).tickets, [ticket(4), ticket(5)]);
});

test("two Any hold for three seconds, compatible third starts LGS", () => {
  const state = createQueueState(NOW);
  add(state, 1, "any", 8);
  add(state, 2, "any", 8);
  assert.equal(nextQueueDeadline(state), NOW + ANY_HOLD_MS);
  assert.equal(advanceQueue(state, NOW + ANY_HOLD_MS - 1).groups.length, 0);
  const result = add(state, 3, "deathmatch", 3, NOW + 2_999);
  assert.equal(state.lock, null);
  assert.deepEqual(group(result), { mode: "deathmatch", capacity: 3, tickets: [ticket(1), ticket(2), ticket(3)] });
});

test("two Any expire to Duel exactly at deadline", () => {
  const state = createQueueState(NOW);
  add(state, 1, "any");
  add(state, 2, "any");
  const result = advanceQueue(state, NOW + ANY_HOLD_MS);
  assert.equal(group(result).mode, "duel");
  assert.equal(group(result).capacity, 2);
});

test("Deathmatch plus Any stays unlocked and Any can be stolen by Duel", () => {
  const state = createQueueState(NOW);
  add(state, 1, "deathmatch");
  add(state, 2, "any");
  assert.equal(state.lock, null);
  const result = add(state, 3, "duel");
  assert.deepEqual(group(result).tickets, [ticket(2), ticket(3)]);
  assert.ok(state.entries[ticket(1)]);
});

test("DM-only never downgrades to duel", () => {
  const state = createQueueState(NOW);
  add(state, 1, "deathmatch");
  add(state, 2, "deathmatch");
  assert.equal(advanceQueue(state, NOW + 100_000).groups.length, 0);
});

test("three lock and expand for two seconds through every capacity 3-8", () => {
  for (let capacity = 3; capacity <= 8; capacity += 1) {
    const state = createQueueState(NOW);
    add(state, 1, "deathmatch", capacity);
    add(state, 2, "any", capacity);
    const third = add(state, 3, "deathmatch", capacity);
    if (capacity === 3) {
      assert.equal(group(third).capacity, 3);
      continue;
    }
    assert.equal(state.lock.deadline, NOW + EXPANSION_MS);
    for (let n = 4; n <= capacity; n += 1) {
      const result = add(state, n, n % 2 ? "deathmatch" : "any", capacity, NOW + 1);
      if (n === capacity) assert.equal(group(result).capacity, capacity);
      else assert.equal(result.groups.length, 0);
    }
  }
});

test("locked members cannot be stolen and partial expansion finalizes at deadline", () => {
  const state = createQueueState(NOW);
  add(state, 1, "any", 8);
  add(state, 2, "any", 8);
  add(state, 3, "deathmatch", 8);
  assert.ok(state.lock);
  assert.equal(add(state, 4, "duel", 8, NOW + 1).groups.length, 0);
  assert.equal(state.entries[ticket(4)].preference, "duel");
  const duel = add(state, 6, "duel", 8, NOW + 1);
  assert.deepEqual(group(duel).tickets, [ticket(4), ticket(6)]);
  const result = advanceQueue(state, NOW + EXPANSION_MS);
  assert.equal(group(result).mode, "deathmatch");
  assert.equal(group(result).capacity, 3);
});

test("minimum compatible target bounds expansion", () => {
  const state = createQueueState(NOW);
  add(state, 1, "deathmatch", 8);
  add(state, 2, "any", 5);
  add(state, 3, "deathmatch", 8);
  add(state, 4, "any", 8);
  const result = add(state, 5, "deathmatch", 8);
  assert.equal(group(result).capacity, 5);
});

test("sequence is primary and ticket is deterministic tie-break", () => {
  const state = createQueueState(NOW);
  add(state, 9, "any");
  add(state, 1, "any");
  const result = add(state, 5, "duel");
  assert.deepEqual(group(result).tickets, [ticket(9), ticket(5)]);
});

test("cancel and disconnect unlock a lock and preserve survivor order", () => {
  const state = createQueueState(NOW);
  add(state, 1, "any"); add(state, 2, "any"); add(state, 3, "deathmatch");
  const result = cancelQueue(state, ticket(2), NOW + 1, "disconnected");
  assert.equal(result.removed[0].reason, "disconnected");
  assert.equal(state.lock, null);
  assert.equal(state.entries[ticket(1)].locked, false);
  assert.equal(state.entries[ticket(3)].locked, false);
});

test("heartbeat watchdog has no ordinary queue timeout", () => {
  const state = createQueueState(NOW);
  add(state, 1, "deathmatch");
  assert.equal(heartbeatQueue(state, ticket(1), NOW + QUEUE_WATCHDOG_MS - 1).type, "heartbeat");
  assert.equal(advanceQueue(state, NOW + QUEUE_WATCHDOG_MS).removed.length, 0);
  const expired = advanceQueue(state, NOW + 2 * QUEUE_WATCHDOG_MS - 1);
  assert.equal(expired.removed.length, 1);
  assert.equal(expired.removed[0].reason, "heartbeat_timeout");
});
