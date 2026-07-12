use std::collections::VecDeque;

use bevy::prelude::*;

use super::{components::MapBlock, player::grid_to_world, GameSeed, RollbackState, MAP_SIZE};

const MAP_DOMAIN: u64 = 0x6d61_705f_726f_756e;
const WALL_PERCENT: u64 = 23;

#[derive(Default, Copy, Clone, PartialEq, Eq)]
pub enum CellType {
    #[default]
    Empty,
    WallBlock,
}

#[derive(Resource)]
pub struct Map<T: Sized, const WIDTH: usize, const HEIGHT: usize> {
    pub cells: [[T; WIDTH]; HEIGHT],
}

impl<const SIZE: usize> Map<CellType, SIZE, SIZE> {
    fn generated(seed: u64) -> Self {
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

        Self { cells }
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
    mut state: ResMut<NextState<RollbackState>>,
) {
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
            if map_data.cells[x][y] != CellType::WallBlock {
                continue;
            }
            commands.spawn((
                MapBlock,
                SpriteBundle {
                    transform: Transform::from_translation(
                        grid_to_world((x as u32, y as u32)).extend(-1.),
                    ),
                    sprite: Sprite {
                        color: Color::rgb(0.2, 0.3, 0.2),
                        custom_size: Some(Vec2::ONE),
                        ..default()
                    },
                    ..default()
                },
            ));
        }
    }
}

pub fn clear_map_sprites(mut commands: Commands, blocks: Query<Entity, With<MapBlock>>) {
    for entity in &blocks {
        commands.entity(entity).despawn_recursive();
    }
}
