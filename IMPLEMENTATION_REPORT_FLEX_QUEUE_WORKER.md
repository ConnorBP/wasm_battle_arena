# Wave 1 protocol-v4 flexible queue Worker implementation report

## Scope

Implemented Worker-only protocol-v4 public flexible matchmaking. No Rust/client files were changed. Legacy `/match`, lobby v2, and ordinary/private lobby v3 routes remain available and retain their prior query behavior.

## Implementation

* Added the canonical public endpoint `/queue/public-v4?protocol=4&preference=any|duel|deathmatch&target=3..8` and no arbitrary queue-room route.
* Added the `MatchQueue` SQLite Durable Object, `MATCH_QUEUE` binding, and `v4` migration to production and local Wrangler configs.
* Added a dependency-free pure reducer with persisted monotonic sequence and ticket tie-break, 3-second Any hold, 2-second locked Deathmatch expansion, Duel stealing rules, no DM downgrade, cancellation/disconnect handling, heartbeat watchdog, queue cap, and absolute alarm deadlines.
* Queue waiting has no ordinary duration timeout. A 45-second heartbeat watchdog removes dead connections. Existing edge admission rate limiting plus per-socket message limiting protects admission and control traffic.
* Assignment decisions (unique `q4_*` room, exact mode/capacity, tickets, and 30-second expiry) are persisted before socket handoff. Tokens are deterministic HMAC-SHA256 credentials generated from the Worker secret and are not persisted or logged.
* Extended EpochLobby query admission for assigned `q4_*` rooms. It verifies the canonical room/mode/capacity/ticket/expiry signature, rejects expired/tampered/mismatched input, and atomically consumes each ticket before TURN/socket work. Replay returns conflict. Non-assigned v3 rooms remain unchanged.
* Bounded all-or-none assignment query parsing and bounded scalar handoff messages were added.

## Tests added (not executed by instruction)

Pure Node tests cover:

* all preference parser values and targets 3–8;
* Duel-vs-Duel, Duel taking Any, two-Any hold/expiry, compatible third, Deathmatch+Any stealing, and DM-only non-downgrade;
* locked expansion and every final capacity 3–8, target bounding, lock non-stealing, and deadline finalization;
* deterministic sequence ordering, cancellation/disconnect lock dissolution, heartbeat/dead watchdog, and absence of ordinary timeout;
* HMAC canonical verification, field/signature tampering, expiry boundary, and one-use replay semantics;
* assignment handoff query all-or-none bounds and public-route restriction.

## Deployment

Set an encrypted `QUEUE_ASSIGNMENT_SECRET` of at least 32 bytes before deploying. It must not be placed in config vars or logs. Apply the Wrangler `v4` Durable Object migration. Route `/queue/*` in addition to existing `/match/*` and `/lobby/*` routes.

## Verification note

Per task constraint, no shell, build, test, npm, Wrangler, git, web, or spawned command was executed. Changes were reviewed statically only.
