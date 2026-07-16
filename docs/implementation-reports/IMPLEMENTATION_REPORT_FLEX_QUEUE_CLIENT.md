# Wave 2 flexible queue client implementation report

## Scope

Implemented the protocol-v4 browser client and Rust/UI integration. No Worker matchmaking policy was changed.

## Client flow

- Added `MatchPreference::{Any, Duel, LastGhostStanding}` independently of `GameMode`.
- Public matchmaking opens `/queue/public-v4?protocol=4&preference=<any|duel|deathmatch>&target=<3..8>`.
- `Any` is the default and prominent recommended UI choice. Specific Duel and Last Ghost Standing choices remain available; the latter exposes a target slider from 3 through 8.
- Private room-code matches bypass protocol 4 and open an exact protocol-v3 lobby with selected mode/capacity.
- The queue WebSocket has only a bounded 15-second opening timeout. Ordinary waiting has no age timeout and is kept alive with a 15-second heartbeat, safely below the 45-second Worker watchdog.
- Cancellation sends the protocol-v4 `cancel` message before close. Disconnect/error state is surfaced through the existing Rust connection state.
- Queue status is exposed as searching, holding briefly for a third, forming LGS count/target, and assigned/joining.

## Assignment security and handoff

The browser treats assignment messages as untrusted. It accepts only bounded scalar fields with:

- protocol exactly 4;
- exact expected 32-lowercase-hex ticket;
- `q4_` plus 32 lowercase hex room;
- exact Duel/2 or Deathmatch/3–8 mode/capacity pairing;
- safe future timestamp represented as a JS safe integer, not expired and no farther than 60 seconds ahead;
- 64-lowercase-hex HMAC token;
- no unknown assignment keys.

After validation, the queue heartbeat is stopped and queue socket is closed. The same Rust transport ID atomically changes to an exact protocol-v3 lobby URL containing `queueTicket`, `queueExpires`, and `queueToken`. Assignment credentials are not persisted. The v3 `welcome` remains the point where TURN credentials are minted/validated and retained only in memory.

The handoff is bounded to 15 seconds; v3 lobby/WebRTC formation retains the existing bounded timeout. `wait_for_players` no longer treats coordinator assignment as game readiness. It installs GGRS only after the exact v3 immutable start and all peer data channels are ready.

## Practice and cancellation

Practice continues while queueing, during status transitions, and during assignment handoff because the game remains in `Matchmaking`. Cleanup remains chained to `OnExit(Matchmaking)`, which now occurs only on actual v3/WebRTC readiness or safe cancellation/failure to Main Menu.

## Tests and harness

Added pure Rust tests for:

- preference-to-wire mapping and `Any` default;
- public/private bypass policy;
- strict queue status scalar reduction;
- no ordinary queue wait timeout with bounded opening/handoff/WebRTC phases;
- assignment scalar validation, tampering, expiry, ticket mismatch, and exact mode/capacity.

Added `scripts/flexible-queue-protocol-smoke.mjs` and wired it into the local smoke runner/package scripts. It exercises:

- Any + Duel immediate Duel;
- Duel + Duel;
- Any + Any hold then Duel;
- Any + Any + Any LGS;
- Deathmatch + Any + Any LGS;
- Deathmatch + Any followed by Duel stealing Any;
- cancellation and disconnect;
- heartbeat-backed waiting and a source policy assertion proving queue wait does not inherit the old two-minute timeout.

The Worker reducer's existing tests own deterministic deadline, watchdog, cancellation/disconnect, lock expansion, and all capacity behavior; the browser harness tests the public socket contract.

## Validation constraints

Per task instruction, no shell, build, test, Cargo, npm, Wrangler, Git, web, or spawned process was run. Changes were reviewed through source reads only.

## Worker changes

One tiny schema/output fix was necessary: `MatchQueue` now publishes bounded `status` messages (`searching`, `holding_for_third`, or `forming` with count/target) after reducer transitions, deduplicated per socket attachment. Arbitration, deadlines, lock policy, assignment signing, and admission policy were not changed.
