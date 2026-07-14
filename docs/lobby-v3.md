# Lobby protocol v3 lifecycle

Protocol 3 is the default lifecycle protocol for selected multiplayer modes. Connect to:

```
wss://<worker>/lobby/<room>?protocol=3&mode=<duel|deathmatch>&capacity=<n>
```

`duel` is exactly 2 players. `deathmatch` is Last Ghost Standing and accepts 3–8 players; eight is the supported ceiling.

The old `/match/<room>` endpoint remains the explicit legacy duel fallback. Lobby v2 (`/lobby/<room>` without `protocol=3`) remains deployed for older clients, but new clients always send `protocol=3`. The Worker never silently downgrades a v3 request: `protocol=3` is required and validated exactly once on the v3 control socket, and a signal missing its `epoch` is rejected rather than reinterpreted as a v2 message.

Protocol-v4 flexible matchmaking hands selected rosters to reserved `q4_*` v3 room names. Initial admission to those names additionally requires the exact short-lived, one-use HMAC assignment fields documented in [matchmaking-v4.md](matchmaking-v4.md). After admission, the normal rotating v3 reconnect identity applies. Ordinary/private v3 room admission is unchanged.

## Identity and reconnect

`welcome` contains `protocol: 3`, a 32-hex `playerId`, a rotating 32-hex `reconnectToken`, `reconnectGraceMs`, strictly validated `iceServers`, and `turnExpiresAt` (Unix milliseconds, or `null` for STUN-only fallback). The browser keeps the latest reconnect identity in `sessionStorage`: reconnects in the same tab/runtime preserve identity; a full browser storage reset (or a new tab) creates a new identity. Every successful reconnect rotates the token, supersedes any older live socket for that identity, and invalidates the previous token. The server stores only SHA-256 token hashes. The browser stores only reconnect identity in `sessionStorage`; short-lived TURN usernames/passwords remain in memory and must never be logged or stored. Every successful reconnect is admitted again and receives freshly minted credentials. There is no browser-callable/public credentials endpoint.

Reconnect honors a grace window. A disconnected roster member is not removed immediately; their seat is reserved until `reconnectGraceMs` elapses, at which point the identity expires and the roster is released to terminal cleanup. A reconnect strictly before that boundary preserves the identity.

An active-roster reconnect does **not** replay the current `start`: a reloaded page has lost its old WebRTC/GGRS transport, while the other peers may still have theirs. After `welcome`, the Worker sends `status:"reconnecting"`, temporarily answers old-round reports/signals with `status:"reconnecting"`, and records a persisted deadline about 300 ms in the future. Reloads from other immutable roster members during that fixed window join the same batch and do not extend it. At the deadline (checked by the Durable Object alarm and incoming processing), if every immutable roster member is connected, the Worker atomically persists one replacement bootstrap and then broadcasts one `start` to all sockets. It preserves player IDs, current profiles, scores, and `matchGeneration`, uses a fresh seed, increments `epoch` exactly once, and resets `round` to zero. Existing clients stage that changed epoch through their normal reset barrier; reloaded clients with no installed roster install it immediately.

If any immutable member is still absent at the batch deadline, no replacement epoch is made. The short batch marker is cleared, the active state remains reserved, and the ordinary 30-second identity grace continues. A later reconnect may open a new short batch; if the roster is not restored before grace expires, normal terminal cleanup applies. This separation prevents a partial roster, duplicate alarm, or staggered message from creating duplicate epoch restarts.

After `welcome`, send a validated `profile`, then `ready`. The control WebSocket stays open independently of epoch WebRTC/GGRS channels.

## Epoch lifecycle

A `start` message contains `protocol`, `epoch`, `round`, `mode`, `capacity`, a 32-hex `seed`, and a canonical roster with profile/score snapshots.

* `ready` never replaces `active`. An immutable active epoch cannot be replaced by ready, profile, presence, or mid-round join events. The sole reconnect exception is the server-authoritative, deadline-batched changed-epoch rollover described above; it never mutates/replays the current bootstrap in place.
* Mid-round joiners are waiting candidates for the next selection; incumbents keep their seat until one leaves.
* If the next canonical roster is unchanged, `round` increments and `epoch` does not.
* If membership changes, `epoch` increments and `round` resets to zero.
* Epoch packet payloads are prefixed with a big-endian epoch and stale packets/signals are dropped.
* Old epoch peers/channels/inbox are torn down before the new immutable bootstrap is exposed.
* Players beyond the selected capacity remain queued for a later immutable roster; they never join an active epoch.

## Outcome consensus

Each active roster member reports the complete canonical outcome for exactly one `(epoch, round)`. Matching duplicates are acknowledged and do not apply scores again. All roster members must agree before `round_commit`; a conflicting live report emits one terminal `round_abort` without score mutation. Stale reports (wrong epoch/round, non-roster reporter, or no active round) are rejected. Terminal decisions are immutable: a late identical report is acknowledged from the decision record, a late conflicting report against a committed round is rejected, and any late report for an aborted round is rejected as stale. Scores are applied exactly once, only at commit.

## Match end, exit, and rematch

At match point, ordinary round advancement stops. Clients have three distinct choices:

* `rematch_request { generation, nonce }` / `rematch_response { generation, nonce, accept }` keeps the lobby control sockets and identities. The generation is exactly the current generation plus one and the nonce is 32 hex characters. A request counts as acceptance, duplicate messages are idempotent, stale generations/nonces are rejected, and simultaneous requests mutually accept the first authoritative proposal.
* `requeue` explicitly places only its sender back in the general queue. Former opponents are sent to main menu, never implicitly queued.
* `leave` exits to main menu. It is also available during play as Exit Lobby.

A rematch proposal expires after 10 seconds. Its absolute deadline is persisted and checked both by the Durable Object alarm and on messages/connections. Denial, timeout, or disconnect releases the entire current roster to main menu. Acceptance resets scores and match state, deterministically advances seed/map, increments the epoch, and sends a fresh immutable `start`; clients tear down and recreate GGRS while retaining the control socket.

## Server messages

The server may send `welcome`, `status`, `presence`, `profile_accepted`, `report_ack`, `round_commit`, `round_abort`, `start`, `signal`, `match_over`, `rematch_pending`, `rematch_accepted`, `rematch_denied`, `match_exit`, `requeue`, `pong`, and `error`. Clients must validate structure, bounds, epoch, and player IDs before acting. Unknown message types are protocol errors. Wire shapes:

* `welcome` — `{ type, protocol:3, playerId, reconnectToken, reconnectGraceMs, iceServers, turnExpiresAt }`
* `start` — `{ type, protocol:3, epoch, round, mode, capacity, seed, roster:[{playerId,index,profile,score}] }`
* `status` — `{ type, protocol:3, status:"active"|"waiting"|"reconnecting", mode, capacity, active:{epoch,round}|null, ready, score, reconnectDeadline? }`; `reconnectDeadline` is present for `reconnecting` and is the current absolute Unix-millisecond batch deadline, or the relevant grace deadline after an incomplete batch.
* `presence` — `{ type, playerId, connected, expired }`
* `profile_accepted` — `{ type }`
* `report_ack` — `{ type, epoch, round, duplicate, received, required }`
* `round_commit` — `{ type, epoch, round, outcomes:[{playerId,placement,scoreDelta}], scores:[{playerId,score}] }`
* `round_abort` — `{ type, epoch, round, reason }`
* `signal` — `{ type, epoch, from, data }`
* `pong` — `{ type, nonce? }`
* `error` — `{ type, error }`

TURN credentials have a six-hour lifetime. A currently connected peer is unaffected by expiry. The protocol does not yet push refreshed credentials down a still-open control socket before a later epoch; a new epoch within the final ten minutes deliberately uses STUN-only configuration, and reconnecting refreshes TURN. A future server-pushed welcome-equivalent is required to remove that limitation without adding a public endpoint.

`report_ack` with `received === required` indicates the report completed a terminal decision; otherwise it is an in-progress or duplicate acknowledgment. A `round_commit` or `round_abort` may be immediately followed by a `start` for the next round when a full roster is still eligible.
