import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { chromium } from "playwright";
import path from "node:path";

const source = await readFile(path.resolve("src/cloudflare_net.js"), "utf8");
const browser = await chromium.launch({ headless: true });
try {
  const page = await browser.newPage();
  await page.setContent("<!doctype html><title>network contract</title>");
  const result = await page.evaluate(async source => {
    class MockWebSocket {
      static OPEN = 1;
      static instances = [];
      constructor(url) { this.url = url; this.readyState = 1; this.sent = []; this.closes = []; MockWebSocket.instances.push(this); }
      send(value) { if (this.throwOnSend) throw new Error("browser websocket send failed"); this.sent.push(value); }
      close(code, reason) { this.closes.push({ code, reason }); this.readyState = 3; }
    }
    class MockPeer {
      static instances = [];
      constructor() { MockPeer.instances.push(this); this.localDescription = { type: "offer", sdp: "mock" }; }
      createDataChannel() {
        return (this.channel = { readyState: "open", bufferedAmount: 0, sent: [], send(value) { if (this.throwOnSend) throw new Error("browser channel send failed"); this.sent.push(value); }, close() {} });
      }
      async createOffer() { return { type: "offer", sdp: "mock" }; }
      async createAnswer() { return { type: "answer", sdp: "mock" }; }
      async setLocalDescription(value) { this.localDescription = value; }
      async setRemoteDescription(value) { this.remoteDescription = value; }
      async addIceCandidate() {}
      close() {}
    }
    window.WebSocket = MockWebSocket;
    window.RTCPeerConnection = MockPeer;
    const blobUrl = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
    try {
      const net = await import(blobUrl);
      const PLAYER_A = "00000000000000000000000000000001";
      const PLAYER_B = "00000000000000000000000000000002";
      const id = net.cloudflare_connect_lobby("wss://signal.example/match", "browser", 0, 2, "Ghost", 0, 0);
      const ws = MockWebSocket.instances.at(-1);
      ws.onmessage({ data: JSON.stringify({ type: "welcome", protocol: 3, playerId: PLAYER_A, reconnectToken: "a".repeat(32), iceServers: [{ urls: "stun:stun.cloudflare.com:3478" }], turnExpiresAt: null }) });
      await new Promise(resolve => setTimeout(resolve, 0));
      ws.onmessage({ data: JSON.stringify({ type: "start", protocol: 3, epoch: 0xffffffff, round: 123, seed: "0123456789abcdef0123456789abcdef", matchGeneration: 0, roster: [{ index: 0, playerId: PLAYER_A, score: 0 }, { index: 1, playerId: PLAYER_B, score: 0 }] }) });
      await new Promise(resolve => setTimeout(resolve, 0));
      const channel = MockPeer.instances.at(-1).channel;
      channel.onopen();
      net.cloudflare_lobby_send(id, 0xffffffff, PLAYER_B, new Uint8Array([4, 5]));
      const frame = channel.sent[0];
      const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
      const telemetry = net.cloudflare_telemetry(id, 0);
      channel.throwOnSend = true;
      net.cloudflare_lobby_send(id, 0xffffffff, PLAYER_B, new Uint8Array([6]));
      return { telemetryType: typeof telemetry, telemetry: String(telemetry), epoch: view.getUint32(0, false), round: view.getUint32(4, false), packet: [...frame.slice(8)], statusAfterFailedSend: net.cloudflare_status(id), close: ws.closes.at(-1) };
    } finally { URL.revokeObjectURL(blobUrl); }
  }, source);
  assert.deepEqual(result, { telemetryType: "bigint", telemetry: "1", epoch: 0xffffffff, round: 123, packet: [4, 5], statusAfterFailedSend: 2, close: { code: 4000, reason: "connection failed" } });
  console.log("PASS: cloudflare_net.js Playwright browser contract tests");
} finally {
  await browser.close();
}
