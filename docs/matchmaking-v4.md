# Dynamic public matchmaking protocol v4

Protocol 4 is a **public queue only**. Connect a WebSocket to the canonical gameplay-version pool:

```text
wss://<worker>/queue/<battle-major-minor-patch>?protocol=4&preference=<any|duel|deathmatch>
```

The query is preference-only. The obsolete public `target` parameter is rejected (it is not ignored). No arbitrary/private queue room route exists. Existing private and public protocol-v3 lobby URLs are unchanged. Protocol 4 selects a roster and hands it to an assigned exact protocol-v3 room; it does not relay game signaling itself.

## Queue lifecycle

The server first sends:

```json
{"type":"queued","protocol":4,"ticket":"<32 hex>","preference":"any"}
```

Queue sockets are long-lived and have no ordinary matchmaking timeout. Clients must send a heartbeat more frequently than every 45 seconds:

```json
{"type":"heartbeat","nonce":"optional bounded string"}
```

The server responds with `heartbeat_ack`. Before staging it may send `searching` or `holding_for_third`. Once an LGS group stages, every member receives its current count, vote count, dynamic strict-majority requirement, and fixed absolute deadline:

```json
{"type":"status","status":"staging","count":5,"votes":2,"votesRequired":3,"deadline":2000000000000}
```

A missing heartbeat/dead connection is removed by the watchdog. Clients may send exactly `{"type":"cancel"}`; cancellation and disconnect immediately remove the ticket unless an assignment deadline at the same timestamp has already won. Message rate/size limits and a 256-entry pool cap protect the Durable Object.

## Deterministic pre-stage arbitration

All choices sort by persisted sequence, then ticket as deterministic tie-break.

* Specific Duel plus specific Duel assigns immediately.
* A specific Duel immediately takes the oldest unlocked Any when another Duel is unavailable.
* Two Any wait three seconds for a compatible third. A specific Duel may take the oldest Any during that hold. A compatible third establishes LGS staging; expiry produces Duel.
* Deathmatch plus Any remains unlocked while waiting, so the Any can still be taken by Duel.
* Deathmatch-only players never downgrade to Duel.
* Three compatible Deathmatch/Any tickets atomically establish staging. Staged tickets cannot be stolen by Duel.

## Dynamic LGS staging

The first three compatible tickets lock a staging group with an absolute deadline exactly 30 seconds after staging begins. That deadline never resets due to joins, votes, withdrawals, leaves, disconnects, Worker hibernation, or alarm rescheduling.

Compatible Any/Deathmatch tickets join in stable queue order until the eight-player ceiling. Assignment starts when any one condition is met:

1. the eighth compatible ticket joins (immediate assignment),
2. the fixed 30-second deadline is reached, or
3. staged members cast a strict majority of start votes: `floor(count / 2) + 1`.

A staged ticket casts its one vote with exactly:

```json
{"type":"vote_start"}
```

It withdraws with exactly:

```json
{"type":"withdraw_start_vote"}
```

Duplicate votes and duplicate withdrawals are idempotent. Joining changes `votesRequired` for the new count but preserves existing votes and the deadline. Leaving or disconnecting removes that member's vote and recomputes the majority. If fewer than three remain, staging dissolves; survivors become unlocked and retain their original queue sequence for pre-stage arbitration.

Durable Object event serialization plus reducer checks make equal-time races deterministic: expiry at timestamp `T` is decided before a join, vote, cancellation, or disconnect also observed at `T`. Decisions, staging state, votes, and absolute deadlines are persisted before handoff.

## Assignment handoff

Every selected client receives a bounded message:

```json
{
  "type":"assigned",
  "protocol":4,
  "room":"q4_<32 hex>",
  "mode":"deathmatch",
  "capacity":5,
  "ticket":"<32 hex>",
  "expiresAt":2000000000000,
  "token":"<64 hex HMAC-SHA256>"
}
```

Connect before `expiresAt` to:

```text
/lobby/<room>?protocol=3&mode=<mode>&capacity=<capacity>&queueTicket=<ticket>&queueExpires=<expiresAt>&queueToken=<token>
```

The token signs the exact canonical protocol version, room, mode, capacity, ticket, and expiry. Assigned `q4_*` rooms require it. EpochLobby rejects malformed/tampered signatures, expiry at or after the boundary, room/mode/capacity/ticket mismatch, and replay. Successful admission consumes the ticket before socket/TURN work. Ordinary/private v3 rooms do not require assignment parameters and retain their existing behavior.

Tokens are credentials: never log or persist them. Queue decisions persist only non-secret assignment fields and reproduce the HMAC for crash-safe handoff. Assignment validity remains 30 seconds.

## Deployment secret

From `cloudflare-worker`, set an encrypted secret with at least 32 bytes of independently generated entropy:

```text
npx wrangler secret put QUEUE_ASSIGNMENT_SECRET
```

Never put this value in Wrangler vars, source, docs, logs, URLs generated server-side, issues, or browser storage. The same Worker secret is available to both `MatchQueue` and `EpochLobby`. The `v4` Wrangler migration creates `MatchQueue`; both production and local configs bind it as `MATCH_QUEUE`.
