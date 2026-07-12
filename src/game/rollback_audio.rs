use bevy::{prelude::*, utils::{HashMap, HashSet}};
use bevy_kira_audio::{prelude::*, AudioSource};

use super::{
    assets::sounds::SfxChannel,
    ggrs_framecount::GGFrameCount,
    networking::ROLLBACK_FPS,
};

#[derive(Component, Reflect, Default)]
pub struct RollbackSound {
    pub clip: Handle<AudioSource>,
    pub start_frame: u32,
    pub sub_key: usize,
}

impl RollbackSound {
    pub fn key(&self) -> (Handle<AudioSource>, usize) {
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
    playing: HashMap<(Handle<AudioSource>, usize), Handle<AudioInstance>>,
    live: HashSet<(Handle<AudioSource>, usize)>,
}

pub fn sync_rollback_sounds(
    mut commands: Commands,
    mut current_state: ResMut<PlaybackStates>,
    mut audio_instances: ResMut<Assets<AudioInstance>>,
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

pub fn remove_finished_sounds(
    frame: Res<GGFrameCount>,
    sounds: Query<(Entity, &RollbackSound)>,
    mut commands: Commands,
    audio_sources: Res<Assets<AudioSource>>,
) {
    for (entity, sound) in &sounds {
        if let Some(source) = audio_sources.get(&sound.clip) {
            let frames_played = frame.frame.wrapping_sub(sound.start_frame);
            let frames_to_play = (source.sound.duration().as_secs_f64() * ROLLBACK_FPS as f64) as u32;
            if frames_played >= frames_to_play {
                commands.entity(entity).despawn();
            }
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
