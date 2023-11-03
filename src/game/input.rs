use bevy::{input::touch::*, prelude::*};
use bevy_ggrs::ggrs;


// constants for encoding movement commands
const INPUT_UP: u8 = 1<< 0;
const INPUT_DOWN: u8 = 1<< 1;
const INPUT_LEFT: u8 = 1<< 2;
const INPUT_RIGHT: u8 = 1<< 3;
const INPUT_FIRE: u8 = 1<< 4;

// const INPUT_ALL: u8 = INPUT_UP
//                     & INPUT_DOWN
//                     & INPUT_LEFT
//                     & INPUT_RIGHT
//                     & INPUT_FIRE;

pub fn touch_test(
    touches: Res<Touches>,
) {
    // There is a lot more information available, see the API docs.
    // This example only shows some very basic things.

    for finger in touches.iter() {
        if touches.just_pressed(finger.id()) {
            println!("A new touch with ID {} just began.", finger.id());
        }
        println!(
            "Finger {} is at position ({},{}), started from ({},{}).",
            finger.id(),
            finger.position().x,
            finger.position().y,
            finger.start_position().x,
            finger.start_position().y,
        );
    }
}

pub fn touch_ev_test(
    mut touch_evr: EventReader<TouchInput>,
) {
    use bevy::input::touch::TouchPhase;
    for ev in touch_evr.iter() {
        // in real apps you probably want to store and track touch ids somewhere
        match ev.phase {
            TouchPhase::Started => {
                info!("[EV] Touch {} started at: {:?}", ev.id, ev.position);
            }
            TouchPhase::Moved => {
                info!("[EV] Touch {} moved to: {:?}", ev.id, ev.position);
            }
            TouchPhase::Ended => {
                info!("[EV] Touch {} ended at: {:?}", ev.id, ev.position);
            }
            TouchPhase::Canceled => {
                info!("[EV] Touch {} cancelled at: {:?}", ev.id, ev.position);
            }
        }
    }
}

pub fn input(
    _: In<ggrs::PlayerHandle>,
    keys: Res<Input<KeyCode>>,
    // mut touch_evr: EventReader<TouchInput>,
    touches: Res<Touches>,
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

    // handle touchscreens
    for finger in touches.iter() {
        if touches.just_pressed(finger.id()) {
            info!("[InSys] The touch {} just started.", finger.id());
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

/// takes a vectorized input from a joystick or touchscreen and crush it down into our binary input format
const DEADZONE: f32 = 0.2;
pub fn input_from_vec(dir: Vec2) -> u8 {
    let mut input = 0;

    // LR
    if dir.x > DEADZONE {
        input |= INPUT_RIGHT;
    }
    if dir.x < -DEADZONE {
        input |= INPUT_LEFT;
    }

    // UD

    if dir.y > DEADZONE {
        input |= INPUT_UP;
    }
    if dir.y < -DEADZONE {
        input |= INPUT_DOWN;
    }
    
    input
}

pub fn fire(input: u8) -> bool {
    input & INPUT_FIRE != 0
}