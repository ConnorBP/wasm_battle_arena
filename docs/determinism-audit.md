# Determinism audit

Audited authoritative rollback state and systems for v0.4.1.

## Enforced rules

- Arena generation and feature/spawn selection use `u64` modulo before conversion to `usize`.
- `Map`, `RoundProgress`, scores, seeds, frame count, player/bullet/effect components are rollback-registered.
- Command-producing player and bullet systems use stable ordering:
  - firing and traps by `PlayerId`
  - bullets by `(owner PlayerId, frame-local bullet id)`
  - pickups by cell then player handle
  - collisions/deaths/scores by stable identities
- Bullet ownership uses stable `PlayerId`; handles are input/sound stream indexes only.
- Speed boost uses fixed constants and a tested 300-frame countdown.
- Rollback audio keys remain full-width `u64`; presentation audio state is excluded from simulation.
- The in-round GGRS systems form one explicit chain and CI rejects schedule ambiguities.
- The local Playwright harness enters `InGame`, runs for 15 seconds, and fails on panic, WASM trap, unreachable, assertion, or schedule conflict.

## Remaining release gates for epoch multiplayer

Before enabling late join/reconnect in production, add:

- canonical cross-peer state digest exchange,
- packet loss/delay/reorder fault injection,
- 2/3/4-browser full-mesh scenarios,
- confirmed-frame epoch transition tests,
- same-process reconnect and page-reload boundary tests,
- repeated-round stress runs on desktop and a named mobile device.
