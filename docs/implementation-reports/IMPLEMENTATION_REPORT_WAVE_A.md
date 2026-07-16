# Wave A Implementation Report

## Scope

Implemented browser-local casual progression, durable preferences, cosmetic unlock UI, and cosmetic presentation repair. No network protocol, matchmaking, rematch behavior, retro theme, or power-up visuals were changed.

## Persistent casual profile

`src/game/progression.rs` owns a versioned profile stored under:

- localStorage key: `ghosties.casual-profile.v1`
- schema version: `1`

The profile persists:

- sanitized player name
- music volume
- effects/SFX volume
- palette ID
- lifetime points
- matches played
- rounds played
- unlocked cosmetic mask
- equipped cosmetic ID
- processed confirmed-outcome IDs for durable duplicate suppression

The decoder treats a malformed envelope or unknown version as a safe default profile. Individual values are validated and normalized:

- names use the existing canonical 24-byte player-name sanitizer and fall back to `Ghost`
- volumes must be finite and are clamped to `0..=100`
- palettes are allowlisted to IDs `0..=3`
- counters are bounded
- unlocks are recomputed from canonical thresholds rather than trusting storage
- unknown or locked equipped cosmetics fall back to Classic
- malformed processed event IDs are discarded

Storage failures are intentionally non-fatal. Browser localStorage access is guarded by JavaScript `try/catch`; native builds use inert storage functions.

Reconnect identity remains separate and unchanged in the networking layer's sessionStorage.

## Rewards and unlocks

Each confirmed round grants:

- 2 participation points
- 3 additional points when the local player is a point winner

A completed match increments `matches_played`; every awarded confirmed outcome increments `rounds_played`.

Cosmetic thresholds are intentionally achievable:

| Cosmetic | Lifetime points required |
| --- | ---: |
| Classic | 0 |
| Crown | 5 |
| Wizard | 12 |
| Bow | 25 |

Rewards are applied only after the P2P session reports the resolved rollback frame as confirmed and the game has entered `RoundEnd`. The stable event ID consists of match ID, session epoch, round number, and resolved confirmed frame. IDs are durably retained, preventing duplicate grants from repeated update frames, rollback replay, or page reload.

## Settings UI

The settings panel now shows lifetime points, rounds, and matches. All cosmetics remain visible. Locked entries are disabled and show their exact lifetime-point requirement. Only unlocked cosmetics can be selected.

Name, palette, music volume, effects volume, and valid equipped cosmetic changes synchronize to the local profile.

## Cosmetic rendering

Player spawn still selects the allowlisted cosmetic image from each synchronized `PlayerProfile`. A presentation repair system now reapplies the expected image handle to every player entity after rollback presentation repair. It keys by stable player ID rather than local handle, so the same path covers the local player and all remote players. Unknown IDs render Classic.

This fixes cosmetic image handles that can otherwise be restored or lost with rollback-managed sprite components, without introducing cosmetic state into gameplay simulation.

## Pure tests added

Focused tests in `src/game/progression.rs` cover:

- schema round trip
- unknown/corrupt schema defaulting
- invalid name, volume, palette, counter, unlock, equipped, and event values
- exact cosmetic thresholds
- duplicate stable event IDs, including after profile encode/decode
- equipped cosmetic existence and unlock validation

## Verification note

Per task constraints, no builds, tests, compilers, package managers, shell commands, or Git commands were run. Changes were reviewed statically only.
