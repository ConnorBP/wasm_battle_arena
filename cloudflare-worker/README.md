# Ghost Battle signaling worker

This Worker provides the legacy `/match` two-browser matcher, fixed-roster `/lobby` protocol v2 compatibility, and lifecycle-safe `/lobby?...&protocol=3`. Protocol 3 supports opt-in two-player Dueling Ghosts and default Last Ghost Standing at 3–8 players, explicit exit/requeue, and server-authoritative same-roster rematches with a persisted 10-second vote deadline. All relay only WebRTC SDP/ICE; game packets remain peer-to-peer except when a restrictive network requires Cloudflare TURN relay. New clients use [protocol v3](../docs/lobby-v3.md); [v2](../docs/lobby-v2.md) remains for older clients. The v3 lifecycle reducer is vendored dependency-free in [`vendor/cloudflare-game-common`](vendor/cloudflare-game-common/README.md) so the Worker and `car_game_ai` share byte-for-byte transition semantics.

## Deploy

1. Set `ALLOWED_ORIGINS` in `wrangler.jsonc` to the exact comma-separated origins hosting the game.
2. Give the rate-limit binding a `namespace_id` unique within your Cloudflare account.
3. Configure the Cloudflare Realtime TURN key as encrypted Worker secrets from this directory:

   ```text
   npx wrangler secret put TURN_KEY_ID
   npx wrangler secret put TURN_KEY_API_TOKEN
   ```

   Enter each value only at Wrangler's interactive prompt. **Do not paste either secret into `wrangler.jsonc`, source, documentation, logs, issues, or browser configuration/storage.** `TURN_KEY_API_TOKEN` needs permission to generate credentials for the key identified by `TURN_KEY_ID`.
4. Run `npx wrangler@latest deploy` from this directory (Wrangler 4.110+ is pinned in `package.json`).
5. If the game host is Cloudflare-proxied, route the Worker under `/match/*` and `/lobby/*`. Otherwise compile the game with `GHOST_BATTLE_SIGNALING_URL=wss://your-worker.workers.dev/match`; the client derives `/lobby` for protocol 3 and keeps `/match` as its legacy fallback.

## Tests

`npm test` runs the pure source tests under `test/` with Node's built-in test runner. They cover protocol parsing/validation and the vendored lifecycle reducer (2–8 supported capacities, active-ready immutability, roster/epoch continuity, mid-round joins, report consensus, terminal idempotence, reconnect token rotation/expiry/supersession, epoch signal validation, rematch generation/nonce idempotence, simultaneous requests, timeout/deny/disconnect, exit, and deterministic seed advancement) without a Workers runtime.

For local development, run `npx wrangler dev` and add the exact local game origin to `ALLOWED_ORIGINS`. The game's `local` Cargo feature uses `ws://127.0.0.1:8787/match`.

## TURN behavior and security

After origin, rate, route/query, reconnect, and room-cap admission, the Worker mints one six-hour (21,600 second) Cloudflare Realtime TURN credential set per accepted connection. There is intentionally **no HTTP/public credentials endpoint**. The credential API token remains a Worker secret; the short-lived username/password appear only in the admitted WebSocket handshake and browser memory. Only `stun.cloudflare.com` and `turn.cloudflare.com` `stun:`, `turn:`, or `turns:` URLs pass strict Worker and browser validation; port 53 is removed.

Missing secrets, credentials API timeout/error, oversized or malformed responses, and rejected URLs all degrade to the public Cloudflare STUN server. They never reject an otherwise admitted matchmaking connection. Reconnecting opens a newly admitted control WebSocket and refreshes credentials. Existing WebRTC connections continue normally after credential expiry, but the current protocol has no public refresh endpoint and does not proactively refresh a still-open v3 control connection before creating a later epoch. Consequently, a new epoch created within the final ten minutes or after expiry deliberately falls back to STUN; users can reconnect to obtain fresh credentials. Adding a server-pushed refresh message is the future safe option.

Telemetry uses `RTCPeerConnection.getStats()` to classify the selected candidate pair as `host`, `srflx`, or `relay` and counts relay use and STUN fallback. It never records URLs, usernames, credentials, SDP, or ICE candidate text.
