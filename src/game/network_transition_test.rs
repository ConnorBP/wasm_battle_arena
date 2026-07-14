//! Feature-gated deterministic driver for real multi-browser GGRS lifecycle QA.
use bevy::prelude::*;
use bevy_ggrs::GgrsSchedule;

use super::{
    ggrs_framecount::GGFrameCount,
    networking::MatchmakingRoom,
    session::{MatchPreference, RoundBootstrap, RoundOutcome},
    GameState, RollbackState, RoundProgress,
};

const TEST_ELIMINATION_FRAME: u32 = 30;

/// Resolve a deterministic authoritative round on both peers. Alternating the
/// winner prevents first-to-three while exercising repeated real GGRS rollover.
pub fn drive_transition_round(
    frame: Res<GGFrameCount>,
    bootstrap: Res<RoundBootstrap>,
    mut progress: ResMut<RoundProgress>,
    mut next: ResMut<NextState<RollbackState>>,
) {
    if frame.frame == 0 {
        transition_event("frame", bootstrap.epoch.0, bootstrap.round.0, frame.frame);
    }
    if frame.frame < TEST_ELIMINATION_FRAME || progress.resolved.is_some() {
        return;
    }
    let winner_index = (bootstrap.round.0 as usize + 1) % bootstrap.roster.len();
    let winner = bootstrap.roster[winner_index].player_id;
    progress.resolved = Some(RoundOutcome::Complete {
        point_winners: vec![winner],
    });
    progress.resolved_frame = Some(frame.frame);
    next.set(RollbackState::RoundEnd);
    transition_event(
        "elimination",
        bootstrap.epoch.0,
        bootstrap.round.0,
        frame.frame,
    );
}

pub fn emit_session_event(bootstrap: Res<RoundBootstrap>, frame: Res<GGFrameCount>) {
    transition_event("session", bootstrap.epoch.0, bootstrap.round.0, frame.frame);
}

fn auto_enter_transition_queue(
    mut room: ResMut<MatchmakingRoom>,
    mut next: ResMut<NextState<GameState>>,
) {
    room.private_code = None;
    room.preference = MatchPreference::Duel;
    next.set(GameState::Matchmaking);
}

pub fn install(app: &mut App) {
    app.add_systems(
        GgrsSchedule,
        drive_transition_round
            .before(super::player::process_deaths)
            .ambiguous_with_all(),
    );
    app.add_systems(OnEnter(GameState::MainMenu), auto_enter_transition_queue);
    app.add_systems(OnEnter(GameState::InGame), emit_session_event);
}

#[cfg(target_arch = "wasm32")]
fn transition_event(kind: &str, epoch: u32, round: u32, frame: u32) {
    emit_transition_event(kind, epoch, round, frame);
}

#[cfg(not(target_arch = "wasm32"))]
fn transition_event(_kind: &str, _epoch: u32, _round: u32, _frame: u32) {}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export function emit_transition_event(kind, epoch, round, frame) {
  const event = { kind, epoch, round, frame };
  (window.__ghostTransitionEvents ??= []).push(event);
  console.info(`GHOST_TRANSITION ${kind} ${epoch}:${round} frame=${frame}`);
}
"#)]
extern "C" {
    fn emit_transition_event(kind: &str, epoch: u32, round: u32, frame: u32);
}
