# Flexible matchmaking protocol v4

Protocol 4 is a **public queue only**. Connect a WebSocket to the one canonical pool:

```text
wss://<worker>/queue/<battle-major-minor-patch>?protocol=4&preference=<any|duel|deathmatch>&target=<3..8>
```

No arbitrary/private queue room route exists. Existing private and public protocol-v3 lobby URLs are unchanged. Protocol 4 selects a roster and then hands it to an assigned protocol-v3 room; it does not relay game signaling itself.

## Queue lifecycle

The server first sends:

```json
{"type":"queued","protocol":4,"ticket":"<32 hex>","preference":"any","target":8}
```

Queue sockets are long-lived and have no ordinary matchmaking timeout. Clients must send a heartbeat more frequently than every 45 seconds:

```json
{"type":"heartbeat","nonce":"optional bounded string"}
```

The server responds with `heartbeat_ack`. As arbitration changes, it also sends bounded UI status updates: `{"type":"status","status":"searching"}`, `{"type":"status","status":"holding_for_third"}`, or `{"type":"status","status":"forming","count":3,"target":8}`. A missing heartbeat/dead connection is removed by the watchdog. Clients may send exactly `{"type":"cancel"}`; cancellation and disconnect immediately remove the ticket. Message rate and size limits and a 256-entry pool cap protect the Durable Object.

## Deterministic arbitration

All choices sort by persisted sequence, then ticket as the deterministic tie-break.

* The oldest specific Duel pairs another specific Duel when available, otherwise it immediately takes the oldest unlocked Any.
* Two Any wait 3 seconds for a compatible third. A specific Duel may take the oldest Any during that hold. A compatible third creates Last Ghost Standing; expiry produces Duel.
* Deathmatch plus Any remains unlocked while waiting; the Any can be taken by Duel.
* Deathmatch-only players never downgrade to Duel.
* Three compatible Deathmatch/Any tickets lock into a group for 2 seconds, expanding toward the smallest member target (maximum 8). Locked members cannot be stolen. Reaching target or deadline creates exact-capacity Last Ghost Standing.
* Cancellation/disconnect during expansion dissolves the partial lock and reconsiders survivors without changing their sequence.

Decisions, lock state, and absolute deadlines are persisted before handoff.

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

The token signs the exact canonical protocol version, room, mode, capacity, ticket, and expiry. Assigned `q4_*` rooms require it. EpochLobby rejects malformed/tampered signatures, expiry at or after the boundary, room/mode/capacity/ticket mismatch, and replay. A successful admission consumes the ticket before socket/TURN work. Ordinary v3 rooms do not require assignment parameters and retain their existing behavior.

Tokens are credentials: never log or persist them. Queue decisions persist only non-secret assignment fields and reproduce the HMAC for crash-safe handoff.

## Deployment secret

From `cloudflare-worker`, set an encrypted secret with at least 32 bytes of independently generated entropy:

```text
npx wrangler secret put QUEUE_ASSIGNMENT_SECRET
```

Never put this value in Wrangler vars, source, docs, logs, URLs generated server-side, issues, or browser storage. The same Worker secret is available to both `MatchQueue` and `EpochLobby`. The `v4` Wrangler migration creates `MatchQueue`; both production and local configs bind it as `MATCH_QUEUE`.
