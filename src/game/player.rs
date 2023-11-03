use std::time::Duration;

use bevy::{prelude::*,math::Vec3Swizzles};
use bevy_ggrs::{PlayerInputs, AddRollbackCommandExtension};
use seeded_random::{Random, Seed};

use super::{components::*, MAP_SIZE, textures::ImageAssets, RollbackState, Scores, GameSeed};
use super::networking::GgrsConfig;
use super::input;


pub fn move_players(
    inputs: Res<PlayerInputs<GgrsConfig>>,
    mut players: Query<(&mut Transform, &mut MoveDir, &Player)>,
) {
    for (mut transform, mut move_dir, player) in &mut players {
        let (input, _) = inputs[player.handle];
        let direction = input::direction(input);

        if direction == Vec2::ZERO {
            continue;
        }

        move_dir.0 = direction;

        let move_speed = 0.13;
        let move_delta = direction * move_speed;

        let old_pos = transform.translation.xy();
        let limit = Vec2::splat(MAP_SIZE as f32 / 2. - 0.5);
        let new_pos = (old_pos + move_delta).clamp(-limit, limit);

        transform.translation.x = new_pos.x;
        transform.translation.y = new_pos.y;
    }
}

pub fn player_look(
    // maybe it's better to use PlayerInput instead of MoveDir but this will do for now
    players: Query<(&MoveDir, &Children), With<Player>>, 
    mut eyes_sprite: Query<&mut TextureAtlasSprite, With<LookTowardsParentMove>>,
) {
    for (move_dir, children) in players.iter() {
        for &child in children.iter() {
            if let Ok(eyes) = &mut eyes_sprite.get_mut(child) {
                // let (input, _) = inputs[player.handle];
                eyes.index = get_directional_sprite(move_dir.0);
            }
        }
    }
}

fn get_directional_sprite(dir: Vec2) -> usize {
    if dir.y < 0. && dir.x > 0. {
        return 0;
    }
    if dir.x.abs() < 0.2 && dir.y < 0. {
        return 1;
    }
    if dir.y < 0. && dir.x < 0. {
        return 2;
    }
    if dir.y.abs() < 0.2 && dir.x < 0. {
        return 3;
    }
    if dir.y > 0. && dir.x < 0. {
        return 4;
    }
    if dir.x.abs() < 0.2 && dir.y > 0. {
        return 5;
    }
    if dir.y > 0. && dir.x > 0. {
        return 6;
    }
    if dir.y.abs() < 0.2 && dir.x > 0. {
        return 7;
    }
    0
}

pub fn spawn_players(
    mut commands: Commands,
    images: Res<ImageAssets>,
    mut seed: ResMut<GameSeed>,
    players: Query<Entity, With<Player>>,
    bullets: Query<Entity, With<Bullet>>,
) {

    info!("spawning players");

    // despawn last games stuff

    for player in &players {
        commands.entity(player).despawn_recursive();
    }

    for bullet in &bullets {
        commands.entity(bullet).despawn_recursive();
    }

    // generate new spawn positions
    let positions = generate_random_positions(2, seed.0);

    // now advance the seed for next spawn
    seed.0 = Random::from_seed(Seed::unsafe_new(seed.0)).gen();

    // p1
    spawn_player(&mut commands, &images, 0, -Vec2::X, grid_to_world(positions[0]).extend(100.), Color::rgb(0.8, 0.2, 0.2));
    // spawn_player(&mut commands, &images, 0, -Vec2::X, grid_to_world((MAP_SIZE-1,MAP_SIZE-1)).extend(100.));

    // p2
    spawn_player(&mut commands, &images, 1, Vec2::X, grid_to_world(positions[1]).extend(100.), Color::rgb(0.15, 0.25, 0.8));
    // spawn_player(&mut commands, &images, 1, Vec2::X, grid_to_world((0,0)).extend(100.));


}

fn spawn_player(
    commands: &mut Commands,
    images: &Res<ImageAssets>,
    handle: usize,
    move_dir: Vec2,
    translation: Vec3,
    color: Color,
) {
    let parent = commands.spawn((
        Player { handle },
        BulletReady(true),
        MoveDir(move_dir),
        SpriteBundle {
            texture: images.ghost.clone(),
            transform: Transform::from_translation(translation),
            sprite: Sprite {
                color,
                custom_size: Some(Vec2::new(1.,1.)),
                ..default()
            },
            ..default()
        }
    ))
    .add_rollback()
    .id();

    let child = commands.spawn((
        SpriteSheetBundle {
            transform: Transform::from_translation(Vec3::new(0.,0.,1.)),
            sprite: TextureAtlasSprite {
                index: 0,
                custom_size: Some(Vec2::new(1.,1.)),
                ..default()
            },
            texture_atlas: images.eyes.clone(),
            ..default()
        },
        LookTowardsParentMove,
    ))
    .add_rollback()
    .id();

    commands.entity(parent).push_children(&[child]);
}

// takes in a grid position from 0 to map_size and outputs a world coordinate
fn grid_to_world(grid_pos: (u32,u32)) -> Vec2 {
    Vec2::new(
        (grid_pos.0 as f32 - MAP_SIZE as f32 / 2.)+0.5,
        (grid_pos.1 as f32 - MAP_SIZE as f32 / 2.)+0.5,
    )
}

fn generate_random_positions(count: usize, base_seed: u64) -> Vec<(u32,u32)> {
    let mut rand = Random::from_seed(Seed::unsafe_new(base_seed));
    let mut positions: Vec<(u32,u32)> = vec![];
    // // generate a position and then check each position for collisions
    for _ in 0..count {
        let mut overlapped = true;
        let mut x = 0;
        let mut y = 0;
        while overlapped {
            // advance the random seed
            rand = Random::from_seed(rand.seed());
            x = rand.u32() % MAP_SIZE;
            // advance the random seed again for y
            rand = Random::from_seed(rand.seed());
            y = rand.u32() % MAP_SIZE;
            // check for overlaps in existing additins
            overlapped = {
                let mut ret = false;
                for &pos in positions.iter() {
                    if x == pos.0 || y == pos.1 {
                        ret = true;
                        break;
                    }
                }
                ret
            };
        }
        // add the new position that has no overlaps to the position list
        positions.push((x,y));
    }
    positions
}

pub fn fire_bullets(
    mut commands: Commands,
    inputs: Res<PlayerInputs<GgrsConfig>>,
    images: Res<ImageAssets>,
    mut players: Query<(&Transform, &Player, &mut BulletReady, &MoveDir), Without<MarkedForDeath>>,
) {
    for (transform, player, mut bullet_ready, move_dir) in &mut players {
        let (input, _) = inputs[player.handle];
        if input::fire(input) && bullet_ready.0 {
            let player_pos = transform.translation.xy();
            let pos = player_pos + move_dir.0 * PLAYER_RADIUS + BULLET_RADIUS;
            commands.spawn((
                Bullet,
                *move_dir,
                SpriteBundle {
                    transform: Transform::from_translation(pos.extend(200.))
                        .with_rotation(Quat::from_rotation_arc_2d(Vec2::X, move_dir.0)),
                    texture: images.bullet.clone(),
                    sprite: Sprite {
                        custom_size: Some(Vec2::new(0.3, 0.1)),
                        ..default()
                    },
                    ..default()
                }
            ))
            .add_rollback();
            bullet_ready.0 = false;
        }
    }
}

pub fn reload_bullet(
    inputs: Res<PlayerInputs<GgrsConfig>>,
    mut bullets: Query<(&mut BulletReady, &Player)>
) {
    for (mut can_fire, player) in &mut bullets {
        let (input, _) = inputs[player.handle];
        if !input::fire(input) {
            can_fire.0 = true;
        }
    }
}

pub fn move_bullets(mut bullets: Query<(&mut Transform, &MoveDir), With<Bullet>>) {
    for (mut transform, dir) in &mut bullets {
        let delta = (dir.0 * 0.35).extend(0.);
        transform.translation += delta;
    }
}

// TODO: Sometimes player death events don't restart the game for one or both clients...
const PLAYER_RADIUS: f32 = 0.5;
const BULLET_RADIUS: f32 = 0.025;
pub fn kill_players(
    mut commands: Commands,
    players: Query<(Entity, &Transform), (Without<Bullet>,(With<Player>,Without<MarkedForDeath>))>,
    bullets: Query<(Entity, &Transform), With<Bullet>>,
) {
    for (player_entity, player_transform) in &players {
        for (bullet_entity, bullet_transform) in &bullets {
            let distance = Vec2::distance(
                player_transform.translation.xy(),
                bullet_transform.translation.xy(),
            );
            if distance < PLAYER_RADIUS + BULLET_RADIUS {
                commands.entity(player_entity).insert(MarkedForDeath::default());
                commands.entity(bullet_entity).despawn_recursive();
            }
        }
    }
}

// we despawn the players after a timer delay in case the network messed up the bullet hit registration
pub fn process_deaths(
    mut commands: Commands,
    mut players: Query<(Entity, &Player, &mut MarkedForDeath)>,
    mut next_state: ResMut<NextState<RollbackState>>,
    mut scores: ResMut<Scores>,
) {
    for (player_entity, player_component, mut marked) in &mut players {

        marked.0.tick(Duration::from_secs_f64(1. / 60.));// tick at the ggrs network framerate of 60 fps

        if marked.0.just_finished() {
            if player_component.handle == 0 {
                scores.1 += 1;
            } else {
                scores.0 += 1;
            }
    
            commands.entity(player_entity).despawn_recursive();
            next_state.set(RollbackState::RoundEnd);
            info!("player died: {scores:?}");
        }
    }
}