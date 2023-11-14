use std::time::Duration;

use bevy::{prelude::*,math::Vec3Swizzles, sprite::collide_aabb::collide};
use bevy_ggrs::{PlayerInputs, AddRollbackCommandExtension};
use bevy_kira_audio::prelude::*;
use seeded_random::{Random, Seed};

use crate::game::rollback_audio::RollbackSoundBundle;

use super::{components::*, MAP_SIZE, assets::{textures::{ImageAssets, spawn_explosion}, sounds::SoundAssets}, RollbackState, Scores, GameSeed, map::{CellType, Map}, rollback_audio::RollbackSound, ggrs_framecount::GGFrameCount, SoundIdSeed};
use super::networking::GgrsConfig;
use super::input;


pub fn move_players(
    inputs: Res<PlayerInputs<GgrsConfig>>,
    mut players: Query<(&mut Transform, &mut MoveDir, &Player)>,
    map_data: Res<Map<CellType, MAP_SIZE, MAP_SIZE>>,
) {
    for (mut transform, mut move_dir, player) in &mut players {
        let (input, _) = inputs[player.handle];
        let direction = input::direction(input);

        if direction == Vec2::ZERO {
            continue;
        }

        move_dir.0 = direction;

        let move_speed = 0.13;
        let old_pos = transform.translation.xy();
        let mut move_delta = direction * move_speed;

        
        // check the cells in the horizontal, vertical, and forward directions
        let h_block = world_to_grid(old_pos + Vec2::new(direction.x.signum(), 0.0));
        let v_block = world_to_grid(old_pos + Vec2::new(0.0, direction.y.signum()));
        let hv_block = world_to_grid(old_pos + Vec2::new(direction.x.signum(), direction.y.signum()));
        let h2_block = world_to_grid(old_pos + Vec2::new(direction.x.signum(), -direction.y.signum()));
        let v2_block = world_to_grid(old_pos + Vec2::new(-direction.x.signum(), direction.y.signum()));
        
        // run check if cell is in valid range
        if let Some(cell) = h_block {
            // now perform finite collision check on cell
            if cell_collide(&map_data, old_pos + Vec2::new(move_delta.x, 0.0), cell) {
                    move_delta.x = 0.0;

            }
        }
        
        // extend horizontal check down by one block
        if let Some(cell) = h2_block {
            // now perform finite collision check on cell
            if cell_collide(&map_data, old_pos + Vec2::new(move_delta.x, 0.0), cell) {
                    move_delta.x = 0.0;

            }
        }

        // run check if cell is in valid range
        if let Some(cell) = v_block {
            // now perform finite collision check on cell
            if cell_collide(&map_data, old_pos + Vec2::new(0.0, move_delta.y), cell) {
                    move_delta.y = 0.0;

            }
        }

        // extend vertical check by one block
        if let Some(cell) = v2_block {
            // now perform finite collision check on cell
            if cell_collide(&map_data, old_pos + Vec2::new(0.0, move_delta.y), cell) {

                    move_delta.y = 0.0;

            }
        }


        if let Some(cell) = hv_block {
            // fine detail collision check with the vertical direction cell
            let player_pos = old_pos + Vec2::new(move_delta.x, move_delta.y);
            if cell_collide(&map_data, player_pos, cell) {
                // check which axis is closest to the square corner (same as center pos) to decide which way to slide
                // this is the solution for the "corner case"
                let diff = player_pos - grid_to_world(cell);
                if diff.x.abs() > diff.y.abs() {   
                    move_delta.x = 0.0;
                } else {
                    move_delta.y = 0.0;
                }
            }
        }

        let limit = Vec2::splat(MAP_SIZE as f32 / 2. - 0.5);
        let new_pos = (old_pos + move_delta).clamp(-limit, limit);





        // let (x,y) = world_to_grid(new_pos);
        // match map_data.cells[x as usize][y as usize] {
        //     CellType::WallBlock => {
        //         let block_pos = grid_to_world((x,y));
        //         // let u = f32::max(block_pos.x.abs(), block_pos.y.abs());
        //         // let p = Vec2::new(block_pos.x / u, block_pos.y / u);
                
                
        //         // new_pos = new_pos.clamp(
        //         //     block_pos + 1.0,
        //         //     block_pos - 1.0
        //         // );
        //     },
        //     _=> {}
        // }

        transform.translation.x = new_pos.x;
        transform.translation.y = new_pos.y;
    }
}

/// returns true if cell at index collides with player position
fn cell_collide(
    map_data: &Res<Map<CellType,MAP_SIZE, MAP_SIZE>>,
    player_pos: Vec2,
    cell: (u32, u32)
) -> bool {
    match map_data.cells[cell.0 as usize][cell.1 as usize] {
        CellType::WallBlock => {
            wall_check(player_pos, grid_to_world(cell))
        },
        _=> {
            false
        }
    }
}

/// Checks if two squares of size 1,1 overlap from two 2D coordinates
fn wall_check(player: Vec2, wall: Vec2) -> bool{
    let col = collide(
        player.extend(0.),
        Vec2::splat(1.0),
        wall.extend(0.0),
        Vec2::splat(1.0)
    );
    col.is_some()
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
    map_data: Res<Map<CellType, MAP_SIZE, MAP_SIZE>>,
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
    let positions = generate_random_positions(2, seed.0, map_data);

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
        },
        Name::new(format!("Player{}", handle)),
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
pub fn grid_to_world(grid_pos: (u32,u32)) -> Vec2 {
    Vec2::new(
        (grid_pos.0 as f32 - MAP_SIZE as f32 / 2.)+0.5,
        (grid_pos.1 as f32 - MAP_SIZE as f32 / 2.)+0.5,
    )
}

pub fn world_to_grid(world_pos: Vec2) -> Option<(u32,u32)> {
    
        let x = ((world_pos.x) + MAP_SIZE as f32 / 2.) as i32;
        let y = ((world_pos.y) + MAP_SIZE as f32 / 2.) as i32;

        // return in bounds result or None
        if x < 0 || x >= MAP_SIZE as i32
        || y < 0 || y >= MAP_SIZE as i32 {
            return None;
        } else {
            return Some((x as u32, y as u32))
        }
}

fn generate_random_positions
(
    count: usize,
    base_seed: u64,
    map_data: Res<Map<CellType, MAP_SIZE, MAP_SIZE>>,
) -> Vec<(u32,u32)> {
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
            x = rand.u32() % MAP_SIZE as u32;
            // advance the random seed again for y
            rand = Random::from_seed(rand.seed());
            y = rand.u32() % MAP_SIZE as u32;
            // check for overlaps in existing additins
            overlapped = {
                let mut ret = false;
                // don't spawn players in walls
                match map_data.cells[x as usize][y as usize] {
                    CellType::WallBlock => {
                        // mark cell as taken and generate a new position for this player
                        ret = true;
                    },
                    _=> {
                        // if not in a wall then check that another player has not been spawned here
                        for &pos in positions.iter() {
                            // check for player overlaps
                            if x == pos.0 || y == pos.1 {
                                ret = true;
                                break;
                            }
                        }
                    }
                }
                // set overlapped var
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
    frame: Res<GGFrameCount>,
    inputs: Res<PlayerInputs<GgrsConfig>>,
    images: Res<ImageAssets>,
    sounds: Res<SoundAssets>,
    mut sound_id: ResMut<SoundIdSeed>,
    mut players: Query<(&Transform, &Player, &mut BulletReady, &MoveDir), Without<MarkedForDeath>>,
) {
    for (transform, player, mut bullet_ready, move_dir) in &mut players {
        let (input, _) = inputs[player.handle];
        if input::fire(input) && bullet_ready.0 {
            let player_pos = transform.translation.xy();
            let pos = player_pos + move_dir.0 * PLAYER_RADIUS + BULLET_RADIUS;
            // spawn bullet entity
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

            let snd = sound_id.next_us(player.handle);
            debug!("firing bullet snd {snd:#00x}");
            commands.spawn(
                (
                    RollbackSoundBundle {
                        sound: RollbackSound {
                            clip: sounds.laser_shoot.clone(),
                            start_frame: frame.frame,
                            sub_key: snd,
                        },
                        transform: Transform::from_translation(pos.extend(200.))
                            .with_rotation(Quat::from_rotation_arc_2d(Vec2::X, move_dir.0)),
                        ..default()
                    },
                )
            )
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

pub fn move_bullets(
    mut commands: Commands,
    map_data: Res<Map<CellType, MAP_SIZE, MAP_SIZE>>,
    mut bullets: Query<(Entity, &mut Transform, &MoveDir), With<Bullet>>
) {
    // map limit for bullet is exactly half map in any direction since the map is centered
    let limit = Vec2::splat(MAP_SIZE as f32 / 2.);
    for (entity, mut transform, dir) in &mut bullets {
        let delta = (dir.0 * 0.35).extend(0.);
        transform.translation += delta;

        // check if bullet is out of map bounds
        let absolute_pos = transform.translation.xy().abs();
        if absolute_pos.x > limit.x || absolute_pos.y > limit.y {
            // bullet out of bounds, despawn it
            commands.entity(entity).despawn_recursive();
        }  
        // check for block hits
        if let Some((x,y)) = world_to_grid(transform.translation.xy()) {
            // if coords are inside of a wall then its a hit
            match map_data.cells[x as usize][y as usize] {
                CellType::WallBlock => {
                    // bullet in a block, despawn it
                    commands.entity(entity).despawn_recursive();
                },
                _ => {},
            }
        }
    }
}

// TODO: Sometimes player death events don't restart the game for one or both clients...
const PLAYER_RADIUS: f32 = 0.5;
const BULLET_RADIUS: f32 = 0.025;
pub fn kill_players(
    images: Res<ImageAssets>,
    sounds: Res<SoundAssets>,
    frame: Res<GGFrameCount>,
    mut sound_id: ResMut<SoundIdSeed>,
    mut commands: Commands,
    players: Query<(Entity, &Player, &Transform), (Without<Bullet>,(Without<MarkedForDeath>))>,
    bullets: Query<(Entity, &Transform), With<Bullet>>,
) {
    for (player_entity, player, player_transform) in &players {
        for (bullet_entity, bullet_transform) in &bullets {
            let distance = Vec2::distance(
                player_transform.translation.xy(),
                bullet_transform.translation.xy(),
            );
            if distance < PLAYER_RADIUS + BULLET_RADIUS {
                // spawn a hit animation
                spawn_explosion(&mut commands, &images, *bullet_transform);

                let snd = sound_id.next_us(player.handle);
                debug!("explosion snd {snd:#00x}");
                commands.spawn(
                    (
                        RollbackSoundBundle {
                            sound: RollbackSound {
                                clip: sounds.swoosh_death.clone(),
                                start_frame: frame.frame,
                                sub_key: snd,
                            },
                            transform: Transform::from_translation(bullet_transform.translation),
                            ..default()
                        },
                    )
                )
                .add_rollback();

                // mark player for death (despawn in half a second if not rolled back)
                commands.entity(player_entity).insert(MarkedForDeath::default());
                // remove the bullet from the game
                commands.entity(bullet_entity).despawn_recursive();
            }
        }
    }
}

// we despawn the players after a timer delay in case the network messed up the bullet hit registration
pub fn process_deaths(
    mut marks: Query<&mut MarkedForDeath, With<Player>>,
    mut next_state: ResMut<NextState<RollbackState>>,
) {
    for mut marked in &mut marks {

        marked.0.tick(Duration::from_secs_f64(1. / 60.));// tick at the ggrs network framerate of 60 fps

        if marked.0.just_finished() {
            next_state.set(RollbackState::RoundEnd);
        }
    }
}

/// when the round ends, kill and score every player currently marked for death regardless of who shot first
pub fn count_points_and_despawn(
    mut commands: Commands,
    players: Query<(Entity, &Player), With<MarkedForDeath>>,
    mut scores: ResMut<Scores>,
) {
    for (player_entity, player_component) in &players {
        if player_component.handle == 0 {
            scores.1 += 1;
        } else {
            scores.0 += 1;
        }

        info!("player died: {scores:?}");

        commands.entity(player_entity).despawn_recursive();
    }
}