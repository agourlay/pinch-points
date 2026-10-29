//! Keeping a finished round: the shelf holds only the rounds a player
//! chose to keep.
//!
//! A round that ends is not filed on its own. Its recording is held while
//! its results card is up, and the card offers to put it on the shelf
//! ([`KEEP_CAP`] at a keyboard, the results table's button on a pad).
//! Leaving the card for a rematch, the menu or the lobby lets it go.
//! Leaving it for the interlude before a series' next round does not: the
//! offer stands there, key and prompt alike, until that round starts or
//! the interlude is left any other way ([`let_the_round_go`]). Online that
//! is the only way a joiner or a spectator gets to keep a round the host
//! moved on from at once, since the host's call walks every peer off its
//! card the moment it lands.
//!
//! Every reader asks [`RoundToKeep::offer`]: the key's handler
//! ([`keep_input`]), the pad's table, the prompt line and the card's own
//! line of keys, so none of them can offer a round the key would not keep.

use crate::app::i18n::Tr;
use bevy::prelude::*;
use std::path::{Path, PathBuf};

/// The letter whose key keeps the round on the results card, as the
/// prompt names it. Not a global letter, which every screen answers, and
/// not a key either stock seat plays with: the seats' keys are live on
/// the versus screen, and the IJKL block's K, the obvious letter, is the
/// second seat's down.
pub const KEEP_CAP: char = 'G';

/// What the results card has to offer the shelf.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum KeepOffer {
    /// No recording of this round on this machine: a replay being
    /// watched (it is on the shelf already), a round picked up from a
    /// code, a late spectator who saw only the end. Nothing is said.
    #[default]
    Nothing,
    /// A recording waiting to be kept.
    Offered,
    /// Kept: the card says so, and the key has nothing more to do.
    Kept,
}

/// A finished round's recording, held from the moment the tide comes in
/// until its results card is left.
#[derive(Resource, Default, Debug)]
pub enum RoundToKeep {
    #[default]
    Nothing,
    Offered(Box<Unkept>),
    Kept,
}

/// A round not yet on the shelf: everything filing it needs, taken when
/// the round ended, so it is filed the same whenever the key is pressed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unkept {
    /// The recording, as the shelf stores it.
    pub text: String,
    /// When the round ended: the file is named for the round, not for the
    /// press that kept it.
    pub stamp: u64,
    /// Who took it, as the card names them, or `None` for a draw (see
    /// [`super::file_name`]).
    pub winner: Option<String>,
}

impl RoundToKeep {
    /// What the card offers: the one answer the key, the pad's table and
    /// both prompts read.
    pub fn offer(&self) -> KeepOffer {
        match self {
            RoundToKeep::Nothing => KeepOffer::Nothing,
            RoundToKeep::Offered(_) => KeepOffer::Offered,
            RoundToKeep::Kept => KeepOffer::Kept,
        }
    }

    /// Put the round on the shelf in `dir` under [`super::free_path`],
    /// then trim the shelf to `cap`, as a pasted code is kept. Only once:
    /// a round already kept, or none to keep, files nothing and answers
    /// `None`. A write that fails leaves the offer standing, so the key
    /// can be tried again.
    pub fn keep_in(&mut self, dir: &Path, cap: u8) -> Option<std::io::Result<PathBuf>> {
        let RoundToKeep::Offered(round) = self else {
            return None;
        };
        let path = super::free_path(dir, round.stamp, round.winner.as_deref());
        if let Err(e) = crate::app::paths::write_atomic(&path, &round.text) {
            return Some(Err(e));
        }
        super::prune_in(dir, cap);
        *self = RoundToKeep::Kept;
        Some(Ok(path))
    }
}

/// The card's word on keeping, as the keyboard's line has it: the key
/// while it is on offer, the confirmation once taken, nothing for a round
/// there is no recording of.
pub fn keep_words(tr: &Tr, offer: KeepOffer) -> Option<&'static str> {
    match offer {
        KeepOffer::Nothing => None,
        KeepOffer::Offered => Some(tr.prompt_keep_replay),
        KeepOffer::Kept => Some(tr.replay_kept),
    }
}

/// What a pad's line puts in front of the results table's buttons: the
/// confirmation once the round is kept. On offer, the table's own row
/// names the button.
pub fn kept_lead(tr: &Tr, offer: KeepOffer) -> Option<&'static str> {
    match offer {
        KeepOffer::Kept => Some(tr.replay_kept),
        KeepOffer::Nothing | KeepOffer::Offered => None,
    }
}

/// Where [`keep_input`] runs, on the results card and on the interlude:
/// one name to order against, the system being scheduled twice.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub struct KeepKey;

/// Leaving the interlude, for the round it leads to or anywhere else:
/// the round before it was not kept, and goes. The next round's own
/// offer is made when its tide comes in.
pub fn let_the_round_go(mut round: ResMut<RoundToKeep>) {
    *round = RoundToKeep::Nothing;
}

/// The key on the results card and on the interlude after it: the round
/// onto the shelf, once.
///
/// Not while the pause card holds the keyboard, nor while a spectator is
/// typing a line to the table, whose letters are the line's.
pub fn keep_input(
    keyboard: crate::app::keycaps::Keyboard,
    pause: Res<crate::app::pause::PauseMenu>,
    chat: Res<crate::app::spectators::SpectatorChat>,
    library: Res<super::Library>,
    settings: Res<crate::app::settings::GameSettings>,
    mut round: ResMut<RoundToKeep>,
) {
    if pause.is_open() || chat.open() || !keyboard.just(KEEP_CAP) {
        return;
    }
    match round.keep_in(&library.dir, settings.replay_cap) {
        Some(Ok(path)) => info!("replay kept as {}", path.display()),
        Some(Err(e)) => warn!("could not keep the replay: {e}"),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::gamepad::scheme::{self, Face};
    use crate::app::i18n::ALL_LANGS;
    use crate::app::play_input::AfterRound;

    /// A scratch shelf of its own, emptied first.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pinch-keep-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch shelf");
        dir
    }

    fn offered(text: &str) -> RoundToKeep {
        RoundToKeep::Offered(Box::new(Unkept {
            text: text.to_string(),
            stamp: 1_700_000_000,
            winner: Some("Bo".to_string()),
        }))
    }

    /// One press files the round once, under the name the round earned
    /// and with its recording whole; the second press files nothing more.
    #[test]
    fn a_kept_round_is_filed_once() {
        let dir = scratch("once");
        let mut round = offered("the recording");
        assert_eq!(round.offer(), KeepOffer::Offered);
        let path = round.keep_in(&dir, 35).expect("on offer").expect("written");
        assert_eq!(
            path,
            dir.join(super::super::file_name(1_700_000_000, Some("Bo")))
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "the recording"
        );
        assert_eq!(round.offer(), KeepOffer::Kept);
        assert!(round.keep_in(&dir, 35).is_none(), "nothing more to keep");
        assert_eq!(super::super::shelf_in(&dir).len(), 1, "no duplicate");
    }

    /// Keeping trims the shelf to the setting, as a round filed always did.
    #[test]
    fn keeping_trims_the_shelf_to_its_cap() {
        let dir = scratch("cap");
        for stamp in 1..=3u64 {
            let path = super::super::free_path(&dir, stamp, None);
            std::fs::write(path, "old").expect("stocked");
        }
        let mut round = offered("new");
        round.keep_in(&dir, 2).expect("on offer").expect("written");
        let shelf = super::super::shelf_in(&dir);
        assert_eq!(shelf.len(), 2);
        assert_eq!(shelf[0].winner, "Bo", "the newest stays");
    }

    /// The keyboard's line and the pad's agree with the offer: the key
    /// while it stands, the confirmation after, silence with nothing to
    /// keep, and the pad's button only while there is something to press
    /// it for.
    #[test]
    fn the_card_says_what_the_key_does() {
        for lang in ALL_LANGS {
            let tr = lang.tr();
            let words = keep_words(tr, KeepOffer::Offered).expect("offered");
            assert!(
                words.starts_with(&format!("{KEEP_CAP}:"))
                    || words.starts_with(&format!("{KEEP_CAP} :")),
                "{lang:?} names the key: {words}"
            );
            assert_eq!(keep_words(tr, KeepOffer::Kept), Some(tr.replay_kept));
            assert_eq!(keep_words(tr, KeepOffer::Nothing), None);
        }
        let results = |keep| Face::Results {
            door: AfterRound::Menu,
            talk: false,
            rematch: None,
            keep,
        };
        let tr = &crate::app::i18n::EN;
        let offered = scheme::legend(
            tr,
            crate::app::gamepad::PadFamily::Xbox,
            &results(KeepOffer::Offered),
        );
        assert!(offered.contains("X: keep replay"), "{offered}");
        for keep in [KeepOffer::Kept, KeepOffer::Nothing] {
            let line = scheme::legend(tr, crate::app::gamepad::PadFamily::Xbox, &results(keep));
            assert!(!line.contains("X:"), "{keep:?}: {line}");
        }
        assert_eq!(kept_lead(tr, KeepOffer::Kept), Some("replay kept"));
    }

    /// The key is nobody else's on the results card or the interlude: not
    /// a global letter, not either stock seat's, and not a letter any of
    /// their faces' pad rows presses for something else.
    #[test]
    fn the_keep_key_is_free_on_the_results_card() {
        assert!(!crate::app::binds::GLOBAL_LETTERS.contains(&KEEP_CAP));
        let caps = crate::app::keycaps::KeyCaps::default();
        let key = caps.key_for(KEEP_CAP).expect("a key");
        for seat in 0..2 {
            let stock = crate::app::binds::SeatBinds::default_for(seat);
            assert!(!stock.0.contains(&key), "seat {seat} plays {key:?}");
        }
        for face in crate::app::gamepad::face::every_face() {
            if !matches!(face, Face::Results { .. } | Face::Interlude { .. }) {
                continue;
            }
            for row in scheme::binds(&face) {
                if row.key == scheme::Key::Cap(KEEP_CAP) {
                    assert_eq!(row.does, scheme::Does::KeepReplay, "{face:?}");
                }
            }
        }
    }

    /// The whole path a press takes: the round over, the key or the pad's
    /// button through the bridge, and the shelf. Leaving the card without
    /// it keeps nothing.
    mod app {
        use super::*;
        use crate::app::gamepad::scheme::Pad;
        use crate::app::gamepad::{PadFace, PadPick};
        use crate::app::replays::Library;

        /// A results card's input on a shelf of its own.
        pub(super) fn card(dir: &Path, round: RoundToKeep) -> App {
            let mut app = App::new();
            app.init_resource::<ButtonInput<KeyCode>>();
            app.init_resource::<crate::app::keycaps::KeyCaps>();
            app.init_resource::<crate::app::cursor::LiveSeatKeys>();
            app.init_resource::<crate::app::pause::PauseMenu>();
            app.init_resource::<crate::app::spectators::SpectatorChat>();
            app.init_resource::<crate::app::settings::GameSettings>();
            app.insert_resource(Library {
                dir: dir.to_path_buf(),
                ..Library::default()
            });
            app.insert_resource(round);
            app.add_systems(Update, keep_input);
            app
        }

        pub(super) fn press_keep(app: &mut App) {
            let key = app
                .world()
                .resource::<crate::app::keycaps::KeyCaps>()
                .key_for(KEEP_CAP)
                .expect("a key");
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.clear();
            keys.press(key);
            app.update();
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(key);
            keys.clear();
        }

        #[test]
        fn the_key_keeps_the_round_once() {
            let dir = scratch("key");
            let mut app = card(&dir, offered("a round"));
            app.update();
            assert!(
                crate::app::replays::shelf_in(&dir).is_empty(),
                "not by itself"
            );
            press_keep(&mut app);
            assert_eq!(crate::app::replays::shelf_in(&dir).len(), 1);
            press_keep(&mut app);
            assert_eq!(crate::app::replays::shelf_in(&dir).len(), 1, "once");
        }

        /// The pause card has the keyboard while it is up.
        #[test]
        fn not_under_the_pause_card() {
            let dir = scratch("paused");
            let mut app = card(&dir, offered("a round"));
            app.insert_resource(crate::app::pause::PauseMenu::raised());
            press_keep(&mut app);
            assert!(crate::app::replays::shelf_in(&dir).is_empty());
        }

        /// A spectator typing a line to the table types the letter.
        #[test]
        fn not_while_a_spectator_types() {
            let dir = scratch("typing");
            let mut app = card(&dir, offered("a round"));
            app.world_mut()
                .resource_mut::<crate::app::spectators::SpectatorChat>()
                .0 = Some("good game".to_string());
            press_keep(&mut app);
            assert!(crate::app::replays::shelf_in(&dir).is_empty());
        }

        /// A pad's X on the results card reaches the key through the
        /// bridge, which presses what the table's row says.
        #[test]
        fn a_pads_button_keeps_it_through_the_bridge() {
            let dir = scratch("pad");
            let mut app = card(&dir, offered("a round"));
            app.init_resource::<crate::app::gamepad::Bridged>();
            app.init_resource::<PadPick>();
            app.init_resource::<Time>();
            app.insert_resource(PadFace(Face::Results {
                door: AfterRound::Menu,
                talk: false,
                rematch: None,
                keep: KeepOffer::Offered,
            }));
            app.add_systems(
                PreUpdate,
                (
                    crate::app::gamepad::pad_menu_bridge,
                    crate::app::gamepad::pad_bridge_release,
                )
                    .chain(),
            );
            let pad = app.world_mut().spawn(Gamepad::default()).id();
            app.update();
            let button = Pad::X.button().expect("a button");
            app.world_mut()
                .entity_mut(pad)
                .get_mut::<Gamepad>()
                .expect("pad")
                .digital_mut()
                .press(button);
            app.update();
            assert_eq!(crate::app::replays::shelf_in(&dir).len(), 1);
            assert_eq!(
                app.world().resource::<RoundToKeep>().offer(),
                KeepOffer::Kept
            );
        }
    }
}
