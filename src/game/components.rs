
use bevy::prelude::*;

#[derive(Component)]
pub struct Player {
    pub handle: usize,
}

#[derive(Component, Reflect, Default)]
pub struct BulletReady(pub bool);

#[derive(Component, Reflect, Default)]
pub struct Bullet;

#[derive(Component, Reflect, Default, Clone, Copy)]
pub struct MoveDir(pub Vec2);

#[derive(Component, Reflect, Default)]
pub struct LookTowardsParentMove;

#[derive(Component, Reflect)]
pub struct MarkedForDeath(pub(crate) Timer);

// marker component for map blocks
#[derive(Component)]
pub struct MapBlock;


// non synced for animation only

/// When paired with a TextureAtlasSprite and an AnimationTimer it will animate once through the frame count
#[derive(Component)]
pub struct AnimateOnce(pub(crate) usize);

#[derive(Component, Deref, DerefMut)]
pub struct AnimationTimer(pub(crate) Timer);

impl Default for MarkedForDeath {
    fn default() -> Self {
        MarkedForDeath(Timer::from_seconds(0.5, TimerMode::Once))
    }
}