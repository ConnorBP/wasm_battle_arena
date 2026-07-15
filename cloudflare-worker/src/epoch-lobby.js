import { DurableObject } from "cloudflare:workers";
import {
  createEpochState, startNextEpoch, submitReport, validateSignal,
  requestRematch, respondRematch, expireRematch, denyRematch, leaveMatch, requeuePlayer,
  markActiveReconnect, rolloverActiveReconnect, requestBoundaryLeave,
} from "./epoch-state.js";
import { rotateReconnectIdentity } from "../vendor/cloudflare-game-common/lifecycle.js";
import {
  MAX_LOBBY_SOCKETS, MAX_MESSAGE_BYTES, RECONNECT_GRACE_MS,
  applyMessageRateLimit, parseEpochClientMessage, parseEpochLobbyQuery, randomHex,
} from "./protocol.js";
import { generateIceServers } from "./turn.js";
import { consumeAssignment } from "./assignment.js";

const KEY = "lobby-v3";

export class EpochLobby extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    this.env = env;
    this.ctx.blockConcurrencyWhile(async () => { this.state = await ctx.storage.get(KEY) ?? null; });
  }

  async fetch(request) {
    if (request.method !== "GET") return text("Method not allowed", 405);
    if (request.headers.get("Upgrade")?.toLowerCase() !== "websocket") return text("WebSocket required", 426);
    const requestUrl = new URL(request.url);
    const parsed = parseEpochLobbyQuery(requestUrl.searchParams);
    if (!parsed.ok) return text(parsed.error, 400);
    const room = requestUrl.pathname.split("/").pop();
    const now = Date.now();
    await this.expire(now);
    await this.expireRematch(now);
    // Expiry may have changed a round or identities; persist those side
    // effects even if this request later early-returns.
    if (this.state) await this.persist();
    // Reject new identities at capacity before consuming a one-use assignment.
    // Reconnects are checked against the superseded identity below.
    if (!parsed.value.playerId && this.live().length >= MAX_LOBBY_SOCKETS) {
      return text("Lobby busy", 503);
    }
    let assignmentAdmitted = false;
    if (!this.state) {
      // Authenticate reserved-room creation first so an invalid request cannot
      // pin an assigned room to attacker-chosen mode/capacity.
      const admission = await this.admitAssignment(parsed.value, room, now);
      if (!admission.ok) return text(admission.error, admission.status);
      assignmentAdmitted = true;
      this.state = createEpochState(parsed.value.mode, parsed.value.capacity, now);
    }
    // Forward-compatible defaults for Durable Objects persisted before Wave C.
    this.state.matchGeneration ??= 0;
    this.state.matchSeed ??= this.state.active?.seed ?? null;
    this.state.matchOver ??= false;
    this.state.rematch ??= null;
    this.state.rematchDecision ??= null;
    this.state.reconnectBatchDeadline ??= null;
    this.state.reconnectBatchEpoch ??= null;
    this.state.reconnectBatchRound ??= null;
    this.state.boundaryDepartures ??= {};
    if (this.state.mode !== parsed.value.mode || this.state.capacity !== parsed.value.capacity) return text("Lobby configuration mismatch", 409);
    if (!assignmentAdmitted) {
      const admission = await this.admitAssignment(parsed.value, room, now);
      if (!admission.ok) return text(admission.error, admission.status);
    }

    let player;
    let token;
    if (parsed.value.playerId) {
      player = this.state.players[parsed.value.playerId];
      const presentedHash = await tokenHash(parsed.value.reconnectToken);
      token = randomHex();
      const rotated = rotateReconnectIdentity(player, presentedHash, await tokenHash(token), now);
      if (!rotated.ok) return text(rotated.code === "reconnect_expired" ? "Reconnect grace period expired" : "Invalid reconnect", rotated.code === "reconnect_expired" ? 410 : 401);
      // A reconnect supersedes this identity's existing live socket, so account
      // for it before applying the lobby socket cap.
      const existing = this.socket(player.playerId);
      const liveCount = this.live().length - (existing ? 1 : 0);
      if (liveCount >= MAX_LOBBY_SOCKETS) return text("Lobby busy", 503);
    } else {
      if (this.live().length >= MAX_LOBBY_SOCKETS) return text("Lobby busy", 503);
      const playerId = this.uniquePlayerId();
      token = randomHex();
      player = {
        playerId, tokenHash: await tokenHash(token), joinedAt: now,
        connected: true, ready: false, expired: false, profile: null,
        score: 0, reconnectUntil: null,
      };
      this.state.players[playerId] = player;
    }

    const old = this.socket(player.playerId);
    if (old) {
      const attachment = old.deserializeAttachment() ?? {};
      attachment.superseded = true;
      old.serializeAttachment(attachment);
      old.close(4001, "reconnected elsewhere");
    }

    // Mint once only after query/config/reconnect/socket-cap admission. A
    // credentials API failure degrades to STUN and never rejects matchmaking.
    const turn = await generateIceServers(this.env);
    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair);
    this.ctx.acceptWebSocket(server);
    server.serializeAttachment({ playerId: player.playerId, superseded: false, rate: { windowStarted: now, windowMessages: 0 } });
    this.send(server, { type: "welcome", protocol: 3, playerId: player.playerId, reconnectToken: token, reconnectGraceMs: RECONNECT_GRACE_MS, ...turn });
    // Never replay the current bootstrap to an active-roster reconnect. A page
    // reload has no old transport to resume, while its still-open peers do. The
    // persisted short batch lets simultaneous reloads converge on one changed
    // epoch that every old client can stage and every empty client can install.
    if (parsed.value.playerId && this.isActive(player.playerId)) {
      markActiveReconnect(this.state, player.playerId, now);
      this.sendStatus(server, player, "reconnecting");
      const rollover = await this.processReconnectRollover(now);
      if (!rollover) await this.persist();
    } else {
      // An unrelated incoming connection can also wake a due persisted batch.
      // Process only after accepting this socket so a reconnect arriving at the
      // boundary is included in the authoritative connected-roster check.
      const rollover = await this.processReconnectRollover(now);
      if (this.isActive(player.playerId)) {
        // This is only defensive (new identities cannot belong to active), and
        // deliberately avoids exposing a resumable active bootstrap.
        this.sendStatus(server, player);
        if (!rollover) await this.persist();
      } else if (player.ready) {
        const start = startNextEpoch(this.state, randomHex(), "reconnect_ready");
        await this.persist();
        if (start) this.broadcastStart(start); else this.sendStatus(server, player);
      } else {
        this.sendStatus(server, player);
        if (!rollover) await this.persist();
      }
    }
    return new Response(null, { status: 101, webSocket: client });
  }

  async admitAssignment(options, room, now) {
    // Ordinary/private protocol-v3 rooms are deliberately unchanged. Assigned
    // q4_* rooms are never public: exact canonical queue admission is required.
    const assignedRoom = /^q4_[0-9a-f]{32}$/.test(room ?? "");
    if (!assignedRoom && !options.assignment) return { ok: true };
    // Once admitted, normal v3 rotating reconnect credentials are sufficient;
    // requiring the already-consumed assignment would make reconnect impossible.
    if (assignedRoom && options.playerId && !options.assignment) return { ok: true };
    if (!assignedRoom || !options.assignment) return { ok: false, status: 401, error: "Queue assignment required" };
    const fields = {
      room,
      mode: options.mode,
      capacity: options.capacity,
      ticket: options.assignment.ticket,
      expiresAt: options.assignment.expiresAt,
    };
    const consumed = await consumeAssignment(
      this.env.QUEUE_ASSIGNMENT_SECRET, fields, options.assignment.token, now, this.ctx.storage,
    );
    if (consumed.ok) return consumed;
    if (consumed.code === "expired") return { ok: false, status: 410, error: "Queue assignment expired" };
    if (consumed.code === "replay") return { ok: false, status: 409, error: "Queue assignment already used" };
    return { ok: false, status: 401, error: "Invalid queue assignment" };
  }

  async webSocketMessage(socket, raw) {
    // Enforce persisted wall-clock reconnect/rematch expiry before every
    // message, not only when an alarm happens to wake the Durable Object.
    const now = Date.now();
    await this.processReconnectRollover(now);
    await this.expireRematch(now);
    if (typeof raw !== "string" || new TextEncoder().encode(raw).byteLength > MAX_MESSAGE_BYTES) return this.violation(socket, "invalid message");
    const attachment = socket.deserializeAttachment();
    if (!attachment?.playerId || attachment.superseded) return this.violation(socket, "invalid session");
    const limited = applyMessageRateLimit(attachment.rate, Date.now());
    attachment.rate = limited.rate;
    socket.serializeAttachment(attachment);
    if (!limited.allowed) return this.violation(socket, "rate exceeded");
    const parsed = parseEpochClientMessage(raw);
    if (!parsed.ok) return this.violation(socket, parsed.error);
    const message = parsed.value;
    const player = this.state.players[attachment.playerId];
    if (!player || player.expired) return this.violation(socket, "expired identity");

    if (message.type === "ping") {
      this.send(socket, { type: "pong", ...(message.nonce === undefined ? {} : { nonce: message.nonce }) });
      return;
    }
    if (message.type === "leave_at_boundary") {
      const result = requestBoundaryLeave(this.state, player.playerId);
      if (result.type === "error") return this.sendError(socket, result.code);
      // Persist before acknowledging. Repeated requests for this same active
      // round are successful no-ops and cannot change the current bootstrap.
      await this.persist();
      this.send(socket, {
        type: "leave_at_boundary_ack", epoch: result.epoch, round: result.round,
        duplicate: result.duplicate,
      });
      return;
    }
    if (message.type === "leave" || message.type === "requeue") {
      const result = message.type === "requeue"
        ? requeuePlayer(this.state, player.playerId)
        : leaveMatch(this.state, player.playerId);
      this.clearReconnectBatch();
      await this.persist();
      this.broadcast({ type: "match_exit", destination: "main_menu", reason: result.reason, roster: result.roster });
      if (message.type === "requeue") {
        // Re-Queue is explicit and applies only to its requester; former
        // opponents are sent to menu and are never silently queued.
        this.send(socket, { type: "requeue", status: "waiting" });
        const start = startNextEpoch(this.state, randomHex(), "explicit_requeue");
        if (start) this.broadcastStart(start); else this.sendStatus(socket, player);
      }
      return;
    }
    if (message.type === "rematch_request" || message.type === "rematch_response") {
      const now = Date.now();
      const result = message.type === "rematch_request"
        ? requestRematch(this.state, player.playerId, message.generation, message.nonce, now)
        : respondRematch(this.state, player.playerId, message.generation, message.nonce, message.accept);
      if (result.type === "accepted") this.clearReconnectBatch();
      await this.persist();
      if (result.type === "pending") this.broadcast({ type: "rematch_pending", generation: result.generation, nonce: result.nonce, requestedBy: result.requestedBy, deadline: result.deadline, accepted: result.accepted, required: result.required });
      else if (result.type === "accepted") {
        this.broadcast({ type: "rematch_accepted", generation: result.generation, nonce: result.nonce });
        if (!result.duplicate && result.next) this.broadcastStart(result.next);
      } else if (result.type === "denied") this.broadcast({ type: "rematch_denied", generation: result.generation, nonce: result.nonce, reason: result.reason, destination: "main_menu" });
      else this.sendError(socket, result.code);
      return;
    }
    if (message.type === "profile") {
      player.profile = { name: message.name, paletteId: message.paletteId, cosmeticId: message.cosmeticId };
      await this.persist();
      this.send(socket, { type: "profile_accepted" });
      return;
    }
    if (message.type === "ready") {
      player.ready = true;
      // startNextEpoch explicitly refuses to replace state.active. Ready from an
      // active member or a mid-round waiter applies only after commit/abort.
      const start = startNextEpoch(this.state, randomHex(), "roster_ready");
      await this.persist();
      if (start) this.broadcastStart(start); else this.sendStatus(socket, player);
      return;
    }
    if (message.type === "signal") {
      if (this.state.reconnectBatchDeadline != null) return this.sendStatus(socket, player, "reconnecting");
      // Stale/wrong-epoch/non-roster signaling is rejected before any relay.
      const check = validateSignal(this.state, attachment.playerId, message.to, message.epoch, message.round);
      if (!check.ok) return this.sendError(socket, "stale_or_invalid_signal");
      const target = this.socket(message.to);
      if (!target) return this.sendError(socket, "target_offline");
      this.send(target, { type: "signal", epoch: check.epoch, round: check.round, from: attachment.playerId, data: message.data });
      return;
    }
    if (message.type === "report") {
      // Freeze consensus while an active reload batch is pending. Otherwise a
      // pair of already-in-flight reports could advance the old transport to a
      // same-epoch round and defeat the required changed-epoch rebuild.
      if (this.state.reconnectBatchDeadline != null) return this.sendStatus(socket, player, "reconnecting");
      const result = submitReport(this.state, attachment.playerId, message.epoch, message.round, message.outcomes, randomHex());
      await this.persist();
      if (result.type === "ack") this.send(socket, reportAckMessage(result));
      else if (result.type === "commit") {
        this.broadcast(commitMessage(result));
        this.finishBoundary(result);
        if (result.matchOver) this.broadcast({ type: "match_over", generation: result.matchGeneration, rematchGeneration: result.matchGeneration + 1 });
        if (result.next) this.broadcastStart(result.next);
      } else if (result.type === "abort") {
        this.broadcast(abortMessage(result));
        this.finishBoundary(result);
        if (result.next) this.broadcastStart(result.next);
      } else this.sendError(socket, result.code);
    }
  }

  async webSocketClose(socket) { await this.disconnect(socket); }
  async webSocketError(socket) { await this.disconnect(socket); }
  async alarm() {
    const now = Date.now();
    await this.processReconnectRollover(now);
    await this.expire(now);
    await this.expireRematch(now);
    if (this.state) await this.persist();
  }

  async disconnect(socket) {
    const attachment = socket.deserializeAttachment();
    if (!attachment?.playerId || attachment.superseded || this.socket(attachment.playerId, socket)) return;
    const player = this.state.players[attachment.playerId];
    if (player) {
      player.connected = false;
      player.reconnectUntil = Date.now() + RECONNECT_GRACE_MS;
      let denial = null;
      if (this.state.rematch && this.state.lastRoster.includes(player.playerId)) denial = denyRematch(this.state, "participant_disconnected");
      await this.persist();
      if (denial?.type === "denied") this.broadcast({ type: "rematch_denied", generation: denial.generation, nonce: denial.nonce, reason: denial.reason, destination: "main_menu" });
      this.broadcastPresence(player);
    }
  }

  async expire(now) {
    if (!this.state) return;
    for (const player of Object.values(this.state.players)) {
      if (player.reconnectUntil === null || player.reconnectUntil > now) continue;
      player.expired = true;
      player.ready = false;
      player.connected = false;
      player.reconnectUntil = null;
      this.broadcastPresence(player);
      if (this.isActive(player.playerId)) {
        const result = leaveMatch(this.state, player.playerId, "disconnect");
        this.clearReconnectBatch();
        this.broadcast({ type: "match_exit", destination: "main_menu", reason: result.reason, roster: result.roster });
      } else if (this.state.reconnectBatchEpoch != null && this.state.lastRoster.includes(player.playerId)) {
        // The active round may already have moved for another lifecycle reason;
        // never leave an orphaned short deadline armed.
        this.clearReconnectBatch();
      }
    }
  }

  async processReconnectRollover(now) {
    if (!this.state || this.state.reconnectBatchDeadline == null || this.state.reconnectBatchDeadline > now) return null;
    // `connected` is persisted for hibernation, but the accepted socket set is
    // authoritative at the boundary. If close delivery trails the alarm, do
    // not roll an absent peer into a new session; begin its normal grace now.
    if (this.state.reconnectBatchDeadline != null && this.state.reconnectBatchDeadline <= now) {
      for (const entry of this.state.active?.roster ?? []) {
        const player = this.state.players[entry.playerId];
        if (player?.connected && !this.socket(entry.playerId)) {
          player.connected = false;
          player.reconnectUntil ??= now + RECONNECT_GRACE_MS;
        }
      }
    }
    const result = rolloverActiveReconnect(this.state, now, randomHex());
    if (!result) return null;
    // State is durable before any peer observes the changed epoch. Repeated
    // alarms/messages see a cleared deadline and cannot increment it again.
    await this.persist();
    if (result.type === "rollover") this.broadcastStart(result.next);
    else if (result.type === "waiting") {
      for (const entry of this.state.active?.roster ?? []) {
        const socket = this.socket(entry.playerId);
        if (socket) this.sendStatus(socket, this.state.players[entry.playerId], "reconnecting");
      }
    }
    return result;
  }

  async expireRematch(now) {
    if (!this.state) return;
    const result = expireRematch(this.state, now);
    if (result?.type === "denied") {
      await this.persist();
      this.broadcast({ type: "rematch_denied", generation: result.generation, nonce: result.nonce, reason: result.reason, destination: "main_menu" });
    }
  }

  clearReconnectBatch() {
    this.state.reconnectBatchDeadline = null;
    this.state.reconnectBatchEpoch = null;
    this.state.reconnectBatchRound = null;
  }

  finishBoundary(result) {
    for (const playerId of result.boundary?.departing ?? []) {
      const socket = this.socket(playerId);
      if (socket) this.send(socket, { type: "match_exit", destination: "main_menu", reason: "boundary_leave", roster: [playerId] });
    }
    // If fewer than the mode minimum remain, terminate only those survivors;
    // unrelated waiting identities stay queued and receive no match_exit.
    for (const playerId of result.boundary?.terminated ?? []) {
      const socket = this.socket(playerId);
      if (socket) this.send(socket, { type: "match_exit", destination: "main_menu", reason: "insufficient_roster", roster: result.boundary.terminated });
    }
  }

  startMessage(active) {
    return { type: "start", protocol: 3, epoch: active.epoch, round: active.round, matchGeneration: this.state.matchGeneration, mode: this.state.mode, capacity: this.state.capacity, seed: active.seed, roster: active.roster };
  }
  broadcastStart(active) {
    const message = this.startMessage(active);
    // Starts are recipient-safe: waiters and boundary departures must not be
    // handed a bootstrap whose immutable roster excludes their identity.
    for (const entry of active.roster) {
      const socket = this.socket(entry.playerId);
      if (socket) this.send(socket, message);
    }
  }
  sendStatus(socket, player, status = this.state.active ? "active" : "waiting") {
    const reconnectDeadline = status === "reconnecting"
      ? (this.state.reconnectBatchDeadline ?? this.activeReconnectGraceDeadline())
      : null;
    this.send(socket, {
      type: "status", protocol: 3, status,
      mode: this.state.mode, capacity: this.state.capacity,
      active: this.state.active ? { epoch: this.state.active.epoch, round: this.state.active.round } : null,
      ready: player.ready, score: player.score,
      ...(status === "reconnecting" ? { reconnectDeadline } : {}),
    });
  }
  activeReconnectGraceDeadline() {
    return (this.state.active?.roster ?? []).reduce((nearest, entry) => {
      const deadline = this.state.players[entry.playerId]?.reconnectUntil;
      return deadline == null ? nearest : Math.min(nearest, deadline);
    }, Date.now() + RECONNECT_GRACE_MS);
  }
  broadcastPresence(player) {
    this.broadcast({ type: "presence", playerId: player.playerId, connected: player.connected, expired: player.expired });
  }
  isActive(playerId) { return this.state.active?.roster.some((entry) => entry.playerId === playerId) ?? false; }
  live() { return this.ctx.getWebSockets().filter((socket) => socket.readyState === 1 && !socket.deserializeAttachment()?.superseded); }
  socket(playerId, except = null) { return this.live().find((socket) => socket !== except && socket.deserializeAttachment()?.playerId === playerId); }
  uniquePlayerId() { let id; do { id = randomHex(); } while (this.state.players[id]); return id; }
  send(socket, message) { try { if (socket.readyState !== 1) return false; socket.send(JSON.stringify(message)); return true; } catch { return false; } }
  broadcast(message) { for (const socket of this.live()) this.send(socket, message); }
  sendError(socket, error) { this.send(socket, { type: "error", error }); }
  violation(socket, error) { this.sendError(socket, error); socket.close(1008, error.slice(0, 123)); }
  async persist() {
    await this.ctx.storage.put(KEY, this.state);
    const reconnectDeadline = Object.values(this.state.players).reduce((nearest, player) =>
      player.reconnectUntil == null ? nearest : Math.min(nearest, player.reconnectUntil), Infinity);
    // Assignment consumption markers only need to outlive their signed expiry.
    // Delete expired markers opportunistically; they are not token material.
    const listed = await this.ctx.storage.list({ prefix: "queue-assignment:" });
    const now = Date.now();
    for (const [key, value] of listed) {
      if ((value?.expiresAt ?? Infinity) <= now) await this.ctx.storage.delete(key);
    }
    // Reconnect batching, identity grace, and rematch expiry share one alarm.
    const next = Math.min(
      reconnectDeadline,
      this.state.reconnectBatchDeadline ?? Infinity,
      this.state.rematch?.deadline ?? Infinity,
    );
    if (Number.isFinite(next)) await this.ctx.storage.setAlarm(next); else await this.ctx.storage.deleteAlarm();
  }
}

function reportAckMessage(result) {
  return {
    type: "report_ack", epoch: result.epoch, round: result.round,
    duplicate: result.duplicate, received: result.received, required: result.required,
  };
}
function commitMessage(result) {
  return {
    type: "round_commit", epoch: result.epoch, round: result.round,
    outcomes: result.outcomes, scores: result.scores,
  };
}
function abortMessage(result) {
  return { type: "round_abort", epoch: result.epoch, round: result.round, reason: result.reason };
}
async function tokenHash(token) {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(token));
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}
function text(body, status) { return new Response(body, { status, headers: { "content-type": "text/plain; charset=utf-8" } }); }
