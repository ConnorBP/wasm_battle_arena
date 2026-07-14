# Protocol-v3 client rollover

The browser transport uses an explicit two-phase handoff for every newer immutable `start`.

1. **Pending:** validate the complete start, retain it as `pendingStart`, and keep the active epoch/round and all active peers unchanged. Pending-epoch signals are roster-validated and buffered with a 256-message bound.
2. **Promote:** after Bevy exits `InGame`, removes the old GGRS `Session` and gameplay entities, and applies deferred commands, Rust calls `promote_pending(old_epoch, old_round)`. Promotion identity-checks and closes the old round once, installs pending state, creates peers, and replays buffered signals in order.

`close_epoch` is epoch-and-round guarded, so stale cleanup cannot close the promoted transport. New GGRS installation waits until every promoted data channel is open and the browser reports `Ready`. Scores always come from the promoted immutable server start.
