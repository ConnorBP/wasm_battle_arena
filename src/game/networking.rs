use bevy::prelude::*;
use bevy_matchbox::{
    prelude::*,
    MatchboxSocket, matchbox_socket::WebRtcChannel,
    // matchbox_socket::{WebRtcSocket, PeerId}
};
use bevy_ggrs::{*, ggrs::PlayerType};
use ggrs::GGRSEvent;

use crate::game::{GameSeed, SoundIdSeed, SoundSeed};

use super::{GameState, toasts::Toasts};
// use matchbox_socket::{WebRtcSocket, PeerId};


pub const ROLLBACK_FPS: usize = 60;

#[derive(Debug)]
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
    // let secure = crate::interface::is_secure();
    // prevent version clashing in lobies from causing non determinism
    #[cfg(not(feature="dev_net"))]
    let room_name = format!(
        "battle-{}-{}-{}",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    );
    #[cfg(feature="dev_net")]
    let room_name = format!(
        "devbattle-{}-{}-{}",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    );
    #[cfg(not(feature="local"))]
    let room_url = format!("wss://matchbox-secure.segfault.site/{room_name}?next=2");
    // let room_url = if secure {
    //     info!("Website is secure, connecting with Secure Web Socket.");
    //     format!("wss://matchbox-secure.segfault.site/{room_name}?next=2")
    // } else {
    //     format!("ws://matchbox.segfault.site:3536/{room_name}?next=2")
    // };
    
    #[cfg(feature="local")]
    let room_url = "ws://127.0.0.1:3536/battle?next=2";

    // let room_url = "ws://matchbox.segfault.site:3536/battle?next=2";
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

    // get local id when assigned to our socket or return from func
    let local_id = match socket.id() {
        Some(id) => id.0.as_u128(),
        _ => return,
    };

    // Check for new connections and
    // xor local id and peer ids together to get session hash
    // should be the same on every client because xor is addative (unordered)
    let session_hash = {
        let mut out_id = u128::MAX;
        out_id ^= local_id;
        for (id,_) in socket.update_peers().iter() {
            out_id ^= id.0.as_u128();
        }

        //shrink down to 64 bits
        (out_id & u128::MAX >> 8) as u64  ^ (out_id >> 8) as u64
    };

    let players = socket.players();

    let min_players = 2;
    if players.len() < min_players {
        // info!("not enough players {players:?} {peer_count}");
        return;
    }



    info!("All peers have joined, going in-game");

    #[cfg(feature="no_delay")]
    let input_delay = 0;
    #[cfg(not(feature="no_delay"))]
    let input_delay = 2;

    // create ggrs p2p session
    let mut session_builder = ggrs::SessionBuilder::<GgrsConfig>::new()
        .with_fps(ROLLBACK_FPS).unwrap()
        .with_num_players(min_players)
        .with_input_delay(input_delay)
        .with_max_prediction_window(40)
        .with_max_frames_behind(42).unwrap();
        

    for (i, player) in players.into_iter().enumerate() {
        if player == PlayerType::Local {
            commands.insert_resource(LocalPlayerHandle(i));
        }
        session_builder = session_builder
            .add_player(player, i)
            .expect("adding player to session");
    }

    info!("taking channel");

    // move the channel out of the socket (required because GGRS takes ownership of it)
    let channel = socket.take_channel(0).unwrap();

    info!("Trying to start ggrs session");

    // start the ggrs session
    let ggrs_session = session_builder
        .start_p2p_session(channel)
        .expect("starting ggrs p2p session");

    // sometimes while pairing peers this is never reached
    info!("Started new session {session_hash:#02x}");

    commands.insert_resource(bevy_ggrs::Session::P2P(ggrs_session));
    // insert session hash to seed our psudo rng
    commands.insert_resource(GameSeed(session_hash));
    // insert sound event id seeds for player 1 and two
    commands.insert_resource(SoundIdSeed((SoundSeed(session_hash.wrapping_add(1)),SoundSeed(session_hash.wrapping_add(2)))));
    next_state.set(GameState::InGame);
}

pub fn log_ggrs_events(
    mut session: ResMut<Session<GgrsConfig>>,
    mut toasts: ResMut<Toasts>,
) {
    match session.as_mut() {
        Session::P2P(s) => {
            for event in s.events() {
                match event {
                    GGRSEvent::Disconnected { addr } => {
                        toasts.error(format!("Peer {addr} disconnected.").into());
                    },
                    x=> {
                        info!("GGRS Event: {x:?}");
                    }
                }
            }
        }
        _ => {},//panic!("This example focuses on p2p."),
    }
}