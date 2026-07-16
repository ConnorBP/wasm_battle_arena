# TURN relay implementation report

## Scope

Implemented Cloudflare Realtime TURN credential minting in the signaling Worker and consumption by both browser WebRTC paths. No public credential endpoint was added.

## Worker

* Added `cloudflare-worker/src/turn.js`.
* Calls `POST https://rtc.live.cloudflare.com/v1/turn/keys/{TURN_KEY_ID}/credentials/generate-ice-servers` with `Authorization: Bearer TURN_KEY_API_TOKEN` and `{ "ttl": 21600 }`.
* Uses a 3-second timeout and 16 KiB response cap; bounds server/URL counts, URL length, username, and credential.
* Allows only exact `stun.cloudflare.com` / `turn.cloudflare.com` hosts with `stun:`, `turn:`, or `turns:` schemes. Port 53 is filtered.
* Missing secrets, API/network/timeout errors, malformed or oversized responses, and invalid data return STUN-only configuration rather than failing matchmaking.
* Minting occurs once per accepted connection, after Worker origin/rate/route admission and Durable Object query/reconnect/socket-cap admission.
* Legacy `matched`, v2 `welcome`, and v3 `welcome` messages include `iceServers` and `turnExpiresAt`.

## Browser

* Strictly revalidates all ICE entries, dimensions, expiry, exact hosts/schemes, credentials, fields, and port 53.
* Invalid configurations use `stun:stun.cloudflare.com:3478`.
* Both legacy and lobby `RTCPeerConnection` constructors use the validated per-session configuration.
* TURN data is held only in memory. `sessionStorage` continues to hold only v3 reconnect identity.
* Reconnect gets a fresh Worker mint.

## Refresh policy

There is intentionally no browser-callable credential endpoint. Existing connections do not need fresh credentials after ICE setup. A long-lived v3 control connection can, however, create a later epoch near or after the six-hour expiration. Proactive refresh is not implemented because doing it safely requires a new authenticated server-pushed protocol message and coordinated peer application; exposing a fetch endpoint would violate the architecture. The limitation is documented. Peer constructors reject TURN within the final ten minutes and fall back to STUN; reconnect refreshes credentials.

## Telemetry

The browser reads the selected candidate pair via `RTCPeerConnection.getStats()` and classifies it as host, server-reflexive, or relay. Rust telemetry exposes host/srflx/relay, relay-use, and STUN-fallback counters. No credentials, URLs, candidates, SDP, or addresses are logged.

## Tests added

Pure source tests cover missing secrets, API errors, malformed/oversized responses, timeout, allowlist, port 53, bounds, secret non-logging, handshake inclusion, mint ordering, both constructors, storage secrecy, reconnect refresh policy, and candidate-pair telemetry source.

Per task constraints, no build, test, npm, Wrangler, Git, shell, web, or spawned command was run.
