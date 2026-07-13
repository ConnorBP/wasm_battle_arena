# cloudflare-game-common (vendored)

Dependency-free Cloudflare/Node lifecycle source shared with `car_game_ai`.

`lifecycle.js` defines immutable active-round installation, roster/epoch continuity, deterministic candidate selection, exactly-once outcome consensus, epoch-scoped signal validation, and reconnect-token rotation. It deliberately imports no project or npm modules and uses only standard ECMAScript (no `crypto`, no `structuredClone`), so it runs unchanged in Cloudflare Workers and Node. Consumers should vendor this directory unchanged and adapt exported functions at their boundary, as `cloudflare-worker/src/epoch-state.js` does.
