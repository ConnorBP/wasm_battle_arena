import { DurableObject } from "cloudflare:workers";
import { signAssignment } from "./assignment.js";
import {
  ASSIGNMENT_TTL_MS, MAX_QUEUE_ENTRIES, advanceQueue, cancelQueue,
  createQueueState, heartbeatQueue, migrateQueueState, nextQueueDeadline, queueEntry,
  startVotesRequired, voteStartQueue, withdrawStartVoteQueue,
} from "./queue-state.js";
import { MAX_MESSAGE_BYTES, applyMessageRateLimit, parseQueueQuery, randomHex } from "./protocol.js";

const STATE_KEY = "queue-v4";
const MAX_QUEUE_MESSAGE_BYTES = Math.min(MAX_MESSAGE_BYTES, 512);

export class MatchQueue extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    this.env = env;
    this.ctx.blockConcurrencyWhile(async () => {
      this.state = migrateQueueState(await ctx.storage.get(STATE_KEY) ?? createQueueState(Date.now()), Date.now());
    });
  }

  async fetch(request) {
    if (request.method !== "GET") return text("Method not allowed", 405);
    if (request.headers.get("Upgrade")?.toLowerCase() !== "websocket") return text("WebSocket required", 426);
    if (typeof this.env.QUEUE_ASSIGNMENT_SECRET !== "string" || new TextEncoder().encode(this.env.QUEUE_ASSIGNMENT_SECRET).byteLength < 32) {
      return text("Queue assignment service unavailable", 503);
    }
    const parsed = parseQueueQuery(new URL(request.url).searchParams);
    if (!parsed.ok) return text(parsed.error, 400);
    if (this.live().length >= MAX_QUEUE_ENTRIES) return text("Queue busy", 503);

    const now = Date.now();
    await this.applyResult(advanceQueue(this.state, now), now);
    if (Object.keys(this.state.entries).length >= MAX_QUEUE_ENTRIES) return text("Queue busy", 503);
    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair);
    const ticket = this.uniqueTicket();
    this.ctx.acceptWebSocket(server);
    server.serializeAttachment({ ticket, rate: { windowStarted: now, windowMessages: 0 } });
    const result = queueEntry(this.state, { ticket, preference: parsed.value.preference }, now);
    this.send(server, {
      type: "queued", protocol: 4, ticket, preference: parsed.value.preference,
    });
    await this.applyResult(result, now);
    return new Response(null, { status: 101, webSocket: client });
  }

  async webSocketMessage(socket, raw) {
    const attachment = socket.deserializeAttachment();
    if (!attachment?.ticket || typeof raw !== "string" || new TextEncoder().encode(raw).byteLength > MAX_QUEUE_MESSAGE_BYTES) {
      return this.violation(socket, "invalid queue message");
    }
    if (!this.state.entries[attachment.ticket]) return this.violation(socket, "queue ticket inactive");
    const limited = applyMessageRateLimit(attachment.rate, Date.now());
    attachment.rate = limited.rate;
    socket.serializeAttachment(attachment);
    if (!limited.allowed) return this.violation(socket, "queue message rate exceeded");
    let message;
    try { message = JSON.parse(raw); } catch { return this.violation(socket, "invalid queue JSON"); }
    if (!message || typeof message !== "object" || Array.isArray(message)) return this.violation(socket, "invalid queue message");
    const now = Date.now();
    if (message.type === "heartbeat" && Object.keys(message).every((key) => ["type", "nonce"].includes(key)) &&
        (message.nonce === undefined || (typeof message.nonce === "string" && message.nonce.length <= 64))) {
      const result = heartbeatQueue(this.state, attachment.ticket, now);
      if (result.type === "error") return this.violation(socket, "queue ticket inactive");
      // At an exact assignment deadline the reducer may consume the ticket;
      // do not acknowledge a heartbeat for an already-decided queue entry.
      if (this.state.entries[attachment.ticket]) {
        this.send(socket, { type: "heartbeat_ack", ...(message.nonce === undefined ? {} : { nonce: message.nonce }) });
      }
      await this.applyResult(result, now);
      return;
    }
    if (message.type === "vote_start" && Object.keys(message).length === 1) {
      const result = voteStartQueue(this.state, attachment.ticket, now);
      if (result.type === "error") return this.violation(socket, "queue ticket is not staging");
      await this.applyResult(result, now);
      return;
    }
    if (message.type === "withdraw_start_vote" && Object.keys(message).length === 1) {
      const result = withdrawStartVoteQueue(this.state, attachment.ticket, now);
      if (result.type === "error") return this.violation(socket, "queue ticket is not staging");
      await this.applyResult(result, now);
      return;
    }
    if (message.type === "cancel" && Object.keys(message).length === 1) {
      await this.applyResult(cancelQueue(this.state, attachment.ticket, now), now);
      this.send(socket, { type: "cancelled" });
      socket.close(1000, "queue cancelled");
      return;
    }
    return this.violation(socket, "unsupported queue message");
  }

  async webSocketClose(socket) { await this.removeSocket(socket, "disconnected"); }
  async webSocketError(socket) { await this.removeSocket(socket, "disconnected"); }
  async alarm() {
    const now = Date.now();
    await this.applyResult(advanceQueue(this.state, now), now);
  }

  async removeSocket(socket, reason) {
    const ticket = socket.deserializeAttachment()?.ticket;
    if (!ticket || !this.state.entries[ticket] || this.socket(ticket, socket)) return;
    const now = Date.now();
    await this.applyResult(cancelQueue(this.state, ticket, now, reason), now);
  }

  async applyResult(result, now) {
    // Materialize random room identities into persisted decisions first. The
    // token itself is reproducible from these fields and is never persisted.
    for (const group of result.groups ?? []) {
      const room = `q4_${randomHex()}`;
      const expiresAt = now + ASSIGNMENT_TTL_MS;
      for (const ticket of group.tickets) {
        this.state.assignments[ticket] = {
          room, mode: group.mode, capacity: group.capacity, ticket, expiresAt,
        };
      }
    }
    for (const [ticket, assignment] of Object.entries(this.state.assignments)) {
      if (assignment.expiresAt <= now) delete this.state.assignments[ticket];
    }
    // Persist the full reducer decision (including lock/deadlines, assignment,
    // and removals) before handing it to clients. This survives hibernation.
    await this.persist();
    for (const removed of result.removed ?? []) {
      const socket = this.socket(removed.ticket);
      if (socket && removed.reason === "heartbeat_timeout") {
        this.send(socket, { type: "error", error: removed.reason });
        socket.close(1008, removed.reason);
      }
    }
    this.publishStatuses();
    await this.replayAssignments(now);
  }

  // Staging snapshots include recipient-local vote membership. Keeping that
  // boolean server-authored lets clients render vote/withdraw safely after
  // joins, disconnects, and Worker hibernation without optimistic state.
  publishStatuses() {
    for (const entry of Object.values(this.state.entries)) {
      const socket = this.socket(entry.ticket);
      if (!socket) continue;
      let status = { type: "status", status: "searching" };
      if (this.state.lock?.tickets.includes(entry.ticket)) {
        status = {
          type: "status", status: "staging",
          count: this.state.lock.tickets.length,
          votes: this.state.lock.votes?.length ?? 0,
          votesRequired: startVotesRequired(this.state.lock.tickets.length),
          deadline: this.state.lock.deadline,
          voted: this.state.lock.votes?.includes(entry.ticket) ?? false,
        };
      } else if (entry.preference === "any" && entry.anyHoldDeadline !== null) {
        status = { type: "status", status: "holding_for_third" };
      }
      const encoded = JSON.stringify(status);
      const attachment = socket.deserializeAttachment() ?? {};
      // `voted` is ticket-local, so deduplication also remains per recipient.
      if (attachment.queueStatus === encoded) continue;
      attachment.queueStatus = encoded;
      socket.serializeAttachment(attachment);
      this.send(socket, status);
    }
  }

  async replayAssignments(now) {
    for (const fields of Object.values(this.state.assignments)) {
      if (fields.expiresAt <= now) continue;
      const socket = this.socket(fields.ticket);
      if (!socket) continue;
      const token = await signAssignment(this.env.QUEUE_ASSIGNMENT_SECRET, fields);
      // Bounded scalar handoff only. Tokens are sent directly and never logged,
      // persisted, broadcast in errors, or included in close reasons.
      this.send(socket, { type: "assigned", protocol: 4, ...fields, token });
      socket.close(1000, "assigned");
    }
  }

  async persist() {
    await this.ctx.storage.put(STATE_KEY, this.state);
    const queueDeadline = nextQueueDeadline(this.state) ?? Infinity;
    const assignmentDeadline = Object.values(this.state.assignments).reduce(
      (nearest, assignment) => Math.min(nearest, assignment.expiresAt), Infinity,
    );
    const deadline = Math.min(queueDeadline, assignmentDeadline);
    if (!Number.isFinite(deadline)) await this.ctx.storage.deleteAlarm();
    else await this.ctx.storage.setAlarm(deadline);
  }
  uniqueTicket() { let ticket; do { ticket = randomHex(); } while (this.state.entries[ticket] || this.state.assignments[ticket]); return ticket; }
  live() { return this.ctx.getWebSockets().filter((socket) => socket.readyState === 1); }
  socket(ticket, except = null) { return this.live().find((socket) => socket !== except && socket.deserializeAttachment()?.ticket === ticket); }
  send(socket, message) { try { if (socket.readyState !== 1) return false; socket.send(JSON.stringify(message)); return true; } catch { return false; } }
  async violation(socket, error) {
    const ticket = socket.deserializeAttachment()?.ticket;
    if (ticket && this.state.entries[ticket]) {
      const now = Date.now();
      await this.applyResult(cancelQueue(this.state, ticket, now, "protocol_error"), now);
    }
    this.send(socket, { type: "error", error });
    socket.close(1008, error.slice(0, 123));
  }
}

function text(body, status) { return new Response(body, { status, headers: { "content-type": "text/plain; charset=utf-8" } }); }
