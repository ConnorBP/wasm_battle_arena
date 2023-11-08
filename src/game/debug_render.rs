// a collection of systems to output visual debugging information

use bevy::{prelude::*, math::Vec3Swizzles};
use super::{player::{grid_to_world, world_to_grid}, components::{MoveDir, Player}};

// marker component for debug blocks
#[derive(Component)]
pub struct DebugBlock;

/// spawns the sprites for display in our debug renderer
pub fn spawn_debug_sprites(
    mut commands: Commands,
    mut dbg_ents: ResMut<DebugEntitiesList>,
) {
    const PLAYER_COUNT: u32 = 2;
    // we draw 6 squares per player
    const DEBUG_SQUARES: u32 = 6;

    const COUNT: u32 = (DEBUG_SQUARES*PLAYER_COUNT);

    for i in 0..COUNT {
        dbg_ents.list.push(
            commands.spawn((
                DebugBlock,
                SpriteBundle {
                    transform: Transform::from_translation(
                        grid_to_world((i % 3 as u32,i/3 as u32))
                        .extend(-1.)
                    ),
                    sprite: Sprite {
                        color: Color::rgb(0.7, 0.7, 0.7),
                        custom_size: Some(Vec2::new(1., 1.)),
                        ..default()
                    },
                    ..default()
                }
            ))
            .id()
        )
    }

}

#[derive(Resource, Default)]
pub struct DebugEntitiesList {
    list: Vec<Entity>,
}


/// System that moves the debug sprites to current player locations.
/// Visualizes worldspace to grid conversion
/// as well as which grid items are being collision tested.
pub fn update_debug_sprites(
    // mut commands: Commands,
    dbg_ents: Res<DebugEntitiesList>,
    mut blocks: Query<(&mut Sprite, &mut Transform), (With<DebugBlock>,Without<Player>)>,
    players: Query<(&Transform, &MoveDir), With<Player>>,
) {
    let mut blocks_iter = blocks.iter_many_mut(&dbg_ents.list);

    for (transform, movedir) in &players {
        // draw player pos
        if let Some((x,y)) = world_to_grid(transform.translation.xy()) {
            match blocks_iter.fetch_next() {
                Some((mut sprite, mut transform)) => {
                    sprite.color = Color::rgb(0.7, movedir.0.x, movedir.0.y);
                    transform.translation = grid_to_world((x as u32,y as u32)).extend(0.);
                },
                _=> {},
            }
        }

        // draw checked squares
        // check the cells in the horizontal, vertical, and forward directions
        let h_block = world_to_grid(transform.translation.xy() + Vec2::new(movedir.0.x.signum(), 0.0));
        let v_block = world_to_grid(transform.translation.xy() + Vec2::new(0.0, movedir.0.y.signum()));
        let hv_block = world_to_grid(transform.translation.xy() + Vec2::new(movedir.0.x.signum(), movedir.0.y.signum()));
        let h2_block = world_to_grid(transform.translation.xy() + Vec2::new(movedir.0.x.signum(), -movedir.0.y.signum()));
        let v2_block = world_to_grid(transform.translation.xy() + Vec2::new(-movedir.0.x.signum(), movedir.0.y.signum()));

        if let Some((x,y)) = h_block {
            match blocks_iter.fetch_next() {
                Some((mut sprite,mut transform)) => {
                    sprite.color = Color::rgb(0.2, 0.2, 0.2);
                    transform.translation = grid_to_world((x as u32,y as u32)).extend(-1.);
                },
                _=> {},
            }
        }

        if let Some((x,y)) = h2_block {
            match blocks_iter.fetch_next() {
                Some((mut sprite,mut transform)) => {
                    sprite.color = Color::rgb(0.4, 0.2, 0.2);
                    transform.translation = grid_to_world((x as u32,y as u32)).extend(-1.);
                },
                _=> {},
            }
        }

        if let Some((x,y)) = v_block {
            match blocks_iter.fetch_next() {
                Some((mut sprite,mut transform)) => {
                    sprite.color = Color::rgb(0.2, 0.2, 0.2);
                    transform.translation = grid_to_world((x as u32,y as u32)).extend(-1.);
                },
                _=> {},
            }
        }

        if let Some((x,y)) = v2_block {
            match blocks_iter.fetch_next() {
                Some((mut sprite,mut transform)) => {
                    sprite.color = Color::rgb(0.2, 0.2, 0.4);
                    transform.translation = grid_to_world((x as u32,y as u32)).extend(-1.);
                },
                _=> {},
            }
        }

        if let Some((x,y)) = hv_block {
            match blocks_iter.fetch_next() {
                Some((mut sprite,mut transform)) => {
                    sprite.color = Color::rgb(0.2, 0.2, 0.2);
                    transform.translation = grid_to_world((x as u32,y as u32)).extend(-1.);
                },
                _=> {},
            }
        }
    }
}