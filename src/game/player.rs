use bevy::{prelude::*,math::{Vec2Swizzles, Vec3Swizzles}};
use bevy_ggrs::{PlayerInputs, AddRollbackCommandExtension};

use super::{components::*, MAP_SIZE, textures::ImageAssets, RollbackState, Scores};
use super::networking::GgrsConfig;
use super::input;


pub fn move_players(
    inputs: Res<PlayerInputs<GgrsConfig>>,
    mut players: Query<(&mut Transform, &mut MoveDir, &Player)>,
) {
    for (mut transform, mut move_dir, player) in &mut players {
        let (input, _) = inputs[player.handle];
        let direction = input::direction(input);

        if direction == Vec2::ZERO {
            continue;
        }

        move_dir.0 = direction;

        let move_speed = 0.13;
        let move_delta = direction * move_speed;

        let old_pos = transform.translation.xy();
        let limit = Vec2::splat(MAP_SIZE as f32 / 2. - 0.5);
        let new_pos = (old_pos + move_delta).clamp(-limit, limit);

        transform.translation.x = new_pos.x;
        transform.translation.y = new_pos.y;
    }
}

pub fn player_look(
    // inputs: Res<PlayerInputs<GgrsConfig>>,
    // players: Query<(&Player, &Children)>,
    players: Query<(&MoveDir, &Children), With<Player>>,

    mut eyes_sprite: Query<&mut TextureAtlasSprite, With<LookTowardsParentMove>>,
) {
    for (move_dir, children) in players.iter() {
        for &child in children.iter() {
            if let Ok(eyes) = &mut eyes_sprite.get_mut(child) {
                // let (input, _) = inputs[player.handle];
                eyes.index = get_directional_sprite(move_dir.0);
            }
        }
    }
}

fn get_directional_sprite(dir: Vec2) -> usize {
    if dir.y < 0. && dir.x > 0. {
        return 0;
    }
    if dir.x.abs() < 0.2 && dir.y < 0. {
        return 1;
    }
    if dir.y < 0. && dir.x < 0. {
        return 2;
    }
    if dir.y.abs() < 0.2 && dir.x < 0. {
        return 3;
    }
    if dir.y > 0. && dir.x < 0. {
        return 4;
    }
    if dir.x.abs() < 0.2 && dir.y > 0. {
        return 5;
    }
    if dir.y > 0. && dir.x > 0. {
        return 6;
    }
    if dir.y.abs() < 0.2 && dir.x > 0. {
        return 7;
    }
    0
}

pub fn spawn_players(
    mut commands: Commands,
    images: Res<ImageAssets>,
    players: Query<Entity, With<Player>>,
    bullets: Query<Entity, With<Bullet>>,
) {

    info!("spawning players");

    // despawn last games stuff

    for player in &players {
        commands.entity(player).despawn_recursive();
    }

    for bullet in &bullets {
        commands.entity(bullet).despawn_recursive();
    }

    // p1
    spawn_player(&mut commands, &images, 0, -Vec2::X, Vec3::new(-2.,0.,100.));

    // p2
    spawn_player(&mut commands, &images, 1, Vec2::X, Vec3::new(2.,0.,100.));

}

fn spawn_player(
    commands: &mut Commands,
    images: &Res<ImageAssets>,
    handle: usize,
    move_dir: Vec2,
    translation: Vec3,
) {
    let parent = commands.spawn((
        Player { handle },
        BulletReady(true),
        MoveDir(move_dir),
        SpriteBundle {
            texture: images.ghost.clone(),
            transform: Transform::from_translation(translation),
            sprite: Sprite {
                // color: Color::rgb(0.3, 1., 0.1),
                custom_size: Some(Vec2::new(1.,1.)),
                ..default()
            },
            ..default()
        }
    ))
    .add_rollback()
    .id();

    let child = commands.spawn((
        SpriteSheetBundle {
            transform: Transform::from_translation(Vec3::new(0.,0.,1.)),
            sprite: TextureAtlasSprite {
                index: 0,
                custom_size: Some(Vec2::new(1.,1.)),
                ..default()
            },
            texture_atlas: images.eyes.clone(),
            ..default()
        },
        LookTowardsParentMove,
    ))
    .add_rollback()
    .id();

    commands.entity(parent).push_children(&[child]);
}


pub fn fire_bullets(
    mut commands: Commands,
    inputs: Res<PlayerInputs<GgrsConfig>>,
    images: Res<ImageAssets>,
    mut players: Query<(&Transform, &Player, &mut BulletReady, &MoveDir)>,
) {
    for (transform, player, mut bullet_ready, move_dir) in &mut players {
        let (input, _) = inputs[player.handle];
        if input::fire(input) && bullet_ready.0 {
            let player_pos = transform.translation.xy();
            let pos = player_pos + move_dir.0 * PLAYER_RADIUS + BULLET_RADIUS;
            commands.spawn((
                Bullet,
                *move_dir,
                SpriteBundle {
                    transform: Transform::from_translation(pos.extend(200.))
                        .with_rotation(Quat::from_rotation_arc_2d(Vec2::X, move_dir.0)),
                    texture: images.bullet.clone(),
                    sprite: Sprite {
                        custom_size: Some(Vec2::new(0.3, 0.1)),
                        ..default()
                    },
                    ..default()
                }
            ))
            .add_rollback();
            bullet_ready.0 = false;
        }
    }
}

pub fn reload_bullet(
    inputs: Res<PlayerInputs<GgrsConfig>>,
    mut bullets: Query<(&mut BulletReady, &Player)>
) {
    for (mut can_fire, player) in &mut bullets {
        let (input, _) = inputs[player.handle];
        if !input::fire(input) {
            can_fire.0 = true;
        }
    }
}

pub fn move_bullets(mut bullets: Query<(&mut Transform, &MoveDir), With<Bullet>>) {
    for (mut transform, dir) in &mut bullets {
        let delta = (dir.0 * 0.35).extend(0.);
        transform.translation += delta;
    }
}

const PLAYER_RADIUS: f32 = 0.5;
const BULLET_RADIUS: f32 = 0.025;
pub fn kill_players(
    mut commands: Commands,
    players: Query<(Entity, &Transform, &Player), Without<Bullet>>,
    bullets: Query<&Transform, With<Bullet>>,
    mut next_state: ResMut<NextState<RollbackState>>,
    mut scores: ResMut<Scores>,
) {
    for (player_entity, player_transform, player) in &players {
        for bullet_transform in &bullets {
            let distance = Vec2::distance(
                player_transform.translation.xy(),
                bullet_transform.translation.xy(),
            );
            if distance < PLAYER_RADIUS + BULLET_RADIUS {
                commands.entity(player_entity).despawn_recursive();
                next_state.set(RollbackState::RoundEnd);

                // new
                if player.handle == 0 {
                    scores.1 += 1;
                } else {
                    scores.0 += 1;
                }
                info!("player died: {scores:?}")
            }
        }
    }
}