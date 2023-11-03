
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

impl Default for MarkedForDeath {
    fn default() -> Self {
        MarkedForDeath(Timer::from_seconds(0.5, TimerMode::Once))
    }
}