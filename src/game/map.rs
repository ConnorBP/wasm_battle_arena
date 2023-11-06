use bevy::prelude::*;

use super::{components::MapBlock, MAP_SIZE, player::grid_to_world, RollbackState};

#[derive(Default, Copy, Clone)]
pub enum CellType {
    // nothing in this cell
    #[default]
    Empty,
    // filled with a basic collidable block type
    WallBlock,
}

#[derive(Resource)]
pub struct Map<T: Sized, const WIDTH: usize, const HEIGHT: usize> {
    pub cells: [[T; WIDTH]; HEIGHT],
}

impl<T: Sized + Default + Copy, const WIDTH: usize, const HEIGHT: usize> Map<T, WIDTH, HEIGHT> {
    fn new() -> Self {

        Self {
            cells: [[T::default(); WIDTH];HEIGHT]
        }
    }
}

impl<const WIDTH: usize, const HEIGHT: usize> Map<CellType, WIDTH, HEIGHT> {
    /// generate some extremely rudementary block layouts based on intervals for testing
    fn test_map() -> Self {
        let mut cells = [[CellType::default(); WIDTH];HEIGHT];
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                if ((x+4) % 6 == 0 || (x+4) % 9 == 0) && !(y % 4 == 0 || (y+1)%4 ==0 ) {
                    cells[x][y] = CellType::WallBlock;
                }
            }
        }

        Self {
            cells
        }
    }
}

/// Generate Map system generates the map and inserts it as a resource at the beginning of each round
pub fn generate_map(
    mut commands: Commands,
    mut state: ResMut<NextState<RollbackState>>,
) {
    let map = Map::<CellType, MAP_SIZE, MAP_SIZE>::test_map();

    commands.insert_resource(map);
    state.set(RollbackState::InRound);
}

/// creates sprites for the maps block types after map generation is complete
pub fn spawn_map_sprites(
    mut commands: Commands,
    map_data: Res<Map<CellType, MAP_SIZE, MAP_SIZE>>,
) {
    for x in 0..MAP_SIZE {
        for y in 0..MAP_SIZE {
            match map_data.cells[x][y] {
                CellType::WallBlock => {
                    commands.spawn((
                        MapBlock,
                        SpriteBundle {
                            transform: Transform::from_translation(
                                grid_to_world((x as u32,y as u32))
                                .extend(-1.)
                            ),
                            sprite: Sprite {
                                color: Color::rgb(0.2, 0.3, 0.2),
                                custom_size: Some(Vec2::new(1., 1.)),
                                ..default()
                            },
                            ..default()
                        }
                    ));
                },
                _=>{},
            }
        }
    }
}

/// clears the screen of all map sprites
pub fn clear_map_sprites(
    mut commands: Commands,
    blocks: Query<Entity, With<MapBlock>>
) {
    for entity in &blocks {
        commands.entity(entity).despawn_recursive();
    }
}