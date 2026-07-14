use bevy::prelude::*;
use bevy_ggrs::{ggrs::PlayerType, *};
use ggrs::GGRSEvent;

use crate::{
    cloudflare_net::{CloudflareSocket, ConnectionState},
    game::{GameSeed, Scores, SoundIdSeed},
};

use super::{
    session::{
        GameMode, MatchId, MatchPreference, PlayerId, PlayerProfile, PlayerScore, RosterEntry,
        RoundBootstrap, RoundNumber, SessionEpoch,
    },
    toasts::Toasts,
    GameState, MAP_SIZE,
};

pub const ROLLBACK_FPS: usize = 60;

fn final_lobby_can_install(state: &ConnectionState, has_lobby_snapshot: bool) -> bool {
    *state == ConnectionState::Ready && has_lobby_snapshot
}

#[cfg(not(feature = "local"))]
const SIGNALING_URL: &str = match option_env!("GHOST_BATTLE_SIGNALING_URL") {
    Some(url) => url,
    None => "",
};
#[cfg(feature = "local")]
const SIGNALING_URL: &str = "ws://127.0.0.1:8787/match";

#[derive(Debug)]
pub struct GgrsConfig;

#[derive(Resource)]
pub struct LocalPlayerHandle(pub usize);

/// Non-rollback handoff state. The old GGRS world remains authoritative until
/// `OnExit(InGame)` has removed it, then the browser pending start is promoted.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EpochRollover {
    pub old: Option<(u32, u32)>,
    pub pending: Option<(u32, u32)>,
    pub promoted: bool,
    pub empty_update_frames: u8,
    pub empty_elapsed_ms: u32,
    pub install_ready: bool,
}

impl EpochRollover {
    fn begin(&mut self, old: (u32, u32), pending: (u32, u32)) {
        self.old = Some(old);
        self.pending = Some(pending);
        self.promoted = false;
        self.empty_update_frames = 0;
        self.empty_elapsed_ms = 0;
        self.install_ready = false;
    }

    pub fn active(&self) -> bool {
        self.old.is_some() && self.pending.is_some()
    }

    fn clear(&mut self) {
        *self = Self::default();
    }
}

#[derive(Resource)]
pub struct MatchmakingRoom {
    pub private_code: Option<String>,
    /// Public protocol-v4 selection. Distinct from the exact v3 game mode.
    pub preference: MatchPreference,
    /// Exact private-room mode/capacity; private rooms bypass protocol 4.
    pub private_mode: GameMode,
    pub private_capacity: u8,
}

impl Default for MatchmakingRoom {
    fn default() -> Self {
        Self {
            private_code: None,
            preference: MatchPreference::Any,
            private_mode: GameMode::Duel,
            private_capacity: 2,
        }
    }
}

pub fn sanitize_room_code(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .take(16)
        .map(|character| character.to_ascii_uppercase())
        .collect()
}

fn versioned_room_name(private_code: Option<&str>) -> String {
    #[cfg(not(feature = "dev_net"))]
    let prefix = "battle";
    #[cfg(feature = "dev_net")]
    let prefix = "devbattle";
    let base = format!(
        "{prefix}-{}-{}-{}",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    );
    match private_code {
        Some(code) => format!("{base}-private-{}", code.to_ascii_lowercase()),
        None => base,
    }
}

impl ggrs::Config for GgrsConfig {
    type Input = u8;
    type State = u8;
    type Address = PlayerId;
}

fn private_lobby_mode_capacity(room: &MatchmakingRoom) -> (u32, u32) {
    match room.private_mode {
        GameMode::Duel => (0, 2),
        GameMode::Deathmatch => (1, room.private_capacity.clamp(3, 8) as u32),
    }
}

pub fn start_cloudflare_socket(
    mut socket: ResMut<CloudflareSocket>,
    room: Res<MatchmakingRoom>,
    profile: Res<super::PendingPlayerProfile>,
) {
    info!("connecting to Cloudflare matchmaking");
    if socket.has_transport() {
        // Persistent protocol-3 control already received the next immutable
        // start; do not open a second identity/control socket.
        return;
    }
    if room.private_code.is_none() {
        socket.connect_queue(
            SIGNALING_URL,
            &versioned_room_name(None),
            room.preference.protocol_name(),
            &profile.name,
            profile.palette_id,
            profile.cosmetic_id,
        );
        return;
    }

    // Private rooms remain direct, exact protocol 3 and never enter the
    // flexible public queue.
    let room_name = versioned_room_name(room.private_code.as_deref());
    let (mode, capacity) = private_lobby_mode_capacity(&room);
    socket.connect_lobby(
        SIGNALING_URL,
        &format!("v3-{room_name}-{mode}-{capacity}"),
        mode,
        capacity,
        &profile.name,
        profile.palette_id,
        profile.cosmetic_id,
    );
}

#[cfg(feature = "sync_test")]
pub fn start_sync_test(
    mut commands: Commands,
    mut next_game_state: ResMut<NextState<GameState>>,
    mut next_menu_state: ResMut<NextState<super::MenuState>>,
) {
    const SYNC_TEST_SEED: u64 = 0x5a17_cafe_d00d_beef;
    info!("starting local sync-test session");
    let bootstrap = RoundBootstrap::duel(SYNC_TEST_SEED);
    let session = ggrs::SessionBuilder::<GgrsConfig>::new()
        .with_num_players(2)
        .with_check_distance(2)
        .with_max_prediction_window(40)
        .with_input_delay(0)
        .add_player(PlayerType::Local, 0)
        .expect("adding sync-test player 0")
        .add_player(PlayerType::Local, 1)
        .expect("adding sync-test player 1")
        .start_synctest_session()
        .expect("starting sync-test session");

    commands.insert_resource(LocalPlayerHandle(0));
    commands.insert_resource(SoundIdSeed::new(SYNC_TEST_SEED, 2));
    commands.insert_resource(Scores::from_bootstrap(&bootstrap));
    commands.insert_resource(super::MatchFlow::Playing);
    commands.insert_resource(super::RoundProgress::default());
    commands.insert_resource(super::ReportedOutcome::default());
    commands.insert_resource(bootstrap);
    commands.insert_resource(Session::SyncTest(session));
    commands.insert_resource(GameSeed(SYNC_TEST_SEED));
    next_menu_state.set(super::MenuState::Main);
    next_game_state.set(GameState::InGame);
    info!("local sync-test session ready");
}

pub fn stop_cloudflare_socket(
    mut socket: ResMut<CloudflareSocket>,
    mut room: ResMut<MatchmakingRoom>,
) {
    socket.disconnect();
    room.private_code = None;
}

pub fn stop_legacy_matchmaking_socket(_socket: Res<CloudflareSocket>) {
    // Legacy ownership moved into the GGRS adapter; lobby control stays in the
    // resource. A zero-id resource is already inert.
}

pub fn cleanup_network_session(
    mut commands: Commands,
    socket: Res<CloudflareSocket>,
    rollover: Res<EpochRollover>,
    mut rollback_state: ResMut<NextState<super::RollbackState>>,
    players: Query<Entity, With<super::components::Player>>,
    bullets: Query<Entity, With<super::components::Bullet>>,
    blocks: Query<Entity, With<super::components::MapBlock>>,
    effects: Query<Entity, With<super::components::AnimateOnce>>,
    pickups: Query<
        Entity,
        Or<(
            With<super::components::SpeedPickup>,
            With<super::components::ShieldPickup>,
        )>,
    >,
) {
    // Normal exits retire the identified active round here. During rollover,
    // promotion happens only after these deferred removals/despawns have been
    // applied by the chained OnExit systems.
    if let (Some(epoch), Some(round)) = (socket.lobby_epoch(), socket.lobby_round()) {
        if !rollover.active() {
            socket.close_epoch_transport(epoch, round);
        }
    }
    commands.remove_resource::<Session<GgrsConfig>>();
    commands.remove_resource::<LocalPlayerHandle>();
    commands.remove_resource::<RoundBootstrap>();
    commands.remove_resource::<super::map::Map<super::map::CellType, MAP_SIZE, MAP_SIZE>>();
    // Do not synthesize/reset scores during rollover. The promoted immutable
    // start carries the server-authoritative committed score snapshot.
    if !rollover.active() {
        commands.insert_resource(super::Scores::default());
    }
    commands.insert_resource(super::MatchFlow::Playing);
    commands.insert_resource(super::RematchFlow::Idle);
    commands.insert_resource(super::RoundEndTimer::default());
    commands.insert_resource(super::ggrs_framecount::GGFrameCount::default());
    rollback_state.set(super::RollbackState::PreRound);
    for entity in players
        .iter()
        .chain(bullets.iter())
        .chain(blocks.iter())
        .chain(effects.iter())
        .chain(pickups.iter())
    {
        commands.entity(entity).despawn_recursive();
    }
}

/// Phase two of rollover. Scheduled after `apply_deferred` on InGame exit so
/// no old Session or gameplay entity can observe the new packet channels.
pub fn promote_pending_rollover(
    mut socket: ResMut<CloudflareSocket>,
    mut rollover: ResMut<EpochRollover>,
    mut next_state: ResMut<NextState<GameState>>,
    mut toasts: ResMut<Toasts>,
) {
    let (Some(old), Some(pending)) = (rollover.old, rollover.pending) else {
        return;
    };
    let promoted =
        socket.pending_epoch_round() == Some(pending) && socket.promote_pending(old.0, old.1);
    if promoted {
        rollover.promoted = true;
    } else {
        rollover.clear();
        toasts.error("Could not promote the next lobby round.".into());
        socket.disconnect();
        next_state.set(GameState::MainMenu);
    }
}

/// Leaves bevy_ggrs without a Session long enough for its private GgrsStage to
/// execute the no-session reset path before a frame-zero replacement session.
fn advance_reset_barrier_state(rollover: &mut EpochRollover, delta_ms: u32) {
    rollover.empty_update_frames = rollover.empty_update_frames.saturating_add(1);
    rollover.empty_elapsed_ms = rollover.empty_elapsed_ms.saturating_add(delta_ms);
    rollover.install_ready = rollover.empty_update_frames >= 4 && rollover.empty_elapsed_ms >= 75;
}

pub fn advance_ggrs_reset_barrier(
    time: Res<Time>,
    session: Option<Res<Session<GgrsConfig>>>,
    mut rollover: ResMut<EpochRollover>,
) {
    if !rollover.active() || !rollover.promoted || rollover.install_ready || session.is_some() {
        return;
    }
    let delta_ms = time.delta().as_millis().min(u32::MAX as u128) as u32;
    advance_reset_barrier_state(&mut rollover, delta_ms);
}

pub fn wait_for_players(
    mut commands: Commands,
    rollover: Option<Res<EpochRollover>>,
    mut socket: ResMut<CloudflareSocket>,
    mut next_state: ResMut<NextState<GameState>>,
    mut toasts: ResMut<Toasts>,
) {
    if rollover.is_some_and(|rollover| rollover.active() && !rollover.install_ready) {
        return;
    }
    let state = socket.state();
    // Coordinator assignment is not readiness. The browser closes v4, opens
    // the exact signed v3 room, receives welcome (and TURN), then completes
    // WebRTC. Only this final immutable lobby snapshot may install GGRS and
    // atomically leave Matchmaking/practice.
    let lobby = socket.lobby_match_info();
    if final_lobby_can_install(&state, lobby.is_some()) {
        return start_lobby_session(
            commands,
            socket,
            next_state,
            toasts,
            lobby.expect("final readiness requires immutable lobby snapshot"),
        );
    }
    let match_info = match state {
        ConnectionState::Ready => {
            let Some(info) = socket.match_info() else {
                toasts.error("Invalid final matchmaking assignment.".into());
                next_state.set(GameState::MainMenu);
                return;
            };
            info
        }
        ConnectionState::Failed(error) => {
            toasts.error(error.into());
            next_state.set(GameState::MainMenu);
            return;
        }
        ConnectionState::Disconnected | ConnectionState::Connecting => return,
    };

    let bootstrap = RoundBootstrap::duel(match_info.seed);
    let (local_handle, remote_handle) = match match_info.player_index {
        0 => (0, 1),
        1 => (1, 0),
        _ => {
            toasts.error("Invalid player assignment.".into());
            next_state.set(GameState::MainMenu);
            return;
        }
    };

    #[cfg(feature = "no_delay")]
    let input_delay = 0;
    #[cfg(not(feature = "no_delay"))]
    let input_delay = 2;

    let session_builder = ggrs::SessionBuilder::<GgrsConfig>::new()
        .with_fps(ROLLBACK_FPS)
        .unwrap()
        .with_num_players(2)
        .with_input_delay(input_delay)
        .with_max_prediction_window(40)
        .with_max_frames_behind(42)
        .unwrap()
        .add_player(
            PlayerType::Local,
            bootstrap
                .handle(local_handle)
                .expect("local handle in roster"),
        )
        .expect("adding local player")
        .add_player(
            PlayerType::Remote(
                bootstrap
                    .roster
                    .iter()
                    .find(|entry| entry.handle == remote_handle)
                    .expect("remote player in roster")
                    .player_id,
            ),
            bootstrap
                .handle(remote_handle)
                .expect("remote handle in roster"),
        )
        .expect("adding remote player");

    let ggrs_session = session_builder
        .start_p2p_session(socket.take_transport())
        .expect("starting ggrs p2p session");

    info!(
        "started Cloudflare-signaled session {:#02x}",
        match_info.seed
    );
    commands.insert_resource(LocalPlayerHandle(local_handle));
    commands.insert_resource(SoundIdSeed::new(match_info.seed, bootstrap.roster.len()));
    commands.insert_resource(Scores::from_bootstrap(&bootstrap));
    commands.insert_resource(super::MatchFlow::Playing);
    commands.insert_resource(super::RematchFlow::Idle);
    commands.insert_resource(super::RoundProgress::default());
    commands.insert_resource(super::ReportedOutcome::default());
    commands.insert_resource(bootstrap);
    commands.insert_resource(bevy_ggrs::Session::P2P(ggrs_session));
    commands.insert_resource(GameSeed(match_info.seed));
    next_state.set(GameState::InGame);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinator_assignment_is_not_final_lobby_readiness() {
        assert!(!final_lobby_can_install(
            &ConnectionState::Connecting,
            false
        ));
        assert!(!final_lobby_can_install(&ConnectionState::Connecting, true));
        assert!(!final_lobby_can_install(&ConnectionState::Ready, false));
        assert!(final_lobby_can_install(&ConnectionState::Ready, true));
    }

    #[test]
    fn public_and_private_transport_policy_is_unambiguous() {
        let public = MatchmakingRoom::default();
        assert!(public.private_code.is_none());
        assert_eq!(public.preference, MatchPreference::Any);

        let private = MatchmakingRoom {
            private_code: Some("ROOM".into()),
            private_mode: GameMode::Deathmatch,
            private_capacity: 5,
            ..Default::default()
        };
        assert!(private.private_code.is_some());
        assert_eq!(private.private_mode, GameMode::Deathmatch);
        assert_eq!(private.private_capacity, 5);
    }

    #[test]
    fn private_lgs_supports_every_exact_capacity_from_three_through_eight() {
        for capacity in 3..=8 {
            let room = MatchmakingRoom {
                private_code: Some("ROOM".into()),
                private_mode: GameMode::Deathmatch,
                private_capacity: capacity,
                ..Default::default()
            };
            let (mode, exact_capacity) = private_lobby_mode_capacity(&room);
            assert_eq!((mode, exact_capacity), (1, capacity as u32));
            let room_name = format!(
                "v3-{}-{mode}-{exact_capacity}",
                versioned_room_name(room.private_code.as_deref())
            );
            assert!(room_name.ends_with(&format!("-1-{capacity}")));
        }
        let duel = MatchmakingRoom::default();
        assert_eq!(private_lobby_mode_capacity(&duel), (0, 2));
    }

    #[test]
    fn two_phase_rollover_state_is_explicit_and_idempotent() {
        let mut rollover = EpochRollover::default();
        assert!(!rollover.active());
        rollover.begin((2, 8), (3, 0));
        assert!(rollover.active());
        assert_eq!(rollover.old, Some((2, 8)));
        assert_eq!(rollover.pending, Some((3, 0)));
        rollover.clear();
        assert!(!rollover.active());
    }

    #[test]
    fn ggrs_reset_barrier_requires_frames_and_elapsed_time() {
        let mut rollover = EpochRollover::default();
        rollover.begin((1, 0), (1, 1));
        rollover.promoted = true;
        for _ in 0..3 {
            advance_reset_barrier_state(&mut rollover, 25);
            assert!(!rollover.install_ready);
        }
        advance_reset_barrier_state(&mut rollover, 0);
        assert!(rollover.install_ready);

        let mut slow = EpochRollover::default();
        slow.begin((2, 0), (2, 1));
        slow.promoted = true;
        for _ in 0..10 {
            advance_reset_barrier_state(&mut slow, 7);
        }
        assert!(!slow.install_ready);
        advance_reset_barrier_state(&mut slow, 5);
        assert!(slow.install_ready);
    }

    #[test]
    fn source_contract_does_not_close_transport_in_epoch_watcher() {
        let source = include_str!("networking.rs");
        let watcher = source
            .rsplit("pub fn watch_lobby_epoch")
            .next()
            .and_then(|tail| tail.split("pub fn poll_lobby_control").next())
            .expect("watcher source");
        assert!(watcher.contains("pending_epoch()"));
        assert!(watcher.contains("pending_round()"));
        assert!(watcher.contains("rollover.begin"));
        assert!(!watcher.contains("close_epoch_transport"));
        assert!(!watcher.contains("remove_resource::<Session"));
    }

    #[test]
    fn private_room_codes_are_canonical_and_bounded() {
        assert_eq!(sanitize_room_code(" ab-c_12! "), "ABC12");
        assert_eq!(sanitize_room_code("abcdefghijklmnopq"), "ABCDEFGHIJKLMNOP");
        assert_eq!(
            versioned_room_name(Some("ROOM42")),
            versioned_room_name(Some("room42"))
        );
        assert!(versioned_room_name(Some("A")).len() <= 64);
    }
}

fn start_lobby_session(
    mut commands: Commands,
    mut socket: ResMut<CloudflareSocket>,
    mut next_state: ResMut<NextState<GameState>>,
    mut toasts: ResMut<Toasts>,
    info: crate::cloudflare_net::LobbyMatchInfo,
) {
    let valid = (info.mode == 0 && info.roster.len() == 2)
        || (info.mode == 1 && (3..=super::session::MAX_LOBBY_PLAYERS).contains(&info.roster.len()));
    let mode = if valid {
        if info.mode == 0 {
            GameMode::Duel
        } else {
            GameMode::Deathmatch
        }
    } else {
        toasts
            .error("Lobby assignment does not match Duel (2) or Last Ghost Standing (3–8).".into());
        next_state.set(GameState::MainMenu);
        return;
    };
    let roster: Vec<_> = info
        .roster
        .iter()
        .map(|(player_id, handle)| RosterEntry {
            player_id: *player_id,
            handle: *handle,
        })
        .collect();
    let profiles = roster
        .iter()
        .map(|entry| PlayerProfile {
            player_id: entry.player_id,
            name: format!("Player {}", entry.handle + 1),
            palette_id: entry.handle as u8,
            cosmetic_id: 0,
        })
        .collect();
    if info.scores.len() != roster.len()
        || roster
            .iter()
            .any(|entry| !info.scores.iter().any(|score| score.0 == entry.player_id))
    {
        toasts.error("Invalid authoritative lobby scores.".into());
        next_state.set(GameState::MainMenu);
        return;
    }
    let scores = roster
        .iter()
        .map(|entry| {
            let score = info
                .scores
                .iter()
                .find(|score| score.0 == entry.player_id)
                .expect("score coverage validated");
            PlayerScore {
                player_id: score.0,
                score: score.1,
            }
        })
        .collect();
    let Ok(bootstrap) = RoundBootstrap::new(
        super::session::LOBBY_PROTOCOL_VERSION,
        MatchId(info.match_id),
        info.seed,
        SessionEpoch(info.epoch),
        RoundNumber(info.round),
        mode,
        roster,
        profiles,
        scores,
    ) else {
        toasts.error("Invalid lobby assignment.".into());
        next_state.set(GameState::MainMenu);
        return;
    };
    let Some(local) = bootstrap
        .roster
        .iter()
        .find(|entry| entry.player_id == info.local_player)
    else {
        toasts.error("Local player missing from lobby roster.".into());
        next_state.set(GameState::MainMenu);
        return;
    };
    let mut builder = ggrs::SessionBuilder::<GgrsConfig>::new()
        .with_fps(ROLLBACK_FPS)
        .unwrap()
        .with_num_players(bootstrap.roster.len())
        .with_input_delay(if cfg!(feature = "no_delay") { 0 } else { 2 })
        .with_max_prediction_window(40)
        .with_max_frames_behind(42)
        .unwrap();
    for entry in &bootstrap.roster {
        let player_type = if entry.player_id == info.local_player {
            PlayerType::Local
        } else {
            PlayerType::Remote(entry.player_id)
        };
        let Ok(next) = builder.add_player(player_type, entry.handle) else {
            toasts.error("Invalid lobby roster.".into());
            next_state.set(GameState::MainMenu);
            return;
        };
        builder = next;
    }
    // Reset rollback resources before the frame-zero replacement is visible.
    commands.insert_resource(super::ggrs_framecount::GGFrameCount::default());
    commands.insert_resource(super::RoundProgress::default());
    commands.insert_resource(super::ReportedOutcome::default());
    commands.insert_resource(super::RoundEndTimer::default());
    socket.set_epoch_round(info.epoch, info.round);
    let Ok(session) = builder.start_p2p_session(socket.take_transport()) else {
        toasts.error("Could not start lobby session.".into());
        next_state.set(GameState::MainMenu);
        return;
    };
    commands.insert_resource(LocalPlayerHandle(local.handle));
    commands.insert_resource(SoundIdSeed::new(info.seed, bootstrap.roster.len()));
    commands.insert_resource(Scores::from_bootstrap(&bootstrap));
    commands.insert_resource(super::MatchFlow::Playing);
    commands.insert_resource(super::RematchFlow::Idle);
    commands.insert_resource(super::RoundProgress::default());
    commands.insert_resource(super::ReportedOutcome::default());
    commands.insert_resource(bootstrap);
    commands.insert_resource(Session::P2P(session));
    commands.insert_resource(GameSeed(info.seed));
    commands.insert_resource(EpochRollover::default());
    next_state.set(GameState::InGame);
}

pub fn report_confirmed_outcome(
    session: Res<Session<GgrsConfig>>,
    socket: Res<CloudflareSocket>,
    bootstrap: Option<Res<RoundBootstrap>>,
    progress: Res<super::RoundProgress>,
    mut reported: ResMut<super::ReportedOutcome>,
) {
    let (Some(bootstrap), Some(outcome), Some(frame)) = (
        bootstrap,
        progress.resolved.as_ref(),
        progress.resolved_frame,
    ) else {
        return;
    };
    let Session::P2P(p2p) = session.as_ref() else {
        return;
    };
    if p2p.confirmed_frame() < frame as i32 {
        return;
    }
    let fingerprint = (bootstrap.epoch.0, bootstrap.round.0, frame);
    if reported.0 == Some(fingerprint) {
        return;
    }
    if socket.report_round(
        bootstrap.epoch.0,
        bootstrap.round.0,
        outcome.point_winners(),
    ) {
        reported.0 = Some(fingerprint);
    }
}

pub fn watch_lobby_epoch(
    socket: Res<CloudflareSocket>,
    bootstrap: Option<Res<RoundBootstrap>>,
    progress: Res<super::RoundProgress>,
    mut rollover: ResMut<EpochRollover>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    let (Some(current), Some(pending_epoch), Some(pending_round)) =
        (bootstrap, socket.pending_epoch(), socket.pending_round())
    else {
        return;
    };
    let pending = (pending_epoch, pending_round);
    let active = (current.epoch.0, current.round.0);
    if pending <= active || (pending.0 == active.0 && progress.resolved.is_none()) {
        return;
    }
    // Phase one only records intent. Active peers, channels, Session and game
    // world remain untouched until the InGame exit cleanup barrier.
    rollover.begin(active, pending);
    next_state.set(GameState::Matchmaking);
}

pub fn poll_lobby_control(
    mut flow: ResMut<super::RematchFlow>,
    socket: Res<CloudflareSocket>,
    mut next_state: ResMut<NextState<GameState>>,
    mut toasts: ResMut<Toasts>,
) {
    if socket.is_waiting_in_queue() {
        return;
    }
    use crate::cloudflare_net::LobbyControlEvent;
    while let Some(event) = socket.poll_control() {
        match event {
            LobbyControlEvent::RematchPending {
                generation,
                nonce,
                deadline_ms,
                accepted,
                required,
            } => {
                *flow = super::RematchFlow::Pending {
                    generation,
                    nonce,
                    deadline_ms,
                    accepted,
                    required,
                };
            }
            LobbyControlEvent::RematchAccepted { generation, nonce } => {
                if matches!(&*flow, super::RematchFlow::Pending { generation: pending_generation, nonce: pending_nonce, .. }
                    if *pending_generation == generation && *pending_nonce == nonce)
                {
                    *flow = super::RematchFlow::Idle;
                }
                // The following immutable start has a larger epoch and causes
                // watch_lobby_epoch to tear down/recreate GGRS.
            }
            LobbyControlEvent::ReturnToMenu { reason } => {
                *flow = super::RematchFlow::Idle;
                toasts.error(format!("Match ended: {reason}").into());
                next_state.set(GameState::MainMenu);
            }
            LobbyControlEvent::Ignored => {}
        }
    }
}

pub fn update_network_telemetry(
    socket: Res<CloudflareSocket>,
    mut telemetry: ResMut<crate::cloudflare_net::NetworkTelemetry>,
) {
    *telemetry = socket.telemetry();
}

pub fn log_ggrs_events(
    mut session: ResMut<Session<GgrsConfig>>,
    socket: Res<CloudflareSocket>,
    rollover: Res<EpochRollover>,
    bootstrap: Option<Res<RoundBootstrap>>,
    mut toasts: ResMut<Toasts>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if let Session::P2P(session) = session.as_mut() {
        for event in session.events() {
            match event {
                GGRSEvent::Disconnected { addr } => {
                    if rollover.active() {
                        info!("ignoring expected old-round disconnect during epoch rollover: {addr:?}");
                        continue;
                    }
                    // Outside a deliberate rollover, a disconnect is fatal for
                    // the whole immutable roster.
                    socket.leave_lobby(false);
                    let size = bootstrap.as_ref().map(|b| b.roster.len()).unwrap_or(2);
                    toasts.error(format!("Peer {addr:?} disconnected; returning all {size} roster players to menu.").into());
                    next_state.set(GameState::MainMenu);
                }
                GGRSEvent::NetworkInterrupted { addr, .. } => {
                    if rollover.active() {
                        info!("ignoring expected old-round interruption during epoch rollover: {addr:?}");
                        continue;
                    }
                    socket.leave_lobby(false);
                    toasts.error(
                        format!("Peer {addr:?} connection interrupted; returning to menu.").into(),
                    );
                    next_state.set(GameState::MainMenu);
                }
                event => info!("GGRS Event: {event:?}"),
            }
        }
    }
}
