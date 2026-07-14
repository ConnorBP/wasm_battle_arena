# Ghost Battle signaling worker

This Worker provides the legacy `/match` two-browser matcher, fixed-roster `/lobby` protocol v2 compatibility, lifecycle-safe `/lobby?...&protocol=3`, and the dynamic public protocol-v4 queue. Protocol 3 supports opt-in two-player Dueling Ghosts and Last Ghost Standing at 3–8 players. Protocol 4 accepts only Duel/Deathmatch/Any preferences (the obsolete public roster-size query is rejected), stages 3–8 compatible LGS players for up to 30 seconds with strict-majority start voting, publishes recipient-specific `voted` status for safe vote/withdraw UI, and securely hands an exact roster to v3. All relay only WebRTC SDP/ICE; game packets remain peer-to-peer except when a restrictive network requires Cloudflare TURN relay. See [protocol v4](../docs/matchmaking-v4.md), [protocol v3](../docs/lobby-v3.md), and legacy [v2](../docs/lobby-v2.md).

## Deploy

1. Set `ALLOWED_ORIGINS` in `wrangler.jsonc` to the exact comma-separated origins hosting the game.
2. Give the rate-limit binding a `namespace_id` unique within your Cloudflare account.
3. Configure the Cloudflare Realtime TURN key as encrypted Worker secrets from this directory:

   ```text
   npx wrangler secret put TURN_KEY_ID
   npx wrangler secret put TURN_KEY_API_TOKEN
   npx wrangler secret put QUEUE_ASSIGNMENT_SECRET
   ```

   `QUEUE_ASSIGNMENT_SECRET` must be an independently generated high-entropy value of at least 32 bytes. It signs short-lived, one-use queue assignments. Configure the same secret for the `MatchQueue` and `EpochLobby` bindings (they are classes in this Worker deployment). Rotate it only when outstanding 30-second assignments may safely be invalidated.

   Enter each value only at Wrangler's interactive prompt. **Do not paste any secret into `wrangler.jsonc`, source, documentation, logs, issues, or browser configuration/storage.** `TURN_KEY_API_TOKEN` needs permission to generate credentials for the key identified by `TURN_KEY_ID`.
4. Run `npx wrangler@latest deploy` from this directory (Wrangler 4.110+ is pinned in `package.json`).
5. If the game host is Cloudflare-proxied, route the Worker under `/match/*`, `/lobby/*`, and `/queue/*`. Otherwise compile the game with `GHOST_BATTLE_SIGNALING_URL=wss://your-worker.workers.dev/match`; clients can derive `/lobby` and `/queue` routes while keeping `/match` as the legacy fallback.

## Tests

`npm test` runs the pure source tests under `test/` with Node's built-in test runner. They cover protocol parsing/validation, v4 pre-stage arbitration, dynamic 3–8 staging/deadlines/voting/cancellation/disconnect/equal-time races, assignment HMAC tamper/replay/expiry semantics, Worker source ordering, and the vendored lifecycle reducer (2–8 supported capacities, active-ready immutability, roster/epoch continuity, mid-round joins, report consensus, terminal idempotence, reconnect token rotation/expiry/supersession, server-authoritative batched active reconnect rollover, single/staggered/simultaneous reloads, score/profile/generation preservation, absent-member grace, epoch signal validation, rematch generation/nonce idempotence, timeout/deny/disconnect, exit, and deterministic seed advancement) without a Workers runtime.

For local development, run `npx wrangler dev` and add the exact local game origin to `ALLOWED_ORIGINS`. The game's `local` Cargo feature uses `ws://127.0.0.1:8787/match`.

## TURN behavior and security

After origin, rate, route/query, reconnect, and room-cap admission, the Worker mints one six-hour (21,600 second) Cloudflare Realtime TURN credential set per accepted connection. There is intentionally **no HTTP/public credentials endpoint**. The credential API token remains a Worker secret; the short-lived username/password appear only in the admitted WebSocket handshake and browser memory. Only `stun.cloudflare.com` and `turn.cloudflare.com` `stun:`, `turn:`, or `turns:` URLs pass strict Worker and browser validation; port 53 is removed.

Missing secrets, credentials API timeout/error, oversized or malformed responses, and rejected URLs all degrade to the public Cloudflare STUN server. They never reject an otherwise admitted matchmaking connection. Reconnecting opens a newly admitted control WebSocket and refreshes credentials. Existing WebRTC connections continue normally after credential expiry, but the current protocol has no public refresh endpoint and does not proactively refresh a still-open v3 control connection before creating a later epoch. Consequently, a new epoch created within the final ten minutes or after expiry deliberately falls back to STUN; users can reconnect to obtain fresh credentials. Adding a server-pushed refresh message is the future safe option.

Telemetry uses `RTCPeerConnection.getStats()` to classify the selected candidate pair as `host`, `srflx`, or `relay` and counts relay use and STUN fallback. It never records URLs, usernames, credentials, SDP, or ICE candidate text.
