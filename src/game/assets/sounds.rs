use bevy::prelude::*;
use bevy_asset_loader::prelude::*;
use bevy_kira_audio::prelude::*;

// jsfxr sound ids
// ray 3ZwhKQRSUmTpaPmioXX3h88MDjkD7i4skvn4mxZVhTzNt6DbwXL3Zac9jvXJquvAnYWtpZw7G46dKJum3HGHQKDgHU7bB8MNdfCLVDXqeymqpjf96HonSmgpC
// laser 3ZwhKQRSUmTpaPmioXX3h88MDjkD7i4skvn4mxZVhTzNt6DbwXL3Zac9jvXJquvAnYWtpZw7G46dKHfVd4YLs6TkmhWfbS2JHocRksyrnJHhbZ6hFo29ZWXYx
// swoosh_death 8qvNiwF3DRwQcmXf5PjRAP5NPPLxXRX6YEfWxTwo8QjtYg1AfvAJhEgVZH3vEcJHHVQ2T9WEejpEVNNpU4s9NcPtWLM8QkFcGgUpxcVzjnneLs6QYHMv9KF3d

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
            music_volume: 100.0,
            sfx_volume: 100.0,
        }
    }
}

pub fn update_volume(
    conf: Res<AudioConfig>,
    // master: Res<Audio>,
    music: Res<AudioChannel<MusicChannel>>,
    sfx: Res<AudioChannel<SfxChannel>>,
) {
    if conf.is_changed() {
        // other chanels do not seem to route through this just yet
        // master.set_volume(conf.master_volume / 100.);

        // scale every channel by master value
        let master_scale = conf.master_volume / 100.;

        music.set_volume((conf.music_volume / 100.) * MAX_MUSIC_VOL * master_scale);
        sfx.set_volume((conf.sfx_volume / 100.) * master_scale);
    }
}

const MAX_MUSIC_VOL: f64 = 0.3;
pub fn start_main_music(
    sounds: Res<SoundAssets>,
    // audio: Res<Audio>,
    audio: Res<AudioChannel<MusicChannel>>,
    mut cfg: ResMut<AudioConfig>,
) {
    audio.play(sounds.menu_music.clone())
        .looped();
        //.with_volume(MAX_MUSIC_VOL);
        // .fade_in(AudioTween::linear(std::time::Duration::from_secs(4)).with_easing(AudioEasing::InOutPowf(2.4)));
        // set the music volume to default. This triggers the update_volume system
        cfg.music_volume = 100.0;
}