import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const workerFiles = ["src/index.js", "src/lobby.js", "src/epoch-lobby.js"];
const clientPath = new URL("../../src/cloudflare_net.js", import.meta.url);
const rustClientPath = new URL("../../src/cloudflare_net.rs", import.meta.url);

async function source(path) {
  return readFile(new URL(`../${path}`, import.meta.url), "utf8");
}

test("all handshakes carry ICE config and expiry without a public credentials route", async () => {
  const [index, lobby, epoch] = await Promise.all(workerFiles.map(source));
  assert.match(index, /type: "matched"[\s\S]*\.\.\.waitingTurn/);
  assert.match(index, /type: "matched"[\s\S]*\.\.\.serverTurn/);
  assert.match(lobby, /type: "welcome"[\s\S]*\.\.\.turn/);
  assert.match(epoch, /type: "welcome"[\s\S]*\.\.\.turn/);
  assert.doesNotMatch(index, /routePath[\s\S]*credentials\/generate/);
  assert.doesNotMatch(index, /TURN_KEY_API_TOKEN[\s\S]*new Response/);
});

test("credentials mint occurs after request and room admission", async () => {
  for (const path of workerFiles) {
    const text = await source(path);
    const mint = path === "src/index.js" ? text.indexOf("generateIceServers(this.env)") : text.indexOf("await generateIceServers");
    assert.ok(mint > text.indexOf("parseLobbyQuery") || path === "src/index.js");
    assert.ok(mint > text.indexOf("sockets.length >= MAX_CLIENTS") || path !== "src/index.js");
    assert.ok(mint > text.indexOf("Lobby busy") || path === "src/index.js");
  }
});

test("both legacy and lobby RTCPeerConnection constructors use validated session config", async () => {
  const text = await readFile(clientPath, "utf8");
  const constructors = [...text.matchAll(/new RTCPeerConnection\(([^\n]+)\)/g)].map((match) => match[1]);
  assert.deepEqual(constructors, ["peerConfiguration(session)"]);
  assert.match(text, /validatedIceConfiguration\(message\)/);
  assert.match(text, /DEFAULT_ICE_SERVERS/);
  assert.match(text, /port === 53/);
});

test("TURN credentials are memory-only and reconnect obtains a fresh welcome", async () => {
  const text = await readFile(clientPath, "utf8");
  const storageWrites = [...text.matchAll(/sessionStorage\.setItem\(([^\n]+)\)/g)].map((match) => match[1]);
  assert.equal(storageWrites.length, 1);
  assert.doesNotMatch(storageWrites[0], /ice|turn|credential|username/i);
  assert.match(text, /new WebSocket\(url\)/);
  assert.match(text, /message\.type === "welcome"[\s\S]*validatedIceConfiguration\(message\)/);
  // There is intentionally no browser-callable refresh endpoint. Reconnect is
  // the only refresh trigger, and each accepted reconnect receives a new mint.
  assert.doesNotMatch(text, /fetch\([^)]*turn/i);
});

test("candidate-pair telemetry classifies host, srflx, and relay without credential fields", async () => {
  const text = await readFile(clientPath, "utf8");
  const rust = await readFile(rustClientPath, "utf8");
  assert.match(text, /peer\.getStats\(\)/);
  assert.match(text, /candidate-pair/);
  assert.match(text, /candidateType/);
  assert.match(text, /"relay"/);
  assert.match(text, /"srflx"/);
  const statsFunction = text.slice(text.indexOf("async function recordCandidatePair"), text.indexOf("function peerConfiguration"));
  assert.doesNotMatch(statsFunction, /username|credential/);
  assert.match(rust, /relay_connections/);
  assert.match(rust, /stun_fallbacks/);
});
