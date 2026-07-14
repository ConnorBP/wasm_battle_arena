# Wave1 Dynamic Public LGS Staging Worker — Implementation Report

## Scope

Implemented the dynamic public protocol-v4 Last Ghost Standing staging behavior in the Cloudflare Worker and pure reducer. No Rust or private protocol-v3 lifecycle behavior was changed.

## Implemented behavior

- Removed the public protocol-v4 `target` query from the accepted schema; requests containing it are rejected as unknown parameters.
- Queue admission and `queued` messages now carry preference only.
- Preserved pre-stage arbitration:
  - Duel + Duel assigns immediately.
  - Duel + Any assigns immediately.
  - two Any hold for three seconds and then Duel unless a compatible third arrives.
  - Deathmatch + Any remains unlocked.
  - the third compatible Any/Deathmatch ticket establishes LGS staging.
- At three compatible tickets, locks a staging group with a fixed 30-second absolute deadline.
- Admits compatible Any/Deathmatch tickets through eight; staged members cannot be stolen by Duel.
- Assigns immediately at eight, at the 30-second boundary, or when start votes reach `floor(n / 2) + 1`.
- Added exact `vote_start` and `withdraw_start_vote` queue messages.
- Votes are one per ticket; duplicate vote and withdrawal messages are idempotent.
- Staging status contains `count`, `votes`, `votesRequired`, and `deadline`.
- Joins dynamically change the threshold without resetting votes or deadline.
- Leaves/disconnects remove that ticket's vote and recompute the threshold without resetting the deadline.
- Falling below three dissolves staging and preserves survivor queue sequence.
- Deadline checks run before same-timestamp joins, votes, and cancellations, giving serialized Durable Object races deterministic expiry-first behavior.
- Added persisted-state migration from the earlier two-second target lock to the 30-second vote staging record without resetting its inferred creation time.
- Signed exact protocol-v3 assignment handoff and 30-second assignment TTL remain unchanged.

## Test coverage authored

Pure source tests cover:

- strict-majority thresholds and vote starts for staging sizes 3–7;
- immediate assignment at eight;
- automatic deadline assignment for every size 3–7;
- fixed deadline across joins, votes, and leaves;
- duplicate votes and withdrawals;
- threshold changes on join;
- disconnect/leave vote removal;
- below-three dissolution and sequence preservation;
- equal-time deadline races against join, vote, and cancellation;
- staged roster protection from Duel stealing;
- two-Any fallback;
- obsolete `target` query rejection.

## Validation constraint

Per task constraints, no shell, build, test runner, npm, Wrangler, Git, web, or spawned process was executed. Changes were reviewed statically only.
