// a collection of systems to output visual debugging information

use bevy::{prelude::*, math::Vec3Swizzles};
use super::{player::{grid_to_world, world_to_grid}, components::{MoveDir, Player}};

// marker component for debug blocks
#[derive(Component)]
pub struct DebugBlock;

/// System that spawns sprites at current player locations.
/// NOTE: Currently it is naive and clears and respawns at locations every frame
/// a more efficient version may check currently spawned cells and lazy update
pub fn spawn_debug_sprites(
    mut commands: Commands,
    blocks: Query<Entity, With<DebugBlock>>,
    players: Query<(&Transform, &MoveDir), With<Player>>,
) {
    // first clear old blocks
    for entity in &blocks {
        commands.entity(entity).despawn_recursive();
    }
    // now spawn debug blocks at player positons
    for (transform, movedir) in &players {
        // draw player pos
        if let Some((x,y)) = world_to_grid(transform.translation.xy()) {
            commands.spawn((
                DebugBlock,
                SpriteBundle {
                    transform: Transform::from_translation(
                        grid_to_world((x as u32,y as u32))
                        .extend(-1.)
                    ),
                    sprite: Sprite {
                        color: Color::rgb(0.7, movedir.0.x, movedir.0.y),
                        custom_size: Some(Vec2::new(1., 1.)),
                        ..default()
                    },
                    ..default()
                }
            ));
        }

        // draw checked squares
        // check the cells in the horizontal, vertical, and forward directions
        let h_block = world_to_grid(transform.translation.xy() + Vec2::new(movedir.0.x.signum(), 0.0));
        let v_block = world_to_grid(transform.translation.xy() + Vec2::new(0.0, movedir.0.y.signum()));
        let hv_block = world_to_grid(transform.translation.xy() + Vec2::new(movedir.0.x.signum(), movedir.0.y.signum()));
        let h2_block = world_to_grid(transform.translation.xy() + Vec2::new(movedir.0.x.signum(), -movedir.0.y.signum()));
        let v2_block = world_to_grid(transform.translation.xy() + Vec2::new(-movedir.0.x.signum(), movedir.0.y.signum()));

        if let Some((x,y)) = h_block {
            commands.spawn((
                DebugBlock,
                SpriteBundle {
                    transform: Transform::from_translation(
                        grid_to_world((x as u32,y as u32))
                        .extend(-1.)
                    ),
                    sprite: Sprite {
                        color: Color::rgb(0.2, 0.2, 0.2),
                        custom_size: Some(Vec2::new(1., 1.)),
                        ..default()
                    },
                    ..default()
                }
            ));
        }

        if let Some((x,y)) = h2_block {
            commands.spawn((
                DebugBlock,
                SpriteBundle {
                    transform: Transform::from_translation(
                        grid_to_world((x as u32,y as u32))
                        .extend(-1.)
                    ),
                    sprite: Sprite {
                        color: Color::rgb(0.4, 0.2, 0.2),
                        custom_size: Some(Vec2::new(1., 1.)),
                        ..default()
                    },
                    ..default()
                }
            ));
        }

        if let Some((x,y)) = v_block {
            commands.spawn((
                DebugBlock,
                SpriteBundle {
                    transform: Transform::from_translation(
                        grid_to_world((x as u32,y as u32))
                        .extend(-1.)
                    ),
                    sprite: Sprite {
                        color: Color::rgb(0.2, 0.2, 0.2),
                        custom_size: Some(Vec2::new(1., 1.)),
                        ..default()
                    },
                    ..default()
                }
            ));
        }

        if let Some((x,y)) = v2_block {
            commands.spawn((
                DebugBlock,
                SpriteBundle {
                    transform: Transform::from_translation(
                        grid_to_world((x as u32,y as u32))
                        .extend(-1.)
                    ),
                    sprite: Sprite {
                        color: Color::rgb(0.2, 0.2, 0.4),
                        custom_size: Some(Vec2::new(1., 1.)),
                        ..default()
                    },
                    ..default()
                }
            ));
        }

        if let Some((x,y)) = hv_block {
            commands.spawn((
                DebugBlock,
                SpriteBundle {
                    transform: Transform::from_translation(
                        grid_to_world((x as u32,y as u32))
                        .extend(-1.)
                    ),
                    sprite: Sprite {
                        color: Color::rgb(0.2, 0.2, 0.2),
                        custom_size: Some(Vec2::new(1., 1.)),
                        ..default()
                    },
                    ..default()
                }
            ));
        }
    }
}