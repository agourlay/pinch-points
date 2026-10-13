//! The settings file: `settings.txt` written out, and read back
//! leniently, so an old or hand-edited file never breaks startup.

use super::{
    CommitScheme, DEADZONE_RANGE, GameSettings, PuzzleSpeed, REPEAT_DELAY_RANGE,
    REPEAT_INTERVAL_RANGE, REPLAY_CAP_MAX, REPLAY_CAP_MIN, SeatInput, UI_SCALE_MAX, UI_SCALE_MIN,
    VOLUME_RANGE, clamp_in,
};
use crate::app::binds::default_binds;
use crate::app::i18n::Lang;
use crate::app::teams::TeamMode;
use crate::sim::MAX_PLAYERS;

/// A switch as the file spells it.
fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// A switch read back: anything but "off" is on, so a switch that a file
/// mangled, or one from before the switch existed, stays on.
fn is_on(value: &str) -> bool {
    value != "off"
}

/// A saved number read and held inside `range`, or `None` when it does
/// not read at all (the setting then keeps its default). Out of range is
/// clamped rather than thrown away: a hand-edited 101 means "loud".
fn parse_in<T: std::str::FromStr + PartialOrd + Copy>(
    value: &str,
    range: &std::ops::RangeInclusive<T>,
) -> Option<T> {
    value.parse().ok().map(|v| clamp_in(v, range))
}

/// The settings file, under the XDG config directory.
fn settings_path() -> std::path::PathBuf {
    crate::app::paths::config_dir().join("settings.txt")
}

impl GameSettings {
    /// The caps come in as a parameter because they are a resource of their
    /// own, not a field: forgetting them leaves `caps` unused, which the
    /// warning pass refuses.
    pub fn to_text(&self, caps: &crate::app::keycaps::KeyCaps) -> String {
        // Destructured with no rest pattern: a new setting refuses to build
        // here until it is written out. The lenient `parse` below would
        // never notice one going unsaved.
        let Self {
            commit,
            seat_input,
            repeat_delay,
            repeat_interval,
            music_on,
            music_volume,
            sfx_on,
            sfx_volume,
            puzzle_speed,
            team_mode,
            language,
            rumble,
            pad_deadzone,
            colorblind,
            fullscreen,
            ui_scale,
            reduced_motion,
            binds,
            names,
            replay_cap,
            last_beach,
            check_updates,
            keyboard,
        } = self;
        format!(
            "commit_scheme: {}\nrepeat_delay: {:.2}\nrepeat_interval: {:.2}\n\
             music_on: {}\nmusic: {}\nsfx_on: {}\nsfx: {}\n\
             puzzle_speed: {}\nteams: {}\nlanguage: {}\n\
             rumble: {}\npad_deadzone: {}\npalette: {}\nfullscreen: {}\nui_scale: {}\n\
             reduced_motion: {}\nnames: {}\nreplay_cap: {}\nbeach: {}\n\
             updates: {}\nkeycaps: {}\nkeyboard: {}\n\
             p1_input: {}\np2_input: {}\n{}",
            commit.key(),
            repeat_delay,
            repeat_interval,
            on_off(*music_on),
            music_volume,
            on_off(*sfx_on),
            sfx_volume,
            puzzle_speed.percent(),
            team_mode.key(),
            language.key(),
            on_off(*rumble),
            pad_deadzone,
            if *colorblind { "colorblind" } else { "classic" },
            on_off(*fullscreen),
            ui_scale,
            on_off(*reduced_motion),
            names.join("|"),
            replay_cap,
            last_beach,
            on_off(*check_updates),
            caps.to_text(),
            keyboard.map_or("auto", crate::app::keycaps::Layout::key),
            seat_input[0].name(),
            seat_input[1].name(),
            crate::app::binds::to_text(binds),
        )
    }

    /// Lenient parse: unknown keys and bad values fall back to defaults, so
    /// an old or hand-edited file never breaks startup. Both halves of the
    /// file come back together: the preferences, and the learned caps
    /// table that rides in it as the `keycaps:` line.
    pub fn parse(text: &str) -> (GameSettings, crate::app::keycaps::KeyCaps) {
        let mut settings = GameSettings::default();
        let mut caps = crate::app::keycaps::KeyCaps::default();
        // Slots whose saved key was retired and moved (see `binds::rehome`).
        let mut moved: Vec<(usize, usize)> = Vec::new();
        for line in text.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "commit_scheme" => settings.commit = CommitScheme::from_key(value),
                "p1_input" => settings.seat_input[0] = SeatInput::from_name(value),
                "p2_input" => settings.seat_input[1] = SeatInput::from_name(value),
                "repeat_delay" => {
                    let read = parse_in(value, &REPEAT_DELAY_RANGE);
                    settings.repeat_delay = read.unwrap_or(settings.repeat_delay);
                }
                "repeat_interval" => {
                    let read = parse_in(value, &REPEAT_INTERVAL_RANGE);
                    settings.repeat_interval = read.unwrap_or(settings.repeat_interval);
                }
                "music_on" => settings.music_on = is_on(value),
                "sfx_on" => settings.sfx_on = is_on(value),
                "music" => {
                    let read = parse_in(value, &VOLUME_RANGE);
                    settings.music_volume = read.unwrap_or(settings.music_volume);
                }
                // Was an on/off toggle before it became a slider; the old
                // words still parse so an existing settings.txt keeps working.
                "sfx" => {
                    settings.sfx_volume = match value {
                        "off" => 0,
                        "on" => 100,
                        _ => parse_in(value, &VOLUME_RANGE).unwrap_or(settings.sfx_volume),
                    };
                }
                "replay_cap" => {
                    let read = parse_in(value, &(REPLAY_CAP_MIN..=REPLAY_CAP_MAX));
                    settings.replay_cap = read.unwrap_or(settings.replay_cap);
                }
                "teams" => settings.team_mode = TeamMode::from_key(value),
                "language" => settings.language = Lang::from_key(value),
                "rumble" => settings.rumble = is_on(value),
                "pad_deadzone" => {
                    let read = parse_in(value, &DEADZONE_RANGE);
                    settings.pad_deadzone = read.unwrap_or(settings.pad_deadzone);
                }
                "palette" => settings.colorblind = value == "colorblind",
                "fullscreen" => settings.fullscreen = is_on(value),
                "ui_scale" => {
                    let read = parse_in(value, &(UI_SCALE_MIN..=UI_SCALE_MAX));
                    settings.ui_scale = read.unwrap_or(settings.ui_scale);
                }
                "reduced_motion" => settings.reduced_motion = value == "on",
                // `a|b|c|d`, so a name may hold spaces. Missing trailing
                // fields simply stay unnamed.
                "names" => {
                    for (seat, name) in value.split('|').take(MAX_PLAYERS).enumerate() {
                        settings.set_name(seat as u8, name.trim());
                    }
                }
                // Only kept if it still parses as an address: the file is
                // hand-editable, and a line of nonsense here would come
                // back as a pre-filled answer the player has to rub out.
                "beach" => {
                    if value.parse::<std::net::SocketAddr>().is_ok() {
                        settings.last_beach = value.to_string();
                    }
                }
                "updates" => settings.check_updates = is_on(value),
                "keycaps" => caps = crate::app::keycaps::KeyCaps::parse(value),
                "keyboard" => settings.keyboard = crate::app::keycaps::Layout::from_key(value),
                "keys_p1" | "keys_p2" => {
                    let seat = usize::from(key.trim() == "keys_p2");
                    if let Some((seat_binds, slots)) = crate::app::binds::parse_seat_moved(value) {
                        settings.binds[seat] = seat_binds;
                        moved.extend(slots.into_iter().map(|slot| (seat, slot)));
                    }
                }
                "puzzle_speed" => {
                    if let Ok(v) = value.parse::<u8>() {
                        settings.puzzle_speed = PuzzleSpeed::from_percent(v);
                    }
                }
                _ => {}
            }
        }
        // The two keyboard seats share one keyboard: a file that binds the
        // same key twice across seats would leave a seat shadowed, so the
        // whole table goes back to stock rather than half-work.
        for (seat, slot) in moved {
            crate::app::binds::rehome(&mut settings.binds, seat, slot);
        }
        if !crate::app::binds::all_distinct(&settings.binds) {
            settings.binds = default_binds();
        }
        // After the loop, not in the arms that read them: the keycaps
        // line may be read after either, and the presumption is only
        // taken where the learned caps leave room for it.
        let (language, keyboard) = (settings.language, settings.keyboard);
        settings.set_language(language, &mut caps);
        settings.set_keyboard(keyboard, &mut caps);
        (settings, caps)
    }

    /// The saved settings and caps, or `None` when there is no file to read.
    ///
    /// The distinction is the whole of the first-run language picker:
    /// defaults and a saved English are the same settings, and only one of
    /// them means nobody has been asked yet. One read rather than a
    /// [`Path::exists`] beside a load, which could disagree.
    ///
    /// [`Path::exists`]: std::path::Path::exists
    pub fn load_saved() -> Option<(GameSettings, crate::app::keycaps::KeyCaps)> {
        std::fs::read_to_string(settings_path())
            .ok()
            .map(|text| GameSettings::parse(&text))
    }

    pub fn load() -> (GameSettings, crate::app::keycaps::KeyCaps) {
        GameSettings::load_saved().unwrap_or_default()
    }

    /// Write the file, caps and all: one file on disk, so one write, and
    /// a caller without the caps table has no way to call it.
    pub fn save(&self, caps: &crate::app::keycaps::KeyCaps) {
        let _ = crate::app::paths::write_atomic(&settings_path(), self.to_text(caps));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::KeyCode;

    #[test]
    fn settings_round_trip_through_text() {
        let settings = GameSettings {
            commit: CommitScheme::Ijkl,
            repeat_delay: 0.35,
            // Both switches off, so the round trip is actually carrying
            // them: left at their default they would survive a `to_text`
            // that never wrote them and a `parse` that never read them.
            music_on: false,
            music_volume: 80,
            sfx_on: false,
            sfx_volume: 30,
            puzzle_speed: PuzzleSpeed::ThreeQuarter,
            team_mode: TeamMode::Trios,
            language: Lang::Fr,
            rumble: false,
            pad_deadzone: 70,
            colorblind: true,
            fullscreen: false,
            ui_scale: 70,
            reduced_motion: true,
            check_updates: false,
            binds: {
                let mut binds = default_binds();
                binds[0].set(crate::app::binds::Action::Remove, KeyCode::Backquote);
                binds[1].set(crate::app::binds::Action::ClearAll, KeyCode::End);
                binds
            },
            replay_cap: 35,
            names: [
                "Anna".into(),
                "Bo Diddley".into(),
                String::new(),
                "Dee".into(),
                "Eve".into(),
                String::new(),
            ],
            last_beach: "10.0.0.5:47777".into(),
            ..GameSettings::default()
        };
        let mut caps = crate::app::keycaps::KeyCaps::parse("KeyW=Z KeyA=Q");
        // Through the setter, so the language's presumed keyboard is in
        // place on this side too - `parse` takes it on the way back in.
        let mut settings = settings;
        settings.set_language(Lang::Fr, &mut caps);
        settings.set_keyboard(Some(crate::app::keycaps::Layout::Azerty), &mut caps);
        let (reparsed, recaps) = GameSettings::parse(&settings.to_text(&caps));
        assert_eq!(reparsed, settings);
        assert_eq!(recaps, caps, "the caps table rides the same file");
    }

    /// A file from when Num Enter was P2's clear-all, with Num . on P1:
    /// both layouts load, P2's clear-all moved to a key neither holds,
    /// rather than the clash putting both seats back to stock.
    #[test]
    fn an_old_num_enter_beside_a_held_num_dot_keeps_both_layouts() {
        use crate::app::binds::{Action, key_name};
        let mut p1 = default_binds()[0];
        p1.0[0] = KeyCode::NumpadDecimal;
        let p1_line: Vec<String> = p1.0.iter().copied().map(key_name).collect();
        let text = format!(
            "keys_p1: {}\nkeys_p2: KeyU KeyK KeyJ KeyL Numpad8 Numpad5 Numpad4 Numpad6 Numpad0 NumpadEnter\n",
            p1_line.join(" ")
        );
        let (settings, _) = GameSettings::parse(&text);
        assert_eq!(settings.binds[0], p1, "P1 as saved");
        assert_eq!(settings.binds[1].key(Action::MoveUp), KeyCode::KeyU);
        assert!(crate::app::binds::all_distinct(&settings.binds));
    }

    /// The `beach` line is kept only while it still parses as an address.
    /// See the arm that reads it for why.
    #[test]
    fn a_kept_beach_address_has_to_still_be_one() {
        assert_eq!(
            GameSettings::parse("beach: 192.168.1.5:47777\n")
                .0
                .last_beach,
            "192.168.1.5:47777"
        );
        assert_eq!(
            GameSettings::parse("beach: [::1]:47777\n").0.last_beach,
            "[::1]:47777",
            "including the six-legged sort"
        );
        for junk in ["beach: 192.168.1.5\n", "beach: over there\n", "beach:\n"] {
            assert_eq!(GameSettings::parse(junk).0.last_beach, "", "{junk:?}");
        }
    }

    /// Bindings only load if the whole table is sane: a file that gives two
    /// seats the same key would leave one of them shadowed, so it is thrown
    /// out wholesale rather than half-applied.
    #[test]
    fn clashing_saved_bindings_fall_back_to_defaults() {
        let stock = default_binds();
        // Seat 2 claiming seat 1's W: legal within its own line, but not
        // across the keyboard.
        let text = "keys_p2: KeyW KeyK KeyJ KeyL Numpad8 Numpad5 Numpad4 Numpad6 \
                    Numpad0 NumpadEnter\n";
        assert_eq!(GameSettings::parse(text).0.binds, stock);
        // A line that is merely unreadable leaves that seat stock too.
        assert_eq!(GameSettings::parse("keys_p1: nonsense\n").0.binds, stock);
    }

    #[test]
    fn parse_is_lenient_and_clamps() {
        // Junk lines and out-of-range values fall back to sane settings.
        let settings = GameSettings::parse(
            "garbage\nmusic: 900\npad_deadzone: 99\nrepeat_delay: 9.0\nwibble: 3\n",
        )
        .0;
        // 900 does not even fit a u8: unparseable values keep the default.
        assert_eq!(settings.music_volume, GameSettings::default().music_volume);
        // Parseable but out-of-range values clamp.
        assert_eq!(GameSettings::parse("music: 101\n").0.music_volume, 100);
        assert_eq!(settings.pad_deadzone, 80);
        assert!(settings.repeat_delay <= 0.5);
        assert_eq!(settings.language, Lang::En);
    }

    /// A settings.txt from before the switches existed comes up with the
    /// sound on: a file that predates a feature must not read as a player
    /// having turned it off.
    #[test]
    fn a_file_without_the_switches_still_has_sound() {
        let old = "music: 60\nsfx: 40\n";
        let settings = GameSettings::parse(old).0;
        assert!(settings.music_on, "no music_on line means music");
        assert!(settings.sfx_on, "no sfx_on line means effects");
        assert_eq!(settings.music_volume, 60);
        assert_eq!(settings.sfx_volume, 40);

        // And a switch that is off silences its half without disturbing
        // the other, or the volume waiting underneath it.
        let quiet = GameSettings::parse("music: 60\nsfx: 40\nmusic_on: off\n").0;
        assert_eq!(quiet.music_gain(), 0.0, "the music is off");
        assert!(quiet.sfx_gain() > 0.0, "the effects are not");
        assert_eq!(quiet.music_volume, 60, "and the slider is where it was");
    }

    /// A settings.txt from before the row existed launches fullscreen, like
    /// a first run: every player who had the game then was given a window
    /// they never chose, not one they asked for.
    #[test]
    fn a_file_without_the_fullscreen_line_fills_the_screen() {
        assert!(GameSettings::parse("music: 60\n").0.fullscreen);
        assert!(!GameSettings::parse("fullscreen: off\n").0.fullscreen);
    }

    /// The sfx row was once an on/off toggle; a settings.txt written by that
    /// version must still load with the volume it implied.
    #[test]
    fn old_sfx_toggle_files_still_load() {
        assert_eq!(GameSettings::parse("sfx: off\n").0.sfx_volume, 0);
        assert_eq!(GameSettings::parse("sfx: on\n").0.sfx_volume, 100);
        assert_eq!(GameSettings::parse("sfx: 65\n").0.sfx_volume, 65);
        assert_eq!(GameSettings::parse("sfx: 900\n").0.sfx_volume, 80);
        assert_eq!(GameSettings::parse("sfx: 101\n").0.sfx_volume, 100);
    }
}
