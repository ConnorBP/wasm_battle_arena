use std::{collections::HashSet, time::Duration};

use bevy::{prelude::*,math::Vec3Swizzles, sprite::collide_aabb::collide};
use bevy_ggrs::{PlayerInputs, AddRollbackCommandExtension};
use crate::game::rollback_audio::RollbackSoundBundle;

use super::{components::*, MAP_SIZE, assets::{textures::{ImageAssets, spawn_explosion}, sounds::SoundAssets}, RollbackState, Scores, GameSeed, map::{splitmix64, CellType, Map}, rollback_audio::RollbackSound, ggrs_framecount::GGFrameCount, SoundIdSeed};
use super::networking::GgrsConfig;
use super::session::RoundBootstrap;
use super::input;


pub fn move_players(
    inputs: Res<PlayerInputs<GgrsConfig>>,
    mut players: Query<(&mut Transform, &mut MoveDir, &Player, Option<&SpeedBoost>)>,
    map_data: Res<Map<CellType, MAP_SIZE, MAP_SIZE>>,
) {
    for (mut transform, mut move_dir, player, speed_boost) in &mut players {
        let (input, _) = inputs[player.handle];
        let direction = input::direction(input);

        if direction == Vec2::ZERO {
            continue;
        }

        move_dir.0 = direction;

        let move_speed = if speed_boost.is_some() { 0.13 * 1.35 } else { 0.13 };
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

/// Checks the slightly inset player collider against a full map cell.
fn wall_check(player: Vec2, wall: Vec2) -> bool{
    const PLAYER_COLLIDER_SIZE: f32 = 0.8;
    let col = collide(
        player.extend(0.),
        Vec2::splat(PLAYER_COLLIDER_SIZE),
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
    bootstrap: Res<RoundBootstrap>,
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

    // Generate positions in canonical handle order.
    let positions = generate_spawn_positions(seed.0, &map_data, bootstrap.roster.len());

    // Domain-separate the next round's map seed from spawn selection.
    seed.0 = splitmix64(seed.0 ^ 0x7370_6177_6e5f_706f);

    const COLORS: [Color; 4] = [
        Color::rgb(0.8, 0.2, 0.2),
        Color::rgb(0.15, 0.25, 0.8),
        Color::rgb(0.2, 0.75, 0.3),
        Color::rgb(0.75, 0.25, 0.75),
    ];
    for entry in &bootstrap.roster {
        let position = positions[entry.handle];
        let world = grid_to_world(position);
        let look = if bootstrap.roster.len() == 2 {
            (grid_to_world(positions[1 - entry.handle]) - world).normalize_or_zero()
        } else {
            (-world).normalize_or_zero()
        };
        spawn_player(
            &mut commands,
            &images,
            entry.handle,
            if look == Vec2::ZERO { Vec2::X } else { look },
            world.extend(100.),
            COLORS[entry.handle],
        );
    }
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

fn generate_spawn_positions(
    base_seed: u64,
    map_data: &Map<CellType, MAP_SIZE, MAP_SIZE>,
    count: usize,
) -> Vec<(u32, u32)> {
    const SPAWN_SELECTION_DOMAIN: u64 = 0x7370_6177_6e5f_7365;
    assert!((2..=4).contains(&count));
    let mut pairs = Vec::new();

    for x in 0..MAP_SIZE {
        for y in 0..MAP_SIZE {
            let mirror = (MAP_SIZE - 1 - x, MAP_SIZE - 1 - y);
            if (x, y) >= mirror
                || x == mirror.0
                || y == mirror.1
                || x.abs_diff(mirror.0) + y.abs_diff(mirror.1) < MAP_SIZE / 2
                || map_data.cells[x][y] != CellType::Empty
                || map_data.cells[mirror.0][mirror.1] != CellType::Empty
            {
                continue;
            }
            pairs.push([(x as u32, y as u32), (mirror.0 as u32, mirror.1 as u32)]);
        }
    }

    let first = if pairs.is_empty() {
        [(0, 0), ((MAP_SIZE - 1) as u32, (MAP_SIZE - 1) as u32)]
    } else {
        pairs[(splitmix64(base_seed ^ SPAWN_SELECTION_DOMAIN) % pairs.len() as u64) as usize]
    };
    let mut positions = vec![first[0], first[1]];

    if count == 3 {
        positions.push(((MAP_SIZE / 2) as u32, (MAP_SIZE / 2) as u32));
    } else if count == 4 {
        let min_distance = (MAP_SIZE / 4) as u32;
        let second_candidates: Vec<_> = pairs
            .iter()
            .copied()
            .filter(|pair| {
                pair.iter().all(|candidate| {
                    first.iter().all(|existing| {
                        candidate.0.abs_diff(existing.0) + candidate.1.abs_diff(existing.1) >= min_distance
                    })
                })
            })
            .collect();
        let second = if second_candidates.is_empty() {
            [(0, (MAP_SIZE - 1) as u32), ((MAP_SIZE - 1) as u32, 0)]
        } else {
            second_candidates[
                (splitmix64(base_seed ^ SPAWN_SELECTION_DOMAIN ^ 1)
                    % second_candidates.len() as u64) as usize
            ]
        };
        positions.extend(second);
    }

    positions
}

#[cfg(test)]
mod spawn_tests {
    use super::*;

    fn empty_map() -> Map<CellType, MAP_SIZE, MAP_SIZE> {
        Map { cells: [[CellType::Empty; MAP_SIZE]; MAP_SIZE] }
    }

    #[test]
    fn spawn_generation_is_deterministic_unique_and_symmetric() {
        let map = empty_map();
        for count in 2..=4 {
            let first = generate_spawn_positions(77, &map, count);
            assert_eq!(first, generate_spawn_positions(77, &map, count));
            assert_eq!(first.len(), count);
            let unique: HashSet<_> = first.iter().collect();
            assert_eq!(unique.len(), count);
            assert!(first.iter().all(|&(x, y)| map.cells[x as usize][y as usize] == CellType::Empty));
        }

        let two = generate_spawn_positions(77, &map, 2);
        assert_eq!(two[1], ((MAP_SIZE - 1) as u32 - two[0].0, (MAP_SIZE - 1) as u32 - two[0].1));
        let four = generate_spawn_positions(77, &map, 4);
        assert_eq!(four[1], ((MAP_SIZE - 1) as u32 - four[0].0, (MAP_SIZE - 1) as u32 - four[0].1));
        assert_eq!(four[3], ((MAP_SIZE - 1) as u32 - four[2].0, (MAP_SIZE - 1) as u32 - four[2].1));

        let generated = Map::<CellType, MAP_SIZE, MAP_SIZE>::generated(91);
        for count in 2..=4 {
            assert!(generate_spawn_positions(91, &generated, count)
                .iter()
                .all(|&(x, y)| generated.cells[x as usize][y as usize] == CellType::Empty));
        }
    }
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
            let pos = player_pos + move_dir.0 * (PLAYER_RADIUS + BULLET_RADIUS);
            // spawn bullet entity
            commands.spawn((
                Bullet { id: frame.frame as u64, owner: player.handle, active: true },
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
                        transform: Transform::from_translation(transform.translation + move_dir.0.extend(0.)),
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

pub fn tick_speed_boost(
    mut commands: Commands,
    mut players: Query<(Entity, &mut SpeedBoost)>,
) {
    for (entity, mut boost) in &mut players {
        if boost.frames_left <= 1 {
            commands.entity(entity).remove::<SpeedBoost>();
        } else {
            boost.frames_left -= 1;
        }
    }
}

pub fn collect_speed_pickups(
    mut commands: Commands,
    frame: Res<GGFrameCount>,
    sounds: Res<SoundAssets>,
    mut sound_id: ResMut<SoundIdSeed>,
    players: Query<(Entity, &Player, &Transform), Without<MarkedForDeath>>,
    pickups: Query<(Entity, &SpeedPickup)>,
) {
    let mut players: Vec<_> = players
        .iter()
        .filter_map(|(entity, player, transform)| {
            world_to_grid(transform.translation.xy())
                .map(|cell| (player.handle, entity, cell, transform.translation))
        })
        .collect();
    players.sort_by_key(|player| player.0);

    let mut pickups: Vec<_> = pickups.iter().collect();
    pickups.sort_by_key(|(_, pickup)| pickup.cell);

    for (pickup_entity, pickup) in pickups {
        let pickup_cell = (pickup.cell.0 as u32, pickup.cell.1 as u32);
        let Some((handle, player_entity, _, position)) = players
            .iter()
            .find(|player| player.2 == pickup_cell)
            .copied()
        else {
            continue;
        };

        commands.entity(player_entity).insert(SpeedBoost { frames_left: 300 });
        commands.entity(pickup_entity).despawn_recursive();
        commands.spawn((RollbackSoundBundle {
            sound: RollbackSound {
                clip: sounds.ray.clone(),
                start_frame: frame.frame,
                sub_key: sound_id.next_us(handle),
            },
            transform: Transform::from_translation(position),
            ..default()
        },)).add_rollback();
    }
}

pub fn collect_shield_pickups(
    mut commands: Commands,
    frame: Res<GGFrameCount>,
    sounds: Res<SoundAssets>,
    mut sound_id: ResMut<SoundIdSeed>,
    players: Query<(Entity, &Player, &Transform), Without<MarkedForDeath>>,
    pickups: Query<(Entity, &ShieldPickup)>,
) {
    let mut players: Vec<_> = players
        .iter()
        .filter_map(|(entity, player, transform)| {
            world_to_grid(transform.translation.xy())
                .map(|cell| (player.handle, entity, cell, transform.translation))
        })
        .collect();
    players.sort_by_key(|player| player.0);

    let mut pickups: Vec<_> = pickups.iter().collect();
    pickups.sort_by_key(|(_, pickup)| pickup.cell);
    for (pickup_entity, pickup) in pickups {
        let pickup_cell = (pickup.cell.0 as u32, pickup.cell.1 as u32);
        let Some((handle, player_entity, _, position)) = players.iter().find(|player| player.2 == pickup_cell).copied() else {
            continue;
        };
        commands.entity(player_entity).insert(ShieldCharges(1));
        commands.entity(pickup_entity).despawn_recursive();
        commands.spawn((RollbackSoundBundle {
            sound: RollbackSound {
                clip: sounds.ray.clone(),
                start_frame: frame.frame,
                sub_key: sound_id.next_us(handle),
            },
            transform: Transform::from_translation(position),
            ..default()
        },)).add_rollback();
    }
}

pub fn trigger_traps(
    mut commands: Commands,
    map_data: Res<Map<CellType, MAP_SIZE, MAP_SIZE>>,
    players: Query<(Entity, &Transform), (With<Player>, Without<MarkedForDeath>)>,
) {
    for (entity, transform) in &players {
        let Some((x, y)) = world_to_grid(transform.translation.xy()) else {
            continue;
        };
        if map_data.cells[x as usize][y as usize] == CellType::Trap {
            commands.entity(entity).insert(MarkedForDeath::default());
        }
    }
}

pub fn move_bullets(
    mut commands: Commands,
    map_data: Res<Map<CellType, MAP_SIZE, MAP_SIZE>>,
    mut bullets: Query<(Entity, &mut Bullet, &mut Transform, &MoveDir)>
) {
    // map limit for bullet is exactly half map in any direction since the map is centered
    let limit = Vec2::splat(MAP_SIZE as f32 / 2.);
    for (entity, mut bullet, mut transform, dir) in &mut bullets {
        let delta = (dir.0 * 0.35).extend(0.);
        transform.translation += delta;

        // check if bullet is out of map bounds
        let absolute_pos = transform.translation.xy().abs();
        if absolute_pos.x > limit.x || absolute_pos.y > limit.y {
            // bullet out of bounds, despawn it
            bullet.active = false;
            commands.entity(entity).despawn_recursive();
        }  
        // check for block hits
        if let Some((x,y)) = world_to_grid(transform.translation.xy()) {
            // if coords are inside of a wall then its a hit
            match map_data.cells[x as usize][y as usize] {
                CellType::WallBlock => {
                    // bullet in a block, despawn it
                    bullet.active = false;
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
    players: Query<(Entity, &Player, &Transform), (Without<Bullet>, Without<MarkedForDeath>)>,
    bullets: Query<(Entity, &Bullet, &Transform)>,
    mut shields: Query<&mut ShieldCharges>,
) {
    let mut players: Vec<_> = players
        .iter()
        .map(|(entity, player, transform)| (player.handle, entity, transform.translation))
        .collect();
    players.sort_by_key(|player| player.0);
    let mut bullets: Vec<_> = bullets
        .iter()
        .filter(|(_, bullet, _)| bullet.active)
        .map(|(entity, bullet, transform)| (bullet.owner, bullet.id, entity, transform.translation))
        .collect();
    bullets.sort_by_key(|bullet| (bullet.0, bullet.1));
    let mut consumed = HashSet::new();

    for (handle, player_entity, player_position) in players {
        let mut shield_available = shields
            .get_mut(player_entity)
            .map(|shield| shield.0 > 0)
            .unwrap_or(false);

        for (_, _, bullet_entity, bullet_position) in bullets.iter().copied() {
            if consumed.contains(&bullet_entity)
                || Vec2::distance(player_position.xy(), bullet_position.xy()) >= PLAYER_RADIUS + BULLET_RADIUS
            {
                continue;
            }
            consumed.insert(bullet_entity);
            commands.entity(bullet_entity).despawn_recursive();

            if shield_available {
                shield_available = false;
                if let Ok(mut shield) = shields.get_mut(player_entity) {
                    shield.0 = shield.0.saturating_sub(1);
                    if shield.0 == 0 {
                        commands.entity(player_entity).remove::<ShieldCharges>();
                    }
                }
                commands.spawn((RollbackSoundBundle {
                    sound: RollbackSound {
                        clip: sounds.ray.clone(),
                        start_frame: frame.frame,
                        sub_key: sound_id.next_us(handle),
                    },
                    transform: Transform::from_translation(bullet_position),
                    ..default()
                },)).add_rollback();
                continue;
            }

            spawn_explosion(&mut commands, &images, Transform::from_translation(bullet_position));
            commands.spawn((RollbackSoundBundle {
                sound: RollbackSound {
                    clip: sounds.swoosh_death.clone(),
                    start_frame: frame.frame,
                    sub_key: sound_id.next_us(handle),
                },
                transform: Transform::from_translation(bullet_position),
                ..default()
            },)).add_rollback();
            commands.entity(player_entity).insert(MarkedForDeath::default());
            break;
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