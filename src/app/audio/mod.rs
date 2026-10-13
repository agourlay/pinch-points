//! Everything the game says out loud: the effects ([`sfx`]), driven by the
//! sim's events, and the music ([`music`]), two shuffled sets of tracks.
//!
//! Anything that happens *somewhere* on the beach is panned to that side of
//! the stereo field ([`sfx::pan_pos`]); banners and stingers, which belong
//! to the whole round rather than a tile, stay centred.
//!
//! Three things can silence the game, and they compose rather than fight:
//! the two settings switches (music and effects, each with its own volume
//! underneath), the master mute on M ([`Muted`]), and the pause card, which
//! puts the music down for as long as it is up. Everything reads them
//! through [`sfx_gain`] and [`music::music_audible`], so no sink is left
//! holding an opinion of its own.

mod music;
mod sfx;

pub use music::{FadingOut, Music, drive_music, fade_music, rotate_music, toggle_mute};
pub use sfx::{Sounds, play_chime, play_denied, play_events, play_lose, play_win};

use bevy::audio::SpatialListener;
use bevy::prelude::*;
use music::MusicPlaylist;

/// The master mute on M: everything off, now, without touching what the
/// player has set.
///
/// Deliberately not a setting. The two switches on the settings card are
/// preferences and are written to disk; this is the key you hit when
/// somebody walks in, and the game comes back the way you left it. So
/// unmuting restores the settings, which may still say off.
#[derive(Resource, Default)]
pub struct Muted(pub bool);

/// Effects gain right now: the settings switch and slider, with the master
/// mute able to veto. Zero means nothing is spawned at all.
pub(crate) fn sfx_gain(settings: &crate::app::settings::GameSettings, muted: &Muted) -> f32 {
    if muted.0 { 0.0 } else { settings.sfx_gain() }
}

pub fn load_sounds(mut commands: Commands, assets: Res<AssetServer>) {
    // One listener, parked at the origin; emitters are placed relative to it
    // by `pan_pos` rather than at their true board positions, so the camera
    // zooming or sliding never changes how the beach sounds.
    commands.spawn(SpatialListener::new(sfx::EAR_GAP));
    commands.insert_resource(MusicPlaylist::new(
        // Calm: the menus, the puzzles and the editor.
        vec![
            // theme.wav predates the sound generator and has no source to
            // re-encode from; the generated loops ship as OGG. It was also
            // mixed 10 dB hotter than the rest and has been turned down to
            // their -23 dB, so it no longer jumps out of the shuffle.
            assets.load("sounds/theme.wav"),
            assets.load("sounds/theme_b.ogg"),
            assets.load("sounds/theme_c.ogg"),
            assets.load("sounds/theme_d.ogg"),
            assets.load("sounds/theme_e.ogg"),
            assets.load("sounds/theme_f.ogg"),
            assets.load("sounds/theme_g.ogg"),
            // The soft three, twice as long and gentler on the ear.
            assets.load("sounds/theme_h.ogg"),
            assets.load("sounds/theme_i.ogg"),
            assets.load("sounds/theme_j.ogg"),
        ],
        // Battle: the dance tracks, drums and a pumping bass, for versus.
        vec![
            assets.load("sounds/theme_k.ogg"),
            assets.load("sounds/theme_l.ogg"),
            assets.load("sounds/theme_m.ogg"),
            assets.load("sounds/theme_n.ogg"),
            assets.load("sounds/theme_o.ogg"),
            assets.load("sounds/theme_p.ogg"),
        ],
        crate::app::effects::VisualRng::from_clock(),
    ));
    commands.insert_resource(Sounds {
        place: assets.load("sounds/place.wav"),
        remove: assets.load("sounds/remove.wav"),
        bank: assets.load("sounds/bank.wav"),
        eat: assets.load("sounds/eat.wav"),
        raid: assets.load("sounds/raid.wav"),
        win: assets.load("sounds/win.wav"),
        lose: assets.load("sounds/lose.wav"),
        takeoff: assets.load("sounds/takeoff.wav"),
        screech: assets.load("sounds/screech.wav"),
        event: assets.load("sounds/event.wav"),
        golden: assets.load("sounds/golden.wav"),
        tier: assets.load("sounds/tier.wav"),
        surge: assets.load("sounds/surge.wav"),
        horn: assets.load("sounds/horn.wav"),
        denied: assets.load("sounds/denied.wav"),
        evict: assets.load("sounds/evict.wav"),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::settings::GameSettings;

    /// The master mute takes the effects with it: reaching for M because
    /// someone walked in should not leave every gull on the beach audible.
    #[test]
    fn the_master_mute_silences_the_effects_too() {
        let settings = GameSettings {
            sfx_on: true,
            sfx_volume: 80,
            ..GameSettings::default()
        };
        assert!(sfx_gain(&settings, &Muted(false)) > 0.0, "heard by default");
        assert_eq!(sfx_gain(&settings, &Muted(true)), 0.0, "M silences them");

        // The switch on the settings card does it on its own, and keeps
        // the volume it was set to for when it comes back on.
        let off = GameSettings {
            sfx_on: false,
            ..settings.clone()
        };
        assert_eq!(sfx_gain(&off, &Muted(false)), 0.0, "switched off");
        assert_eq!(off.sfx_volume, 80, "the slider is where it was left");
    }
}
