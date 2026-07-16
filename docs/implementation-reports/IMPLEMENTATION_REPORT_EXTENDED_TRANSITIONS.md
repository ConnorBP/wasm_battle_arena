# Extended real transition harness implementation report

## Scope and isolation

The lifecycle harness is compiled only by Cargo feature `network_transition_test`. URL parsing, structured browser events, command bridge, deterministic outcomes, and auto-matchmaking all live in `src/game/network_transition_test.rs`, behind the existing feature gate in `src/game/mod.rs`. The single Rust transport observation helper is also gated by that feature. A normal/default/release production build contains no transition URL parser, event queue, command API, or harness getter.

The harness uses real protocol-v3 control WebSockets, WebRTC data channels, and GGRS sessions. Browser commands dispatch through the same Rust `CloudflareSocket::request_rematch` and `CloudflareSocket::leave_lobby(true)` APIs used by the game UI. No production lifecycle is replaced or mocked.

## Scenarios

- `rollover`: two isolated browser contexts compare four immutable sessions, require matching epoch/round sequences, and require frame zero on every replacement.
- `active_disconnect`: closes one browser after both peers report an active-round checkpoint; the survivor must return to main menu without a replacement session.
- `rollover_disconnect`: closes one browser only after the feature driver observes the old GGRS session removed inside the reset barrier; the survivor must return cleanly.
- `reconnect`: waits for a committed non-zero score, reloads both isolated pages within grace, and requires the same per-peer identity, identical score snapshot, unchanged epoch/round, and a continued active GGRS checkpoint.
- `rematch`: deterministic real rounds take one player to first-to-three. Both pages invoke the feature bridge, which calls the real client rematch API. The assertions require larger generation/epoch, round zero, all-zero scores, and a changed seed.
- `requeue`: reaches first-to-three, invokes the real client requeue API, disconnects the old lobby transport through the normal Rust lifecycle, and requires a newly observable protocol-v4 searching connection.
- `changed_roster`: starts exactly three real LGS browsers, then admits a waiting fourth. The currently deployed Worker API cannot express “incumbent departs at the round boundary while preserving the rest of the roster”: `leave` intentionally releases/aborts the complete active roster. The harness exposes this as a structured capability value and records a clear bounded skip after verifying the practical supported portion. It does not pretend the unsupported transition passed.

## Event contract and safety

Every event has `schema: 1`, a fixed scenario name, state/kind strings, unsigned epoch/round/frame/generation numbers, bounded seed/identity/detail strings, a structured bounded score snapshot, and at most eight validated canonical roster IDs. Console output is one JSON object prefixed by `GHOST_TRANSITION`; artifacts consume the in-page structured list directly. The inline bridge slices strings, freezes events/API objects, allowlists scenarios, and exposes only one pending command slot.

Scenario assertions are bounded and have no retry loop. Polling only observes asynchronous browser state until one absolute deadline. Browser failures, panics, traps, malformed events, mismatched peer sequences, and unsupported API results fail immediately. The changed-roster limitation is a documented capability skip rather than a retry or false pass.

## Build, CI, and artifacts

- `scripts/run-network-transition-smoke.sh` builds the feature artifact, starts loopback Wrangler/static hosting, and runs each scenario in a unique room.
- `scripts/network-transition-smoke.mjs` never starts/spawns infrastructure; it owns browser orchestration and writes per-scenario `result.json` plus failure screenshots.
- `package.json` exposes `smoke:network-transition` and `smoke:network-transition:all`.
- `.github/workflows/pages.yml` runs the transition suite after the production Pages artifact has already been built. This preserves production-content isolation.

## Files changed

- `src/game/network_transition_test.rs`
- `src/cloudflare_net.rs` (feature-only local identity observation)
- `scripts/network-transition-smoke.mjs`
- `scripts/run-network-transition-smoke.sh`
- `package.json`
- `.github/workflows/pages.yml`
- `README.md`
- `IMPLEMENTATION_REPORT_EXTENDED_TRANSITIONS.md`

## Validation status

No shell command, build, test, Cargo, npm, Git, Web request, or spawned process was executed, per task restriction. The central follow-up is a normal formatting/type-check pass followed by the local harness. In particular, the Bevy 0.11 system signatures and the inline wasm-bindgen bridge should be compiler-validated before merge.
