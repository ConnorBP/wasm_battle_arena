use std::time::Duration;

use bevy::ecs::system::Command;
use bevy::{prelude::*, render::camera::ScalingMode};
use bevy_ggrs::{GgrsAppExtension, GgrsPlugin, GgrsSchedule};
use bevy_asset_loader::prelude::*;
use bevy_kira_audio::prelude::*;
use bevy_roll_safe::prelude::*;
use bevy_egui::EguiPlugin;

mod components;
mod map;
mod player;
mod input;
mod networking;
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

#[derive(Resource, Reflect, Default, Debug)]
#[reflect(Resource)]
pub struct Scores(u32, u32);

#[derive(Resource, Reflect, Default, Debug)]
#[reflect(Resource)]
pub struct GameSeed(u64);

#[derive(Resource, Reflect, Default, Debug)]
#[reflect(Resource)]
pub struct SoundIdSeed(pub(crate) (SoundSeed,SoundSeed));

impl SoundIdSeed {
    /// Moves random seed by one and returns value
    #[allow(dead_code)]
    pub fn next(&mut self, handle: usize) -> u64 {
        match handle {
            0 => {self.0.0.next()},
            1 => {self.0.1.next()},
            _ => {0}
        }
    }
    /// same as next but returns as usize for code cleanliness
    pub fn next_us(&mut self, handle: usize) -> usize {
        self.next(handle) as usize
    }
}

#[derive(Reflect,Default,Debug)]
pub struct SoundSeed(u64);


// custom system sets

#[derive(Debug, Hash, PartialEq, Eq, Clone, SystemSet)]
struct CommandFlush;

impl SoundSeed {
    /// Moves random seed by one and returns value
    #[allow(dead_code)]
    pub fn next(&mut self) -> u64 {
        // use previous output as seed for the next to "chain" them
        self.0 = Random::from_seed(Seed::unsafe_new(self.0)).gen();
        self.0
    }
    /// same as next but returns as usize for code cleanliness
    pub fn next_us(&mut self) -> usize {
        // use previous output as seed for the next to "chain" them
        self.0 = Random::from_seed(Seed::unsafe_new(self.0)).gen();
        self.0 as usize
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
    ));

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
            // .rollback_resource_with_copy::<FrameCount>()
            .register_rollback_resource::<GGFrameCount>()
            .register_rollback_component::<Player>()
            .register_rollback_component::<Transform>()
            .register_rollback_component::<Bullet>()
            .register_rollback_component::<BulletReady>()
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
    .insert_resource(ClearColor(Color::rgb(0.43,0.43,0.63)))
    .insert_resource(SpacialAudio { max_distance: 20. })
    .init_resource::<AudioConfig>()
    .init_resource::<toasts::Toasts>()
    .init_resource::<RoundEndTimer>()
    .init_resource::<Scores>()
    .init_resource::<GGFrameCount>()
    .init_resource::<PlaybackStates>()
    // add custom audio channels
    .add_audio_channel::<MusicChannel>()
    .add_audio_channel::<SfxChannel>()
    .add_systems(OnEnter(GameState::MainMenu),
    (
        start_main_music,
    )
    )
    .add_systems(
        OnEnter(GameState::Matchmaking),
        (setup, start_matchbox_socket),
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
            update_in_game_controls_ui
                .run_if(in_state(GameState::InGame).and_then(in_state(MenuState::Main))),
            update_matchmaking_ui.run_if(in_state(GameState::Matchmaking)),
            update_respawn_ui.run_if(in_state(RollbackState::RoundEnd)),

            // audio volume update in response to ui
            update_volume,

            wait_for_players.run_if(in_state(GameState::Matchmaking)),
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
            // touch_test,
            // touch_ev_test,
            
            move_players,
            reload_bullet,
            fire_bullets
                .before(CommandFlush)
                .after(move_players)
                .after(reload_bullet),
            move_bullets
                .before(CommandFlush)
                .after(fire_bullets),
            kill_players
                .before(CommandFlush)
                .after(move_bullets)
                .after(fire_bullets)
                .after(move_players),


            process_deaths
                .after(CommandFlush)
                .after(kill_players),

            // remove finished rollback sounds
            remove_finished_sounds.before(CommandFlush),
            


            apply_deferred.in_set(CommandFlush),
            
            sync_rollback_sounds
                .after(CommandFlush)
                .after(remove_finished_sounds)
                // run after any system that spawns a sound
                .after(move_bullets)
                .after(fire_bullets),
            // increase frame count at the end only during rounds
            increase_frame_system
                .after(sync_rollback_sounds)
                .after(remove_finished_sounds)
                .after(process_deaths)
                .after(kill_players)
                .after(move_bullets)
                .after(fire_bullets),
        
        ).after(apply_state_transition::<RollbackState>)
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