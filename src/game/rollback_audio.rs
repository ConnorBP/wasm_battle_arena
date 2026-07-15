use bevy::{
    prelude::*,
    utils::{HashMap, HashSet},
};
use bevy_kira_audio::{prelude::*, AudioSource};

use super::{
    assets::sounds::{AudioConfig, SfxChannel},
    ggrs_framecount::GGFrameCount,
    networking::ROLLBACK_FPS,
};

#[derive(Component, Reflect)]
pub struct RollbackSound {
    pub clip: Handle<AudioSource>,
    pub start_frame: u32,
    pub sub_key: u64,
    pub volume: f64,
}

impl Default for RollbackSound {
    fn default() -> Self {
        Self {
            clip: default(),
            start_frame: 0,
            sub_key: 0,
            volume: 1.0,
        }
    }
}

impl RollbackSound {
    pub fn key(&self) -> (Handle<AudioSource>, u64) {
        (self.clip.clone(), self.sub_key)
    }
}

#[derive(Bundle, Default)]
pub struct RollbackSoundBundle {
    pub sound: RollbackSound,
    pub emitter: AudioEmitter,
    pub transform: Transform,
    pub global: GlobalTransform,
}

#[derive(Resource, Reflect, Default)]
pub struct PlaybackStates {
    playing: HashMap<(Handle<AudioSource>, u64), Handle<AudioInstance>>,
    live: HashSet<(Handle<AudioSource>, u64)>,
}

pub fn sync_rollback_sounds(
    mut commands: Commands,
    mut current_state: ResMut<PlaybackStates>,
    mut sounds: Query<(Entity, &RollbackSound, Option<&mut AudioEmitter>)>,
    sfx_audio: Res<AudioChannel<SfxChannel>>,
    frame: Res<GGFrameCount>,
) {
    const MAX_SOUND_DELAY: u32 = 10;
    current_state.live.clear();

    for (entity, sound, emitter) in &mut sounds {
        let key = sound.key();
        current_state.live.insert(key.clone());

        let instance = if let Some(instance) = current_state.playing.get(&key) {
            instance.clone()
        } else {
            if frame.frame.wrapping_sub(sound.start_frame) > MAX_SOUND_DELAY {
                continue;
            }
            let instance = sfx_audio.play(sound.clip.clone()).handle();
            current_state.playing.insert(key, instance.clone());
            instance
        };

        if let Some(mut emitter) = emitter {
            if !emitter.instances.contains(&instance) {
                emitter.instances.push(instance);
            }
        } else {
            let mut emitter = AudioEmitter::default();
            emitter.instances.push(instance);
            commands.entity(entity).insert(emitter);
        }
    }
}

const MAX_SOUND_DISTANCE: f32 = 20.0;

fn rollback_sound_volume(
    distance: f32,
    master_volume: f64,
    sfx_volume: f64,
    base_volume: f64,
) -> f64 {
    let distance_volume = (1.0 - distance / MAX_SOUND_DISTANCE)
        .clamp(0.0, 1.0)
        .powi(2);
    distance_volume as f64 * base_volume * master_volume / 100.0 * sfx_volume / 100.0
}

pub fn update_rollback_sound_spatial_audio(
    receiver: Query<&GlobalTransform, With<AudioReceiver>>,
    emitters: Query<(&GlobalTransform, &RollbackSound, &AudioEmitter)>,
    config: Res<AudioConfig>,
    mut audio_instances: ResMut<Assets<AudioInstance>>,
) {
    let Ok(receiver_transform) = receiver.get_single() else {
        return;
    };

    for (emitter_transform, sound, emitter) in &emitters {
        let sound_path = emitter_transform.translation() - receiver_transform.translation();
        let volume = rollback_sound_volume(
            sound_path.length(),
            config.master_volume,
            config.sfx_volume,
            sound.volume,
        );
        let panning = if sound_path.length_squared() == 0.0 {
            0.5
        } else {
            (receiver_transform.right().angle_between(sound_path).cos() + 1.0) / 2.0
        };

        for handle in &emitter.instances {
            if let Some(instance) = audio_instances.get_mut(handle) {
                instance.set_volume(volume, AudioTween::default());
                instance.set_panning(panning as f64, AudioTween::default());
            }
        }
    }
}

fn stop_interrupted(
    current_state: &mut PlaybackStates,
    audio_instances: &mut Assets<AudioInstance>,
) {
    let interrupted: Vec<_> = current_state
        .playing
        .keys()
        .filter(|key| !current_state.live.contains(*key))
        .cloned()
        .collect();
    for key in interrupted {
        if let Some(handle) = current_state.playing.remove(&key) {
            if let Some(instance) = audio_instances.get_mut(&handle) {
                instance.stop(AudioTween::linear(std::time::Duration::from_millis(100)));
            }
        }
    }
}

pub fn reconcile_rollback_sounds(
    mut current_state: ResMut<PlaybackStates>,
    mut audio_instances: ResMut<Assets<AudioInstance>>,
    sounds: Query<&RollbackSound>,
) {
    current_state.live.clear();
    current_state
        .live
        .extend(sounds.iter().map(RollbackSound::key));
    stop_interrupted(&mut current_state, &mut audio_instances);
}

const SOUND_CUE_LIFETIME_FRAMES: u32 = ROLLBACK_FPS as u32 * 5;

fn sound_cue_finished(current_frame: u32, start_frame: u32) -> bool {
    current_frame.wrapping_sub(start_frame) >= SOUND_CUE_LIFETIME_FRAMES
}

pub fn remove_finished_sounds(
    frame: Res<GGFrameCount>,
    sounds: Query<(Entity, &RollbackSound)>,
    mut commands: Commands,
) {
    for (entity, sound) in &sounds {
        if sound_cue_finished(frame.frame, sound.start_frame) {
            commands.entity(entity).despawn();
        }
    }
}

pub fn clear_sounds(
    mut commands: Commands,
    sounds: Query<Entity, With<RollbackSound>>,
    mut current_state: ResMut<PlaybackStates>,
    mut audio_instances: ResMut<Assets<AudioInstance>>,
) {
    for (_, handle) in current_state.playing.drain() {
        if let Some(instance) = audio_instances.get_mut(&handle) {
            instance.stop(AudioTween::linear(std::time::Duration::from_millis(100)));
        }
    }
    current_state.live.clear();
    for entity in &sounds {
        commands.entity(entity).despawn_recursive();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cue_expiration_is_deterministic_and_wrap_safe() {
        assert!(!sound_cue_finished(SOUND_CUE_LIFETIME_FRAMES - 1, 0));
        assert!(sound_cue_finished(SOUND_CUE_LIFETIME_FRAMES, 0));
        let start = u32::MAX - SOUND_CUE_LIFETIME_FRAMES + 2;
        assert!(!sound_cue_finished(0, start));
        assert!(sound_cue_finished(1, start));
    }

    #[test]
    fn rollback_sound_volume_combines_master_sfx_distance_and_base_gain() {
        assert_eq!(rollback_sound_volume(0.0, 50.0, 80.0, 0.65), 0.26);
        assert_eq!(rollback_sound_volume(10.0, 100.0, 100.0, 1.0), 0.25);
        assert_eq!(rollback_sound_volume(20.0, 100.0, 100.0, 1.0), 0.0);
    }
}
