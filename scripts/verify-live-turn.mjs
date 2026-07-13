import { chromium } from "playwright";
import { randomBytes } from "node:crypto";

const origin = "https://ghost.segfault.site";
const worker = "wss://ghost-battle-signaling.connor-postma.workers.dev";
const room = `turn-check-${Date.now().toString(36)}-${randomBytes(4).toString("hex")}`;
const browser = await chromium.launch({ headless: true });
try {
  const page = await browser.newPage();
  await page.goto(origin, { waitUntil: "domcontentloaded", timeout: 120_000 });
  const result = await page.evaluate(async ({ worker, room }) => {
    const welcome = await new Promise((resolve, reject) => {
      const ws = new WebSocket(`${worker}/lobby/${room}?protocol=3&mode=duel&capacity=2`);
      const timer = setTimeout(() => { ws.close(); reject(new Error("welcome timeout")); }, 20_000);
      ws.onerror = () => { clearTimeout(timer); reject(new Error("websocket error")); };
      ws.onmessage = event => {
        const message = JSON.parse(event.data);
        if (message.type === "welcome") { clearTimeout(timer); ws.close(); resolve(message); }
      };
    });
    const servers = Array.isArray(welcome.iceServers) ? welcome.iceServers : [];
    const urls = servers.flatMap(server => typeof server.urls === "string" ? [server.urls] : Array.isArray(server.urls) ? server.urls : []);
    const hasTurn = urls.some(url => url.startsWith("turn:") || url.startsWith("turns:"));
    const noPort53 = urls.every(url => !/:(?:53)(?:\?|$)/.test(url));
    const futureExpiry = Number.isFinite(welcome.turnExpiresAt) && welcome.turnExpiresAt > Date.now();
    if (!hasTurn || !noPort53 || !futureExpiry) return { hasTurn, noPort53, futureExpiry, connected: false, relay: false };

    const pc1 = new RTCPeerConnection({ iceServers: servers, iceTransportPolicy: "relay" });
    const pc2 = new RTCPeerConnection({ iceServers: servers, iceTransportPolicy: "relay" });
    pc1.onicecandidate = ({ candidate }) => { if (candidate) pc2.addIceCandidate(candidate).catch(() => {}); };
    pc2.onicecandidate = ({ candidate }) => { if (candidate) pc1.addIceCandidate(candidate).catch(() => {}); };
    const opened = new Promise(resolve => {
      const timer = setTimeout(() => resolve(false), 30_000);
      const channel = pc1.createDataChannel("turn-check");
      channel.onopen = () => { clearTimeout(timer); resolve(true); };
    });
    pc2.ondatachannel = event => { event.channel.onmessage = () => {}; };
    await pc1.setLocalDescription(await pc1.createOffer());
    await pc2.setRemoteDescription(pc1.localDescription);
    await pc2.setLocalDescription(await pc2.createAnswer());
    await pc1.setRemoteDescription(pc2.localDescription);
    const connected = await opened;
    let relay = false;
    if (connected) {
      for (const pc of [pc1, pc2]) {
        const stats = await pc.getStats();
        for (const stat of stats.values()) {
          if (stat.type === "candidate-pair" && stat.state === "succeeded" && stat.nominated) {
            const local = stats.get(stat.localCandidateId);
            const remote = stats.get(stat.remoteCandidateId);
            relay ||= local?.candidateType === "relay" || remote?.candidateType === "relay";
          }
        }
      }
    }
    pc1.close(); pc2.close();
    return { hasTurn, noPort53, futureExpiry, connected, relay };
  }, { worker, room });
  console.log(JSON.stringify(result));
  if (!result.hasTurn || !result.noPort53 || !result.futureExpiry || !result.connected || !result.relay) process.exitCode = 1;
} finally {
  await browser.close();
}
