# Networking lifecycle audit implementation

## Changed files

- `src/cloudflare_net.rs` — protocol-3 browser control, reconnect credential rotation/storage, epoch framing/rejection, persistent control ownership, defensive message handling, report deduplication foundation, telemetry resource.
- `src/game/networking.rs` — protocol-3/profile connection and round-aware immutable bootstrap installation.
- `src/game/session.rs` — protocol version constant, pure roster continuity rule and tests.
- `cloudflare-worker/src/protocol.js` — strict protocol-3 report/signal validation.
- `cloudflare-worker/src/epoch-lobby.js` — persistent control lifecycle, reconnect rotation, immutable active epoch, defensive control routing, presence/status, commit/abort flow.
- `cloudflare-worker/src/epoch-state.js` — adapter to vendored shared lifecycle reducer.
- `cloudflare-worker/vendor/cloudflare-game-common/lifecycle.js` — dependency-free shared reducer compatible with `car_game_ai`.
- `cloudflare-worker/test/epoch-state.test.js` — pure lifecycle ordering, continuity, joining, duplicate/conflict, and terminal-decision tests.
- `cloudflare-worker/README.md`, `docs/lobby-v3.md` — deployment and wire/lifecycle documentation.

## Assumptions

- Competitive sessions remain fixed at exactly two Duel players or four Last Ghost Standing players because the Rust bootstrap/GGRS session requires immutable full capacity.
- Same-runtime reconnect means the same browser tab/session (`sessionStorage`). A page reload in that tab retains and rotates identity; a new tab or cleared storage joins as a new identity.
- `/match/<room>` is the legacy v2-style two-player fallback. Selected multiplayer mode uses explicit `protocol=3`; Worker protocol-v2 routing remains available to old clients.
- Server lifecycle state is authoritative for epoch/round continuity. Gameplay remains deterministic peer-to-peer and no gameplay/art/audio logic was changed.

## Known risks / follow-up

- No build or test was run, as required. The inline WASM JavaScript remains the highest integration-risk area and should be browser-smoke-tested.
- `NetworkTelemetry` is exposed as a Bevy resource; inline transport counters should be wired through additional WASM exports if production dashboards need live values rather than lifecycle instrumentation points.
- Profile snapshots are accepted by the Worker bootstrap. The current Rust bootstrap still generates display defaults from roster handles; consuming server profile fields requires additional FFI getters if desired.
- TURN is still unavailable, so restrictive NAT behavior is unchanged.

## Central validation commands (not run)

```text
cargo fmt --all -- --check
cargo test --all-targets
cargo check --target wasm32-unknown-unknown
cd cloudflare-worker && npm test
cd cloudflare-worker && npx wrangler deploy --dry-run
```

Recommended browser validation: open two/four clients, finish two rounds with unchanged roster, replace one player after a round, reload an active tab inside/outside reconnect grace, inject stale epoch packets/signals, and verify one score application per committed `(epoch, round)`.
