use std::time::Duration;

use bevy::{prelude::*, render::camera::ScalingMode};
use bevy_ggrs::{GgrsAppExtension, GgrsPlugin, GgrsSchedule};
use bevy_asset_loader::prelude::*;
use bevy_kira_audio::prelude::*;
use bevy_roll_safe::prelude::*;
use bevy_egui::EguiPlugin;
use crate::cloudflare_net::CloudflareNetPlugin;

mod components;
mod map;
mod player;
mod input;
mod networking;
pub(crate) mod session;
mod rollback_audio;
mod assets;
mod gui;
mod toasts;
mod ggrs_framecount;

#[cfg(feature="debug_render")]
mod debug_render;

use components::*;
use map::*;
use player::*;
use input::*;
use networking::*;
use rollback_audio::*;
use assets::textures::*;
use assets::sounds::*;
use gui::*;
use toasts::*;
use ggrs_framecount::*;
use session::{PlayerScore, RoundBootstrap, RoundOutcome};

use seeded_random::Random;
use seeded_random::Seed;

pub const MAP_SIZE: usize = 41;
pub const GRID_WIDTH: f32 = 0.05;

#[derive(States, Clone, Eq, PartialEq, Debug, Hash, Default)]
pub enum GameState {
    #[default]
    AssetLoading,
    MainMenu,
    Matchmaking,
    InGame,
}

#[derive(States, Clone, Eq, PartialEq, Debug, Hash, Default, Reflect)]
pub enum RollbackState {
    /// When the map generation or other setup is running before the round starts
    #[default]
    PreRound,
    /// When the characters running and gunning
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

/// Rollback-safe scores in canonical stable-player-ID order.
#[derive(Resource, Reflect, Default, Debug, Clone, PartialEq, Eq)]
#[reflect(Resource)]
pub struct Scores(Vec<PlayerScore>);

#[derive(Reflect, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Elimination {
    pub player_id: session::PlayerId,
    pub frame: u32,
}

#[derive(Resource, Reflect, Default, Debug, Clone, PartialEq, Eq)]
#[reflect(Resource)]
pub struct RoundProgress {
    pub eliminated: Vec<Elimination>,
    pub disconnected: Vec<session::PlayerId>,
    pub resolved: Option<RoundOutcome>,
    pub resolved_frame: Option<u32>,
}

#[derive(Resource, Default)]
pub struct ReportedOutcome(pub Option<(u32, u32, u32)>);

impl RoundProgress {
    pub fn record_elimination(&mut self, elimination: Elimination) {
        if self.eliminated.iter().any(|entry| entry.player_id == elimination.player_id) {
            return;
        }
        self.eliminated.push(elimination);
        self.eliminated.sort_by_key(|entry| entry.player_id);
    }
}

impl Scores {
    pub fn from_bootstrap(bootstrap: &RoundBootstrap) -> Self {
        let mut entries = bootstrap.scores.clone();
        entries.sort_by_key(|entry| entry.player_id);
        Self(entries)
    }

    pub fn entries(&self) -> &[PlayerScore] {
        &self.0
    }

    pub fn apply_outcome(&mut self, outcome: &RoundOutcome) {
        for player_id in outcome.point_winners() {
            if let Ok(index) = self
                .0
                .binary_search_by_key(player_id, |entry| entry.player_id)
            {
                self.0[index].score += 1;
            }
        }
    }
}

#[derive(Resource, Debug, Clone)]
pub struct PendingPlayerProfile {
    pub name: String,
    pub palette_id: u8,
    pub cosmetic_id: u8,
}

impl Default for PendingPlayerProfile {
    fn default() -> Self {
        Self { name: "Ghost".into(), palette_id: 0, cosmetic_id: 0 }
    }
}

#[derive(Resource, Reflect, Default, Debug)]
#[reflect(Resource)]
pub struct GameSeed(u64);

#[derive(Resource, Reflect, Default, Debug)]
#[reflect(Resource)]
pub struct SoundIdSeed(pub(crate) Vec<SoundSeed>);

impl SoundIdSeed {
    pub fn new(match_seed: u64, players: usize) -> Self {
        Self(
            (0..players)
                .map(|handle| SoundSeed(match_seed.wrapping_add(handle as u64 + 1)))
                .collect(),
        )
    }

    pub fn next(&mut self, handle: usize) -> u64 {
        self.0
            .get_mut(handle)
            .expect("sound seed exists for every roster handle")
            .next()
    }

}

#[derive(Reflect, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoundSeed(u64);

impl SoundSeed {
    #[cfg(test)]
    pub fn from_seed(seed: u64) -> Self {
        Self(seed)
    }

    /// Moves random seed by one and returns value
    #[allow(dead_code)]
    pub fn next(&mut self) -> u64 {
        // use previous output as seed for the next to "chain" them
        self.0 = Random::from_seed(Seed::unsafe_new(self.0)).gen();
        self.0
    }
}

pub fn run() {
    let mut app = App::new();

    #[cfg(feature="debug_render")]
    {
        app.add_systems(Startup, debug_render::spawn_debug_sprites);
        app.add_systems(Update, debug_render::update_debug_sprites);
        app.init_resource::<debug_render::DebugEntitiesList>();
    }

    app
    .add_state::<GameState>()
    .add_state::<MenuState>()
    .add_loading_state(
        LoadingState::new(GameState::AssetLoading).continue_to_state(GameState::MainMenu)
    )
    .add_collection_to_loading_state::<_, ImageAssets>(GameState::AssetLoading)
    .add_collection_to_loading_state::<_, SoundAssets>(GameState::AssetLoading)
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
        AudioPlugin,
        CloudflareNetPlugin,
    ));

    #[cfg(feature = "sync_test")]
    app.add_systems(OnEnter(MenuState::SyncTest), start_sync_test);
    #[cfg(feature = "auto_sync_test")]
    app.add_systems(OnEnter(GameState::MainMenu), enter_sync_test_automatically);
    #[cfg(feature = "auto_mobile_input_test")]
    app.add_systems(OnEnter(GameState::MainMenu), enter_mobile_input_test_automatically);

    #[cfg(feature="debug_render")]
    {
        use bevy_inspector_egui::{
            prelude::*,
            quick::WorldInspectorPlugin,
        };
        use bevy_egui::{egui, EguiPlugin, EguiContexts};
        // add the inspector plugin after default plugins are added
        // app.add_plugins((WorldInspectorPlugin::new()));

        /// A group of related system sets, used for controlling the order of systems. Systems can be
        /// added to any number of sets.
        #[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
        enum DebugSet {
            BeforeRound,
            Round,
            AfterRound,
        }


        app.add_plugins((
            bevy_inspector_egui::DefaultInspectorConfigPlugin,
        ))
        .configure_sets(Update,
            (DebugSet::BeforeRound, DebugSet::Round, DebugSet::AfterRound).chain(),
        )
        .add_systems(Update, (
            inspector_ui.in_set(DebugSet::BeforeRound),
        ));
        

        fn inspector_ui(world: &mut World) {
            let egui_context = {
                let mut query = world.query::<&mut bevy_egui::EguiContext>();

                query
                .get_single_mut(world)
                .expect("getting EGUI context for inspector")
                .get_mut()
                .clone()
            };
        
            

            egui::Window::new("DEBUG")
            .show(&egui_context, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // equivalent to `WorldInspectorPlugin`
                    // bevy_inspector_egui::bevy_inspector::ui_for_world(world, ui);
        
                    // egui::CollapsingHeader::new("Materials").show(ui, |ui| {
                    //     bevy_inspector_egui::bevy_inspector::ui_for_assets::<StandardMaterial>(world, ui);
                    // });

                    ui.heading("Players");
                    bevy_inspector_egui::bevy_inspector::ui_for_world_entities_filtered
                    ::<With<Player>>
                    (
                        world,
                        ui,
                        true
                    );
        
                    // ui.heading("Other Entities");
                    egui::CollapsingHeader::new("Other Entities")
                    .show(ui, |ui| {
                        ui.label("Body");
                        bevy_inspector_egui::bevy_inspector::ui_for_world_entities_filtered
                        ::<Without<Player>>
                        (
                            world,
                            ui,
                            true
                        );
                    });
                    
                });
            });
        }
    }

    app.add_ggrs_plugin(
        GgrsPlugin::<networking::GgrsConfig>::new()
            .with_input_system(input)
            .register_roll_state::<RollbackState>()
            .register_rollback_resource::<RoundEndTimer>()
            .register_rollback_resource::<Scores>()
            .register_rollback_resource::<GameSeed>()
            .register_rollback_resource::<SoundIdSeed>()
            .register_rollback_resource::<Map<CellType, MAP_SIZE, MAP_SIZE>>()
            .register_rollback_resource::<RoundProgress>()
            // .rollback_resource_with_copy::<FrameCount>()
            .register_rollback_resource::<GGFrameCount>()
            .register_rollback_component::<Player>()
            .register_rollback_component::<Transform>()
            .register_rollback_component::<Bullet>()
            .register_rollback_component::<BulletReady>()
            .register_rollback_component::<SpeedPickup>()
            .register_rollback_component::<SpeedBoost>()
            .register_rollback_component::<ShieldPickup>()
            .register_rollback_component::<ShieldCharges>()
            .register_rollback_component::<MoveDir>()
            .register_rollback_component::<LookTowardsParentMove>()
            .register_rollback_component::<MarkedForDeath>()

            // for rollback audio
            .register_rollback_component::<RollbackSound>()
            // .register_rollback_component::<AudioEmitter>()

            // rollback names of entities
            .register_rollback_component::<Name>()

            // register sprite bundle as rollback components
            // Temp fix until bevy_ggrs fixes rollback
            .register_rollback_component::<Sprite>()
            .register_rollback_component::<GlobalTransform>()
            .register_rollback_component::<Handle<Image>>()
            .register_rollback_component::<Visibility>()
            .register_rollback_component::<ComputedVisibility>()
            .register_rollback_component::<Handle<TextureAtlas>>()       
            .register_rollback_component::<TextureAtlasSprite>() 
    )
    .insert_resource(ClearColor(Color::BLACK))
    .insert_resource(SpacialAudio { max_distance: 20. })
    .init_resource::<AudioConfig>()
    .init_resource::<MatchmakingRoom>()
    .init_resource::<PendingPlayerProfile>()
    .init_resource::<toasts::Toasts>()
    .init_resource::<RoundEndTimer>()
    .init_resource::<Scores>()
    .init_resource::<GGFrameCount>()
    .init_resource::<PlaybackStates>()
    .init_resource::<Map<CellType, MAP_SIZE, MAP_SIZE>>()
    .init_resource::<RoundProgress>()
    .init_resource::<ReportedOutcome>()
    // add custom audio channels
    .add_audio_channel::<MusicChannel>()
    .add_audio_channel::<SfxChannel>()
    .add_systems(Startup, setup)
    .add_systems(OnEnter(GameState::MainMenu), start_main_music)
    .add_systems(OnEnter(GameState::Matchmaking), start_cloudflare_socket)
    .add_systems(OnExit(GameState::Matchmaking), stop_cloudflare_socket)
    .add_systems(
        OnExit(GameState::InGame),
        (clear_sounds, cleanup_network_session).chain(),
    )
    .add_systems(
        Update,
        (
            // logging output
            display_toasts,
            log_ggrs_events.run_if(in_state(GameState::InGame)),
            // menu system
            handle_menu_input,
            update_main_menu
                .run_if(in_state(GameState::MainMenu))
                .run_if(in_state(MenuState::Main)),
            update_settings_ui
                .run_if(in_state(MenuState::Settings)),
            update_direct_connect_ui
                .run_if(in_state(GameState::MainMenu).and_then(in_state(MenuState::DirectConnect))),
            update_in_game_controls_ui
                .run_if(in_state(GameState::InGame).and_then(in_state(MenuState::Main))),
            update_matchmaking_ui.run_if(in_state(GameState::Matchmaking)),
            update_respawn_ui.run_if(in_state(RollbackState::RoundEnd)),

            // audio volume update in response to ui
            update_volume,

            wait_for_players.run_if(in_state(GameState::Matchmaking)),
            report_confirmed_outcome.run_if(in_state(GameState::InGame)),
            watch_lobby_epoch.run_if(in_state(GameState::InGame)),
            (player_look, camera_follow, ears_follow, update_score_ui, animate_effects).run_if(in_state(GameState::InGame)),
        ),
    )
    .add_roll_state::<RollbackState>(GgrsSchedule)
    .add_systems(
        OnEnter(RollbackState::PreRound),
        (
            clear_map_sprites,
        )
    )
    .add_systems(
        GgrsSchedule,
        generate_map
            .ambiguous_with(round_end_timeout)
            .ambiguous_with(process_deaths)
            .ambiguous_with(apply_deferred)
            .distributive_run_if(in_state(RollbackState::PreRound))
            .after(apply_state_transition::<RollbackState>),
    )
    .add_systems(
        OnEnter(RollbackState::InRound),
        (
            spawn_map_sprites,
            spawn_players,
        )
    )
    .add_systems(
        GgrsSchedule,
        (
            tick_speed_boost,
            move_players,
            reload_bullet,
            collect_speed_pickups,
            collect_shield_pickups,
            trigger_traps,
            fire_bullets,
            move_bullets,
            kill_players,
            remove_finished_sounds,
            apply_deferred,
            process_deaths,
            sync_rollback_sounds,
            increase_frame_system,
        )
        .chain()
        .after(apply_state_transition::<RollbackState>)
        .distributive_run_if(in_state(RollbackState::InRound)),
    )
    .add_systems(
        OnExit(RollbackState::InRound),
        (
            count_points_and_despawn,
            clear_sounds,
        )
    )
    .add_systems(
        GgrsSchedule,
        round_end_timeout
            .ambiguous_with(process_deaths)
            .ambiguous_with(apply_deferred)
            .distributive_run_if(in_state(RollbackState::RoundEnd))
            .after(apply_state_transition::<RollbackState>),
    ).run();
}

#[cfg(feature = "auto_mobile_input_test")]
fn enter_mobile_input_test_automatically(mut state: ResMut<NextState<MenuState>>) {
    state.set(MenuState::DirectConnect);
}

#[cfg(feature = "auto_sync_test")]
fn enter_sync_test_automatically(mut state: ResMut<NextState<MenuState>>) {
    state.set(MenuState::SyncTest);
}

fn setup(mut commands: Commands) {
    let mut camera_bundle = Camera2dBundle::default();
    camera_bundle.projection.scaling_mode = ScalingMode::AutoMax { max_width: 10., max_height: 10. };
    commands
    .spawn(camera_bundle);

    // spawn our "ears" entity which we will make follow the players position
    commands.spawn(
        (
            AudioReceiver,
            Transform::default(),
            GlobalTransform::default(),
        )
    );

    commands.spawn(SpriteBundle {
        transform: Transform::from_xyz(0., 0., -2.),
        sprite: Sprite {
            color: Color::rgb(0.08, 0.09, 0.12),
            custom_size: Some(Vec2::splat(MAP_SIZE as f32)),
            ..default()
        },
        ..default()
    });

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
        // break out early because we are only one local player
        break;
    }
}

fn ears_follow(
    player_handle: Option<Res<LocalPlayerHandle>>,
    players: Query<(&Player, &Transform)>,
    mut ears: Query<&mut Transform, (With<AudioReceiver>,Without<Player>)>,
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

        for mut transform in &mut ears {
            transform.translation = pos;
        }
        // break out early because we are only one local player
        break;
    }
}

fn round_end_timeout(
    mut timer: ResMut<RoundEndTimer>,
    mut state: ResMut<NextState<RollbackState>>
) {
    timer.tick(Duration::from_secs_f64(1. / 60.));// tick at the ggrs network framerate of 60 fps

    if timer.just_finished() {
        state.set(RollbackState::PreRound);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::schedule::{LogLevel, ScheduleBuildSettings};

    #[test]
    fn in_round_schedule_has_no_ambiguous_conflicts() {
        let mut schedule = Schedule::default();
        schedule.set_build_settings(ScheduleBuildSettings {
            ambiguity_detection: LogLevel::Error,
            ..default()
        });
        schedule.add_systems((
            tick_speed_boost,
            move_players,
            reload_bullet,
            collect_speed_pickups,
            collect_shield_pickups,
            trigger_traps,
            fire_bullets,
            move_bullets,
            kill_players,
            remove_finished_sounds,
            apply_deferred,
            process_deaths,
            sync_rollback_sounds,
            increase_frame_system,
        ).chain());

        assert!(schedule.initialize(&mut World::new()).is_ok());
    }
}