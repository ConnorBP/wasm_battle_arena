# Wave C — Matchmaking, Exit, High Capacity, and Rematch

## Implemented

- Protocol-3 Last Ghost Standing is now the prominent/default queue; Dueling Ghosts is an explicit two-player opt-in. The menu describes both rules and exposes LGS capacities 3–8 (two players use the distinct Duel rules).
- Worker protocol validation, reports, lifecycle selection, Rust bootstrap validation, round resolution, player color assignment, and deterministic spawn selection support Duel at 2 and LGS at 3–8, including the target ceiling of eight.
- Active rosters remain immutable. Excess eligible players remain queued and cannot enter the active epoch. Exit/disconnect releases the current roster rather than promoting a waiter into a running match.
- Added an in-game **Exit Lobby** control and distinct match-end actions: **Rematch (Same Lobby)**, **Re-Queue (General Queue)**, and **Main Menu**.
- Added server-authoritative rematch state keyed by monotonic match generation plus nonce. Requests count as acceptance; simultaneous requests mutually accept the first authoritative proposal. Duplicate requests/responses are idempotent and stale generations/nonces are rejected.
- Rematch requests have a persisted 10-second deadline. The Durable Object schedules the nearest reconnect/rematch alarm and also checks expiry on incoming connections/messages. Denial, timeout, and rematch-time disconnect return the whole current roster to menu and never implicitly queue opponents.
- Accepted rematches preserve control sockets and stable identities/roster, reset scores, increment epoch, reset round, deterministically advance the 128-bit seed, broadcast a fresh immutable start, and cause the client to retire old WebRTC/GGRS epoch channels before installing a new GGRS session.
- The browser transport now queues typed rematch/control notifications for Bevy. Match UI shows the acceptance count and accept/deny controls.
- Legacy `/match` and lobby-v2 paths remain available; modern protocol 3 is default.

## Coverage added

- Worker pure tests cover all supported capacities (Duel 2, LGS 3–8) and excess queuing, simultaneous requests, accepted recreation, duplicate idempotence, stale generation/nonce rejection, denial, exact timeout boundary, disconnect policy, whole-roster exit, score reset, immutable identity roster, and deterministic seed advancement.
- Existing Rust tests were extended for all LGS capacities 3–8, mode labels, bootstrap validation, and deterministic unique spawn generation through eight players.

## Constraints / notes

- No ceiling reduction was required: GGRS session construction is already parameterized by roster length and supports the requested eight handles; full-mesh WebRTC creates seven peers per client at capacity eight.
- Per task constraints, no shell, compiler, build, test runner, package manager, Wrangler, Git, network, or orchestration command was used. Tests were authored but not executed.
