# Changelog

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
