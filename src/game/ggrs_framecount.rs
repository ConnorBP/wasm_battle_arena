use bevy::prelude::*;



// for keeping track of Current GGRS frame until we upgrade to the bevy 0.12 compatible ggrs
#[derive(Resource, Default, Reflect, Hash, Clone, Copy)]
#[reflect(Hash)]
pub struct GGFrameCount {
    pub frame: u32,
}

pub fn increase_frame_system(mut frame_count: ResMut<GGFrameCount>) {
    frame_count.frame = frame_count.frame.wrapping_add(1);
}