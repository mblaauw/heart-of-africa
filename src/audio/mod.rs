//! Ambient audio (DESIGN.md §8.4: audio middleware was deferred until it was
//! actually wanted — Phase 6 wants it). Render-side only: the sim never
//! imports this module. The loop is manifest-addressed (`audio.ambient`);
//! missing audio means silence, never a failure.

use macroquad::audio::{self, PlaySoundParams, Sound};

use crate::content::load::AssetManifest;
use crate::sim::events::Weather;

/// Looping ambient wind with weather-aware volume.
pub struct Ambient {
    wind: Option<Sound>,
    playing: bool,
    weather: Option<Weather>,
}

impl Ambient {
    /// Load the ambient loop from the manifest; silent when absent.
    pub async fn load(assets: Option<&AssetManifest>) -> Ambient {
        let wind = match assets.and_then(|a| a.path("audio.ambient")) {
            Some(path) => audio::load_sound(path.to_str().unwrap_or_default())
                .await
                .ok(),
            None => None,
        };
        Ambient {
            wind,
            playing: false,
            weather: None,
        }
    }

    /// Keep the loop playing; adjust volume when the weather changes.
    pub fn tick(&mut self, weather: Weather) {
        let Some(sound) = &self.wind else {
            return;
        };
        if !self.playing {
            audio::play_sound(
                sound,
                PlaySoundParams {
                    looped: true,
                    volume: 0.12,
                },
            );
            self.playing = true;
        }
        if self.weather != Some(weather) {
            let volume = match weather {
                Weather::Clear => 0.12,
                Weather::Heatwave => 0.08,  // air still, pressing
                Weather::Duststorm => 0.20, // the wind is the story
            };
            audio::set_sound_volume(sound, volume);
            self.weather = Some(weather);
        }
    }
}
