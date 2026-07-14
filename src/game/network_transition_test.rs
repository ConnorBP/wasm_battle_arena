//! Feature-only deterministic driver and browser bridge for real WASM/WebRTC/GGRS lifecycle QA.
//!
//! This module is compiled only with `network_transition_test`; release builds
//! contain neither URL parsing, browser commands, nor transition events.
use bevy::prelude::*;
use bevy_ggrs::{GgrsSchedule, Session};

use crate::cloudflare_net::{CloudflareSocket, QueueStatus};

use super::{
    ggrs_framecount::GGFrameCount,
    networking::{EpochRollover, GgrsConfig, MatchmakingRoom},
    session::{MatchPreference, PlayerId, RoundBootstrap, RoundOutcome},
    GameState, MatchFlow, RollbackState, RoundProgress, Scores,
};

const TEST_ELIMINATION_FRAME: u32 = 30;
const TEST_CHECKPOINT_FRAME: u32 = 10;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum TransitionScenario {
    #[default]
    Rollover,
    ActiveDisconnect,
    RolloverDisconnect,
    Reconnect,
    Rematch,
    Requeue,
    ChangedRoster,
}

impl TransitionScenario {
    fn parse(value: &str) -> Self {
        match value {
            "active_disconnect" => Self::ActiveDisconnect,
            "rollover_disconnect" => Self::RolloverDisconnect,
            "reconnect" => Self::Reconnect,
            "rematch" => Self::Rematch,
            "requeue" => Self::Requeue,
            "changed_roster" => Self::ChangedRoster,
            _ => Self::Rollover,
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn name(self) -> &'static str {
        match self {
            Self::Rollover => "rollover",
            Self::ActiveDisconnect => "active_disconnect",
            Self::RolloverDisconnect => "rollover_disconnect",
            Self::Reconnect => "reconnect",
            Self::Rematch => "rematch",
            Self::Requeue => "requeue",
            Self::ChangedRoster => "changed_roster",
        }
    }
}

#[derive(Resource, Default)]
struct TransitionHarness {
    scenario: TransitionScenario,
    room: String,
    entered_queue: bool,
    endpoint: Option<(u32, u32)>,
    barrier: Option<(u32, u32)>,
    rollover_pending: Option<(u32, u32)>,
    requeue_requested: bool,
    fresh_queue_reported: bool,
}

fn configure_harness(mut harness: ResMut<TransitionHarness>) {
    install_transition_bridge();
    harness.scenario = TransitionScenario::parse(&transition_scenario());
    harness.room = sanitize_test_room(&transition_room());
    transition_event(
        "harness_ready",
        harness.scenario,
        "startup",
        None,
        0,
        0,
        0,
        0,
        "",
        "",
        "",
        "feature-only bridge installed",
    );
}

fn sanitize_test_room(value: &str) -> String {
    let value: String = value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .take(16)
        .map(|character| character.to_ascii_uppercase())
        .collect();
    if value.is_empty() {
        "GTTRANSITION".into()
    } else {
        value
    }
}

/// Resolve an authoritative round on every peer. Most scenarios alternate the
/// winner to avoid reaching match point. Endpoint scenarios deliberately award
/// the same stable player three rounds in a row.
fn drive_transition_round(
    frame: Res<GGFrameCount>,
    bootstrap: Res<RoundBootstrap>,
    harness: Res<TransitionHarness>,
    mut progress: ResMut<RoundProgress>,
    mut next: ResMut<NextState<RollbackState>>,
) {
    if frame.frame == 0 {
        emit_bootstrap_event(
            "frame",
            &harness,
            &bootstrap,
            None,
            frame.frame,
            "round frame zero",
        );
    } else if frame.frame == TEST_CHECKPOINT_FRAME {
        emit_bootstrap_event(
            "checkpoint",
            &harness,
            &bootstrap,
            None,
            frame.frame,
            "active round checkpoint",
        );
    }
    if frame.frame < TEST_ELIMINATION_FRAME || progress.resolved.is_some() {
        return;
    }

    let winner_index = match harness.scenario {
        TransitionScenario::Rematch | TransitionScenario::Requeue => 0,
        TransitionScenario::ChangedRoster => bootstrap.roster.len() - 1,
        _ => (bootstrap.round.0 as usize + 1) % bootstrap.roster.len(),
    };
    let winner = bootstrap.roster[winner_index].player_id;
    progress.resolved = Some(RoundOutcome::Complete {
        point_winners: vec![winner],
    });
    progress.resolved_frame = Some(frame.frame);
    next.set(RollbackState::RoundEnd);
    emit_bootstrap_event(
        "elimination",
        &harness,
        &bootstrap,
        None,
        frame.frame,
        &format!("winner={}", player_text(winner)),
    );
}

fn emit_session_event(
    bootstrap: Res<RoundBootstrap>,
    frame: Res<GGFrameCount>,
    scores: Res<Scores>,
    socket: Res<CloudflareSocket>,
    harness: Res<TransitionHarness>,
) {
    emit_bootstrap_event(
        "session",
        &harness,
        &bootstrap,
        Some((&scores, &socket)),
        frame.frame,
        "real GGRS session installed",
    );
}

fn auto_enter_transition_queue(
    mut harness: ResMut<TransitionHarness>,
    mut room: ResMut<MatchmakingRoom>,
    mut next: ResMut<NextState<GameState>>,
) {
    transition_event(
        "menu",
        harness.scenario,
        "main_menu",
        None,
        0,
        0,
        0,
        0,
        "",
        "",
        "",
        if harness.entered_queue {
            "returned"
        } else {
            "initial"
        },
    );
    // A clean return to menu is observable and terminal. Only initial startup
    // auto-enters matchmaking; a disconnect must not silently requeue.
    if harness.entered_queue {
        return;
    }
    harness.entered_queue = true;
    room.private_code = Some(harness.room.clone());
    if harness.scenario == TransitionScenario::ChangedRoster {
        room.private_mode = super::session::GameMode::Deathmatch;
        room.private_capacity = 3;
    } else {
        room.private_mode = super::session::GameMode::Duel;
        room.private_capacity = 2;
    }
    room.preference = MatchPreference::Duel;
    next.set(GameState::Matchmaking);
}

fn emit_matchmaking_event(harness: Res<TransitionHarness>) {
    transition_event(
        "matchmaking",
        harness.scenario,
        "matchmaking",
        None,
        0,
        0,
        0,
        0,
        "",
        "",
        "",
        if harness.requeue_requested {
            "requeue"
        } else {
            "initial"
        },
    );
}

fn observe_rollover_barrier(
    rollover: Res<EpochRollover>,
    session: Option<Res<Session<GgrsConfig>>>,
    mut harness: ResMut<TransitionHarness>,
) {
    // OnExit(InGame) promotes the pending start before Matchmaking update runs.
    // Preserve the tuple from EpochRollover itself rather than depending on
    // browser pendingStart, which promotion consumes.
    let pending = rollover.pending.or(harness.rollover_pending);
    let Some(pending) = pending else { return };
    if harness.rollover_pending != Some(pending) {
        harness.rollover_pending = Some(pending);
        transition_event(
            "rollover_pending",
            harness.scenario,
            "matchmaking",
            None,
            pending.0,
            pending.1,
            0,
            0,
            "",
            "",
            "",
            "replacement bootstrap received",
        );
    }
    if rollover.promoted
        && !rollover.install_ready
        && session.is_none()
        && harness.barrier != Some(pending)
    {
        harness.barrier = Some(pending);
        transition_event(
            "reset_barrier",
            harness.scenario,
            "matchmaking",
            None,
            pending.0,
            pending.1,
            0,
            0,
            "",
            "",
            "",
            "old GGRS session removed",
        );
    }
}

fn capture_rollover_pending(socket: Res<CloudflareSocket>, mut harness: ResMut<TransitionHarness>) {
    let Some(pending) = socket.pending_epoch_round() else {
        return;
    };
    if harness.rollover_pending != Some(pending) {
        harness.rollover_pending = Some(pending);
        transition_event(
            "rollover_pending",
            harness.scenario,
            "ingame",
            None,
            pending.0,
            pending.1,
            0,
            0,
            "",
            "",
            "",
            "replacement bootstrap received",
        );
    }
}

fn observe_match_endpoint(
    flow: Res<MatchFlow>,
    bootstrap: Option<Res<RoundBootstrap>>,
    scores: Res<Scores>,
    socket: Res<CloudflareSocket>,
    mut harness: ResMut<TransitionHarness>,
) {
    let (MatchFlow::MatchOver { winner }, Some(bootstrap)) = (&*flow, bootstrap) else {
        return;
    };
    let key = (bootstrap.epoch.0, bootstrap.round.0);
    if harness.endpoint == Some(key) {
        return;
    }
    harness.endpoint = Some(key);
    emit_bootstrap_event(
        "match_over",
        &harness,
        &bootstrap,
        Some((&scores, &socket)),
        0,
        &format!("winner={}", player_text(*winner)),
    );
}

fn poll_browser_command(
    bootstrap: Option<Res<RoundBootstrap>>,
    mut socket: ResMut<CloudflareSocket>,
    mut room: ResMut<MatchmakingRoom>,
    mut harness: ResMut<TransitionHarness>,
    mut next: ResMut<NextState<GameState>>,
) {
    let command = take_transition_command();
    if command.is_empty() {
        return;
    }
    match command.as_str() {
        "rematch" if harness.scenario == TransitionScenario::Rematch => {
            let Some(bootstrap) = bootstrap else { return };
            let generation = socket.match_generation().unwrap_or(0).saturating_add(1);
            // Every peer uses the same bounded nonce. The call below is the real
            // production client API, not a synthetic protocol message.
            let nonce = format!(
                "{:032x}",
                bootstrap.match_id.0 ^ generation as u128 ^ 0x7265_6d61_7463_685f_7465_7374u128
            );
            let sent = socket.request_rematch(generation, &nonce);
            emit_bootstrap_event(
                "rematch_api",
                &harness,
                &bootstrap,
                None,
                0,
                if sent { "sent" } else { "rejected" },
            );
        }
        "requeue" if harness.scenario == TransitionScenario::Requeue => {
            let sent = socket.leave_lobby(true);
            if sent {
                harness.requeue_requested = true;
                room.private_code = None;
                room.preference = MatchPreference::Duel;
                socket.disconnect();
                next.set(GameState::Matchmaking);
            }
            transition_event(
                "requeue_api",
                harness.scenario,
                "ingame",
                bootstrap.as_deref(),
                bootstrap.as_ref().map_or(0, |value| value.epoch.0),
                bootstrap.as_ref().map_or(0, |value| value.round.0),
                0,
                0,
                "",
                "",
                "",
                if sent { "sent" } else { "rejected" },
            );
        }
        _ => transition_event(
            "command_rejected",
            harness.scenario,
            "unknown",
            bootstrap.as_deref(),
            0,
            0,
            0,
            0,
            "",
            "",
            "",
            "command unavailable in scenario",
        ),
    }
}

fn observe_fresh_queue(socket: Res<CloudflareSocket>, mut harness: ResMut<TransitionHarness>) {
    if !harness.requeue_requested || harness.fresh_queue_reported {
        return;
    }
    if matches!(
        socket.queue_status(),
        Some(QueueStatus::Searching | QueueStatus::HoldingForThird)
    ) {
        harness.fresh_queue_reported = true;
        transition_event(
            "fresh_queue",
            harness.scenario,
            "matchmaking",
            None,
            0,
            0,
            0,
            0,
            "",
            "",
            "",
            "new protocol-v4 queue connection",
        );
    }
}

fn emit_bootstrap_event(
    kind: &str,
    harness: &TransitionHarness,
    bootstrap: &RoundBootstrap,
    live: Option<(&Scores, &CloudflareSocket)>,
    frame: u32,
    detail: &str,
) {
    let roster = bootstrap
        .roster
        .iter()
        .map(|entry| player_text(entry.player_id))
        .collect::<Vec<_>>()
        .join(",");
    let score_source = live
        .map(|value| value.0.entries())
        .unwrap_or(&bootstrap.scores);
    let scores = score_source
        .iter()
        .map(|entry| format!("{}={}", player_text(entry.player_id), entry.score))
        .collect::<Vec<_>>()
        .join(",");
    let identity = live
        .and_then(|(_, socket)| socket.local_player_id())
        .map(player_text)
        .unwrap_or_default();
    let generation = live
        .and_then(|(_, socket)| socket.match_generation())
        .unwrap_or(0);
    transition_event(
        kind,
        harness.scenario,
        "ingame",
        Some(bootstrap),
        bootstrap.epoch.0,
        bootstrap.round.0,
        frame,
        generation,
        &format!("{:032x}", bootstrap.match_seed as u128),
        &identity,
        &roster,
        &format!("scores={scores};{detail}"),
    );
}

fn player_text(player: PlayerId) -> String {
    format!("{:032x}", player.0)
}

pub fn install(app: &mut App) {
    app.init_resource::<TransitionHarness>()
        .add_systems(Startup, configure_harness)
        .add_systems(
            GgrsSchedule,
            drive_transition_round
                .before(super::player::process_deaths)
                .ambiguous_with_all(),
        )
        .add_systems(
            OnEnter(GameState::MainMenu),
            auto_enter_transition_queue.after(super::networking::stop_cloudflare_socket),
        )
        .add_systems(OnEnter(GameState::Matchmaking), emit_matchmaking_event)
        .add_systems(OnEnter(GameState::InGame), emit_session_event)
        .add_systems(
            Update,
            (
                capture_rollover_pending.run_if(in_state(GameState::InGame)),
                observe_rollover_barrier.run_if(in_state(GameState::Matchmaking)),
                observe_match_endpoint.run_if(in_state(GameState::InGame)),
                poll_browser_command,
                observe_fresh_queue.run_if(in_state(GameState::Matchmaking)),
            )
                .chain(),
        );
}

#[cfg(target_arch = "wasm32")]
fn transition_event(
    kind: &str,
    scenario: TransitionScenario,
    state: &str,
    _bootstrap: Option<&RoundBootstrap>,
    epoch: u32,
    round: u32,
    frame: u32,
    generation: u32,
    seed: &str,
    identity: &str,
    roster: &str,
    detail: &str,
) {
    emit_transition_event(
        kind,
        scenario.name(),
        state,
        epoch,
        round,
        frame,
        generation,
        seed,
        identity,
        roster,
        detail,
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn transition_event(
    _kind: &str,
    _scenario: TransitionScenario,
    _state: &str,
    _bootstrap: Option<&RoundBootstrap>,
    _epoch: u32,
    _round: u32,
    _frame: u32,
    _generation: u32,
    _seed: &str,
    _identity: &str,
    _roster: &str,
    _detail: &str,
) {
}

#[cfg(not(target_arch = "wasm32"))]
fn transition_scenario() -> String {
    "rollover".into()
}
#[cfg(not(target_arch = "wasm32"))]
fn transition_room() -> String {
    "GTTRANSITION".into()
}
#[cfg(not(target_arch = "wasm32"))]
fn take_transition_command() -> String {
    String::new()
}
#[cfg(not(target_arch = "wasm32"))]
fn install_transition_bridge() {}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
const TRANSITION_SCENARIOS = new Set(["rollover", "active_disconnect", "rollover_disconnect", "reconnect", "rematch", "requeue", "changed_roster"]);
export function transition_scenario() {
  const value = new URL(location.href).searchParams.get("ghost_transition") || "rollover";
  return TRANSITION_SCENARIOS.has(value) ? value : "rollover";
}
export function transition_room() {
  return new URL(location.href).searchParams.get("ghost_room") || "GTTRANSITION";
}
export function install_transition_bridge() {
  window.__ghostTransitionEvents = [];
  window.__ghostTransitionCommand = "";
  window.__ghostTransitionApi = Object.freeze({
    rematch() { if (window.__ghostTransitionCommand) return false; window.__ghostTransitionCommand = "rematch"; return true; },
    requeue() { if (window.__ghostTransitionCommand) return false; window.__ghostTransitionCommand = "requeue"; return true; },
    capabilities() { return Object.freeze({ changedRosterBoundaryDeparture: false }); }
  });
}
export function take_transition_command() {
  const value = typeof window.__ghostTransitionCommand === "string" ? window.__ghostTransitionCommand : "";
  window.__ghostTransitionCommand = "";
  return value;
}
export function emit_transition_event(kind, scenario, state, epoch, round, frame, generation, seed, identity, roster, detail) {
  const text = value => typeof value === "string" ? value.slice(0, 512) : "";
  const rawDetail = text(detail);
  const parts = rawDetail.split(";");
  const scoreText = parts[0].startsWith("scores=") ? parts.shift().slice(7) : "";
  const scores = scoreText.split(",").filter(Boolean).slice(0, 8).flatMap(pair => {
    const [playerId, rawScore] = pair.split("=");
    const score = Number(rawScore);
    return /^[0-9a-f]{32}$/.test(playerId || "") && Number.isSafeInteger(score) && score >= 0 && score <= 0xffffffff
      ? [Object.freeze({ playerId, score })] : [];
  });
  const event = Object.freeze({
    schema: 1, kind: text(kind), scenario: text(scenario), state: text(state),
    epoch: Number(epoch) >>> 0, round: Number(round) >>> 0, frame: Number(frame) >>> 0,
    generation: Number(generation) >>> 0, seed: text(seed), identity: text(identity),
    roster: text(roster) ? text(roster).split(",").filter(Boolean).slice(0, 8) : [],
    scores: Object.freeze(scores), detail: parts.join(";")
  });
  (window.__ghostTransitionEvents ??= []).push(event);
  console.info(`GHOST_TRANSITION ${JSON.stringify(event)}`);
}
"#)]
extern "C" {
    fn transition_scenario() -> String;
    fn transition_room() -> String;
    fn install_transition_bridge();
    fn take_transition_command() -> String;
    fn emit_transition_event(
        kind: &str,
        scenario: &str,
        state: &str,
        epoch: u32,
        round: u32,
        frame: u32,
        generation: u32,
        seed: &str,
        identity: &str,
        roster: &str,
        detail: &str,
    );
}
