import test from "node:test";
import assert from "node:assert/strict";
import {
  ANY_HOLD_MS, STAGING_MS, QUEUE_WATCHDOG_MS, advanceQueue,
  cancelQueue, createQueueState, heartbeatQueue, nextQueueDeadline, queueEntry,
  startVotesRequired, voteStartQueue, withdrawStartVoteQueue,
} from "../src/queue-state.js";

const NOW = 1_000_000;
const ticket = (n) => n.toString(16).padStart(32, "0");
function add(state, n, preference, now = NOW) {
  return queueEntry(state, { ticket: ticket(n), preference }, now);
}
function group(result) { return result.groups[0]; }
function stage(count = 3) {
  const state = createQueueState(NOW);
  let result;
  for (let n = 1; n <= count; n += 1) result = add(state, n, n % 2 ? "deathmatch" : "any");
  return { state, result };
}

test("pre-stage Duel arbitration and Deathmatch+Any unlocked behavior are preserved", () => {
  const duelState = createQueueState(NOW);
  add(duelState, 1, "duel");
  assert.deepEqual(group(add(duelState, 2, "duel")), {
    mode: "duel", capacity: 2, tickets: [ticket(1), ticket(2)],
  });

  const anyState = createQueueState(NOW);
  add(anyState, 1, "any"); add(anyState, 2, "any");
  assert.deepEqual(group(add(anyState, 3, "duel")).tickets, [ticket(1), ticket(3)]);

  const mixed = createQueueState(NOW);
  add(mixed, 1, "deathmatch"); add(mixed, 2, "any");
  assert.equal(mixed.lock, null);
  assert.deepEqual(group(add(mixed, 3, "duel")).tickets, [ticket(2), ticket(3)]);
  assert.ok(mixed.entries[ticket(1)]);

  const dmOnly = createQueueState(NOW);
  add(dmOnly, 1, "deathmatch"); add(dmOnly, 2, "deathmatch");
  assert.equal(advanceQueue(dmOnly, NOW + 100_000).groups.length, 0);
});

test("two Any hold for three seconds, accept a third, or fall back to Duel", () => {
  const state = createQueueState(NOW);
  add(state, 1, "any"); add(state, 2, "any");
  assert.equal(nextQueueDeadline(state), NOW + ANY_HOLD_MS);
  assert.equal(advanceQueue(state, NOW + ANY_HOLD_MS - 1).groups.length, 0);
  add(state, 3, "deathmatch", NOW + ANY_HOLD_MS - 1);
  assert.equal(state.lock.tickets.length, 3);

  const fallback = createQueueState(NOW);
  add(fallback, 1, "any"); add(fallback, 2, "any");
  assert.equal(group(advanceQueue(fallback, NOW + ANY_HOLD_MS)).mode, "duel");
});

test("three compatible tickets stage for one fixed thirty-second deadline", () => {
  const { state } = stage(3);
  assert.equal(state.lock.deadline, NOW + STAGING_MS);
  assert.deepEqual(state.lock.votes, []);
  assert.equal(nextQueueDeadline(state), NOW + STAGING_MS);

  add(state, 4, "any", NOW + 5_000);
  voteStartQueue(state, ticket(1), NOW + 6_000);
  cancelQueue(state, ticket(4), NOW + 7_000);
  assert.equal(state.lock.deadline, NOW + STAGING_MS);
});

test("strict majority thresholds are correct for every staging size 3-7", () => {
  for (let count = 3; count <= 7; count += 1) {
    assert.equal(startVotesRequired(count), Math.floor(count / 2) + 1);
    const { state } = stage(count);
    const required = startVotesRequired(count);
    for (let n = 1; n < required; n += 1) {
      assert.equal(voteStartQueue(state, ticket(n), NOW + 1).groups.length, 0, `count ${count}, vote ${n}`);
    }
    const result = voteStartQueue(state, ticket(required), NOW + 1);
    assert.equal(group(result).capacity, count);
  }
});

test("the eighth compatible ticket assigns immediately", () => {
  const { state } = stage(7);
  const result = add(state, 8, "any", NOW + 1);
  assert.deepEqual(group(result), {
    mode: "deathmatch", capacity: 8,
    tickets: Array.from({ length: 8 }, (_, index) => ticket(index + 1)),
  });
});

test("deadline automatically assigns every staging size 3-7", () => {
  for (let count = 3; count <= 7; count += 1) {
    const { state } = stage(count);
    assert.equal(advanceQueue(state, NOW + STAGING_MS - 1).groups.length, 0);
    assert.equal(group(advanceQueue(state, NOW + STAGING_MS)).capacity, count);
  }
});

test("votes and withdrawals are idempotent and limited to one vote per ticket", () => {
  const { state } = stage(5);
  assert.equal(voteStartQueue(state, ticket(1), NOW + 1).groups.length, 0);
  assert.equal(voteStartQueue(state, ticket(1), NOW + 2).groups.length, 0);
  assert.deepEqual(state.lock.votes, [ticket(1)]);
  withdrawStartVoteQueue(state, ticket(1), NOW + 3);
  withdrawStartVoteQueue(state, ticket(1), NOW + 4);
  assert.deepEqual(state.lock.votes, []);
});

test("joins change the majority threshold without resetting votes or deadline", () => {
  const { state } = stage(3);
  const deadline = state.lock.deadline;
  voteStartQueue(state, ticket(1), NOW + 1);
  add(state, 4, "any", NOW + 2);
  assert.equal(state.lock.deadline, deadline);
  assert.deepEqual(state.lock.votes, [ticket(1)]);
  assert.equal(startVotesRequired(state.lock.tickets.length), 3);
  assert.equal(voteStartQueue(state, ticket(2), NOW + 3).groups.length, 0);
  assert.equal(group(voteStartQueue(state, ticket(3), NOW + 4)).capacity, 4);
});

test("leave and disconnect remove votes, recompute threshold, and retain deadline", () => {
  const { state } = stage(6);
  const deadline = state.lock.deadline;
  voteStartQueue(state, ticket(1), NOW + 1);
  voteStartQueue(state, ticket(2), NOW + 1);
  voteStartQueue(state, ticket(6), NOW + 1);
  const left = cancelQueue(state, ticket(6), NOW + 2, "disconnected");
  assert.equal(left.groups.length, 0);
  assert.deepEqual(state.lock.votes, [ticket(1), ticket(2)]);
  assert.equal(state.lock.tickets.length, 5);
  assert.equal(startVotesRequired(5), 3);
  assert.equal(state.lock.deadline, deadline);
  assert.equal(group(voteStartQueue(state, ticket(3), NOW + 3)).capacity, 5);
});

test("leaving recomputes the threshold and can start with retained votes", () => {
  const { state } = stage(6);
  voteStartQueue(state, ticket(1), NOW + 1);
  voteStartQueue(state, ticket(2), NOW + 1);
  voteStartQueue(state, ticket(3), NOW + 1);
  // Six needs four votes; removing a non-voter makes three a majority of five.
  const result = cancelQueue(state, ticket(6), NOW + 2);
  assert.equal(group(result).capacity, 5);
  assert.deepEqual(group(result).tickets, [ticket(1), ticket(2), ticket(3), ticket(4), ticket(5)]);
});

test("falling below three dissolves staging and preserves original sequence", () => {
  const state = createQueueState(NOW);
  add(state, 1, "any"); add(state, 2, "deathmatch"); add(state, 3, "deathmatch");
  cancelQueue(state, ticket(2), NOW + 1);
  assert.equal(state.lock, null);
  assert.equal(state.entries[ticket(1)].sequence, 0);
  assert.equal(state.entries[ticket(3)].sequence, 2);
  // A later Duel steals the oldest compatible Any while the DM survivor waits.
  const result = add(state, 4, "duel", NOW + 2);
  assert.deepEqual(group(result).tickets, [ticket(1), ticket(4)]);
});

test("a staged group cannot be stolen by Duel", () => {
  const { state } = stage(3);
  add(state, 4, "duel", NOW + 1);
  const result = add(state, 5, "duel", NOW + 2);
  assert.deepEqual(group(result).tickets, [ticket(4), ticket(5)]);
  assert.deepEqual(state.lock.tickets, [ticket(1), ticket(2), ticket(3)]);
});

test("equal-time deadline wins deterministically over join, vote, and leave", () => {
  for (const operation of [
    (state) => add(state, 4, "any", NOW + STAGING_MS),
    (state) => voteStartQueue(state, ticket(1), NOW + STAGING_MS),
    (state) => cancelQueue(state, ticket(1), NOW + STAGING_MS),
  ]) {
    const { state } = stage(3);
    const result = operation(state);
    assert.equal(group(result).capacity, 3);
    assert.deepEqual(group(result).tickets, [ticket(1), ticket(2), ticket(3)]);
  }
});

test("heartbeat disconnect removes its vote and recomputes staging", () => {
  const state = createQueueState(NOW);
  add(state, 1, "deathmatch"); add(state, 2, "deathmatch");
  heartbeatQueue(state, ticket(1), NOW + 10_000);
  add(state, 3, "deathmatch", NOW + 20_000);
  add(state, 4, "any", NOW + 20_000);
  add(state, 5, "deathmatch", NOW + 20_000);
  voteStartQueue(state, ticket(1), NOW + 20_001);
  voteStartQueue(state, ticket(2), NOW + 20_001);
  const result = advanceQueue(state, NOW + QUEUE_WATCHDOG_MS);
  assert.deepEqual(result.removed, [{ ticket: ticket(2), reason: "heartbeat_timeout" }]);
  assert.equal(state.lock.tickets.length, 4);
  assert.deepEqual(state.lock.votes, [ticket(1)]);
  assert.equal(startVotesRequired(4), 3);
  assert.equal(state.lock.deadline, NOW + 20_000 + STAGING_MS);
});
