# Changelog

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
