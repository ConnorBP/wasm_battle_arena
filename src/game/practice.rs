//! Local target practice shown while the network matchmaking flow continues.
//!
//! Nothing in this module is rollback-authoritative. Practice owns its entities
//! and resources, runs only in `GameState::Matchmaking`, and is removed before
//! the game session is allowed to present its arena.

use std::collections::HashSet;

use bevy::{input::touch::Touches, prelude::*, window::PrimaryWindow};
use bevy_kira_audio::prelude::AudioReceiver;

use super::assets::textures::ImageAssets;

const ARENA_MIN: Vec2 = Vec2::new(-4.25, -3.25);
const ARENA_MAX: Vec2 = Vec2::new(4.25, 3.25);
const PLAYER_RADIUS: f32 = 0.45;
const TARGET_RADIUS: f32 = 0.42;
const SHOT_RADIUS: f32 = 0.12;
const PLAYER_SPEED: f32 = 4.8;
const SHOT_SPEED: f32 = 9.5;
const SHOT_LIFETIME: f32 = 1.35;
const FIRE_COOLDOWN: f32 = 0.18;
const TARGET_RESPAWN_DELAY: f32 = 0.28;
const TOUCH_DEADZONE: f32 = 20.0;

#[derive(Component, Debug, Clone, Copy)]
pub struct PracticePlayer {
    facing: Vec2,
}

impl Default for PracticePlayer {
    fn default() -> Self {
        Self { facing: Vec2::X }
    }
}

#[derive(Component, Debug, Clone, Copy)]
pub struct Target {
    velocity: Vec2,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct Shot {
    velocity: Vec2,
    lifetime: f32,
}

/// Marker used to guarantee that decorations and active projectiles are all
/// removed during the Matchmaking exit schedule.
#[derive(Component, Debug)]
pub(super) struct PracticeOwned;

#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PracticeScore {
    pub score: u32,
    pub streak: u32,
    pub best_streak: u32,
}

#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct PracticeCooldown {
    pub remaining: f32,
}

#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct PracticeSpawn {
    sequence: u32,
    pending: u8,
    until_next: f32,
}

#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct PracticeTouch {
    movement_id: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TouchPoint {
    id: u64,
    start: Vec2,
    current: Vec2,
}

/// Returns movement and fire without consulting any network input resource.
/// A finger which started on the left is exclusively movement; any finger
/// which started on the right is exclusively fire.
fn touch_controls(
    screen_width: f32,
    points: &[TouchPoint],
    state: &mut PracticeTouch,
) -> (Vec2, bool) {
    if state
        .movement_id
        .is_some_and(|id| !points.iter().any(|point| point.id == id))
    {
        state.movement_id = None;
    }

    let midpoint = screen_width * 0.5;
    if state.movement_id.is_none() {
        state.movement_id = points
            .iter()
            .filter(|point| point.start.x < midpoint)
            .map(|point| point.id)
            .min();
    }

    let movement = state
        .movement_id
        .and_then(|id| points.iter().find(|point| point.id == id))
        .map(|point| {
            let screen_delta = point.current - point.start;
            let world_delta = Vec2::new(screen_delta.x, -screen_delta.y);
            if world_delta.length() > TOUCH_DEADZONE {
                world_delta.normalize_or_zero()
            } else {
                Vec2::ZERO
            }
        })
        .unwrap_or(Vec2::ZERO);
    let fire = points.iter().any(|point| point.start.x >= midpoint);
    (movement, fire)
}

fn clamp_movement(position: Vec2, delta: Vec2) -> Vec2 {
    (position + delta).clamp(
        ARENA_MIN + Vec2::splat(PLAYER_RADIUS),
        ARENA_MAX - Vec2::splat(PLAYER_RADIUS),
    )
}

fn tick_cooldown(remaining: f32, delta_seconds: f32) -> f32 {
    (remaining - delta_seconds.max(0.0)).max(0.0)
}

fn bounce_target(position: Vec2, velocity: Vec2, delta_seconds: f32) -> (Vec2, Vec2) {
    let lower = ARENA_MIN + Vec2::splat(TARGET_RADIUS);
    let upper = ARENA_MAX - Vec2::splat(TARGET_RADIUS);
    let mut next = position + velocity * delta_seconds.max(0.0);
    let mut bounced = velocity;

    if next.x < lower.x {
        next.x = lower.x;
        bounced.x = bounced.x.abs();
    } else if next.x > upper.x {
        next.x = upper.x;
        bounced.x = -bounced.x.abs();
    }
    if next.y < lower.y {
        next.y = lower.y;
        bounced.y = bounced.y.abs();
    } else if next.y > upper.y {
        next.y = upper.y;
        bounced.y = -bounced.y.abs();
    }
    (next, bounced)
}

fn shot_hits_target(shot: Vec2, target: Vec2) -> bool {
    shot.distance_squared(target) <= (SHOT_RADIUS + TARGET_RADIUS).powi(2)
}

fn register_hit(score: &mut PracticeScore) {
    score.streak = score.streak.saturating_add(1);
    score.best_streak = score.best_streak.max(score.streak);
    score.score = score
        .score
        .saturating_add(10 + score.streak.saturating_sub(1) * 2);
}

fn shot_should_cleanup(position: Vec2, lifetime: f32) -> bool {
    lifetime <= 0.0
        || position.x < ARENA_MIN.x - 0.5
        || position.x > ARENA_MAX.x + 0.5
        || position.y < ARENA_MIN.y - 0.5
        || position.y > ARENA_MAX.y + 0.5
}

fn cleanup_plan(entities: impl IntoIterator<Item = Entity>) -> Vec<Entity> {
    entities.into_iter().collect()
}

pub fn setup_practice(mut commands: Commands, images: Res<ImageAssets>) {
    commands.insert_resource(PracticeScore::default());
    commands.insert_resource(PracticeCooldown::default());
    commands.insert_resource(PracticeTouch::default());

    // A dark, bounded pad remains readable over the persistent checker floor.
    commands.spawn((
        PracticeOwned,
        SpriteBundle {
            transform: Transform::from_xyz(0.0, 0.0, 18.0),
            sprite: Sprite {
                color: Color::rgba(0.025, 0.04, 0.065, 0.82),
                custom_size: Some(ARENA_MAX - ARENA_MIN),
                ..default()
            },
            ..default()
        },
        Name::new("practice: waiting arena"),
    ));
    for (position, size) in [
        (
            Vec2::new(0.0, ARENA_MIN.y),
            Vec2::new(ARENA_MAX.x - ARENA_MIN.x, 0.10),
        ),
        (
            Vec2::new(0.0, ARENA_MAX.y),
            Vec2::new(ARENA_MAX.x - ARENA_MIN.x, 0.10),
        ),
        (
            Vec2::new(ARENA_MIN.x, 0.0),
            Vec2::new(0.10, ARENA_MAX.y - ARENA_MIN.y),
        ),
        (
            Vec2::new(ARENA_MAX.x, 0.0),
            Vec2::new(0.10, ARENA_MAX.y - ARENA_MIN.y),
        ),
    ] {
        commands.spawn((
            PracticeOwned,
            SpriteBundle {
                transform: Transform::from_translation(position.extend(19.0)),
                sprite: Sprite {
                    color: Color::rgb(0.35, 0.88, 0.94),
                    custom_size: Some(size),
                    ..default()
                },
                ..default()
            },
            Name::new("practice: arena boundary"),
        ));
    }

    commands.spawn((
        PracticeOwned,
        PracticePlayer::default(),
        SpriteBundle {
            texture: images.ghost.clone(),
            transform: Transform::from_xyz(-2.7, -1.7, 30.0),
            sprite: Sprite {
                color: Color::rgb(0.25, 0.82, 0.95),
                custom_size: Some(Vec2::splat(0.9)),
                ..default()
            },
            ..default()
        },
        Name::new("practice: player"),
    ));

    let mut spawn = PracticeSpawn::default();
    for _ in 0..3 {
        spawn_target(&mut commands, &images, &mut spawn);
    }
    commands.insert_resource(spawn);
}

fn spawn_target(commands: &mut Commands, images: &ImageAssets, spawn: &mut PracticeSpawn) {
    const POSITIONS: [Vec2; 8] = [
        Vec2::new(2.8, 1.8),
        Vec2::new(1.1, -1.8),
        Vec2::new(-0.4, 1.9),
        Vec2::new(3.1, -0.4),
        Vec2::new(-2.0, 1.4),
        Vec2::new(0.5, 0.2),
        Vec2::new(-2.8, -0.2),
        Vec2::new(2.0, 0.8),
    ];
    const VELOCITIES: [Vec2; 8] = [
        Vec2::new(-1.15, 0.72),
        Vec2::new(0.92, 1.08),
        Vec2::new(1.32, -0.55),
        Vec2::new(-0.78, -1.12),
        Vec2::new(1.08, 0.84),
        Vec2::new(-1.30, 0.48),
        Vec2::new(0.72, -1.22),
        Vec2::new(-0.94, 1.02),
    ];
    let index = spawn.sequence as usize % POSITIONS.len();
    spawn.sequence = spawn.sequence.wrapping_add(1);
    commands.spawn((
        PracticeOwned,
        Target {
            velocity: VELOCITIES[index],
        },
        SpriteBundle {
            texture: images.ghost.clone(),
            transform: Transform::from_translation(POSITIONS[index].extend(28.0)),
            sprite: Sprite {
                color: Color::rgb(1.0, 0.38, 0.28),
                custom_size: Some(Vec2::splat(TARGET_RADIUS * 2.0)),
                ..default()
            },
            ..default()
        },
        Name::new("practice: moving target"),
    ));
}

pub fn reset_practice_view(
    mut followers: ParamSet<(
        Query<&mut Transform, (With<Camera>, Without<AudioReceiver>)>,
        Query<&mut Transform, (With<AudioReceiver>, Without<Camera>)>,
    )>,
) {
    for mut transform in &mut followers.p0() {
        transform.translation.x = 0.0;
        transform.translation.y = 0.0;
        transform.rotation = Quat::IDENTITY;
        transform.scale = Vec3::ONE;
    }
    for mut transform in &mut followers.p1() {
        *transform = Transform::default();
    }
}

pub fn update_practice_player(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<Input<KeyCode>>,
    touches: Res<Touches>,
    windows: Query<&Window, With<PrimaryWindow>>,
    images: Res<ImageAssets>,
    mut touch_state: ResMut<PracticeTouch>,
    mut cooldown: ResMut<PracticeCooldown>,
    mut players: Query<(&mut Transform, &mut PracticePlayer)>,
) {
    let delta_seconds = time.delta_seconds().min(0.1);
    cooldown.remaining = tick_cooldown(cooldown.remaining, delta_seconds);

    let mut movement = Vec2::ZERO;
    if keys.any_pressed([KeyCode::W, KeyCode::Up]) {
        movement.y += 1.0;
    }
    if keys.any_pressed([KeyCode::S, KeyCode::Down]) {
        movement.y -= 1.0;
    }
    if keys.any_pressed([KeyCode::A, KeyCode::Left]) {
        movement.x -= 1.0;
    }
    if keys.any_pressed([KeyCode::D, KeyCode::Right]) {
        movement.x += 1.0;
    }
    let mut fire = keys.any_pressed([KeyCode::Space, KeyCode::Return]);

    if let Ok(window) = windows.get_single() {
        let points: Vec<_> = touches
            .iter()
            .map(|finger| TouchPoint {
                id: finger.id(),
                start: finger.start_position(),
                current: finger.position(),
            })
            .collect();
        let (touch_movement, touch_fire) =
            touch_controls(window.width(), &points, &mut touch_state);
        movement += touch_movement;
        fire |= touch_fire;
    }
    movement = movement.normalize_or_zero();

    let Ok((mut transform, mut facing)) = players.get_single_mut() else {
        return;
    };
    if movement != Vec2::ZERO {
        facing.facing = movement;
        let next = clamp_movement(
            transform.translation.truncate(),
            movement * PLAYER_SPEED * delta_seconds,
        );
        transform.translation.x = next.x;
        transform.translation.y = next.y;
    }

    if fire && cooldown.remaining <= 0.0 {
        let direction = if facing.facing == Vec2::ZERO {
            Vec2::X
        } else {
            facing.facing.normalize()
        };
        let origin = transform.translation.truncate() + direction * (PLAYER_RADIUS + SHOT_RADIUS);
        commands.spawn((
            PracticeOwned,
            Shot {
                velocity: direction * SHOT_SPEED,
                lifetime: SHOT_LIFETIME,
            },
            SpriteBundle {
                texture: images.bullet.clone(),
                transform: Transform::from_translation(origin.extend(31.0))
                    .with_rotation(Quat::from_rotation_arc_2d(Vec2::X, direction)),
                sprite: Sprite {
                    color: Color::rgb(1.0, 0.86, 0.28),
                    custom_size: Some(Vec2::new(0.32, 0.12)),
                    ..default()
                },
                ..default()
            },
            Name::new("practice: shot"),
        ));
        cooldown.remaining = FIRE_COOLDOWN;
    }
}

pub fn move_practice_targets(time: Res<Time>, mut targets: Query<(&mut Transform, &mut Target)>) {
    let delta_seconds = time.delta_seconds().min(0.1);
    for (mut transform, mut target) in &mut targets {
        let (position, velocity) = bounce_target(
            transform.translation.truncate(),
            target.velocity,
            delta_seconds,
        );
        transform.translation.x = position.x;
        transform.translation.y = position.y;
        target.velocity = velocity;
    }
}

pub fn move_practice_shots(
    mut commands: Commands,
    time: Res<Time>,
    mut score: ResMut<PracticeScore>,
    mut shots: Query<(Entity, &mut Transform, &mut Shot)>,
) {
    let delta_seconds = time.delta_seconds().min(0.1);
    for (entity, mut transform, mut shot) in &mut shots {
        transform.translation += (shot.velocity * delta_seconds).extend(0.0);
        shot.lifetime -= delta_seconds;
        if shot_should_cleanup(transform.translation.truncate(), shot.lifetime) {
            commands.entity(entity).despawn_recursive();
            score.streak = 0;
        }
    }
}

pub fn resolve_practice_hits(
    mut commands: Commands,
    shots: Query<(Entity, &Transform), With<Shot>>,
    targets: Query<(Entity, &Transform), With<Target>>,
    mut score: ResMut<PracticeScore>,
    mut spawn: ResMut<PracticeSpawn>,
) {
    let mut consumed_targets = HashSet::new();
    for (shot_entity, shot_transform) in &shots {
        let Some((target_entity, _)) = targets.iter().find(|(target_entity, target_transform)| {
            !consumed_targets.contains(target_entity)
                && shot_hits_target(
                    shot_transform.translation.truncate(),
                    target_transform.translation.truncate(),
                )
        }) else {
            continue;
        };
        consumed_targets.insert(target_entity);
        commands.entity(shot_entity).despawn_recursive();
        commands.entity(target_entity).despawn_recursive();
        register_hit(&mut score);
        if spawn.pending == 0 {
            spawn.until_next = TARGET_RESPAWN_DELAY;
        }
        spawn.pending = spawn.pending.saturating_add(1);
    }
}

pub fn respawn_practice_targets(
    mut commands: Commands,
    time: Res<Time>,
    images: Res<ImageAssets>,
    mut spawn: ResMut<PracticeSpawn>,
) {
    if spawn.pending == 0 {
        return;
    }
    spawn.until_next = tick_cooldown(spawn.until_next, time.delta_seconds().min(0.1));
    if spawn.until_next <= 0.0 {
        spawn_target(&mut commands, &images, &mut spawn);
        spawn.pending -= 1;
        if spawn.pending > 0 {
            spawn.until_next = TARGET_RESPAWN_DELAY;
        }
    }
}

pub fn cleanup_practice(mut commands: Commands, entities: Query<Entity, With<PracticeOwned>>) {
    for entity in cleanup_plan(entities.iter()) {
        commands.entity(entity).despawn_recursive();
    }
    commands.remove_resource::<PracticeScore>();
    commands.remove_resource::<PracticeCooldown>();
    commands.remove_resource::<PracticeSpawn>();
    commands.remove_resource::<PracticeTouch>();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_is_clamped_inside_the_waiting_arena() {
        let upper = clamp_movement(Vec2::ZERO, Vec2::splat(100.0));
        let lower = clamp_movement(Vec2::ZERO, Vec2::splat(-100.0));
        assert_eq!(upper, ARENA_MAX - Vec2::splat(PLAYER_RADIUS));
        assert_eq!(lower, ARENA_MIN + Vec2::splat(PLAYER_RADIUS));
        assert_eq!(
            clamp_movement(Vec2::new(1.0, 1.0), Vec2::X),
            Vec2::new(2.0, 1.0)
        );
    }

    #[test]
    fn cooldown_ticks_to_zero_without_underflow() {
        assert!((tick_cooldown(FIRE_COOLDOWN, 0.08) - 0.10).abs() < 0.000_001);
        assert_eq!(tick_cooldown(0.04, 0.08), 0.0);
        assert_eq!(tick_cooldown(0.0, 1.0), 0.0);
        assert_eq!(tick_cooldown(0.5, -1.0), 0.5);
    }

    #[test]
    fn targets_bounce_and_remain_readable_inside_bounds() {
        let start = ARENA_MAX - Vec2::splat(TARGET_RADIUS + 0.01);
        let (position, velocity) = bounce_target(start, Vec2::splat(2.0), 1.0);
        assert_eq!(position, ARENA_MAX - Vec2::splat(TARGET_RADIUS));
        assert!(velocity.x < 0.0 && velocity.y < 0.0);
    }

    #[test]
    fn hit_radius_includes_edges_and_rejects_misses() {
        assert!(shot_hits_target(
            Vec2::ZERO,
            Vec2::X * (SHOT_RADIUS + TARGET_RADIUS)
        ));
        assert!(!shot_hits_target(
            Vec2::ZERO,
            Vec2::X * (SHOT_RADIUS + TARGET_RADIUS + 0.01)
        ));
    }

    #[test]
    fn scoring_rewards_and_tracks_a_streak() {
        let mut score = PracticeScore::default();
        register_hit(&mut score);
        register_hit(&mut score);
        assert_eq!(score.score, 22);
        assert_eq!(score.streak, 2);
        assert_eq!(score.best_streak, 2);
        score.streak = 0;
        register_hit(&mut score);
        assert_eq!(score.score, 32);
        assert_eq!(score.best_streak, 2);
    }

    #[test]
    fn cleanup_catches_expired_out_of_bounds_and_active_entities() {
        assert!(shot_should_cleanup(Vec2::ZERO, 0.0));
        assert!(shot_should_cleanup(Vec2::new(ARENA_MAX.x + 1.0, 0.0), 1.0));
        assert!(!shot_should_cleanup(Vec2::ZERO, 1.0));

        let active_shot = Entity::from_raw(7);
        let target = Entity::from_raw(8);
        assert_eq!(
            cleanup_plan([active_shot, target]),
            vec![active_shot, target]
        );
    }

    #[test]
    fn touch_keeps_left_movement_separate_from_right_fire() {
        let left = TouchPoint {
            id: 2,
            start: Vec2::new(100.0, 200.0),
            current: Vec2::new(150.0, 150.0),
        };
        let right = TouchPoint {
            id: 9,
            start: Vec2::new(700.0, 200.0),
            current: Vec2::new(600.0, 400.0),
        };
        let mut state = PracticeTouch::default();
        let (movement, fire) = touch_controls(800.0, &[right, left], &mut state);
        assert_eq!(state.movement_id, Some(2));
        assert!((movement - Vec2::new(1.0, 1.0).normalize()).length() < 0.0001);
        assert!(fire);

        let (movement, fire) = touch_controls(800.0, &[right], &mut state);
        assert_eq!(movement, Vec2::ZERO);
        assert_eq!(state.movement_id, None);
        assert!(fire);
    }
}
