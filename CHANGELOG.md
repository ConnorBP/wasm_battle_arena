# Changelog

## 0.8.0 - 2026-07-14

- Replaced public Last Ghost Standing target counts with dynamic 3–8-player staging lobbies.
- Added strict-majority Vote to Start controls and a fixed 30-second automatic start countdown.
- Added vote withdrawal, dynamic vote thresholds, immediate eight-player starts, and disconnect-safe staging dissolution.
- Preserved exact 3–8-player Last Ghost Standing capacities for private room-code matches.
- Added a one-release compatibility bridge for already-loaded 0.7.x queue clients.

## 0.7.1 - 2026-07-13

- Fixed an Any Mode browser crash caused by polling protocol-v3 control messages during protocol-v4 queue waiting.
- Hardened queue/lobby browser transport accessors and corrected application WebSocket close codes.
- Added an actual WASM Any Mode click-and-wait regression smoke test.

## 0.7.0 - 2026-07-13

- Added Any Mode as the recommended default public queue with flexible Duel and Last Ghost Standing assignment.
- Removed the ordinary two-minute matchmaking timeout; heartbeat liveness now permits long target-practice waits.
- Added deterministic three-second two-player holds and two-second Last Ghost Standing expansion windows up to eight players.
- Added version-isolated queue pools and short-lived one-use signed queue-to-lobby assignments.
- Added queue status feedback for searching, waiting briefly for a third ghost, forming a larger arena, and secure handoff.

## 0.6.0 - 2026-07-13

- Added local target practice with moving targets, score, streaks, and desktop/touch controls while waiting in matchmaking.
- Added a mobile-safe multiplayer menu overlay with Resume, Settings, Exit Lobby, and Main Menu actions.
- Constrained menus, score/status panels, and match-over controls to narrow portrait and landscape safe areas.
- Added a visible Cancel Matchmaking action and guaranteed practice cleanup before multiplayer begins.

## 0.5.1 - 2026-07-13

- Added short-lived Cloudflare Realtime TURN credentials for restrictive campus, corporate, and carrier NATs.
- Added validated UDP/TCP/TLS relay configuration with safe STUN fallback and relay candidate telemetry.
- Added a production relay-only verification script that never exposes TURN credentials or network addresses.

## 0.5.0 - 2026-07-13

- Made 3–8 player Last Ghost Standing the default mode and retained Dueling Ghosts as an opt-in two-player mode.
- Added server-authoritative, race-safe same-lobby rematches with accept/deny, ten-second timeout, deterministic fresh maps, and distinct Re-Queue behavior.
- Added local persistent player names, audio/settings preferences, points progression, and earned cosmetic unlocks.
- Fixed local and remote cosmetic rendering and added visible shield and speed-boost effects.
- Added retro procedural arena textures and a cohesive pixel-art egui theme with mobile-friendly controls.
- Added an in-game Exit Lobby action, queued mid-round joins, reconnect hardening, epoch-scoped signaling, and eight-player roster support.
- Added comprehensive Worker, Rust, WASM, mobile, and local 2/3/4/8-client multiplayer validation.

## 0.4.1 - 2026-07-12

- Fixed the Bevy schedule ambiguity panic that occurred when entering a match.
- Added a regression test that rejects conflicting in-round system schedules.

## 0.4.0 - 2026-07-12

- Replaced the dedicated Matchbox server with Cloudflare Worker signaling and browser WebRTC transport.
- Added automatic GitHub Pages deployment from `master`.
- Added deterministic generated arenas with fair mirrored spawns.
- Added trap tiles and rollback-safe speed and shield pickups.
- Fixed movement through one-block corridors and duplicate background music.
- Improved mobile viewport handling, simultaneous movement/fire, and touch deadzones.
- Reduced the deployed WebAssembly and asset payload.
- Removed unmaintained dependencies and enabled weekly dependency updates.
- Added stable player/session/bootstrap contracts as groundwork for future deathmatch and reconnect support.
