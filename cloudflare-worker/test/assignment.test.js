import test from "node:test";
import assert from "node:assert/strict";
import { consumeAssignment, signAssignment, verifyAssignment } from "../src/assignment.js";

const secret = "test-only-secret-that-is-at-least-thirty-two-bytes";
const fields = {
  room: `q4_${"a".repeat(32)}`,
  mode: "deathmatch",
  capacity: 5,
  ticket: "b".repeat(32),
  expiresAt: 2_000_000_000_000,
};

test("assignment signs and verifies canonical fields", async () => {
  const token = await signAssignment(secret, fields);
  assert.match(token, /^[0-9a-f]{64}$/);
  assert.equal(await verifyAssignment(secret, fields, token), true);
});

test("assignment rejects signature tampering and every canonical mismatch", async () => {
  const token = await signAssignment(secret, fields);
  assert.equal(await verifyAssignment(secret, fields, `${token.slice(0, -1)}${token.endsWith("0") ? "1" : "0"}`), false);
  for (const changed of [
    { room: `q4_${"c".repeat(32)}` },
    { mode: "duel", capacity: 2 },
    { capacity: 4 },
    { ticket: "d".repeat(32) },
    { expiresAt: fields.expiresAt + 1 },
  ]) assert.equal(await verifyAssignment(secret, { ...fields, ...changed }, token), false);
});

test("expiry boundary and replay are rejected by one-use admission", async () => {
  const token = await signAssignment(secret, fields);
  const records = new Map();
  const storage = {
    get: async (key) => records.get(key),
    put: async (key, value) => records.set(key, value),
  };
  assert.deepEqual(await consumeAssignment(secret, fields, token, fields.expiresAt - 1, storage), { ok: true });
  assert.deepEqual(await consumeAssignment(secret, fields, token, fields.expiresAt - 1, storage), { ok: false, code: "replay" });
  assert.deepEqual(await consumeAssignment(secret, fields, token, fields.expiresAt, storage), { ok: false, code: "expired" });
});
