
use bevy::prelude::*;

use super::session::PlayerId;

#[derive(Component, Reflect, Default)]
pub struct Player {
    pub handle: usize,
    pub player_id: PlayerId,
}

/// Deterministic number of rollback frames until this player may fire again.
/// Zero means ready. This is frame-based so held keyboard and touch fire use
/// exactly the same cadence.
#[derive(Component, Reflect, Default)]
pub struct BulletReady(pub u8);

#[derive(Component, Reflect, Default, Clone, Copy)]
pub struct Bullet {
    pub id: u64,
    pub owner: PlayerId,
    pub owner_handle: usize,
    pub active: bool,
}

#[derive(Component, Reflect, Default, Clone, Copy)]
pub struct SpeedPickup {
    pub cell: (u16, u16),
}

#[derive(Component, Reflect, Default, Clone, Copy)]
pub struct SpeedBoost {
    pub frames_left: u16,
}

#[derive(Component, Reflect, Default, Clone, Copy)]
pub struct ShieldPickup {
    pub cell: (u16, u16),
}

#[derive(Component, Reflect, Default, Clone, Copy)]
pub struct ShieldCharges(pub u8);

#[derive(Component, Reflect, Default, Clone, Copy)]
pub struct MoveDir(pub Vec2);

#[derive(Component, Reflect, Default)]
pub struct LookTowardsParentMove;

#[derive(Component, Reflect)]
pub struct MarkedForDeath {
    pub(crate) timer: Timer,
    pub(crate) frame: u32,
}

impl MarkedForDeath {
    pub fn at(frame: u32) -> Self {
        Self {
            timer: Timer::from_seconds(0.5, TimerMode::Once),
            frame,
        }
    }
}

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
        MarkedForDeath::at(0)
    }
}