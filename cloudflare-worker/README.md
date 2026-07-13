# Ghost Battle signaling worker

This Worker provides the legacy `/match` two-browser matcher, fixed-roster `/lobby` protocol v2 compatibility, and lifecycle-safe `/lobby?...&protocol=3`. Protocol 3 supports opt-in two-player Dueling Ghosts and default Last Ghost Standing at 3–8 players, explicit exit/requeue, and server-authoritative same-roster rematches with a persisted 10-second vote deadline. All relay only WebRTC SDP/ICE; game packets remain peer-to-peer. New clients use [protocol v3](../docs/lobby-v3.md); [v2](../docs/lobby-v2.md) remains for older clients. The v3 lifecycle reducer is vendored dependency-free in [`vendor/cloudflare-game-common`](vendor/cloudflare-game-common/README.md) so the Worker and `car_game_ai` share byte-for-byte transition semantics.

## Deploy

1. Set `ALLOWED_ORIGINS` in `wrangler.jsonc` to the exact comma-separated origins hosting the game.
2. Give the rate-limit binding a `namespace_id` unique within your Cloudflare account.
3. Run `npx wrangler@latest deploy` from this directory (Wrangler 4.110+ is pinned in `package.json`).
4. If the game host is Cloudflare-proxied, route the Worker under `/match/*` and `/lobby/*`. Otherwise compile the game with `GHOST_BATTLE_SIGNALING_URL=wss://your-worker.workers.dev/match`; the client derives `/lobby` for protocol 3 and keeps `/match` as its legacy fallback.

## Tests

`npm test` runs the pure source tests under `test/` with Node's built-in test runner. They cover protocol parsing/validation and the vendored lifecycle reducer (2–8 supported capacities, active-ready immutability, roster/epoch continuity, mid-round joins, report consensus, terminal idempotence, reconnect token rotation/expiry/supersession, epoch signal validation, rematch generation/nonce idempotence, simultaneous requests, timeout/deny/disconnect, exit, and deterministic seed advancement) without a Workers runtime.

For local development, run `npx wrangler dev` and add the exact local game origin to `ALLOWED_ORIGINS`. The game's `local` Cargo feature uses `ws://127.0.0.1:8787/match`.

The browser transport uses Cloudflare's public STUN server. It has no TURN fallback, so peers behind restrictive NAT/firewall configurations may be unable to connect.
