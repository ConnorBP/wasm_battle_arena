# Wave 1 Matchmaking Target Practice — Implementation Report

## Scope delivered

Matchmaking now opens a small, visible local waiting arena containing:

- one controllable ghost using the existing ghost texture;
- three bounded, moving ghost targets with readable red tint;
- local shots using the existing bullet texture;
- held-fire cooldown, hit detection, score, current streak, best streak, and delayed target replacement;
- desktop controls: WASD or arrow keys to move, Space or Enter to fire;
- touch controls: one movement finger that starts on the left half, with independently tracked fire touches that start on the right half;
- a practice HUD which leaves the existing “Waiting for opponent to join...” matchmaking status visible.

## Lifecycle and isolation

`src/game/practice.rs` owns the practice components and resources:

- `PracticePlayer`
- `Target`
- `Shot`
- `PracticeScore`
- `PracticeCooldown`
- `PracticeSpawn`
- `PracticeTouch`

The practice setup is registered on `OnEnter(GameState::Matchmaking)`. All simulation systems are regular Bevy `Update` systems guarded by `in_state(GameState::Matchmaking)`. Cleanup runs on `OnExit(GameState::Matchmaking)` and removes every practice-owned entity, including active shots, before gameplay presentation begins. Practice resources are also removed.

Camera and audio receiver transforms are reset when entering and leaving matchmaking, preventing a previous gameplay camera/listener offset from hiding the waiting arena or carrying practice positioning into a match.

Practice is intentionally absent from:

- `GgrsSchedule`;
- GGRS rollback component/resource registration;
- `RollbackState` variants and transitions;
- socket, lobby, bootstrap, profile, progression, multiplayer score, and map-seed mutation.

Its score and streak are disposable local feedback only.

## Focused tests added

Pure unit tests in `src/game/practice.rs` cover:

1. player movement clamping;
2. cooldown ticking and saturation;
3. target boundary bounce;
4. shot/target hit radius;
5. score and streak updates;
6. active/expired shot cleanup predicates and cleanup collection;
7. split left-touch movement/right-touch fire behavior.

## Documentation

The README feature list now mentions local target practice and its desktop/touch controls.

## Validation note

Per task constraints, no shell, build, test, Cargo, npm, Git, web, or spawned process was executed. The implementation was completed by static file inspection and edits only.
