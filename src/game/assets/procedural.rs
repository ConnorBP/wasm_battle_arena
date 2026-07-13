//! Colors, dimensions, and deterministic decoration for repository-generated
//! pixel art. The arena intentionally uses a small hand-picked palette; no
//! runtime-downloaded or externally authored textures are involved.

use bevy::prelude::*;

pub const TRAP_SIZE: f32 = 0.70;
pub const PICKUP_SIZE: f32 = 0.55;

const FLOOR_PALETTE: [Color; 4] = [
    Color::rgb(0.055, 0.065, 0.085),
    Color::rgb(0.065, 0.078, 0.098),
    Color::rgb(0.075, 0.086, 0.11),
    Color::rgb(0.085, 0.096, 0.12),
];
const WALL_PALETTE: [Color; 4] = [
    Color::rgb(0.12, 0.22, 0.16),
    Color::rgb(0.15, 0.27, 0.19),
    Color::rgb(0.18, 0.31, 0.21),
    Color::rgb(0.20, 0.34, 0.23),
];

/// Stable integer noise used only for decoration. It is deliberately separate
/// from map generation, so changing art never changes authoritative gameplay.
pub fn decoration_hash(x: usize, y: usize, salt: u32) -> u32 {
    let mut value =
        (x as u32).wrapping_mul(0x9e37_79b9) ^ (y as u32).wrapping_mul(0x85eb_ca6b) ^ salt;
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value.wrapping_mul(0x846c_a68b) ^ (value >> 16)
}

/// Checkerboard plus low-amplitude tiled noise, selected from the restricted
/// four-color floor palette.
pub fn floor_tile_color(x: usize, y: usize) -> Color {
    let checker = (x + y) & 1;
    let noise = (decoration_hash(x, y, 0x464c_4f52) >> 30) as usize;
    FLOOR_PALETTE[(checker + noise).min(FLOOR_PALETTE.len() - 1)]
}

pub fn floor_dither_visible(x: usize, y: usize) -> bool {
    decoration_hash(x, y, 0x4449_5448) % 11 == 0
}

pub fn floor_dither_color(x: usize, y: usize) -> Color {
    FLOOR_PALETTE[2 + (decoration_hash(x, y, 0x5049_584c) as usize & 1)]
}

pub fn wall_foundation_color() -> Color {
    WALL_PALETTE[0]
}

/// Coordinate-dependent brick variation remains visual-only and deterministic.
pub fn wall_brick_color(x: usize, y: usize, neighbors: usize) -> Color {
    let variation = decoration_hash(x, y, 0x4252_4943) as usize & 1;
    WALL_PALETTE[(1 + variation + usize::from(neighbors >= 3)).min(3)]
}

pub fn wall_face_color(shade: f32) -> Color {
    Color::rgb(0.20 * shade, 0.32 * shade, 0.23 * shade)
}

pub fn wall_mortar_color() -> Color {
    Color::rgb(0.075, 0.13, 0.10)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoration_is_deterministic_and_coordinate_varied() {
        for &(x, y) in &[(0, 0), (1, 2), (40, 40), (17, 9)] {
            assert_eq!(decoration_hash(x, y, 123), decoration_hash(x, y, 123));
            assert_eq!(floor_tile_color(x, y), floor_tile_color(x, y));
        }
        assert_ne!(decoration_hash(1, 2, 123), decoration_hash(2, 1, 123));
        assert_ne!(wall_mortar_color(), wall_foundation_color());
    }

    #[test]
    fn dither_is_sparse_but_present_across_arena() {
        let count = (0..41)
            .flat_map(|x| (0..41).map(move |y| (x, y)))
            .filter(|&(x, y)| floor_dither_visible(x, y))
            .count();
        assert!(
            count > 80 && count < 260,
            "unexpected dither count: {count}"
        );
    }
}
