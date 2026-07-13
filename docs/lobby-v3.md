# Lobby protocol v3 lifecycle

Protocol 3 is the default lifecycle protocol for selected multiplayer modes. Connect to:

```
wss://<worker>/lobby/<room>?protocol=3&mode=<duel|deathmatch>&capacity=<n>
```

The old `/match/<room>` endpoint remains the explicit legacy duel fallback. Lobby v2 (`/lobby/<room>` without `protocol=3`) remains deployed for older clients, but new clients always send `protocol=3`. The Worker never silently downgrades a v3 request: `protocol=3` is required and validated exactly once on the v3 control socket, and a signal missing its `epoch` is rejected rather than reinterpreted as a v2 message.

## Identity and reconnect

`welcome` contains `protocol: 3`, a 32-hex `playerId`, a rotating 32-hex `reconnectToken`, and `reconnectGraceMs`. The browser keeps the latest credentials in `sessionStorage`: reconnects in the same tab/runtime preserve identity; a full browser storage reset (or a new tab) creates a new identity. Every successful reconnect rotates the token, supersedes any older live socket for that identity, and invalidates the previous token. The server stores only SHA-256 token hashes.

Reconnect honors a grace window. A disconnected roster member is not removed immediately; their seat is reserved until `reconnectGraceMs` elapses, at which point the identity expires and an active round is aborted. A reconnect strictly before the boundary preserves the identity and replays the immutable `start` bootstrap.

After `welcome`, send a validated `profile`, then `ready`. The control WebSocket stays open independently of epoch WebRTC/GGRS channels. Reconnect of an active member replays the immutable `start` bootstrap without creating or replacing an epoch.

## Epoch lifecycle

A `start` message contains `protocol`, `epoch`, `round`, `mode`, `capacity`, a 32-hex `seed`, and a canonical roster with profile/score snapshots.

* `ready` never replaces `active`. An immutable active epoch cannot be replaced by ready, profile, presence, reconnect, or mid-round join events.
* Mid-round joiners are waiting candidates for the next selection; incumbents keep their seat until one leaves.
* If the next canonical roster is unchanged, `round` increments and `epoch` does not.
* If membership changes, `epoch` increments and `round` resets to zero.
* Epoch packet payloads are prefixed with a big-endian epoch and stale packets/signals are dropped.
* Old epoch peers/channels/inbox are torn down before the new immutable bootstrap is exposed.

## Outcome consensus

Each active roster member reports the complete canonical outcome for exactly one `(epoch, round)`. Matching duplicates are acknowledged and do not apply scores again. All roster members must agree before `round_commit`; a conflicting live report emits one terminal `round_abort` without score mutation. Stale reports (wrong epoch/round, non-roster reporter, or no active round) are rejected. Terminal decisions are immutable: a late identical report is acknowledged from the decision record, a late conflicting report against a committed round is rejected, and any late report for an aborted round is rejected as stale. Scores are applied exactly once, only at commit.

## Server messages

The server may send `welcome`, `status`, `presence`, `profile_accepted`, `report_ack`, `round_commit`, `round_abort`, `start`, `signal`, `pong`, and `error`. Clients must validate structure, bounds, epoch, and player IDs before acting. Unknown message types are protocol errors. Wire shapes:

* `welcome` — `{ type, protocol:3, playerId, reconnectToken, reconnectGraceMs }`
* `start` — `{ type, protocol:3, epoch, round, mode, capacity, seed, roster:[{playerId,index,profile,score}] }`
* `status` — `{ type, protocol:3, status:"active"|"waiting", mode, capacity, active:{epoch,round}|null, ready, score }`
* `presence` — `{ type, playerId, connected, expired }`
* `profile_accepted` — `{ type }`
* `report_ack` — `{ type, epoch, round, duplicate, received, required }`
* `round_commit` — `{ type, epoch, round, outcomes:[{playerId,placement,scoreDelta}], scores:[{playerId,score}] }`
* `round_abort` — `{ type, epoch, round, reason }`
* `signal` — `{ type, epoch, from, data }`
* `pong` — `{ type, nonce? }`
* `error` — `{ type, error }`

`report_ack` with `received === required` indicates the report completed a terminal decision; otherwise it is an in-progress or duplicate acknowledgment. A `round_commit` or `round_abort` may be immediately followed by a `start` for the next round when a full roster is still eligible.
