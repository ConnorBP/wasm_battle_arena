import test from "node:test";
import assert from "node:assert/strict";
import { generateIceServers, STUN_ONLY_ICE_SERVERS, validateIceServers } from "../src/turn.js";

const env = { TURN_KEY_ID: "key-id", TURN_KEY_API_TOKEN: "server-secret" };
const good = {
  iceServers: [
    { urls: "stun:stun.cloudflare.com:3478" },
    { urls: ["turn:turn.cloudflare.com:3478?transport=udp", "turns:turn.cloudflare.com:5349?transport=tcp"], username: "short-user", credential: "short-password" },
  ],
};

function response(value, { status = 200, contentLength } = {}) {
  const bytes = new TextEncoder().encode(typeof value === "string" ? value : JSON.stringify(value));
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: { get: (name) => name === "content-length" ? (contentLength ?? String(bytes.byteLength)) : null },
    arrayBuffer: async () => bytes.buffer,
  };
}

function assertFallback(result) {
  assert.deepEqual(result, { iceServers: STUN_ONLY_ICE_SERVERS, turnExpiresAt: null });
}

test("missing TURN secrets is STUN-only and does not call the API", async () => {
  let calls = 0;
  assertFallback(await generateIceServers({}, { fetchImpl: async () => { calls += 1; } }));
  assertFallback(await generateIceServers({ TURN_KEY_ID: "id" }, { fetchImpl: async () => { calls += 1; } }));
  assert.equal(calls, 0);
});

test("credentials request uses the private Cloudflare API, bearer token, and six-hour TTL", async () => {
  let request;
  const result = await generateIceServers(env, {
    now: 1_000,
    fetchImpl: async (url, init) => { request = { url, init }; return response(good); },
  });
  assert.equal(request.url, "https://rtc.live.cloudflare.com/v1/turn/keys/key-id/credentials/generate-ice-servers");
  assert.equal(request.init.method, "POST");
  assert.equal(request.init.headers.authorization, "Bearer server-secret");
  assert.deepEqual(JSON.parse(request.init.body), { ttl: 21600 });
  assert.deepEqual(result.iceServers, good.iceServers);
  assert.equal(result.turnExpiresAt, 21_601_000);
});

test("API errors, malformed JSON, and oversized bodies degrade to STUN", async () => {
  assertFallback(await generateIceServers(env, { fetchImpl: async () => response({}, { status: 500 }) }));
  assertFallback(await generateIceServers(env, { fetchImpl: async () => response("not-json") }));
  assertFallback(await generateIceServers(env, { fetchImpl: async () => response(good, { contentLength: String(16 * 1024 + 1) }) }));
  assertFallback(await generateIceServers(env, { fetchImpl: async () => response("x".repeat(16 * 1024 + 1), { contentLength: null }) }));
  assertFallback(await generateIceServers(env, { fetchImpl: async () => { throw new Error("network unavailable"); } }));
});

test("timeout aborts and degrades to STUN", async () => {
  const result = await generateIceServers(env, {
    timeoutMs: 1,
    fetchImpl: (_url, init) => new Promise((_resolve, reject) => {
      init.signal.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError")));
    }),
  });
  assertFallback(result);
});

test("strict allowlist accepts only Cloudflare STUN/TURN hosts and schemes", () => {
  assert.deepEqual(validateIceServers(good.iceServers), good.iceServers);
  for (const url of [
    "turn:evil.example:3478", "turn:turn.cloudflare.com.evil.example:3478",
    "https://turn.cloudflare.com", "stun:turn.cloudflare.com:3478",
    "turn:stun.cloudflare.com:3478", "turn:TURN.cloudflare.com:3478",
    "turn:turn.cloudflare.com:3478?transport=sctp", "turn:turn.cloudflare.com:99999",
  ]) {
    assert.equal(validateIceServers([{ urls: url, username: "u", credential: "p" }]), null, url);
  }
});

test("port 53 is filtered and cannot reach browser ICE configuration", () => {
  assert.deepEqual(validateIceServers([
    { urls: ["turn:turn.cloudflare.com:53?transport=udp", "turn:turn.cloudflare.com:3478?transport=udp"], username: "u", credential: "p" },
  ]), [{ urls: "turn:turn.cloudflare.com:3478?transport=udp", username: "u", credential: "p" }]);
  assert.equal(validateIceServers([{ urls: "turn:turn.cloudflare.com:53", username: "u", credential: "p" }]), null);
});

test("credentials, URL lists, and response dimensions are bounded", async () => {
  assert.equal(validateIceServers([{ urls: "turn:turn.cloudflare.com", username: "u".repeat(513), credential: "p" }]), null);
  assert.equal(validateIceServers([{ urls: "turn:turn.cloudflare.com", username: "u", credential: "p".repeat(513) }]), null);
  assert.equal(validateIceServers(Array.from({ length: 9 }, () => ({ urls: "stun:stun.cloudflare.com" }))), null);
  assertFallback(await generateIceServers(env, { fetchImpl: async () => response({ iceServers: [{ urls: "stun:stun.cloudflare.com" }] }) }));
});

test("source contains no credentials and generator never logs secrets", async () => {
  let logged = false;
  const previous = console.error;
  console.error = () => { logged = true; };
  try { await generateIceServers(env, { fetchImpl: async () => { throw new Error("server-secret"); } }); }
  finally { console.error = previous; }
  assert.equal(logged, false);
  assert.notEqual(env.TURN_KEY_API_TOKEN, good.iceServers[1].credential);
});
