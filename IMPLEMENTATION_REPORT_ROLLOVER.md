# Protocol-v3 Epoch/Round Rollover Implementation Report

## Scope

Implemented a two-phase client rollover for the persistent protocol-v3 lobby transport. No commands, builds, tests, package managers, Wrangler, Git, browsers, or network access were used, per task constraints.

## Browser transport

- A newer validated `start` received while a round is active is stored as `pendingStart`; active epoch/round, roster, channels, peers, inbox, and readiness are not changed.
- Signals for the pending epoch are validated against the pending roster and buffered in arrival order with the existing 256-item bound. Overflow fails the transport rather than growing memory without limit.
- Added pending epoch/round exports and Rust getters.
- Added explicit `cloudflare_lobby_promote_pending` / `CloudflareSocket::promote_pending`.
  - Requires exact active old epoch/round and exact pending epoch/round.
  - Closes the identified old round exactly once.
  - Installs pending immutable state, creates new peers, and replays buffered signals in order through the serialized signal chain.
  - Returns `false` for stale, duplicate, or mismatched promotion.
- Epoch close now takes epoch and round. Identity and one-close guards prevent delayed old cleanup from closing a promoted round.
- Peer/channel callbacks capture epoch and round, so callbacks from retired transports cannot fail or mutate the promoted transport.
- New transport status becomes `Ready` only after every promoted peer data channel opens. Rust therefore creates the new GGRS session only after promoted channels are ready.
- Immutable-start roster scores are exported and used as the new bootstrap's authoritative scores rather than resetting them locally.

## Rust lifecycle

- Added non-rollback `EpochRollover` resource.
- `watch_lobby_epoch` reads only pending epoch/round and records rollover intent; it no longer closes channels or removes GGRS directly.
- Rollover transitions to `Matchmaking` while retaining the persistent control socket.
- `OnExit(InGame)` now performs:
  1. old Session/resource/game-entity cleanup,
  2. `apply_deferred` as an explicit barrier,
  3. pending transport promotion.
- Ordinary exits still close the active identified round. Rollover exits leave closing to the atomic promotion operation.
- Intentional `Disconnected` and `NetworkInterrupted` GGRS events are ignored only while `EpochRollover` is active. The same events remain fatal during a real active round.
- Matchmaking installation waits for browser `Ready`, then creates GGRS from the promoted transport and authoritative immutable bootstrap.

## Tests and harness

- Added Rust source-contract coverage for pending buffering, identity-guarded promotion, and the no-close/no-Session-removal epoch watcher contract.
- Added a small `EpochRollover` state test.
- Extended the local protocol browser harness to commit and observe two consecutive ordinary round rollovers on the same long-lived lobby sockets.

## Files

- `src/cloudflare_net.rs`
- `src/game/networking.rs`
- `src/game/mod.rs`
- `scripts/multiplayer-protocol-smoke.mjs`
- `ROLLOVER_PROTOCOL.md`
- `IMPLEMENTATION_REPORT_ROLLOVER.md`

## Validation status

Not executed. The task explicitly prohibited shell/build/test/cargo/npm/Wrangler/browser execution. Changes were reviewed by reading the edited source only.
