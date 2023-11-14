// rollback audio from https://johanhelsing.studio/posts/cargo-space-devlog-4

use bevy::prelude::*;
use bevy::utils::HashMap;
use bevy::utils::HashSet;
use bevy_ggrs::prelude::*;
use bevy_kira_audio::prelude::*;
use bevy_kira_audio::AudioSource;

use super::assets::sounds::SfxChannel;
use super::ggrs_framecount::GGFrameCount;
use super::networking::ROLLBACK_FPS;

#[derive(Component, Reflect, Default)]
pub struct RollbackSound {
    /// the actual sound effect to play
    pub clip: Handle<AudioSource>,
    /// when the sound effect should have started playing
    pub start_frame: u32,
    /// differentiates several unique instances of the same sound playing at once. (we'll get back to this) 
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
    /// the location sound emitter
    pub emitter: AudioEmitter,
    /// transform to place the emitter at
    pub transform: Transform,
    /// global transform for the entity
    pub global: GlobalTransform,
}

/// The "Actual" state.
/// 
/// I'm using bevy_kira for sound, but this could probably work similarly with bevy_audio.
#[derive(Resource, Reflect, Default)]
pub struct PlaybackStates {
    playing: HashMap<(Handle<AudioSource>, usize), Handle<AudioInstance>>,
    live: bevy::utils::hashbrown::HashSet<usize>,
}

impl PlaybackStates {
    /// a split borrow helper to make the borrow checker shut the hell up
    pub fn split(&mut self) -> (&mut HashMap<(Handle<AudioSource>, usize), Handle<AudioInstance>>, &bevy::utils::hashbrown::HashSet<usize>) {
        (&mut self.playing, &self.live)
    }
}

pub fn sync_rollback_sounds(
    mut current_state: ResMut<PlaybackStates>,
    mut audio_instances: ResMut<Assets<AudioInstance>>,
    mut sounds_query: Query<(&RollbackSound, &mut AudioEmitter)>,
    // audio: Res<Audio>,
    sfx_audio: Res<AudioChannel<SfxChannel>>,
    frame: Res<GGFrameCount>,
) {
    // remove any finished sound effects
    // current_state.playing.retain(|_, handle| {
    //     !matches!(
    //         audio_instances.state(handle),
    //         PlaybackState::Stopped | PlaybackState::Stopping { .. }
    //     )
    // });

    {
        // clear live map
        current_state.live.clear();
    }

    // start/update sound effects
    for (rollback_sound, mut emitter) in &mut sounds_query {
        let key = rollback_sound.key();
        if emitter.instances.is_empty() {
            // start sound

            let frames_late = frame.frame.wrapping_sub(rollback_sound.start_frame);
            const MAX_SOUND_DELAY: u32 = 10;
            // ignore any sound effects that are *really* late
            // todo: make configurable
            if frames_late <= MAX_SOUND_DELAY {
                if frames_late > 0 {
                    // todo: seek if time critical
                    info!(
                        "playing sound effect {} frames late",
                        frames_late
                    );
                }
                // start the sound
                let instance_handle = sfx_audio.play(rollback_sound.clip.clone()).handle();
                // insert the sound into our emitter (for positional output)
                emitter.instances.push(instance_handle);
            }
        } else {
            // already playing

        }
        // if current_state.playing.contains_key(&key) {
        //     // already playing
        //     // todo: compare frames and seek if time critical
        // } else {
        //     // assert_eq!(1u32.wrapping_sub(1), 0);
        //     let frames_late = frame.frame.wrapping_sub(rollback_sound.start_frame);
        //     const MAX_SOUND_DELAY: u32 = 10;
        //     // ignore any sound effects that are *really* late
        //     // todo: make configurable
        //     if frames_late <= MAX_SOUND_DELAY {
        //         if frames_late > 0 {
        //             // todo: seek if time critical
        //             info!(
        //                 "playing sound effect {} frames late",
        //                 frames_late
        //             );
        //         }
        //         let instance_handle = audio.play(rollback_sound.clip.clone()).handle();
        //         current_state
        //             .playing
        //             .insert(key.to_owned(), instance_handle);
        //     }
        // }

        // we keep track of `RollbackSound`s still existing, 
        // so we can remove any sound effects not present later
        current_state.live.insert(rollback_sound.key().1.to_owned());
    }

    // stop interrupted sound effects
    // some condition then instance.stop(AudioTween::linear(std::time::Duration::from_millis(100)));
    

    // THIS BREAKS AUDIO AND CAUSES IT TO NOT BE MARKED AS PLAYING


    // get a split reference to our state to make mr borrower happy
    // let (playing,live) = current_state.split();
    // for (_, instance_handle) in playing
    //     .extract_if(|(_, key), _| !live.contains(key))
    // {
    //     if let Some(instance) = audio_instances.get_mut(&instance_handle) {
    //         // todo: add config to use linear tweening, stop or keep playing as appropriate
    //         // instance.stop(default()); // immediate
    //         instance.stop(AudioTween::linear(std::time::Duration::from_millis(100)));
    //     } else {
    //         error!("Audio instance not found");
    //     }
    // }    

}

/// removes sounds that have finished playing
pub fn remove_finished_sounds(
    frame: Res<GGFrameCount>,
    query: Query<(Entity, &RollbackSound)>,
    mut commands: Commands,
    audio_sources: Res<Assets<AudioSource>>,
) {
    for (entity, rollback_sound) in query.iter() {
        // perf: cache frames_to_play instead of checking audio_sources every frame?
        if let Some(audio_source) = audio_sources.get(&rollback_sound.clip) {
            let frames_played = frame.frame.wrapping_sub(rollback_sound.start_frame);
            let seconds_to_play = audio_source.sound.duration().as_secs_f64();
            let frames_to_play = (seconds_to_play * ROLLBACK_FPS as f64) as u32;

            if frames_played >= frames_to_play as u32 {
                commands.entity(entity).despawn();
            }
        }
    }
}

/// when the round ends, clear all remaining sound entities before respawn
pub fn clear_sounds(
    mut commands: Commands,
    sounds: Query<Entity, With<RollbackSound>>,
) {
    for entity in &sounds {
        commands.entity(entity).despawn_recursive();
    }
}