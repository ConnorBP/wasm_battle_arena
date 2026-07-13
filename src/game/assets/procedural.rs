//! Colors and dimensions for visuals that are generated without texture assets.

use bevy::prelude::*;

pub const TRAP_SIZE: f32 = 0.70;
pub const PICKUP_SIZE: f32 = 0.55;

pub fn wall_foundation_color() -> Color {
    Color::rgb(0.10, 0.16, 0.12)
}

pub fn wall_face_color(shade: f32) -> Color {
    Color::rgb(0.20 * shade, 0.32 * shade, 0.23 * shade)
}

pub fn trap_color() -> Color {
    Color::rgb(0.75, 0.12, 0.20)
}

pub fn speed_pickup_color() -> Color {
    Color::rgb(0.15, 0.85, 0.95)
}

pub fn shield_pickup_color() -> Color {
    Color::rgb(0.95, 0.75, 0.15)
}

/// Visible warning field outside the compact duel arena. Deliberately distinct
/// from the black world background and solid green walls.
pub fn void_color() -> Color {
    Color::rgb(0.20, 0.035, 0.25)
}
