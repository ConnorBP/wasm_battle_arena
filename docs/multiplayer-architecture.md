# Multiplayer session architecture

## Constraints

GGRS 0.9 has a fixed player registry. Every handle from `0..num_players` must be registered before a `P2PSession` starts. Players cannot be added, rebound, or fully reconnected to that running session; disconnecting a slot is one-way.

Therefore:

- A GGRS handle is an epoch-local input slot, never a persistent player identity.
- Roster changes, late joins, and full reconnects happen only between rounds.
- Each accepted roster change starts a new session epoch and a new `P2PSession`.
- Round state is rebuilt from a canonical `RoundBootstrap`, not copied from one peer.
- Deathmatch is initially capped at four players because browser transport uses a full WebRTC mesh.

## Canonical boundary state

`RoundBootstrap` defines the protocol version, match identity and seed, epoch, round, game mode, sorted stable roster, contiguous epoch handles, and sorted scores. All peers must agree on the same value before starting an epoch.

Current duel matchmaking creates temporary match-scoped `PlayerId` values from the synchronized seed. These are an adapter for the existing two-player protocol, not persistent identities. A future lobby protocol will issue opaque fixed-width hexadecimal player IDs and reconnect tokens.

## Planned phases

1. Generalize deterministic game rules and score/spawn state to stable identities and up to four players.
2. Keep a versioned Cloudflare lobby control socket open for the match and queue late joins/reconnects.
3. Build a full-mesh browser transport with source-preserving peer addresses.
4. Confirm round outcomes, agree on the next bootstrap, and replace GGRS sessions at boundaries.
5. Add fault injection, multi-browser tests, deterministic state digests, and focused security/performance audits.
