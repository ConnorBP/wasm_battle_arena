# Wave 2 Mobile Menu / Pause Overlay Implementation Report

## Scope

Implemented the mobile-safe in-game menu flow in `src/game/gui.rs` and its Bevy scheduling in `src/game/mod.rs`. No build, test, package, shell, or version-control commands were run, per task constraints.

## Behavior

- Added `MenuState::Pause`.
- Escape transitions are now pure and game-state aware:
  - In game `Main -> Pause`
  - In game `Pause -> Main` (resume)
  - In game `Settings -> Pause`
  - Main-menu Settings still returns to `Main`.
- Replaced the floating gear and adjacent exit control with one compact, explicit `☰ MENU` touch target.
- Added a pause overlay with:
  - Resume
  - Settings
  - Exit Lobby
  - Main Menu
- The overlay explicitly says multiplayer continues and the player is not paused.
- Pause does not alter `GameState`, GGRS scheduling, network polling, telemetry, rollback systems, or input. Those remain scheduled by `GameState::InGame`, independent of `MenuState::Pause`.
- Exit Lobby and Main Menu both notify the Worker with `leave_lobby(false)`, return to `GameState::MainMenu`, and reset menu state. Neither requeues.
- Existing `OnExit(GameState::InGame)` cleanup remains the single safe cleanup path for GGRS/session entities/resources.
- Entering Main Menu also resets `MenuState::Main`, preventing stale Pause/Settings state after cleanup.
- The native mobile player-name and room-code bridge calls remain intact.
- Opening Pause hides any stale native bridge control; entering Settings restores the player-name bridge.

## Responsive Layout

- Introduced a shared screen-safe rectangle inset by at least 12 egui points.
- Pause and match-over panels are constrained to that rectangle and vertically scroll.
- Main menu content now shares one scrollable central panel instead of an unconstrained top panel.
- Narrow mode selectors and palette controls wrap.
- Slider widths clamp to available width.
- Interactive theme minimum remains 44 x 44 points; primary pause/back controls explicitly use at least 44-point height.
- Score rows wrap within the safe width.
- Status HUD is width-constrained inside safe bounds.
- Match-over/rematch actions wrap and the panel scrolls in short landscape viewports.

## Tests Added

Pure Rust tests cover:

- Escape transitions.
- Settings Back destination by `GameState`.
- Pause actions and Worker-leave/no-requeue effects.
- Continued in-game runtime scheduling while Pause or Settings is open.
- Portrait and landscape safe bounds.
- Extreme narrow viewport bounds.
- Minimum touch sizing and responsive margins.

## Browser Smoke

Extended `scripts/smoke-mobile-input.mjs` to exercise:

- iPhone portrait.
- Pixel landscape.
- Existing native text bridge focus/value behavior.
- Canvas viewport bounds in both orientations.

The in-game Pause overlay is not clicked by this smoke because its special build intentionally auto-enters the room-code UI and does not establish a multiplayer/GGRS game. Menu transitions and actions are covered by deterministic pure tests instead.

## Validation Status

Not executed by design. The changes require normal CI/build validation after integration.
