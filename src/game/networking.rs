use bevy::prelude::*;
use bevy_ggrs::{ggrs::PlayerType, *};
use ggrs::GGRSEvent;

use crate::{
    cloudflare_net::{CloudflareSocket, ConnectionState},
    game::{GameSeed, SoundIdSeed, SoundSeed},
};

use super::{toasts::Toasts, GameState, MAP_SIZE};

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

impl ggrs::Config for GgrsConfig {
    type Input = u8;
    type State = u8;
    type Address = u8;
}

pub fn start_cloudflare_socket(mut socket: ResMut<CloudflareSocket>) {
    #[cfg(not(feature = "dev_net"))]
    let room_name = format!(
        "battle-{}-{}-{}",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    );
    #[cfg(feature = "dev_net")]
    let room_name = format!(
        "devbattle-{}-{}-{}",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    );

    info!("connecting to Cloudflare matchmaking: {SIGNALING_URL}/{room_name}");
    socket.connect(SIGNALING_URL, &room_name);
}

pub fn stop_cloudflare_socket(mut socket: ResMut<CloudflareSocket>) {
    socket.disconnect();
}

pub fn cleanup_network_session(
    mut commands: Commands,
    mut rollback_state: ResMut<NextState<super::RollbackState>>,
    players: Query<Entity, With<super::components::Player>>,
    bullets: Query<Entity, With<super::components::Bullet>>,
    blocks: Query<Entity, With<super::components::MapBlock>>,
    effects: Query<Entity, With<super::components::AnimateOnce>>,
    pickups: Query<Entity, With<super::components::SpeedPickup>>,
) {
    commands.remove_resource::<Session<GgrsConfig>>();
    commands.remove_resource::<LocalPlayerHandle>();
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
        .add_player(PlayerType::Local, local_handle)
        .expect("adding local player")
        .add_player(PlayerType::Remote(remote_handle as u8), remote_handle)
        .expect("adding remote player");

    let ggrs_session = session_builder
        .start_p2p_session(socket.take_transport())
        .expect("starting ggrs p2p session");

    info!("started Cloudflare-signaled session {:#02x}", match_info.seed);
    commands.insert_resource(LocalPlayerHandle(local_handle));
    commands.insert_resource(bevy_ggrs::Session::P2P(ggrs_session));
    commands.insert_resource(GameSeed(match_info.seed));
    commands.insert_resource(SoundIdSeed((
        SoundSeed(match_info.seed.wrapping_add(1)),
        SoundSeed(match_info.seed.wrapping_add(2)),
    )));
    next_state.set(GameState::InGame);
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
                    toasts.error(format!("Peer {addr} disconnected.").into());
                    next_state.set(GameState::MainMenu);
                }
                event => info!("GGRS Event: {event:?}"),
            }
        }
    }
}
