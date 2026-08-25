//! The first-run language picker: the screen a fresh install opens on,
//! and the only screen that is not reachable from the menu.
//!
//! It exists because the menu itself is written in a language, and a
//! player who does not read that one has to guess their way to the
//! settings dial through seven rows of words they cannot read. Asking once,
//! before anything else is said, costs a keypress and removes the guess.
//!
//! The cursor *is* the setting: moving it writes `settings.language`
//! straight away, so the header, the prompt and the note under the card
//! are all already speaking the language under the cursor. That is the
//! preview, and it is why the rows themselves carry no translated words -
//! only a flag and the language's own name for itself, both of which read
//! the same whatever the game is currently set to.
//!
//! Around the card: a crab and a gull at its shoulders, and a pale flock
//! of both behind it. All of them are the game's own sprites, and they say
//! what is being started here in the one language every player reads.

use crate::app::art::Art;
use crate::app::company::{self, Perch};
use crate::app::cycle::Cycle;
use crate::app::i18n::{ALL_LANGS, Lang};
use crate::app::settings::GameSettings;
use crate::app::{Screen, menu_ui, palette};
use bevy::prelude::*;

#[derive(Component)]
pub struct LanguageUi;

/// One row of the picker, by its index into [`ALL_LANGS`].
#[derive(Component)]
pub struct LanguageRow(pub usize);

/// The note under the card. Under it rather than in the header's status
/// slot, which on a 1280-wide window puts it in the far top corner, a
/// screen's width from the list it is reassuring anybody about.
#[derive(Component)]
pub struct LanguageNote;

/// Bigger than the chip on the settings dial: here the flag is the thing
/// being chosen rather than a decoration beside it.
const FLAG_W: f32 = 33.0;
const FLAG_H: f32 = 22.0;
const ROW_FONT: f32 = 22.0;
/// Wide enough for the longest native name at [`ROW_FONT`] - Nederlands,
/// at ten characters - so every name starts at the same x.
const NAME_W: f32 = 190.0;

/// The card's own width: a flag, the gap after it, the widest name, and
/// the card's padding on both sides. Only the test reads it, to work out
/// where the flock may hang.
#[cfg(test)]
const CARD_W: f32 = FLAG_W + 12.0 + NAME_W + 2.0 * 22.0;

pub fn enter_language(mut commands: Commands, settings: Res<GameSettings>, art: Res<Art>) {
    commands
        .spawn((LanguageUi, menu_ui::between_bars()))
        .with_children(|wrap| {
            // The flock first, so it sits behind the card.
            company::flock(wrap, &art, &FLOCK);
            // The card between the two big ones, so the row centres on the
            // list rather than on the crab.
            wrap.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(company::CRITTER_GAP),
                ..default()
            })
            .with_children(|line| {
                company::shoulder(line, &art, crate::app::company::Company::Crab, 0.0);
                line.spawn(menu_ui::screen_card()).with_children(|card| {
                    for (index, lang) in ALL_LANGS.iter().enumerate() {
                        card.spawn((LanguageRow(index), menu_ui::card_row()))
                            .with_children(|row| {
                                row.spawn((
                                    ImageNode::new(art.flag(*lang)),
                                    Node {
                                        width: Val::Px(FLAG_W),
                                        height: Val::Px(FLAG_H),
                                        flex_shrink: 0.0,
                                        margin: UiRect::right(Val::Px(12.0)),
                                        ..default()
                                    },
                                ));
                                row.spawn((
                                    Text::new(lang.native_name()),
                                    TextFont {
                                        font_size: FontSize::Px(ROW_FONT),
                                        ..default()
                                    },
                                    TextLayout::no_wrap(),
                                    TextColor(palette::IDLE_ROW),
                                    Node {
                                        width: Val::Px(NAME_W),
                                        ..default()
                                    },
                                ));
                            });
                    }
                });
                company::shoulder(line, &art, crate::app::company::Company::Gull, 1.7);
            });
            wrap.spawn((
                LanguageNote,
                Text::new(settings.tr().pick_language_later),
                TextFont {
                    font_size: FontSize::Px(15.0),
                    ..default()
                },
                TextLayout::no_wrap(),
                TextColor(palette::PARCHMENT.with_alpha(0.40)),
            ));
        });
}

/// The flock behind the card: gulls in the air above it, crabs on the sand
/// below, each as a fraction of the frame with the size it is drawn at.
///
/// Placed by hand rather than scattered at random. This screen is looked
/// at once, on one run, and the one look it gets should be the composed
/// one: nothing overlapping the card, nothing bunched in a corner.
const FLOCK: [Perch; 7] = [
    crate::app::company::Perch::gull(0.09, 0.13, 46.0),
    crate::app::company::Perch::gull(0.25, 0.04, 30.0),
    crate::app::company::Perch::gull(0.88, 0.09, 40.0),
    crate::app::company::Perch::gull(0.70, 0.02, 26.0),
    crate::app::company::Perch::crab(0.12, 0.80, 42.0),
    crate::app::company::Perch::crab(0.35, 0.92, 28.0),
    crate::app::company::Perch::crab(0.86, 0.84, 48.0),
];

/// The screen a boot opens on: the picker until a settings file exists to
/// say the question has already been answered.
///
/// Takes the answer rather than looking for the file, so the rule can be
/// checked without one, and so the read happens once at startup beside
/// the load it shares.
pub fn opening_screen(saved: bool) -> Screen {
    if saved {
        Screen::default()
    } else {
        Screen::Language
    }
}

/// Whether the settings file may be written yet.
///
/// [`opening_screen`] reads "first run" off the absence of that file and
/// off nothing else, so any write before a language has been taken
/// answers the picker's question on the player's behalf: they close the
/// window on the first screen, having chosen nothing, and are never asked
/// again. Two things would otherwise do exactly that - a keyboard that
/// learns a cap from a press in the picker, and the start-up keymap read
/// - so both ask here first.
///
/// Nothing is lost by waiting. The caps stay in the resource, and the
/// Enter that takes the language writes them out with it.
pub fn may_save(screen: &Screen) -> bool {
    !matches!(screen, Screen::Language)
}

/// What a keypress does to the picker: where the cursor lands, and
/// whether that was the player taking it.
///
/// Pure, and separate from the system below, because the system's answer
/// to "taken" is to write the settings file - which in a test is the
/// tester's own. The rule is worth checking; the write is not.
fn step(keys: &ButtonInput<KeyCode>, current: Lang) -> (Lang, bool) {
    let at = menu_ui::nav(keys, current.index(), ALL_LANGS.len());
    let taken = menu_ui::enter(keys);
    (ALL_LANGS[at], taken)
}

/// W/S walks the list and sets the language as it goes; Enter keeps it.
///
/// Enter writes the file, and that is what stops the screen coming back:
/// the picker is chosen at boot on the absence of that file and on
/// nothing else. Quitting without choosing writes nothing and is asked
/// again next time, which is the right answer to a window closed on the
/// first screen.
///
/// Escape is deliberately not wired up. There is nowhere behind this
/// screen to go, and the key that quits the game is a poor thing to leave
/// under a finger on the first screen anybody sees.
pub fn language_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<GameSettings>,
    mut caps: ResMut<crate::app::keycaps::KeyCaps>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    let (lang, taken) = step(&keys, settings.language);
    if lang != settings.language {
        settings.set_language(lang, &mut caps);
    }
    if taken {
        settings.save(&caps);
        next_screen.set(Screen::Menu);
    }
}

pub fn update_language_ui(
    settings: Res<GameSettings>,
    mut rows: Query<(&LanguageRow, &mut BackgroundColor, &Children)>,
    mut text: Query<(&mut TextColor, &mut Node)>,
    mut note: Query<&mut Text, With<LanguageNote>>,
) {
    // The note is the one line on this card with words in it, so it is
    // the one line that has to follow the cursor into another language.
    for mut line in &mut note {
        menu_ui::set_text(&mut line, settings.tr().pick_language_later);
    }
    let picked = settings.language.index();
    for (row, mut fill, children) in &mut rows {
        let on = row.0 == picked;
        menu_ui::set_bg(&mut fill, menu_ui::band(on));
        for child in children {
            let Ok((mut colour, _)) = text.get_mut(*child) else {
                continue;
            };
            menu_ui::set_color(
                &mut colour,
                if on {
                    palette::SELECTED_ROW
                } else {
                    palette::IDLE_ROW
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pressing(key: KeyCode) -> ButtonInput<KeyCode> {
        let mut keys = ButtonInput::default();
        keys.press(key);
        keys
    }

    /// The cursor and the setting are one value, so walking down the list
    /// has to reach every language: one that cannot be walked to is one
    /// nobody on a fresh install can choose.
    #[test]
    fn walking_down_reaches_every_language() {
        let mut at = Lang::default();
        let mut seen = vec![at];
        for _ in 1..ALL_LANGS.len() {
            at = step(&pressing(KeyCode::KeyS), at).0;
            seen.push(at);
        }
        for lang in ALL_LANGS {
            assert!(seen.contains(&lang), "{lang:?} cannot be walked to");
        }
        // And the lap closes rather than stopping at the bottom.
        assert_eq!(step(&pressing(KeyCode::KeyS), at).0, Lang::default());
    }

    /// Up from the first row wraps to the last, so the languages at the
    /// bottom are one keypress away rather than five.
    #[test]
    fn walking_up_wraps_to_the_end() {
        let up = step(&pressing(KeyCode::KeyW), Lang::default()).0;
        assert_eq!(up, *ALL_LANGS.last().unwrap());
    }

    /// Moving the cursor is not choosing. Only Enter is, or the first
    /// press of W would write the file and the screen would never be
    /// seen again.
    #[test]
    fn only_enter_takes_it() {
        for key in [
            KeyCode::KeyW,
            KeyCode::KeyS,
            KeyCode::ArrowUp,
            KeyCode::Escape,
        ] {
            assert!(!step(&pressing(key), Lang::default()).1, "{key:?} took it");
        }
        for key in [KeyCode::Enter, KeyCode::NumpadEnter] {
            let (lang, taken) = step(&pressing(key), Lang::Es);
            assert!(taken, "{key:?} did not take it");
            // And takes the row the cursor is on, not the one it opened on.
            assert_eq!(lang, Lang::Es);
        }
    }

    /// The whole of the feature: no settings file means nobody has been
    /// asked, and a settings file means they have. Saved English and
    /// defaulted English are the same settings and must not be the same
    /// answer here, or the picker either never appears or never stops.
    #[test]
    fn only_a_fresh_install_is_asked() {
        assert_eq!(opening_screen(false), Screen::Language);
        assert_eq!(opening_screen(true), Screen::default());
        assert_ne!(Screen::default(), Screen::Language);
    }

    /// The flock is hung on the frame by hand, and a hand can hang one
    /// behind the card - where the card's near-solid fill shows it through
    /// as a smudge under a language name. Gulls fly and crabs do not, so
    /// they belong on their own halves of the sky and the sand.
    #[test]
    fn the_flock_leaves_the_card_alone() {
        company::flock_is_hung_clear(&FLOCK, company::keep_clear(CARD_W), (0.15, 0.80));
    }

    /// The picker opens on whatever `Lang::default()` is, and that has to
    /// be a language the list actually holds.
    #[test]
    fn the_default_language_is_on_the_list() {
        assert!(ALL_LANGS.contains(&Lang::default()));
        assert_eq!(ALL_LANGS[Lang::default().index()], Lang::default());
    }
}
