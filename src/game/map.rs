use std::collections::VecDeque;

use bevy::prelude::*;

use bevy_ggrs::AddRollbackCommandExtension;

use super::{
    assets::procedural::{shield_pickup_color, speed_pickup_color, trap_color, wall_face_color, wall_foundation_color, PICKUP_SIZE, TRAP_SIZE},
    components::{MapBlock, ShieldPickup, SpeedPickup}, player::grid_to_world,
    GameSeed, RollbackState, RoundProgress, MAP_SIZE,
};

const MAP_DOMAIN: u64 = 0x6d61_705f_726f_756e;
const TRAP_DOMAIN: u64 = 0x7472_6170_5f70_6169;
const PICKUP_DOMAIN: u64 = 0x7069_636b_7570_7061;
const SHIELD_DOMAIN: u64 = 0x7368_6965_6c64_7061;
const WALL_PERCENT: u64 = 23;

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Reflect)]
pub enum CellType {
    #[default]
    Empty,
    WallBlock,
    Trap,
    SpeedPickup,
    ShieldPickup,
}

#[derive(Resource, Reflect, Clone)]
#[reflect(Resource)]
pub struct Map<T: Sized + Default + Copy, const WIDTH: usize, const HEIGHT: usize> {
    pub cells: [[T; WIDTH]; HEIGHT],
}

impl<T: Default + Copy, const WIDTH: usize, const HEIGHT: usize> Default for Map<T, WIDTH, HEIGHT> {
    fn default() -> Self {
        Self { cells: [[T::default(); WIDTH]; HEIGHT] }
    }
}

impl<const SIZE: usize> Map<CellType, SIZE, SIZE> {
    pub(crate) fn generated(seed: u64) -> Self {
        let mut cells = [[CellType::Empty; SIZE]; SIZE];
        let center = SIZE / 2;

        for x in 0..SIZE {
            for y in 0..SIZE {
                let mirror = (SIZE - 1 - x, SIZE - 1 - y);
                if (x, y) > mirror || x == 0 || y == 0 || x + 1 == SIZE || y + 1 == SIZE {
                    continue;
                }
                if x == center || y == center {
                    continue;
                }

                let coordinate = ((x as u64) << 32) | y as u64;
                if splitmix64(seed ^ MAP_DOMAIN ^ coordinate) % 100 < WALL_PERCENT {
                    cells[x][y] = CellType::WallBlock;
                    cells[mirror.0][mirror.1] = CellType::WallBlock;
                }
            }
        }

        keep_center_region(&mut cells, center);
        let empty_cells = cells
            .iter()
            .flatten()
            .filter(|cell| **cell == CellType::Empty)
            .count();
        if empty_cells * 2 < SIZE * SIZE {
            cells = [[CellType::Empty; SIZE]; SIZE];
        }

        place_feature_pair(&mut cells, seed ^ TRAP_DOMAIN, center, CellType::Trap);
        place_feature_pair(&mut cells, seed ^ PICKUP_DOMAIN, center, CellType::SpeedPickup);
        place_feature_pair(&mut cells, seed ^ SHIELD_DOMAIN, center, CellType::ShieldPickup);
        Self { cells }
    }
}

fn place_feature_pair<const SIZE: usize>(
    cells: &mut [[CellType; SIZE]; SIZE],
    seed: u64,
    center: usize,
    feature: CellType,
) {
    let mut candidates = Vec::new();
    for x in 1..SIZE.saturating_sub(1) {
        for y in 1..SIZE.saturating_sub(1) {
            let mirror = (SIZE - 1 - x, SIZE - 1 - y);
            if (x, y) >= mirror || x == center || y == center {
                continue;
            }
            if cells[x][y] == CellType::Empty && cells[mirror.0][mirror.1] == CellType::Empty {
                candidates.push([(x, y), mirror]);
            }
        }
    }

    if candidates.is_empty() {
        return;
    }
    let index = (splitmix64(seed) % candidates.len() as u64) as usize;
    for &(x, y) in &candidates[index] {
        cells[x][y] = feature;
    }
}

fn keep_center_region<const SIZE: usize>(cells: &mut [[CellType; SIZE]; SIZE], center: usize) {
    let mut reachable = [[false; SIZE]; SIZE];
    let mut queue = VecDeque::from([(center, center)]);
    reachable[center][center] = true;

    while let Some((x, y)) = queue.pop_front() {
        for (next_x, next_y) in [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ] {
            if next_x < SIZE
                && next_y < SIZE
                && !reachable[next_x][next_y]
                && cells[next_x][next_y] == CellType::Empty
            {
                reachable[next_x][next_y] = true;
                queue.push_back((next_x, next_y));
            }
        }
    }

    for x in 0..SIZE {
        for y in 0..SIZE {
            if cells[x][y] == CellType::Empty && !reachable[x][y] {
                cells[x][y] = CellType::WallBlock;
            }
        }
    }
}

pub(crate) fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

pub fn generate_map(
    mut commands: Commands,
    mut seed: ResMut<GameSeed>,
    mut progress: ResMut<RoundProgress>,
    mut state: ResMut<NextState<RollbackState>>,
) {
    *progress = RoundProgress::default();
    commands.insert_resource(Map::<CellType, MAP_SIZE, MAP_SIZE>::generated(seed.0));
    seed.0 = splitmix64(seed.0 ^ MAP_DOMAIN);
    state.set(RollbackState::InRound);
}

pub fn spawn_map_sprites(
    mut commands: Commands,
    map_data: Res<Map<CellType, MAP_SIZE, MAP_SIZE>>,
) {
    for x in 0..MAP_SIZE {
        for y in 0..MAP_SIZE {
            let wall_neighbors = [
                x > 0 && map_data.cells[x - 1][y] == CellType::WallBlock,
                x + 1 < MAP_SIZE && map_data.cells[x + 1][y] == CellType::WallBlock,
                y > 0 && map_data.cells[x][y - 1] == CellType::WallBlock,
                y + 1 < MAP_SIZE && map_data.cells[x][y + 1] == CellType::WallBlock,
            ].into_iter().filter(|neighbor| *neighbor).count();
            let (color, size) = match map_data.cells[x][y] {
                CellType::WallBlock => (
                    if wall_neighbors == 0 { wall_foundation_color() } else { wall_face_color(1.0 + wall_neighbors as f32 * 0.035) },
                    Vec2::splat(if wall_neighbors < 2 { 0.86 } else { 0.96 }),
                ),
                CellType::Trap => (trap_color(), Vec2::splat(TRAP_SIZE)),
                CellType::SpeedPickup => {
                    commands.spawn((
                        SpeedPickup { cell: (x as u16, y as u16) },
                        SpriteBundle {
                            transform: Transform::from_translation(
                                grid_to_world((x as u32, y as u32)).extend(0.),
                            ),
                            sprite: Sprite {
                                color: speed_pickup_color(),
                                custom_size: Some(Vec2::splat(PICKUP_SIZE)),
                                ..default()
                            },
                            ..default()
                        },
                    )).add_rollback();
                    continue;
                }
                CellType::ShieldPickup => {
                    commands.spawn((
                        ShieldPickup { cell: (x as u16, y as u16) },
                        SpriteBundle {
                            transform: Transform::from_translation(
                                grid_to_world((x as u32, y as u32)).extend(0.),
                            ),
                            sprite: Sprite {
                                color: shield_pickup_color(),
                                custom_size: Some(Vec2::splat(PICKUP_SIZE)),
                                ..default()
                            },
                            ..default()
                        },
                    )).add_rollback();
                    continue;
                }
                CellType::Empty => continue,
            };
            commands.spawn((
                MapBlock,
                SpriteBundle {
                    transform: Transform::from_translation(
                        grid_to_world((x as u32, y as u32)).extend(-1.),
                    ),
                    sprite: Sprite {
                        color,
                        custom_size: Some(size),
                        ..default()
                    },
                    ..default()
                },
            ));
        }
    }
}

pub fn clear_map_sprites(
    mut commands: Commands,
    blocks: Query<Entity, With<MapBlock>>,
    pickups: Query<Entity, Or<(With<SpeedPickup>, With<ShieldPickup>)>>,
) {
    for entity in blocks.iter().chain(pickups.iter()) {
        commands.entity(entity).despawn_recursive();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_map_is_deterministic_symmetric_and_has_one_trap_pair() {
        let first = Map::<CellType, MAP_SIZE, MAP_SIZE>::generated(42);
        let second = Map::<CellType, MAP_SIZE, MAP_SIZE>::generated(42);
        let mut traps = 0;
        let mut pickups = 0;
        let mut shields = 0;

        for x in 0..MAP_SIZE {
            for y in 0..MAP_SIZE {
                assert_eq!(first.cells[x][y], second.cells[x][y]);
                assert_eq!(first.cells[x][y], first.cells[MAP_SIZE - 1 - x][MAP_SIZE - 1 - y]);
                if matches!(first.cells[x][y], CellType::Trap | CellType::SpeedPickup | CellType::ShieldPickup) {
                    assert!(x > 0 && y > 0 && x + 1 < MAP_SIZE && y + 1 < MAP_SIZE);
                    assert_ne!(x, MAP_SIZE / 2);
                    assert_ne!(y, MAP_SIZE / 2);
                }
                traps += usize::from(first.cells[x][y] == CellType::Trap);
                pickups += usize::from(first.cells[x][y] == CellType::SpeedPickup);
                shields += usize::from(first.cells[x][y] == CellType::ShieldPickup);
            }
        }

        assert_eq!(traps, 2);
        assert_eq!(pickups, 2);
        assert_eq!(shields, 2);
        assert_eq!(first.cells[MAP_SIZE / 2][MAP_SIZE / 2], CellType::Empty);
        assert_eq!(first.cells[0][0], CellType::Empty);
        assert_eq!(first.cells[MAP_SIZE - 1][MAP_SIZE - 1], CellType::Empty);
    }
}
