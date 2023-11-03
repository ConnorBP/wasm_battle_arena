use std::time::Duration;

use bevy::{prelude::*, render::camera::ScalingMode};
use bevy_ggrs::{GgrsAppExtension, GgrsPlugin, GgrsSchedule};
use bevy_asset_loader::prelude::*;
use bevy_roll_safe::prelude::*;
use bevy_egui::EguiPlugin;

mod components;
mod player;
mod input;
mod networking;
mod textures;
mod gui;

use components::*;
use player::*;
use input::*;
use networking::*;
use textures::*;
use gui::*;

const MAP_SIZE: u32 = 41;
const GRID_WIDTH: f32 = 0.05;

#[derive(States, Clone, Eq, PartialEq, Debug, Hash, Default)]
pub enum GameState {
    #[default]
    AssetLoading,
    Matchmaking,
    InGame,
}

#[derive(States, Clone, Eq, PartialEq, Debug, Hash, Default, Reflect)]
pub enum RollbackState {
    /// When the characters running and gunning
    #[default]
    InRound,
    /// When one character is dead, and we're transitioning to the next round
    RoundEnd,
}

#[derive(Resource, Reflect, Deref, DerefMut)]
#[reflect(Resource)]
pub struct RoundEndTimer(Timer);

impl Default for RoundEndTimer {
    fn default() -> Self {
        RoundEndTimer(Timer::from_seconds(1.0, TimerMode::Repeating))
    }
}

#[derive(Resource, Reflect, Default, Debug)]
#[reflect(Resource)]
pub struct Scores(u32, u32);

#[derive(Resource, Reflect, Default, Debug)]
#[reflect(Resource)]
pub struct GameSeed(u64);

pub fn run() {
    App::new()
    .add_state::<GameState>()
    .add_loading_state(
        LoadingState::new(GameState::AssetLoading).continue_to_state(GameState::Matchmaking)
    )
    .add_collection_to_loading_state::<_, ImageAssets>(GameState::AssetLoading)
    .add_plugins((
        DefaultPlugins
        .set(WindowPlugin {
            primary_window: Some(Window {
                // fill the entire browser window
                fit_canvas_to_parent: true,
                // don't hijack keyboard shortcuts like F5 CTRL+R
                prevent_default_event_handling: false,
                ..default()
            }),
            ..default()
        })
        .set(ImagePlugin::default_nearest()),// set pixel art render mode
        EguiPlugin,
    ))
    .add_ggrs_plugin(
        GgrsPlugin::<networking::GgrsConfig>::new()
            .with_input_system(input)
            .register_roll_state::<RollbackState>()
            .register_rollback_resource::<RoundEndTimer>()
            .register_rollback_resource::<Scores>()
            .register_rollback_resource::<GameSeed>()
            .register_rollback_component::<Transform>()
            .register_rollback_component::<BulletReady>()
            .register_rollback_component::<MoveDir>()
            .register_rollback_component::<LookTowardsParentMove>()
            .register_rollback_component::<MarkedForDeath>()
    )
    .insert_resource(ClearColor(Color::rgb(0.43,0.43,0.63)))
    .init_resource::<RoundEndTimer>()
    .init_resource::<Scores>()
    .add_systems(
        OnEnter(GameState::Matchmaking),
        (setup, start_matchbox_socket),
    )
    .add_systems(
        Update,
        (
            wait_for_players.run_if(in_state(GameState::Matchmaking)),
            (player_look, camera_follow, update_score_ui).run_if(in_state(GameState::InGame)),
            update_matchmaking_ui.run_if(in_state(GameState::Matchmaking)),
            update_respawn_ui.run_if(in_state(RollbackState::RoundEnd)),
        ),
    )
    .add_roll_state::<RollbackState>(GgrsSchedule)
    .add_systems(OnEnter(RollbackState::InRound), spawn_players)
    .add_systems(
        GgrsSchedule,
        (
            move_players,
            reload_bullet,
            fire_bullets.after(move_players).after(reload_bullet),
            move_bullets.after(fire_bullets),
            kill_players.after(move_bullets).after(move_players),
            process_deaths.after(kill_players),
        )
            .after(apply_state_transition::<RollbackState>)
            .distributive_run_if(in_state(RollbackState::InRound)),
    )
    .add_systems(
        GgrsSchedule,
        round_end_timeout
            .ambiguous_with(process_deaths)
            .distributive_run_if(in_state(RollbackState::RoundEnd))
            .after(apply_state_transition::<RollbackState>),
    )
    .run();
}

fn setup(mut commands: Commands) {
    let mut camera_bundle = Camera2dBundle::default();
    camera_bundle.projection.scaling_mode = ScalingMode::FixedVertical(10.);
    commands.spawn(camera_bundle);

    // Horizontal lines
    for i in 0..=MAP_SIZE {
        commands.spawn(SpriteBundle {
            transform: Transform::from_translation(Vec3::new(
                0.,
                i as f32 - MAP_SIZE as f32 / 2.,
                0.,
            )),
            sprite: Sprite {
                color: Color::rgb(0.27, 0.27, 0.27),
                custom_size: Some(Vec2::new(MAP_SIZE as f32, GRID_WIDTH)),
                ..default()
            },
            ..default()
        });
    }

    // Vertical lines
    for i in 0..=MAP_SIZE {
        commands.spawn(SpriteBundle {
            transform: Transform::from_translation(Vec3::new(
                i as f32 - MAP_SIZE as f32 / 2.,
                0.,
                0.,
            )),
            sprite: Sprite {
                color: Color::rgb(0.27, 0.27, 0.27),
                custom_size: Some(Vec2::new(GRID_WIDTH, MAP_SIZE as f32)),
                ..default()
            },
            ..default()
        });
    }
}

fn camera_follow(
    player_handle: Option<Res<LocalPlayerHandle>>,
    players: Query<(&Player, &Transform)>,
    mut cameras: Query<&mut Transform, (With<Camera>,Without<Player>)>,
) {
    let player_handle = match player_handle {
        Some(handle) => handle.0,
        None => return, // Session hasn't started yet
    };

    for (player, player_transform) in &players {
        if player.handle != player_handle {
            continue;
        }

        let pos = player_transform.translation;

        for mut transform in &mut cameras {
            transform.translation.x = pos.x;
            transform.translation.y = pos.y;
        }
    }
}

fn round_end_timeout(
    mut timer: ResMut<RoundEndTimer>,
    mut state: ResMut<NextState<RollbackState>>
) {
    timer.tick(Duration::from_secs_f64(1. / 60.));// tick at the ggrs network framerate of 60 fps

    if timer.just_finished() {
        state.set(RollbackState::InRound);
    }
}