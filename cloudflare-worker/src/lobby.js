import { DurableObject } from "cloudflare:workers";
import {
  MAX_LOBBY_SOCKETS,
  MAX_MESSAGE_BYTES,
  RECONNECT_GRACE_MS,
  applyMessageRateLimit,
  canonicalRoster,
  parseClientMessage,
  parseLobbyQuery,
  randomHex,
} from "./protocol.js";
import { generateIceServers } from "./turn.js";

const STATE_KEY = "lobby-v2";

export class Lobby extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    this.env = env;
    this.ctx.blockConcurrencyWhile(async () => {
      this.state = await this.ctx.storage.get(STATE_KEY) ?? null;
    });
  }

  async fetch(request) {
    if (request.method !== "GET") return text("Method not allowed", 405);
    if (request.headers.get("Upgrade")?.toLowerCase() !== "websocket") {
      return text("WebSocket upgrade required", 426);
    }

    const parsed = parseLobbyQuery(new URL(request.url).searchParams);
    if (!parsed.ok) return text(parsed.error, 400);
    const options = parsed.value;
    if (this.state &&
        (this.state.mode !== options.mode || this.state.capacity !== options.capacity)) {
      return text("Lobby mode or capacity does not match existing room", 409);
    }

    const now = Date.now();
    await this.expireDisconnected(now);

    if (!this.state) {
      this.state = {
        version: 2,
        mode: options.mode,
        capacity: options.capacity,
        players: {},
        epoch0: null,
        createdAt: now,
      };
    }

    let player;
    let reconnectToken;
    if (options.playerId) {
      player = this.state.players[options.playerId];
      if (!player || player.tokenHash !== await tokenHash(options.reconnectToken)) {
        return text("Invalid reconnect credentials", 401);
      }
      if (player.expired || (!this.findSocket(player.playerId) &&
          player.reconnectUntil !== null && player.reconnectUntil <= now)) {
        return text("Reconnect grace period expired", 410);
      }
      reconnectToken = randomHex();
      player.tokenHash = await tokenHash(reconnectToken);
      player.disconnectedAt = null;
      player.reconnectUntil = null;
      player.expired = false;
    } else {
      const playerId = this.uniquePlayerId();
      reconnectToken = randomHex();
      player = {
        playerId,
        tokenHash: await tokenHash(reconnectToken),
        role: this.state.epoch0 ? "waiting" : "candidate",
        joinedAt: now,
        disconnectedAt: null,
        reconnectUntil: null,
        expired: false,
      };
      this.state.players[playerId] = player;
    }

    const oldSocket = this.findSocket(player.playerId);
    const liveSocketCount = this.liveSockets().length - (oldSocket ? 1 : 0);
    if (liveSocketCount >= MAX_LOBBY_SOCKETS) return text("Lobby busy", 503);
    if (oldSocket) {
      const oldAttachment = oldSocket.deserializeAttachment() ?? {};
      oldAttachment.superseded = true;
      oldSocket.serializeAttachment(oldAttachment);
      oldSocket.close(4001, "reconnected elsewhere");
    }

    const didStart = !this.state.epoch0 && this.freezeRosterIfFull(now);
    await this.persistAndSchedule();

    // Mint once only after query/config/reconnect/socket-cap admission. TURN
    // service failure deliberately does not reject the admitted identity.
    const turn = await generateIceServers(this.env);
    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair);
    this.ctx.acceptWebSocket(server);
    server.serializeAttachment({
      playerId: player.playerId,
      superseded: false,
      rate: { windowStarted: now, windowMessages: 0 },
    });

    this.send(server, {
      type: "welcome",
      protocol: 2,
      playerId: player.playerId,
      reconnectToken,
      reconnectGraceMs: RECONNECT_GRACE_MS,
      ...turn,
    });
    if (didStart) this.broadcastStart();
    else this.sendStatus(server, player);

    return new Response(null, { status: 101, webSocket: client });
  }

  async webSocketMessage(socket, rawMessage) {
    if (typeof rawMessage !== "string") return this.closeViolation(socket, "text messages required");
    if (new TextEncoder().encode(rawMessage).byteLength > MAX_MESSAGE_BYTES) {
      return this.closeViolation(socket, "message too large");
    }

    const attachment = socket.deserializeAttachment();
    if (!attachment?.playerId || attachment.superseded) {
      return this.closeViolation(socket, "invalid session");
    }
    const limited = applyMessageRateLimit(attachment.rate, Date.now());
    attachment.rate = limited.rate;
    socket.serializeAttachment(attachment);
    if (!limited.allowed) return this.closeViolation(socket, "message rate exceeded");

    const parsed = parseClientMessage(rawMessage);
    if (!parsed.ok) return this.closeViolation(socket, parsed.error);
    const message = parsed.value;
    if (message.type === "ping") {
      this.send(socket, { type: "pong", ...(message.nonce === undefined ? {} : { nonce: message.nonce }) });
      return;
    }

    const epoch = this.state?.epoch0;
    if (!epoch) return this.sendError(socket, "roster is not frozen");
    const rosterIds = new Set(epoch.roster.map((entry) => entry.playerId));
    if (!rosterIds.has(attachment.playerId)) return this.sendError(socket, "waiting players cannot signal");
    if (!rosterIds.has(message.to) || message.to === attachment.playerId) {
      return this.sendError(socket, "signal target is not another roster player");
    }
    const target = this.findSocket(message.to);
    if (!target) return this.sendError(socket, "signal target is offline");
    this.send(target, {
      type: "signal",
      epoch: 0,
      from: attachment.playerId,
      data: message.data,
    });
  }

  async webSocketClose(socket) {
    await this.markDisconnected(socket);
  }

  async webSocketError(socket) {
    await this.markDisconnected(socket);
    try { socket.close(1011, "control socket error"); } catch { /* already closed */ }
  }

  async alarm() {
    await this.expireDisconnected(Date.now());
    await this.persistAndSchedule();
  }

  freezeRosterIfFull(now) {
    const candidates = Object.values(this.state.players).filter((player) =>
      player.role === "candidate" && !player.expired && player.reconnectUntil === null
    );
    if (candidates.length < this.state.capacity) return false;

    const roster = canonicalRoster(candidates.slice(0, this.state.capacity));
    const rosterIds = new Set(roster.map((entry) => entry.playerId));
    for (const player of Object.values(this.state.players)) {
      player.role = rosterIds.has(player.playerId) ? "roster" : "waiting";
    }
    this.state.epoch0 = {
      epoch: 0,
      frozenAt: now,
      seed: randomHex(),
      roster: roster.map(({ playerId, index }) => ({ playerId, index })),
    };
    return true;
  }

  sendStatus(socket, player) {
    if (player.role === "roster" && this.state.epoch0) {
      this.send(socket, this.startMessage());
      return;
    }
    this.send(socket, {
      type: "status",
      status: "waiting",
      mode: this.state.mode,
      capacity: this.state.capacity,
      started: this.state.epoch0 !== null,
      epoch: this.state.epoch0 ? 0 : null,
    });
  }

  broadcastStart() {
    const roster = new Set(this.state.epoch0.roster.map((entry) => entry.playerId));
    const message = this.startMessage();
    for (const socket of this.liveSockets()) {
      if (roster.has(socket.deserializeAttachment()?.playerId)) this.send(socket, message);
    }
  }

  startMessage() {
    return {
      type: "start",
      epoch: 0,
      mode: this.state.mode,
      capacity: this.state.capacity,
      seed: this.state.epoch0.seed,
      roster: this.state.epoch0.roster,
    };
  }

  async markDisconnected(socket) {
    const attachment = socket.deserializeAttachment();
    if (!attachment?.playerId || attachment.superseded || !this.state) return;
    if (this.findSocket(attachment.playerId, socket)) return;
    const player = this.state.players[attachment.playerId];
    if (!player) return;
    const now = Date.now();
    player.disconnectedAt = now;
    player.reconnectUntil = now + RECONNECT_GRACE_MS;
    await this.persistAndSchedule();
  }

  async expireDisconnected(now) {
    if (!this.state) return;
    let changed = false;
    for (const [playerId, player] of Object.entries(this.state.players)) {
      if (player.reconnectUntil === null || player.reconnectUntil > now) continue;
      if (player.role === "roster") {
        player.reconnectUntil = null;
        player.expired = true;
      } else {
        delete this.state.players[playerId];
      }
      changed = true;
    }
    if (changed) await this.ctx.storage.put(STATE_KEY, this.state);
  }

  async persistAndSchedule() {
    if (!this.state) return;
    await this.ctx.storage.put(STATE_KEY, this.state);
    const nearest = Object.values(this.state.players).reduce((value, player) =>
      player.reconnectUntil === null ? value : Math.min(value, player.reconnectUntil), Infinity);
    if (Number.isFinite(nearest)) await this.ctx.storage.setAlarm(nearest);
    else await this.ctx.storage.deleteAlarm();
  }

  uniquePlayerId() {
    let playerId;
    do { playerId = randomHex(); } while (this.state.players[playerId]);
    return playerId;
  }

  liveSockets() {
    return this.ctx.getWebSockets().filter((socket) =>
      socket.readyState === 1 && !socket.deserializeAttachment()?.superseded
    );
  }

  findSocket(playerId, except = null) {
    return this.liveSockets().find((socket) =>
      socket !== except && socket.deserializeAttachment()?.playerId === playerId
    );
  }

  send(socket, message) {
    try {
      if (socket.readyState !== 1) return false;
      socket.send(JSON.stringify(message));
      return true;
    } catch {
      return false;
    }
  }

  sendError(socket, reason) {
    this.send(socket, { type: "error", error: reason });
  }

  closeViolation(socket, reason) {
    this.sendError(socket, reason);
    socket.close(1008, reason.slice(0, 123));
  }
}

async function tokenHash(token) {
  const bytes = new TextEncoder().encode(token);
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function text(body, status) {
  return new Response(body, { status, headers: { "content-type": "text/plain; charset=utf-8" } });
}
