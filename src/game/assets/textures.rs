use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use bevy_asset_loader::prelude::*;

use crate::game::{
    components::{AnimateOnce, AnimationTimer, ExplosionCue},
    ggrs_framecount::GGFrameCount,
    session::PlayerId,
};

#[derive(AssetCollection, Resource)]
pub struct ImageAssets {
    #[asset(path = "textures/objects/bullet.png")]
    pub bullet: Handle<Image>,
    #[asset(path = "textures/character/ghost_base.png")]
    pub ghost: Handle<Image>,
    #[asset(path = "textures/character/cosmetics/ghost_crown.png")]
    pub ghost_crown: Handle<Image>,
    #[asset(path = "textures/character/cosmetics/ghost_wizard.png")]
    pub ghost_wizard: Handle<Image>,
    #[asset(path = "textures/character/cosmetics/ghost_bow.png")]
    pub ghost_bow: Handle<Image>,
    #[asset(texture_atlas(
        tile_size_x = 16.,
        tile_size_y = 16.,
        columns = 8,
        rows = 1,
        padding_x = 0.,
        padding_y = 0.,
        offset_x = 0.,
        offset_y = 0.
    ))]
    #[asset(path = "textures/character/eyes.png")]
    pub eyes: Handle<TextureAtlas>,
    #[asset(texture_atlas(
        tile_size_x = 16.,
        tile_size_y = 16.,
        columns = 3,
        rows = 1,
        padding_x = 0.,
        padding_y = 0.,
        offset_x = 0.,
        offset_y = 0.
    ))]
    #[asset(path = "textures/fx/boom.png")]
    pub explosion: Handle<TextureAtlas>,
}

pub fn animate_effects(
    time: Res<Time>,
    mut query: Query<(&AnimateOnce, &mut AnimationTimer, &mut TextureAtlasSprite)>,
) {
    for (frame_count, mut timer, mut sprite) in &mut query {
        timer.tick(time.delta());
        if timer.just_finished() && sprite.index + 1 < frame_count.0 {
            sprite.index += 1;
        }
    }
}

#[derive(Resource, Default)]
pub struct PresentedExplosions {
    spawned: HashMap<(u32, PlayerId), Entity>,
}

pub fn clear_explosion_presentations(
    mut commands: Commands,
    mut presented: ResMut<PresentedExplosions>,
) {
    for (_, entity) in presented.spawned.drain() {
        commands.entity(entity).despawn_recursive();
    }
}

const EXPLOSION_CUE_LIFETIME_FRAMES: u32 = 18;

fn explosion_cue_finished(current_frame: u32, start_frame: u32) -> bool {
    current_frame.wrapping_sub(start_frame) >= EXPLOSION_CUE_LIFETIME_FRAMES
}

pub fn remove_finished_explosion_cues(
    mut commands: Commands,
    frame: Res<GGFrameCount>,
    cues: Query<(Entity, &ExplosionCue)>,
) {
    for (entity, cue) in &cues {
        if explosion_cue_finished(frame.frame, cue.frame) {
            commands.entity(entity).despawn_recursive();
        }
    }
}

/// Reconciles rollback-safe cue records into presentation-only animation entities.
/// A cue disappearing after a rollback also removes its old visual.
pub fn sync_explosion_cues(
    mut commands: Commands,
    images: Res<ImageAssets>,
    cues: Query<(&ExplosionCue, &Transform)>,
    mut presented: ResMut<PresentedExplosions>,
) {
    let mut live = HashSet::new();
    for (cue, transform) in &cues {
        let key = (cue.frame, cue.player_id);
        live.insert(key);
        presented.spawned.entry(key).or_insert_with(|| {
            commands
                .spawn((
                    SpriteSheetBundle {
                        sprite: TextureAtlasSprite {
                            index: 0,
                            custom_size: Some(Vec2::new(1., 1.)),
                            ..default()
                        },
                        texture_atlas: images.explosion.clone(),
                        transform: *transform,
                        ..default()
                    },
                    AnimateOnce(3),
                    AnimationTimer(Timer::from_seconds(0.1, TimerMode::Repeating)),
                ))
                .id()
        });
    }

    presented.spawned.retain(|key, entity| {
        if live.contains(key) {
            true
        } else {
            commands.entity(*entity).despawn_recursive();
            false
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cue_lifetime_is_exact_and_handles_frame_wraparound() {
        assert!(!explosion_cue_finished(17, 0));
        assert!(explosion_cue_finished(18, 0));
        assert!(!explosion_cue_finished(3, u32::MAX - 13));
        assert!(explosion_cue_finished(3, u32::MAX - 14));
    }
}
