# Wave 1 External Networking JavaScript Report

## Scope

The browser networking implementation was extracted from the Rust `inline_js` attribute into `src/cloudflare_net.js`. The wasm-facing export names and argument/return ABI remain unchanged for the active protocol-v4 queue and protocol-v3 lobby paths. Rust now binds the ES module with:

```rust
#[wasm_bindgen(module = "/src/cloudflare_net.js")]
```

This is wasm-bindgen's project-root external-module form. For the `web` target, wasm-bindgen emits the module under `out/snippets` and points the generated glue import at it. Deployments must preserve the complete generated `out` tree. This also matches Trunk's documented project-root module-path convention if the project moves to Trunk.

## Direct tests added

- `npm run test:network-js` imports the real ES module in Node with browser API fakes. It directly verifies:
  - telemetry has the JavaScript `bigint` type required by a wasm-bindgen Rust `u64` import;
  - absent optional lobby collections do not throw;
  - packet framing contains unsigned, big-endian epoch and round fields;
  - u32 protocol bounds reject an out-of-range epoch;
  - normal and failure WebSocket closes use legal codes (`1000` and `4000`);
  - a thrown data-channel send fails the session and is not counted as sent.
- `npm run test:network-js:browser` imports the same source as an ES module in Chromium and repeats the BigInt, epoch/round framing, failed-send, and close-code contracts in an actual browser JS engine.
- The Pages workflow runs both suites after installing its pinned Playwright Chromium.

These replace Rust tests that merely searched the Rust source text for JavaScript strings. Rust unit tests remain for Rust-owned scalar conversion and validation behavior.

## Warning/dead-code cleanup

Only networking code proven disconnected from the current entry path was removed:

- `CloudflareSocket::connect` and the legacy `/match/v2` Rust setup path;
- the legacy `wait_for_players` GGRS fallback and its inert `stop_legacy_matchmaking_socket` system;
- the standalone `CloudflareSocket::match_generation` accessor (rematch generation continues to flow through control events; gameplay did not read the accessor);
- Rust-only timeout/assignment mirror helpers that existed solely for source/reducer tests;
- source-string tests for JavaScript and obsolete UI text.

The active queue/lobby, WebRTC mesh, rematch, rollover, report, framing, telemetry, and gameplay behavior were not intentionally changed.

## Safety hardening during extraction

`cloudflare_lobby_send` now treats `channels` as optional. Queue handles intentionally do not have to expose every lobby collection, and a send attempted before assignment increments dropped telemetry rather than throwing. Existing failure behavior for an actual open channel's thrown `send` remains unchanged.

## Build/deploy notes

No commands, builds, tests, package managers, shells, or network tools were executed, per task constraints. Changes were made and reviewed statically with read/edit/write operations only. The workflow and local `deploy.bat` continue invoking wasm-bindgen with `--target web`; the generated snippets directory must not be pruned from `out`.
