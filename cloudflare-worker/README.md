# Ghost Battle signaling worker

This Worker provides the legacy `/match` two-browser matcher and the fixed-roster `/lobby` protocol v2. Both relay only WebRTC SDP/ICE; game packets remain peer-to-peer. See [`../docs/lobby-v2.md`](../docs/lobby-v2.md) for the v2 wire protocol.

## Deploy

1. Set `ALLOWED_ORIGINS` in `wrangler.jsonc` to the exact comma-separated origins hosting the game.
2. Give the rate-limit binding a `namespace_id` unique within your Cloudflare account.
3. Run `npx wrangler@latest deploy` from this directory (or use Wrangler 4.36+).
4. If the game host is Cloudflare-proxied, route the Worker under `/match/*` and `/lobby/*`. Otherwise compile the game with `GHOST_BATTLE_SIGNALING_URL=wss://your-worker.workers.dev/match` for the legacy client.

For local development, run `npx wrangler dev` and add the exact local game origin to `ALLOWED_ORIGINS`. The game's `local` Cargo feature uses `ws://127.0.0.1:8787/match`.

The browser transport uses Cloudflare's public STUN server. It has no TURN fallback, so peers behind restrictive NAT/firewall configurations may be unable to connect.
