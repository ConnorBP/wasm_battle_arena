use bevy::prelude::*;
use bevy_asset_loader::prelude::*;

use super::components::{AnimateOnce, AnimationTimer};

#[derive(AssetCollection, Resource)]
pub struct ImageAssets {
    #[asset(path = "bullet.png")]
    pub bullet: Handle<Image>,
    #[asset(path = "ghost_base.png")]
    pub ghost: Handle<Image>,
    #[asset(texture_atlas(tile_size_x = 16., tile_size_y = 16., columns = 8, rows = 1, padding_x = 0., padding_y = 0., offset_x = 0., offset_y = 0.))]
    #[asset(path = "eyes.png")]
    pub eyes: Handle<TextureAtlas>,
    #[asset(texture_atlas(tile_size_x = 16., tile_size_y = 16., columns = 3, rows = 1, padding_x = 0., padding_y = 0., offset_x = 0., offset_y = 0.))]
    #[asset(path = "boom.png")]
    pub explosion: Handle<TextureAtlas>,
}

pub fn animate_effects(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(
        Entity,
        &AnimateOnce,
        &mut AnimationTimer,
        &mut TextureAtlasSprite,
    )>,
) {
    for (ent, max_indice, mut timer, mut sprite) in &mut query {
        timer.tick(time.delta());
        if timer.just_finished() {
            if sprite.index == max_indice.0-1 {
                // kill the animation entity when the animation ends
                commands.entity(ent).despawn_recursive();
                continue;
            }
            sprite.index +=1;
        }
    }
}

pub fn spawn_explosion(
    commands: &mut Commands,
    images: &Res<ImageAssets>,
    transform: Transform,
) {
    // spawn an explosion (not synced)
    commands.spawn((
        SpriteSheetBundle {
            sprite: TextureAtlasSprite {
                index: 0,
                custom_size: Some(Vec2::new(1.,1.)),
                ..default()
            },
            texture_atlas: images.explosion.clone(),
            transform,
            ..default()
        },
        AnimateOnce(3),
        AnimationTimer(Timer::from_seconds(0.1, TimerMode::Repeating)),
    ));
}