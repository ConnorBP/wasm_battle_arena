use std::{collections::HashSet, time::Duration};

use bevy::{prelude::*, math::Vec3Swizzles};
use bevy_ggrs::{PlayerInputs, AddRollbackCommandExtension};
use crate::game::rollback_audio::RollbackSoundBundle;

use super::{components::*, MAP_SIZE, assets::{textures::{ImageAssets, spawn_explosion}, sounds::SoundAssets}, Elimination, RollbackState, RoundProgress, Scores, GameSeed, map::{splitmix64, CellType, Map}, rollback_audio::RollbackSound, ggrs_framecount::GGFrameCount, SoundIdSeed};
use super::networking::GgrsConfig;
use super::session::{round_outcome, PlayerId, RoundBootstrap, RoundOutcome};
use super::input;


pub fn move_players(
    inputs: Res<PlayerInputs<GgrsConfig>>,
    mut players: Query<(&mut Transform, &mut MoveDir, &Player, Option<&SpeedBoost>), Without<MarkedForDeath>>,
    map_data: Res<Map<CellType, MAP_SIZE, MAP_SIZE>>,
) {
    for (mut transform, mut move_dir, player, speed_boost) in &mut players {
        let (input, _) = inputs[player.handle];
        let direction = input::direction(input);

        if direction == Vec2::ZERO {
            continue;
        }

        move_dir.0 = direction;

        let move_speed = movement_speed(speed_boost.is_some());
        let old_pos = transform.translation.xy();
        let requested_delta = direction * move_speed;
        let move_delta = resolve_player_movement(&map_data, old_pos, requested_delta);
        let limit = Vec2::splat(map_data.active_size as f32 / 2. - 0.5);
        let new_pos = (old_pos + move_delta).clamp(-limit, limit);

        transform.translation.x = new_pos.x;
        transform.translation.y = new_pos.y;
    }
}

const BASE_MOVE_SPEED: f32 = 0.13;
const BOOSTED_MOVE_SPEED: f32 = 0.1755;
const SPEED_BOOST_FRAMES: u16 = 300;

fn movement_speed(boosted: bool) -> f32 {
    if boosted { BOOSTED_MOVE_SPEED } else { BASE_MOVE_SPEED }
}

fn tick_boost_frames(frames_left: u16) -> Option<u16> {
    match frames_left {
        0 | 1 => None,
        frames => Some(frames - 1),
    }
}

const PLAYER_COLLIDER_HALF_SIZE: f32 = 0.4;
const WALL_COLLIDER_HALF_SIZE: f32 = 0.5;

fn resolve_player_movement(
    map_data: &Map<CellType, MAP_SIZE, MAP_SIZE>,
    old_pos: Vec2,
    requested_delta: Vec2,
) -> Vec2 {
    if !player_hits_wall(map_data, old_pos + requested_delta) {
        return requested_delta;
    }
    let horizontal = Vec2::new(requested_delta.x, 0.0);
    let vertical = Vec2::new(0.0, requested_delta.y);
    let horizontal_clear = !player_hits_wall(map_data, old_pos + horizontal);
    let vertical_clear = !player_hits_wall(map_data, old_pos + vertical);
    match (horizontal_clear, vertical_clear) {
        (true, false) => horizontal,
        (false, true) => vertical,
        (true, true) if requested_delta.x.abs() >= requested_delta.y.abs() => horizontal,
        (true, true) => vertical,
        (false, false) => Vec2::ZERO,
    }
}

fn player_hits_wall(map_data: &Map<CellType, MAP_SIZE, MAP_SIZE>, player_pos: Vec2) -> bool {
    map_data.cells.iter().enumerate().any(|(x, column)| {
        column.iter().enumerate().any(|(y, cell)| {
            matches!(*cell, CellType::WallBlock | CellType::Void)
                && wall_check(player_pos, grid_to_world((x as u32, y as u32)))
        })
    })
}

fn wall_check(player: Vec2, wall: Vec2) -> bool {
    let collision_distance = PLAYER_COLLIDER_HALF_SIZE + WALL_COLLIDER_HALF_SIZE;
    let distance = (player - wall).abs();
    distance.x < collision_distance && distance.y < collision_distance
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
        let profile = bootstrap.profiles.iter().find(|profile| profile.player_id == entry.player_id).expect("validated profile exists");
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
            entry.player_id,
            if look == Vec2::ZERO { Vec2::X } else { look },
            world.extend(100.),
            COLORS[profile.palette_id as usize],
            profile.cosmetic_id,
            &profile.name,
        );
    }
}

fn spawn_player(
    commands: &mut Commands,
    images: &Res<ImageAssets>,
    handle: usize,
    player_id: PlayerId,
    move_dir: Vec2,
    translation: Vec3,
    color: Color,
    cosmetic_id: u8,
    display_name: &str,
) {
    let parent = commands.spawn((
        Player { handle, player_id },
        BulletReady(0),
        MoveDir(move_dir),
        SpriteBundle {
            texture: match cosmetic_id {
                1 => images.ghost_crown.clone(),
                2 => images.ghost_wizard.clone(),
                3 => images.ghost_bow.clone(),
                _ => images.ghost.clone(),
            },
            transform: Transform::from_translation(translation),
            sprite: Sprite {
                color,
                custom_size: Some(Vec2::new(1.,1.)),
                ..default()
            },
            ..default()
        },
        Name::new(display_name.to_owned()),
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
        Map { cells: [[CellType::Empty; MAP_SIZE]; MAP_SIZE], active_size: MAP_SIZE }
    }

    fn map_with_walls(walls: &[(usize, usize)]) -> Map<CellType, MAP_SIZE, MAP_SIZE> {
        let mut map = empty_map();
        for &(x, y) in walls {
            map.cells[x][y] = CellType::WallBlock;
        }
        map
    }

    #[test]
    fn wall_collision_and_sliding_are_stable() {
        let wall = grid_to_world((20, 20));
        assert!(!wall_check(wall + Vec2::new(0.9, 0.0), wall));
        assert!(wall_check(wall + Vec2::new(0.899, 0.0), wall));

        let map = map_with_walls(&[(20, 20)]);
        let old = wall + Vec2::new(-1.0, 0.2);
        let resolved = resolve_player_movement(&map, old, Vec2::new(0.2, 0.1));
        assert_eq!(resolved.x, 0.0);
        assert_eq!(resolved.y, 0.1);
    }

    #[test]
    fn held_fire_cooldown_is_exactly_twelve_frames() {
        assert!((10..=12).contains(&FIRE_COOLDOWN_FRAMES));
        let mut cooldown = FIRE_COOLDOWN_FRAMES;
        let mut shot_frames = vec![0];
        for frame in 1..=FIRE_COOLDOWN_FRAMES as usize * 2 {
            cooldown = tick_fire_cooldown(cooldown);
            if cooldown == 0 {
                shot_frames.push(frame);
                cooldown = FIRE_COOLDOWN_FRAMES;
            }
        }
        assert_eq!(shot_frames, vec![0, 12, 24]);
        assert_eq!(tick_fire_cooldown(0), 0);
    }

    #[test]
    fn speed_boost_runs_for_exactly_300_deterministic_frames() {
        let mut frames = Some(SPEED_BOOST_FRAMES);
        let mut distance = 0.0;
        let mut boosted_frames = 0;
        for _ in 0..SPEED_BOOST_FRAMES {
            boosted_frames += usize::from(frames.is_some());
            distance += movement_speed(frames.is_some());
            frames = frames.and_then(tick_boost_frames);
        }
        assert_eq!(boosted_frames, SPEED_BOOST_FRAMES as usize);
        assert_eq!(frames, None);

        let replay = (0..SPEED_BOOST_FRAMES).fold((Some(SPEED_BOOST_FRAMES), 0.0f32), |(frames, position), _| {
            (frames.and_then(tick_boost_frames), position + movement_speed(frames.is_some()))
        });
        assert_eq!(replay.0, None);
        assert_eq!(replay.1.to_bits(), distance.to_bits());
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

/// One shot every 12 rollback frames while fire is held (5 shots/second at
/// 60 Hz). The cooldown is sampled from the shared input bit, so desktop and
/// mobile have identical deterministic behavior.
pub const FIRE_COOLDOWN_FRAMES: u8 = 12;

fn tick_fire_cooldown(frames_left: u8) -> u8 {
    frames_left.saturating_sub(1)
}

pub fn fire_bullets(
    mut commands: Commands,
    frame: Res<GGFrameCount>,
    inputs: Res<PlayerInputs<GgrsConfig>>,
    images: Res<ImageAssets>,
    sounds: Res<SoundAssets>,
    mut sound_id: ResMut<SoundIdSeed>,
    mut players: Query<(Entity, &Transform, &Player, &mut BulletReady, &MoveDir), Without<MarkedForDeath>>,
) {
    let mut firing: Vec<_> = players.iter().filter_map(|(entity, transform, player, ready, direction)| {
        let (input, _) = inputs[player.handle];
        (input::fire(input) && ready.0 == 0).then_some((player.player_id, player.handle, entity, *transform, *direction))
    }).collect();
    firing.sort_by_key(|entry| entry.0);
    for (player_id, handle, entity, transform, move_dir) in firing {
            let player_pos = transform.translation.xy();
            let pos = player_pos + move_dir.0 * (PLAYER_RADIUS + BULLET_RADIUS);
            // spawn bullet entity
            commands.spawn((
                Bullet { id: frame.frame as u64, owner: player_id, owner_handle: handle, active: true },
                move_dir,
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

            let snd = sound_id.next(handle);
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
            

            if let Ok((_, _, _, mut ready, _)) = players.get_mut(entity) {
                ready.0 = FIRE_COOLDOWN_FRAMES;
            }
    }
}

pub fn reload_bullet(mut bullets: Query<&mut BulletReady, With<Player>>) {
    for mut cooldown in &mut bullets {
        cooldown.0 = tick_fire_cooldown(cooldown.0);
    }
}

pub fn tick_speed_boost(
    mut commands: Commands,
    mut players: Query<(Entity, &mut SpeedBoost)>,
) {
    for (entity, mut boost) in &mut players {
        if let Some(frames) = tick_boost_frames(boost.frames_left) {
            boost.frames_left = frames;
        } else {
            commands.entity(entity).remove::<SpeedBoost>();
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

        commands.entity(player_entity).insert(SpeedBoost { frames_left: SPEED_BOOST_FRAMES });
        commands.entity(pickup_entity).despawn_recursive();
        commands.spawn((RollbackSoundBundle {
            sound: RollbackSound {
                clip: sounds.ray.clone(),
                start_frame: frame.frame,
                sub_key: sound_id.next(handle),
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
                sub_key: sound_id.next(handle),
            },
            transform: Transform::from_translation(position),
            ..default()
        },)).add_rollback();
    }
}

pub fn trigger_traps(
    mut commands: Commands,
    frame: Res<GGFrameCount>,
    sounds: Res<SoundAssets>,
    mut sound_id: ResMut<SoundIdSeed>,
    mut progress: ResMut<RoundProgress>,
    map_data: Res<Map<CellType, MAP_SIZE, MAP_SIZE>>,
    players: Query<(Entity, &Player, &Transform), Without<MarkedForDeath>>,
) {
    let mut trapped: Vec<_> = players.iter().filter_map(|(entity, player, transform)| {
        let (x, y) = world_to_grid(transform.translation.xy())?;
        (map_data.cells[x as usize][y as usize] == CellType::Trap)
            .then_some((player.player_id, player.handle, entity, transform.translation))
    }).collect();
    trapped.sort_by_key(|entry| entry.0);
    for (player_id, handle, entity, position) in trapped {
        progress.record_elimination(Elimination { player_id, frame: frame.frame });
        commands.entity(entity).insert(MarkedForDeath::at(frame.frame));
        commands.spawn((RollbackSoundBundle {
            sound: RollbackSound {
                clip: sounds.swoosh_death.clone(),
                start_frame: frame.frame,
                sub_key: sound_id.next(handle),
            },
            transform: Transform::from_translation(position),
            ..default()
        },)).add_rollback();
    }
}

pub fn move_bullets(
    mut commands: Commands,
    frame: Res<GGFrameCount>,
    sounds: Res<SoundAssets>,
    mut sound_id: ResMut<SoundIdSeed>,
    map_data: Res<Map<CellType, MAP_SIZE, MAP_SIZE>>,
    mut bullets: Query<(Entity, &mut Bullet, &mut Transform, &MoveDir)>
) {
    let limit = Vec2::splat(MAP_SIZE as f32 / 2.);
    let mut order: Vec<_> = bullets.iter().map(|(entity, bullet, _, _)| (bullet.owner, bullet.id, entity)).collect();
    order.sort_by_key(|entry| (entry.0, entry.1));
    for (_, _, entity) in order {
        let Ok((_, mut bullet, mut transform, dir)) = bullets.get_mut(entity) else { continue; };
        let delta = (dir.0 * 0.35).extend(0.);
        transform.translation += delta;

        // check if bullet is out of map bounds
        let absolute_pos = transform.translation.xy().abs();
        if absolute_pos.x > limit.x || absolute_pos.y > limit.y {
            // bullet out of bounds, despawn it
            bullet.active = false;
            commands.entity(entity).despawn_recursive();
            continue;
        }  
        // check for block hits
        if let Some((x,y)) = world_to_grid(transform.translation.xy()) {
            // if coords are inside of a wall then its a hit
            match map_data.cells[x as usize][y as usize] {
                CellType::WallBlock | CellType::Void => {
                    // bullet in a block, despawn it
                    bullet.active = false;
                    commands.entity(entity).despawn_recursive();
                    commands.spawn((RollbackSoundBundle {
                        sound: RollbackSound {
                            clip: sounds.ray.clone(),
                            start_frame: frame.frame,
                            sub_key: sound_id.next(bullet.owner_handle),
                        },
                        transform: Transform::from_translation(transform.translation),
                        ..default()
                    },)).add_rollback();
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
    mut progress: ResMut<RoundProgress>,
    mut commands: Commands,
    players: Query<(Entity, &Player, &Transform), (Without<Bullet>, Without<MarkedForDeath>)>,
    bullets: Query<(Entity, &Bullet, &Transform)>,
    mut shields: Query<&mut ShieldCharges>,
) {
    let mut players: Vec<_> = players
        .iter()
        .map(|(entity, player, transform)| (player.handle, player.player_id, entity, transform.translation))
        .collect();
    players.sort_by_key(|player| player.0);
    let mut bullets: Vec<_> = bullets
        .iter()
        .filter(|(_, bullet, _)| bullet.active)
        .map(|(entity, bullet, transform)| (bullet.owner, bullet.id, entity, transform.translation))
        .collect();
    bullets.sort_by_key(|bullet| (bullet.0, bullet.1));
    let mut consumed = HashSet::new();

    for (handle, player_id, player_entity, player_position) in players {
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
                        sub_key: sound_id.next(handle),
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
                    sub_key: sound_id.next(handle),
                },
                transform: Transform::from_translation(bullet_position),
                ..default()
            },)).add_rollback();
            progress.record_elimination(Elimination { player_id, frame: frame.frame });
            commands.entity(player_entity).insert(MarkedForDeath::at(frame.frame));
            break;
        }
    }
}

// we despawn the players after a timer delay in case the network messed up the bullet hit registration
pub fn process_deaths(
    mut marked_players: Query<&mut MarkedForDeath>,
    bootstrap: Res<RoundBootstrap>,
    mut progress: ResMut<RoundProgress>,
    mut next_state: ResMut<NextState<RollbackState>>,
) {
    let mut finished_frames = Vec::new();
    for mut marked in &mut marked_players {
        marked.timer.tick(Duration::from_secs_f64(1. / 60.));
        if marked.timer.just_finished() {
            finished_frames.push(marked.frame);
        }
    }
    let Some(&resolved_frame) = finished_frames.iter().min() else { return; };

    let roster: Vec<_> = bootstrap.roster.iter().map(|entry| entry.player_id).collect();
    let eliminated: Vec<_> = match bootstrap.mode {
        super::session::GameMode::Duel => progress.eliminated.iter()
            .filter(|entry| entry.frame == resolved_frame)
            .map(|entry| entry.player_id)
            .collect(),
        super::session::GameMode::Deathmatch => progress.eliminated.iter()
            .map(|entry| entry.player_id)
            .collect(),
    };
    let outcome = round_outcome(bootstrap.mode, &roster, &eliminated, &progress.disconnected);
    if matches!(outcome, RoundOutcome::Complete { .. }) {
        progress.resolved = Some(outcome);
        progress.resolved_frame = Some(resolved_frame);
        next_state.set(RollbackState::RoundEnd);
    }
}

/// Resolve the completed round in stable identity order, then despawn all
/// eliminated entities. Duel scoring intentionally retains the deployed rule
/// where every death awards the opposing player a point.
pub fn count_points_and_despawn(
    mut commands: Commands,
    players: Query<(Entity, &Player), With<MarkedForDeath>>,
    progress: Res<RoundProgress>,
    mut scores: ResMut<Scores>,
) {
    if let Some(outcome) = &progress.resolved {
        scores.apply_outcome(outcome);
    }

    for (player_entity, _) in &players {
        commands.entity(player_entity).despawn_recursive();
    }
    info!("round ended: {scores:?}");
}