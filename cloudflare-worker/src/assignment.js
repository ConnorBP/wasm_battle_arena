import { ASSIGNED_ROOM_PATTERN, QUEUE_TICKET_PATTERN, QUEUE_TOKEN_PATTERN } from "./protocol.js";

const encoder = new TextEncoder();
const MAX_SECRET_BYTES = 256;

/** Stable, delimiter-safe signed bytes. Never include the token in logs/errors. */
export function canonicalAssignment({ room, mode, capacity, ticket, expiresAt }) {
  return `v4\n${room}\n${mode}\n${capacity}\n${ticket}\n${expiresAt}`;
}

export function validateAssignmentFields(value) {
  return Boolean(value &&
    ASSIGNED_ROOM_PATTERN.test(value.room) &&
    (value.mode === "duel" || value.mode === "deathmatch") &&
    Number.isInteger(value.capacity) &&
    ((value.mode === "duel" && value.capacity === 2) ||
      (value.mode === "deathmatch" && value.capacity >= 3 && value.capacity <= 8)) &&
    QUEUE_TICKET_PATTERN.test(value.ticket) &&
    Number.isSafeInteger(value.expiresAt) && value.expiresAt >= 0);
}

export async function signAssignment(secret, fields, cryptoImpl = globalThis.crypto) {
  if (!validSecret(secret) || !validateAssignmentFields(fields)) throw new Error("invalid assignment signing input");
  const key = await cryptoImpl.subtle.importKey(
    "raw", encoder.encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["sign"],
  );
  const bytes = await cryptoImpl.subtle.sign("HMAC", key, encoder.encode(canonicalAssignment(fields)));
  return hex(new Uint8Array(bytes));
}

export async function verifyAssignment(secret, fields, token, cryptoImpl = globalThis.crypto) {
  if (!validSecret(secret) || !validateAssignmentFields(fields) || !QUEUE_TOKEN_PATTERN.test(token ?? "")) return false;
  const key = await cryptoImpl.subtle.importKey(
    "raw", encoder.encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["verify"],
  );
  return cryptoImpl.subtle.verify(
    "HMAC", key, fromHex(token), encoder.encode(canonicalAssignment(fields)),
  );
}

/** Verify and atomically consume one ticket in the caller's serialized DO. */
export async function consumeAssignment(secret, fields, token, now, storage, cryptoImpl = globalThis.crypto) {
  if (!validateAssignmentFields(fields)) return { ok: false, code: "invalid" };
  if (fields.expiresAt <= now) return { ok: false, code: "expired" };
  if (!await verifyAssignment(secret, fields, token, cryptoImpl)) return { ok: false, code: "invalid" };
  const key = `queue-assignment:${fields.ticket}`;
  if (await storage.get(key)) return { ok: false, code: "replay" };
  await storage.put(key, { expiresAt: fields.expiresAt });
  return { ok: true };
}

function validSecret(secret) {
  if (typeof secret !== "string") return false;
  const length = encoder.encode(secret).byteLength;
  return length >= 32 && length <= MAX_SECRET_BYTES;
}
function hex(bytes) {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}
function fromHex(value) {
  const output = new Uint8Array(value.length / 2);
  for (let index = 0; index < output.length; index += 1) output[index] = Number.parseInt(value.slice(index * 2, index * 2 + 2), 16);
  return output;
}
