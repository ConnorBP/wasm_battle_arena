import { DurableObject } from "cloudflare:workers";

const MAX_CLIENTS = 64;
const MAX_MESSAGE_BYTES = 16 * 1024;
const MAX_MESSAGES_PER_SECOND = 120;
const MAX_MESSAGES_PER_CONNECTION = 256;
const STALE_AFTER_MS = 2 * 60 * 1000;

export default {
  async fetch(request, env) {
    if (request.method !== "GET") return new Response("Method not allowed", { status: 405 });
    const url = new URL(request.url);
    const match = /^\/match\/([A-Za-z0-9_-]{1,64})$/.exec(url.pathname);
    if (!match) return new Response("Not found", { status: 404 });
    if (request.headers.get("Upgrade")?.toLowerCase() !== "websocket") {
      return new Response("WebSocket upgrade required", { status: 426 });
    }

    const allowedOrigins = (env.ALLOWED_ORIGINS || "")
      .split(",")
      .map((origin) => origin.trim())
      .filter(Boolean);
    if (!allowedOrigins.includes(request.headers.get("Origin"))) {
      return new Response("Origin not allowed", { status: 403 });
    }

    const rateKey = request.headers.get("CF-Connecting-IP") || "unknown";
    if (env.MATCHMAKING_RATE_LIMITER) {
      const { success } = await env.MATCHMAKING_RATE_LIMITER.limit({ key: rateKey });
      if (!success) return new Response("Too many matchmaking attempts", { status: 429 });
    }

    return env.MATCHMAKER.get(env.MATCHMAKER.idFromName(match[1])).fetch(request);
  },
};

export class Matchmaker extends DurableObject {
  async fetch(request) {
    if (request.method !== "GET") return new Response("Method not allowed", { status: 405 });
    if (request.headers.get("Upgrade")?.toLowerCase() !== "websocket") {
      return new Response("WebSocket upgrade required", { status: 426 });
    }

    const now = Date.now();
    const sockets = this.ctx.getWebSockets().filter((socket) => {
      const attachment = socket.deserializeAttachment();
      if (
        socket.readyState === 1 &&
        attachment &&
        attachment.expiresAt > now
      ) return true;
      socket.close(1008, "stale signaling session");
      return false;
    });
    if (sockets.length >= MAX_CLIENTS) {
      return new Response("Matchmaking room busy", { status: 503 });
    }

    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair);
    const waiting = sockets.find((socket) => socket.deserializeAttachment()?.state === "waiting");

    this.ctx.acceptWebSocket(server);
    server.serializeAttachment(this.attachment("waiting", null, null, now));

    if (!waiting) {
      this.safeSend(server, { type: "waiting" });
    } else {
      const pairId = crypto.randomUUID();
      const seed = this.randomSeed();
      waiting.serializeAttachment(this.attachment("matched", pairId, 0, now));
      server.serializeAttachment(this.attachment("matched", pairId, 1, now));
      const waitingMatched = this.safeSend(waiting, { type: "matched", index: 0, seed });
      const serverMatched = this.safeSend(server, { type: "matched", index: 1, seed });
      if (!waitingMatched || !serverMatched) {
        this.teardownPair(waiting, "matchmaking peer unavailable");
        waiting.close(1011, "matchmaking peer unavailable");
        server.close(1011, "matchmaking peer unavailable");
      }
    }

    await this.scheduleExpiration();
    return new Response(null, { status: 101, webSocket: client });
  }

  async webSocketMessage(socket, message) {
    const attachment = socket.deserializeAttachment();
    if (!attachment || attachment.state !== "matched" || typeof message !== "string") {
      return this.reject(socket, "invalid signaling state");
    }
    if (new TextEncoder().encode(message).byteLength > MAX_MESSAGE_BYTES) {
      return this.reject(socket, "signaling message too large");
    }

    const now = Date.now();
    if (now - attachment.windowStarted >= 1000) {
      attachment.windowStarted = now;
      attachment.windowMessages = 0;
    }
    attachment.windowMessages += 1;
    attachment.totalMessages += 1;
    attachment.expiresAt = now + STALE_AFTER_MS;
    if (
      attachment.windowMessages > MAX_MESSAGES_PER_SECOND ||
      attachment.totalMessages > MAX_MESSAGES_PER_CONNECTION
    ) return this.reject(socket, "signaling rate exceeded");

    let signal;
    try { signal = JSON.parse(message); }
    catch { return this.reject(socket, "invalid signaling JSON"); }

    const peer = this.findPeer(socket, attachment.pairId);
    if (!peer) return this.reject(socket, "peer unavailable");
    const peerAttachment = peer.deserializeAttachment();
    peerAttachment.expiresAt = attachment.expiresAt;
    peer.serializeAttachment(peerAttachment);

    if (signal?.type === "connected" && signal.data === null) {
      attachment.connected = true;
      socket.serializeAttachment(attachment);
      if (peerAttachment?.connected) {
        attachment.state = "connected";
        peerAttachment.state = "connected";
        socket.serializeAttachment(attachment);
        peer.serializeAttachment(peerAttachment);
        socket.close(1000, "connected");
        peer.close(1000, "connected");
      } else {
        await this.scheduleExpiration();
      }
      return;
    }

    if (!this.validSignal(signal, attachment)) {
      return this.reject(socket, "invalid signaling message");
    }

    if (signal.type === "offer") {
      attachment.phase = "offered";
      const peerAttachment = peer.deserializeAttachment();
      peerAttachment.phase = "offered";
      peer.serializeAttachment(peerAttachment);
    } else if (signal.type === "answer") {
      attachment.phase = "answered";
      const peerAttachment = peer.deserializeAttachment();
      peerAttachment.phase = "answered";
      peer.serializeAttachment(peerAttachment);
    }
    socket.serializeAttachment(attachment);

    if (!this.safeSend(peer, message)) {
      this.teardownPair(socket, "signaling peer unavailable");
      socket.close(1011, "signaling peer unavailable");
    }
    await this.scheduleExpiration();
  }

  webSocketClose(socket, code, reason) {
    if (code === 1000 && reason === "connected") return;
    this.teardownPair(socket, "peer-left");
  }

  webSocketError(socket) {
    this.teardownPair(socket, "peer-left");
    socket.close(1011, "signaling error");
  }

  async alarm() {
    const now = Date.now();
    for (const socket of this.ctx.getWebSockets()) {
      const attachment = socket.deserializeAttachment();
      if ((attachment?.expiresAt ?? 0) <= now) {
        this.teardownPair(socket, "signaling timeout");
        if (attachment) {
          attachment.state = "closed";
          attachment.pairId = null;
          socket.serializeAttachment(attachment);
        }
        socket.close(1008, "signaling timeout");
      }
    }
    await this.scheduleExpiration();
  }

  attachment(state, pairId, index, now) {
    return {
      state,
      pairId,
      index,
      phase: "matched",
      connected: false,
      expiresAt: now + STALE_AFTER_MS,
      windowStarted: now,
      windowMessages: 0,
      totalMessages: 0,
    };
  }

  randomSeed() {
    const bytes = crypto.getRandomValues(new Uint8Array(8));
    return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  }

  validSignal(signal, sender) {
    if (!signal || typeof signal !== "object") return false;
    if (signal.type === "offer" || signal.type === "answer") {
      const validRole = signal.type === "offer"
        ? sender.index === 0 && sender.phase === "matched"
        : sender.index === 1 && sender.phase === "offered";
      return (
        validRole &&
        signal.data?.type === signal.type &&
        typeof signal.data.sdp === "string" &&
        signal.data.sdp.length <= 12 * 1024
      );
    }
    if (signal.type !== "ice") return false;
    if (signal.data === null) return true;
    return (
      typeof signal.data === "object" &&
      typeof signal.data.candidate === "string" &&
      signal.data.candidate.length <= 2048 &&
      (signal.data.sdpMid == null ||
        (typeof signal.data.sdpMid === "string" && signal.data.sdpMid.length <= 64)) &&
      (signal.data.sdpMLineIndex == null ||
        (Number.isInteger(signal.data.sdpMLineIndex) &&
          signal.data.sdpMLineIndex >= 0 && signal.data.sdpMLineIndex <= 255))
    );
  }

  findPeer(socket, pairId) {
    return this.ctx.getWebSockets().find((candidate) =>
      candidate !== socket &&
      candidate.readyState === 1 &&
      candidate.deserializeAttachment()?.state === "matched" &&
      candidate.deserializeAttachment()?.pairId === pairId
    );
  }

  safeSend(socket, message) {
    try {
      if (socket.readyState !== 1) return false;
      socket.send(typeof message === "string" ? message : JSON.stringify(message));
      return true;
    } catch {
      return false;
    }
  }

  reject(socket, reason) {
    this.safeSend(socket, { type: "error", data: reason });
    this.teardownPair(socket, reason);
    socket.close(1008, reason);
  }

  teardownPair(socket, reason) {
    const attachment = socket.deserializeAttachment();
    if (!attachment?.pairId) return;
    const peer = this.findPeer(socket, attachment.pairId);
    attachment.state = "closed";
    attachment.pairId = null;
    socket.serializeAttachment(attachment);
    if (!peer) return;
    const peerAttachment = peer.deserializeAttachment();
    peerAttachment.state = "closed";
    peerAttachment.pairId = null;
    peer.serializeAttachment(peerAttachment);
    this.safeSend(peer, { type: "peer-left", data: reason });
    peer.close(1011, "peer left signaling");
  }

  async scheduleExpiration() {
    const expiration = this.ctx.getWebSockets().reduce((nearest, socket) => {
      const attachment = socket.deserializeAttachment();
      const live = socket.readyState === 1 &&
        (attachment?.state === "waiting" || attachment?.state === "matched");
      return live ? Math.min(nearest, attachment.expiresAt) : nearest;
    }, Infinity);
    if (Number.isFinite(expiration)) await this.ctx.storage.setAlarm(expiration);
    else await this.ctx.storage.deleteAlarm();
  }
}
