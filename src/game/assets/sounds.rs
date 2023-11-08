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


pub fn start_main_music(
    sounds: Res<SoundAssets>,
    audio: Res<Audio>,
) {
    audio.play(sounds.menu_music.clone())
        .looped()
        .with_volume(0.3)
        .fade_in(AudioTween::linear(std::time::Duration::from_secs(4)).with_easing(AudioEasing::InOutPowf(2.4)));
}