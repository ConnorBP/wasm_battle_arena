import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const source = await readFile(new URL("../src/epoch-lobby.js", import.meta.url), "utf8");

test("active reconnect sends status instead of replaying current start", () => {
  assert.match(source, /parsed\.value\.playerId && this\.isActive\(player\.playerId\)/);
  assert.match(source, /markActiveReconnect\(this\.state, player\.playerId, now\)/);
  assert.match(source, /this\.sendStatus\(server, player, "reconnecting"\)/);
  assert.doesNotMatch(source, /this\.send\(server, this\.startMessage\(this\.state\.active\)\)/);
});

test("reconnect rollover persists before its single start broadcast", () => {
  const method = source.slice(source.indexOf("async processReconnectRollover"), source.indexOf("async expireRematch"));
  assert.match(method, /reconnectBatchDeadline > now\) return null/);
  assert.match(method, /rolloverActiveReconnect\(this\.state, now, randomHex\(\)\)/);
  assert.ok(method.indexOf("await this.persist()") < method.indexOf("this.broadcastStart(result.next)"));
  assert.match(method, /result\.type === "rollover"/);
});

test("batch deadline shares the durable alarm with 30 second grace", () => {
  assert.match(source, /this\.state\.reconnectBatchDeadline \?\? Infinity/);
  assert.match(source, /this\.state\.reconnectBatchDeadline != null/);
  assert.match(source, /player\.reconnectUntil = Date\.now\(\) \+ RECONNECT_GRACE_MS/);
  assert.match(source, /await this\.processReconnectRollover\(now\)/);
});

test("boundary departure persists before acknowledgement and exits recipients only", () => {
  const handler = source.slice(source.indexOf('message.type === "leave_at_boundary"'), source.indexOf('message.type === "leave"'));
  assert.match(handler, /requestBoundaryLeave\(this\.state, player\.playerId\)/);
  assert.ok(handler.indexOf("await this.persist()") < handler.indexOf('type: "leave_at_boundary_ack"'));
  const finish = source.slice(source.indexOf("  finishBoundary(result) {"), source.indexOf("  startMessage(active) {"));
  assert.match(finish, /this\.socket\(playerId\)/);
  assert.doesNotMatch(finish, /this\.broadcast\(/);
});

test("changed starts are sent only to immutable roster sockets", () => {
  const method = source.slice(source.indexOf("  broadcastStart(active) {"), source.indexOf("  sendStatus(socket", source.indexOf("  broadcastStart(active) {")));
  assert.match(method, /for \(const entry of active\.roster\)/);
  assert.match(method, /this\.socket\(entry\.playerId\)/);
  assert.doesNotMatch(method, /this\.broadcast\(/);
});
