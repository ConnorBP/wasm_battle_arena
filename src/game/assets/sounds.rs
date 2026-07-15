use bevy::prelude::*;
use bevy_asset_loader::prelude::*;
use bevy_kira_audio::prelude::*;

#[derive(AssetCollection, Resource)]
pub struct SoundAssets {
    #[asset(path = "sfx/laser_shoot.ogg")]
    pub laser_shoot: Handle<bevy_kira_audio::AudioSource>,
    #[asset(path = "sfx/ray.ogg")]
    pub ray: Handle<bevy_kira_audio::AudioSource>,
    #[asset(path = "sfx/swoosh_death.ogg")]
    pub swoosh_death: Handle<bevy_kira_audio::AudioSource>,
    #[asset(path = "music/menu.ogg")]
    pub menu_music: Handle<bevy_kira_audio::AudioSource>,
}

// custom audio channels

#[derive(Resource)]
pub struct MusicChannel;
#[derive(Resource)]
pub struct SfxChannel;

#[derive(Resource)]
pub struct AudioConfig {
    pub master_volume: f64,
    pub music_volume: f64,
    pub sfx_volume: f64,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            master_volume: 100.0,
            music_volume: 55.0,
            sfx_volume: 100.0,
        }
    }
}

pub fn update_volume(conf: Res<AudioConfig>, music: Res<AudioChannel<MusicChannel>>) {
    if conf.is_changed() {
        // Rollback SFX apply master volume alongside positional attenuation.
        let master_scale = conf.master_volume / 100.;

        music.set_volume((conf.music_volume / 100.) * MAX_MUSIC_VOL * master_scale);
    }
}

const MAX_MUSIC_VOL: f64 = 0.3;
pub fn start_main_music(
    sounds: Res<SoundAssets>,
    audio: Res<AudioChannel<MusicChannel>>,
    mut started: Local<bool>,
) {
    if *started {
        return;
    }
    audio.play(sounds.menu_music.clone()).looped();
    *started = true;
}
