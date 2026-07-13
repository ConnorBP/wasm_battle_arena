# Deathmatch rules

These rules are deterministic gameplay policy. Transport, matchmaking, and the deployed two-player wire protocol are unchanged.

## Stable roster and scores

- A round has a canonical roster sorted by stable `PlayerId`.
- Duel has exactly two players. Deathmatch has two through four players.
- Rollback score state contains one entry per roster player, keyed and sorted by `PlayerId`; entity iteration order and per-epoch GGRS handles do not define score ownership.
- Score UI displays every score in canonical roster order.

## Round outcomes

An eliminated or disconnected player is unavailable for round-outcome purposes. Duplicate and unknown IDs do not affect an outcome.

### Duel

Existing deployed semantics are preserved:

- The first elimination completes the round and awards one point to the opponent.
- If both players are eliminated simultaneously, each opponent receives one point (one point for each death).

### Deathmatch

- A round continues while at least two players remain available.
- When exactly one player remains, the round completes and that sole survivor receives one point.
- If every remaining player is eliminated or disconnected simultaneously, the round completes as a wipe and nobody receives a point.

Outcome calculation is pure and independent of roster, elimination, disconnection, entity-query, or packet input order. Winners are always returned in canonical `PlayerId` order.
