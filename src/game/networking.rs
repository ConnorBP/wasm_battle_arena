use bevy::prelude::*;
use bevy_matchbox::{
    prelude::*,
    MatchboxSocket,
    // matchbox_socket::{WebRtcSocket, PeerId}
};
use bevy_ggrs::{*, ggrs::PlayerType};

use super::GameState;
// use matchbox_socket::{WebRtcSocket, PeerId};
pub struct GgrsConfig;

#[derive(Resource)]
pub struct LocalPlayerHandle(pub usize);

impl ggrs::Config for GgrsConfig {
    // 4-directions + fire fits easily in a single byte
    type Input = u8;
    type State = u8;
    type Address = PeerId;
}

pub fn start_matchbox_socket(mut commands: Commands) {
    let room_url = "ws://127.0.0.1:3536/battle?next=2";
    info!("connecting to matchbox server: {room_url}");
    commands.insert_resource(MatchboxSocket::new_ggrs(room_url));
}

pub fn wait_for_players(
    mut commands: Commands,
    mut socket: ResMut<MatchboxSocket<SingleChannel>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if socket.get_channel(0).is_err() {
        info!("failed to get socket");
        return; // we've already started
    }

    // Check for new connections
    socket.update_peers();
    let players = socket.players();
    let peer_count = socket.connected_peers().count();


    let min_players = 2;
    if players.len() < min_players {
        info!("not enough players {players:?} {peer_count}");
        return;
    }

    info!("All peers have joined, going in-game");

    // create ggrs p2p session
    let mut session_builder = ggrs::SessionBuilder::<GgrsConfig>::new()
        .with_num_players(min_players)
        .with_input_delay(2);

    for (i, player) in players.into_iter().enumerate() {
        if player == PlayerType::Local {
            commands.insert_resource(LocalPlayerHandle(i));
        }
        session_builder = session_builder
            .add_player(player, i)
            .expect("adding player to session");
    }

    // move the channel out of the socket (required because GGRS takes ownership of it)
    let channel = socket.take_channel(0).unwrap();

    // start the ggrs session
    let ggrs_session = session_builder
        .start_p2p_session(channel)
        .expect("starting ggrs p2p session");

    commands.insert_resource(bevy_ggrs::Session::P2P(ggrs_session));
    next_state.set(GameState::InGame);
}