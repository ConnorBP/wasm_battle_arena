use bevy::prelude::*;
use bevy_ggrs::{ggrs::PlayerType, *};
use ggrs::GGRSEvent;

use crate::{
    cloudflare_net::{CloudflareSocket, ConnectionState},
    game::{GameSeed, Scores, SoundIdSeed},
};

use super::{session::{PlayerId, RoundBootstrap}, toasts::Toasts, GameState, MAP_SIZE};

pub const ROLLBACK_FPS: usize = 60;

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

#[derive(Resource, Default)]
pub struct MatchmakingRoom {
    pub private_code: Option<String>,
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

pub fn start_cloudflare_socket(
    mut socket: ResMut<CloudflareSocket>,
    room: Res<MatchmakingRoom>,
) {
    let room_name = versioned_room_name(room.private_code.as_deref());
    info!("connecting to Cloudflare matchmaking");
    socket.connect(SIGNALING_URL, &room_name);
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
    commands.insert_resource(super::RoundProgress::default());
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

pub fn cleanup_network_session(
    mut commands: Commands,
    mut rollback_state: ResMut<NextState<super::RollbackState>>,
    players: Query<Entity, With<super::components::Player>>,
    bullets: Query<Entity, With<super::components::Bullet>>,
    blocks: Query<Entity, With<super::components::MapBlock>>,
    effects: Query<Entity, With<super::components::AnimateOnce>>,
    pickups: Query<Entity, Or<(
        With<super::components::SpeedPickup>,
        With<super::components::ShieldPickup>,
    )>>,
) {
    commands.remove_resource::<Session<GgrsConfig>>();
    commands.remove_resource::<LocalPlayerHandle>();
    commands.remove_resource::<RoundBootstrap>();
    commands.remove_resource::<super::map::Map<
        super::map::CellType,
        MAP_SIZE,
        MAP_SIZE,
    >>();
    commands.insert_resource(super::Scores::default());
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

pub fn wait_for_players(
    mut commands: Commands,
    mut socket: ResMut<CloudflareSocket>,
    mut next_state: ResMut<NextState<GameState>>,
    mut toasts: ResMut<Toasts>,
) {
    let match_info = match socket.state() {
        ConnectionState::Ready => socket.match_info().expect("ready match has assignment"),
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
            bootstrap.handle(local_handle).expect("local handle in roster"),
        )
        .expect("adding local player")
        .add_player(
            PlayerType::Remote(bootstrap.roster.iter().find(|entry| entry.handle == remote_handle).expect("remote player in roster").player_id),
            bootstrap.handle(remote_handle).expect("remote handle in roster"),
        )
        .expect("adding remote player");

    let ggrs_session = session_builder
        .start_p2p_session(socket.take_transport())
        .expect("starting ggrs p2p session");

    info!("started Cloudflare-signaled session {:#02x}", match_info.seed);
    commands.insert_resource(LocalPlayerHandle(local_handle));
    commands.insert_resource(SoundIdSeed::new(match_info.seed, bootstrap.roster.len()));
    commands.insert_resource(Scores::from_bootstrap(&bootstrap));
    commands.insert_resource(bootstrap);
    commands.insert_resource(bevy_ggrs::Session::P2P(ggrs_session));
    commands.insert_resource(GameSeed(match_info.seed));
    next_state.set(GameState::InGame);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_room_codes_are_canonical_and_bounded() {
        assert_eq!(sanitize_room_code(" ab-c_12! "), "ABC12");
        assert_eq!(sanitize_room_code("abcdefghijklmnopq"), "ABCDEFGHIJKLMNOP");
        assert_eq!(versioned_room_name(Some("ROOM42")), versioned_room_name(Some("room42")));
        assert!(versioned_room_name(Some("A")).len() <= 64);
    }
}

pub fn log_ggrs_events(
    mut session: ResMut<Session<GgrsConfig>>,
    mut toasts: ResMut<Toasts>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if let Session::P2P(session) = session.as_mut() {
        for event in session.events() {
            match event {
                GGRSEvent::Disconnected { addr } => {
                    toasts.error(format!("Peer {addr:?} disconnected.").into());
                    next_state.set(GameState::MainMenu);
                }
                event => info!("GGRS Event: {event:?}"),
            }
        }
    }
}
