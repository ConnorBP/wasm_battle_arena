# Wave 2 Dynamic LGS Client/UI Implementation Report

## Scope

Implemented the protocol-v4 dynamic Last Ghost Standing client path and matchmaking UI. Public matchmaking is now preference-only; exact capacity remains exclusively a private protocol-v3 setting.

## Client protocol

- Removed the public roster-size field from `MatchmakingRoom`, `CloudflareSocket::connect_queue`, the WASM bridge, the queue URL, session state, and queued-acknowledgement validation.
- Added strict parsing for staging status fields:
  - `count`: 3–8
  - `votes`: 0–count
  - `votesRequired`: exactly `floor(count / 2) + 1`
  - `deadline`: positive JavaScript-safe absolute millisecond timestamp
  - recipient-specific `voted`: boolean, and impossible `voted: true` / zero-vote snapshots are rejected
- Added `QueueStatus::Staging` with all UI-relevant fields.
- Added `vote_start` and `withdraw_start_vote`; both return `false` without sending unless the local queue snapshot is staging and the requested transition is locally valid.
- Preserved unbounded ordinary queue waiting, heartbeat liveness, signed exact v3 handoff, and local target practice during matchmaking.

## Worker status compatibility

The staging status publisher now includes a tiny recipient-specific `voted` boolean computed from that socket's ticket. The shared vote set remains server authoritative. Protocol documentation records this field.

## UI

- Removed the public LGS capacity slider.
- Staging prominently displays:
  - assembled LGS ghost count,
  - votes / strict-majority votes required,
  - auto-start seconds remaining.
- Displays **Vote to Start** or **Withdraw Vote** from server-reported local vote state.
- Keeps **Cancel** available.
- Private matchmaking still prominently supports exact LGS capacities 3–8 and direct protocol-v3 connection.

## Tests and harness

- Expanded Rust unit coverage for staging count/vote/majority/deadline bounds and safe voting outside staging.
- Added exact private LGS capacity coverage for every value 3–8.
- Added source assertions against reintroducing the obsolete public roster-size field.
- Reworked the Playwright flexible queue harness for target-free acknowledgement/URLs, dynamic 3→4 staging, recipient vote state, vote withdrawal, fixed deadlines, majority start, fill-to-eight start, and cancellation vote recomputation.

## Verification constraint

Per task instruction, no shell, build, test, Cargo, npm, Wrangler, Git, web, or spawned process was executed. Changes were reviewed statically only.
