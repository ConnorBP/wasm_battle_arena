import assert from "node:assert/strict";

const moduleUrl = new URL("../src/cloudflare_net.js", import.meta.url);
let importNumber = 0;

class MockChannel {
  constructor() {
    this.readyState = "open";
    this.bufferedAmount = 0;
    this.sent = [];
  }
  send(value) {
    if (this.throwOnSend) throw new Error("channel send failed");
    this.sent.push(value);
  }
  close() { this.readyState = "closed"; }
}

class MockPeer {
  static instances = [];
  constructor() {
    this.connectionState = "new";
    this.localDescription = { type: "offer", sdp: "mock" };
    this.remoteDescription = null;
    this.channel = null;
    MockPeer.instances.push(this);
  }
  createDataChannel() { return (this.channel = new MockChannel()); }
  async createOffer() { return { type: "offer", sdp: "mock" }; }
  async createAnswer() { return { type: "answer", sdp: "mock" }; }
  async setLocalDescription(value) { this.localDescription = value; }
  async setRemoteDescription(value) { this.remoteDescription = value; }
  async addIceCandidate() {}
  async getStats() { return new Map(); }
  close() { this.connectionState = "closed"; }
}

class MockWebSocket {
  static OPEN = 1;
  static instances = [];
  constructor(url) {
    this.url = url;
    this.readyState = MockWebSocket.OPEN;
    this.sent = [];
    this.closes = [];
    MockWebSocket.instances.push(this);
  }
  send(value) {
    if (this.throwOnSend) throw new Error("websocket send failed");
    this.sent.push(value);
  }
  close(code, reason) {
    this.closes.push({ code, reason });
    this.readyState = 3;
  }
  message(value) { this.onmessage?.({ data: JSON.stringify(value) }); }
}

function installBrowserMocks() {
  MockWebSocket.instances.length = 0;
  MockPeer.instances.length = 0;
  globalThis.window = {
    setTimeout: () => 1,
    clearTimeout() {},
    setInterval: () => 1,
    clearInterval() {},
  };
  globalThis.location = { protocol: "https:", host: "game.example" };
  globalThis.sessionStorage = {
    values: new Map(),
    getItem(key) { return this.values.get(key) ?? null; },
    setItem(key, value) { this.values.set(key, String(value)); },
    removeItem(key) { this.values.delete(key); },
  };
  globalThis.WebSocket = MockWebSocket;
  globalThis.RTCPeerConnection = MockPeer;
}

async function freshModule() {
  installBrowserMocks();
  return import(`${moduleUrl.href}?test=${++importNumber}`);
}

const tick = () => new Promise(resolve => setTimeout(resolve, 0));
const PLAYER_A = "00000000000000000000000000000001";
const PLAYER_B = "00000000000000000000000000000002";
const SEED = "0123456789abcdef0123456789abcdef";

async function readyLobby(net, { epoch = 7, round = 9 } = {}) {
  const id = net.cloudflare_connect_lobby("wss://signal.example/match", "room", 0, 2, "Ghost", 0, 0);
  const ws = MockWebSocket.instances.at(-1);
  ws.message({
    type: "welcome", protocol: 3, playerId: PLAYER_A,
    reconnectToken: "a".repeat(32), iceServers: [{ urls: "stun:stun.cloudflare.com:3478" }],
    turnExpiresAt: null,
  });
  await tick();
  ws.message({
    type: "start", protocol: 3, epoch, round, seed: SEED, matchGeneration: 3,
    roster: [
      { index: 0, playerId: PLAYER_A, score: 1 },
      { index: 1, playerId: PLAYER_B, score: 2 },
    ],
  });
  await tick();
  const channel = MockPeer.instances.at(-1).channel;
  channel.onopen?.();
  return { id, ws, channel };
}

{
  const net = await freshModule();
  const missing = net.cloudflare_telemetry(404, 0);
  assert.equal(typeof missing, "bigint");
  assert.equal(missing, 0n, "u64 ABI must always return a non-negative BigInt");

  const id = net.cloudflare_connect_queue("wss://signal.example/match", "compat", "any", "Ghost", 0, 0);
  assert.doesNotThrow(() => net.cloudflare_lobby_send(id, 0, PLAYER_B, new Uint8Array([1])));
  assert.equal(net.cloudflare_telemetry(id, 2), 1n, "optional queue collections must not throw");
  net.cloudflare_close_lobby(id);
  assert.deepEqual(MockWebSocket.instances[0].closes.at(-1), { code: 1000, reason: "client closed" });
}

{
  const net = await freshModule();
  const { id, channel } = await readyLobby(net, { epoch: 0xffffffff, round: 0xfffffffe });
  net.cloudflare_lobby_send(id, 0xffffffff, PLAYER_B, new Uint8Array([10, 20, 30]));
  assert.equal(channel.sent.length, 1);
  const bytes = channel.sent[0];
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  assert.equal(view.getUint32(0, false), 0xffffffff, "epoch frame is unsigned big-endian u32");
  assert.equal(view.getUint32(4, false), 0xfffffffe, "round frame is unsigned big-endian u32");
  assert.deepEqual([...bytes.slice(8)], [10, 20, 30]);
  assert.equal(net.cloudflare_telemetry(id, 0), 1n);
}

{
  const net = await freshModule();
  const id = net.cloudflare_connect_lobby("", "bounds", 0, 2, "Ghost", 0, 0);
  const ws = MockWebSocket.instances.at(-1);
  ws.message({ type: "welcome", protocol: 3, playerId: PLAYER_A, reconnectToken: "b".repeat(32), iceServers: [{ urls: "stun:stun.cloudflare.com:3478" }], turnExpiresAt: null });
  await tick();
  ws.message({ type: "start", protocol: 3, epoch: 0x1_0000_0000, round: 0, seed: SEED, roster: [{ index: 0, playerId: PLAYER_A, score: 0 }, { index: 1, playerId: PLAYER_B, score: 0 }] });
  await tick();
  assert.equal(net.cloudflare_status(id), 2, "out-of-u32 epoch must fail validation");
  assert.deepEqual(ws.closes.at(-1), { code: 4000, reason: "connection failed" });
}

{
  const net = await freshModule();
  const { id, ws, channel } = await readyLobby(net);
  channel.throwOnSend = true;
  net.cloudflare_lobby_send(id, 7, PLAYER_B, new Uint8Array([1]));
  assert.equal(net.cloudflare_status(id), 2, "failed data-channel sends must fail the session");
  assert.match(net.cloudflare_error(id), /channel send failed/);
  assert.deepEqual(ws.closes.at(-1), { code: 4000, reason: "connection failed" });

  // Failed sends are not counted as successful packets.
  assert.equal(net.cloudflare_telemetry(id, 0), 0n);
}

// Per-peer liveness watchdog contract.
{
const net = await freshModule();
const { id: stallId } = await readyLobby(net, { epoch: 1, round: 0 });
assert.equal(net.cloudflare_lobby_stalled(stallId), false);
}

// Boundary departure uses the real open protocol-v3 control socket and has no
// local transport side effect before the server's terminal round boundary.
{
  const net = await freshModule();
  const { id, ws } = await readyLobby(net, { epoch: 2, round: 3 });
  assert.equal(net.cloudflare_lobby_leave_at_boundary(id), true);
  assert.deepEqual(JSON.parse(ws.sent.at(-1)), { type: "leave_at_boundary" });
  assert.equal(net.cloudflare_lobby_epoch(id), 2);
  assert.equal(net.cloudflare_lobby_round(id), 3);
}

console.log("PASS: cloudflare_net.js direct Node contract tests");
