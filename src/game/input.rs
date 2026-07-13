use bevy::{input::touch::*, prelude::*, window::PrimaryWindow};
use bevy_ggrs::ggrs;

// constants for encoding movement commands
const INPUT_UP: u8 = 1 << 0;
const INPUT_DOWN: u8 = 1 << 1;
const INPUT_LEFT: u8 = 1 << 2;
const INPUT_RIGHT: u8 = 1 << 3;
const INPUT_FIRE: u8 = 1 << 4;

// const INPUT_ALL: u8 = INPUT_UP
//                     & INPUT_DOWN
//                     & INPUT_LEFT
//                     & INPUT_RIGHT
//                     & INPUT_FIRE;

// pub fn touch_test(
//     touches: Res<Touches>,
// ) {
//     // There is a lot more information available, see the API docs.
//     // This example only shows some very basic things.

//     for finger in touches.iter() {
//         if touches.just_pressed(finger.id()) {
//             println!("A new touch with ID {} just began.", finger.id());
//         }
//         println!(
//             "Finger {} is at position ({},{}), started from ({},{}).",
//             finger.id(),
//             finger.position().x,
//             finger.position().y,
//             finger.start_position().x,
//             finger.start_position().y,
//         );
//     }
// }

// pub fn touch_ev_test(
//     mut touch_evr: EventReader<TouchInput>,
// ) {
//     use bevy::input::touch::TouchPhase;
//     for ev in touch_evr.iter() {
//         // in real apps you probably want to store and track touch ids somewhere
//         match ev.phase {
//             TouchPhase::Started => {
//                 info!("[EV] Touch {} started at: {:?}", ev.id, ev.position);
//             }
//             TouchPhase::Moved => {
//                 info!("[EV] Touch {} moved to: {:?}", ev.id, ev.position);
//             }
//             TouchPhase::Ended => {
//                 info!("[EV] Touch {} ended at: {:?}", ev.id, ev.position);
//             }
//             TouchPhase::Canceled => {
//                 info!("[EV] Touch {} cancelled at: {:?}", ev.id, ev.position);
//             }
//         }
//     }
// }

/// Local state for keeping track of which touch id is the current movement touchid
#[derive(Default)]
pub struct TouchMap(pub(crate) Option<u64>);

pub fn input(
    _: In<ggrs::PlayerHandle>,
    keys: Res<Input<KeyCode>>,
    // mut touch_evr: EventReader<TouchInput>,
    touches: Res<Touches>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut touch_map: Local<TouchMap>,
) -> u8 {
    let mut input = 0u8;

    if keys.any_pressed([KeyCode::Up, KeyCode::W]) {
        input |= INPUT_UP;
    }
    if keys.any_pressed([KeyCode::Down, KeyCode::S]) {
        input |= INPUT_DOWN;
    }
    if keys.any_pressed([KeyCode::Left, KeyCode::A]) {
        input |= INPUT_LEFT
    }
    if keys.any_pressed([KeyCode::Right, KeyCode::D]) {
        input |= INPUT_RIGHT;
    }
    if keys.any_pressed([KeyCode::Space, KeyCode::Return]) {
        input |= INPUT_FIRE;
    }

    let Ok(window) = window.get_single() else {
        return input;
    };

    if touch_map
        .0
        .is_some_and(|id| !touches.iter().any(|finger| finger.id() == id))
    {
        touch_map.0 = None;
    }

    let screen_midpoint = window.width() / 2.0;
    let deadzone = (window.width().min(window.height()) * 0.08).clamp(32.0, 72.0);
    for finger in touches.iter() {
        if finger.start_position().x >= screen_midpoint {
            input |= INPUT_FIRE;
            continue;
        }

        if touch_map.0.is_none() {
            touch_map.0 = Some(finger.id());
        }
        if touch_map.0 == Some(finger.id()) {
            input |= input_from_vec(finger.start_position() - finger.position(), deadzone);
        }
    }

    // for ev in touch_evr.iter() {
    //     // in real apps you probably want to store and track touch ids somewhere
    //     match ev.phase {
    //         TouchPhase::Started => {
    //             println!("Touch {} started at: {:?}", ev.id, ev.position);
    //         }
    //         TouchPhase::Moved => {
    //             println!("Touch {} moved to: {:?}", ev.id, ev.position);
    //         }
    //         TouchPhase::Ended => {
    //             println!("Touch {} ended at: {:?}", ev.id, ev.position);
    //         }
    //         TouchPhase::Canceled => {
    //             println!("Touch {} cancelled at: {:?}", ev.id, ev.position);
    //         }
    //     }
    // }

    input
}

pub fn direction(input: u8) -> Vec2 {
    let mut direction = Vec2::ZERO;
    if input & INPUT_UP != 0 {
        direction.y += 1.;
    }
    if input & INPUT_DOWN != 0 {
        direction.y -= 1.;
    }
    if input & INPUT_RIGHT != 0 {
        direction.x += 1.;
    }
    if input & INPUT_LEFT != 0 {
        direction.x -= 1.;
    }
    direction.normalize_or_zero()
}

// const fn const_norm(vec: Vec2) -> Vec2 {
//     let dot = vec.x * vec.x + vec.y * vec.y;
//     let len = f32::sqrt(dot);
//     let rcp = 1.0 / len;
//     let norm = Vec2 { x: vec.x * rcp, y: vec.y * rcp };
//     norm
// }

/// takes a vectorized input from a joystick or touchscreen and crush it down into our binary input format
const AXIS_DEADZONE: f32 = 0.2;
// magic pre calulated normalized variable for when x and y are both 1
const DIAGONAL_NORMALIZED: f32 = 0.707107;
const UNIT_TL: Vec2 = Vec2 {
    x: DIAGONAL_NORMALIZED,
    y: DIAGONAL_NORMALIZED,
};
const UNIT_TR: Vec2 = Vec2 {
    x: -DIAGONAL_NORMALIZED,
    y: DIAGONAL_NORMALIZED,
};
const UNIT_BL: Vec2 = Vec2 {
    x: DIAGONAL_NORMALIZED,
    y: -DIAGONAL_NORMALIZED,
};
const UNIT_BR: Vec2 = Vec2 {
    x: -DIAGONAL_NORMALIZED,
    y: -DIAGONAL_NORMALIZED,
};
fn input_from_vec(dir: Vec2, deadzone: f32) -> u8 {
    let mut input = 0;

    let magnitude = dir.length();

    // only apply input when magnitude is greater than the deadzone value
    if magnitude > deadzone {
        let dir = dir.normalize_or_zero();

        let left = dir.distance_squared(Vec2::X);
        let topleft = dir.distance_squared(UNIT_TL);
        let top = dir.distance_squared(Vec2::Y);
        let topright = dir.distance_squared(UNIT_TR);
        let right = dir.distance_squared(-Vec2::X);

        let bottomleft = dir.distance_squared(UNIT_BL);
        let bottom = dir.distance_squared(-Vec2::Y);
        let bottomright = dir.distance_squared(UNIT_BR);

        if top < AXIS_DEADZONE {
            return INPUT_UP;
        } else if bottom < AXIS_DEADZONE {
            return INPUT_DOWN;
        }

        if left < right {
            if left < AXIS_DEADZONE {
                // we are in the vertical axis deadzone so move straight left
                input |= INPUT_LEFT;
            } else if topleft < left {
                input |= INPUT_LEFT | INPUT_UP;
            } else if bottomleft < left {
                input |= INPUT_LEFT | INPUT_DOWN;
            }
        } else {
            if right < AXIS_DEADZONE {
                // we are in the vertical axis deadzone so move straight left
                input |= INPUT_RIGHT;
            } else if topright < right {
                input |= INPUT_RIGHT | INPUT_UP;
            } else if bottomright < right {
                input |= INPUT_RIGHT | INPUT_DOWN;
            }
        }
    }

    input
}

pub fn fire(input: u8) -> bool {
    input & INPUT_FIRE != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_deadzone_scales_without_changing_directions() {
        for deadzone in [32.0, 72.0] {
            assert_eq!(input_from_vec(Vec2::X * deadzone, deadzone), 0);
            assert_eq!(
                input_from_vec(Vec2::X * (deadzone + 1.0), deadzone),
                INPUT_LEFT
            );
            assert_eq!(
                input_from_vec(-Vec2::X * (deadzone + 1.0), deadzone),
                INPUT_RIGHT
            );
            assert_eq!(
                input_from_vec(Vec2::Y * (deadzone + 1.0), deadzone),
                INPUT_UP
            );
            assert_eq!(
                input_from_vec(-Vec2::Y * (deadzone + 1.0), deadzone),
                INPUT_DOWN
            );

            let diagonal = deadzone + 1.0;
            assert_eq!(
                input_from_vec(Vec2::new(diagonal, diagonal), deadzone),
                INPUT_LEFT | INPUT_UP
            );
            assert_eq!(
                input_from_vec(Vec2::new(-diagonal, diagonal), deadzone),
                INPUT_RIGHT | INPUT_UP
            );
            assert_eq!(
                input_from_vec(Vec2::new(diagonal, -diagonal), deadzone),
                INPUT_LEFT | INPUT_DOWN
            );
            assert_eq!(
                input_from_vec(Vec2::new(-diagonal, -diagonal), deadzone),
                INPUT_RIGHT | INPUT_DOWN
            );
        }
    }
}
