//! The editor's keys: what each one does, how the help and the prompts
//! spell them, and the dials' plus and minus.

use super::*;

/// The arrow keys and the edge each names: a wall toggled on that side of
/// the cursor's tile, or, in a playtest, the way an arrow points.
pub(super) const ARROWS: [(KeyCode, Direction); 4] = [
    (KeyCode::ArrowUp, Direction::Up),
    (KeyCode::ArrowDown, Direction::Down),
    (KeyCode::ArrowLeft, Direction::Left),
    (KeyCode::ArrowRight, Direction::Right),
];

/// The editor's keys that follow P1's bindings, read by the editor and
/// named by its prompts from this one answer.
pub(crate) struct EditorKeys {
    /// P1's movement keys, which walk the editor's cursor whatever they
    /// are bound to (see `cursor::move_cursor`).
    pub moves: [KeyCode; 4],
    /// The wall toggles: the arrow keys, or P1's place keys while the
    /// arrows are P1's movement keys, which would leave no key to build a
    /// wall with.
    pub walls: [(KeyCode, Direction); 4],
    /// A playtest's placements and its removal: P1's, as a puzzle reads
    /// them.
    pub place: [(KeyCode, Direction); 4],
    pub remove: KeyCode,
}

impl EditorKeys {
    pub(crate) fn of(settings: &GameSettings) -> Self {
        // The editor seats nobody else at the keyboard: P1's scheme as set.
        let map = crate::app::cursor::keymap(settings, 0, settings.commit_in_play(false));
        let moves = map.move_keys();
        let walls = match ARROWS.iter().any(|(key, _)| moves.contains(key)) {
            true => map.places,
            false => ARROWS,
        };
        Self {
            moves,
            walls,
            place: map.places,
            remove: map.remove,
        }
    }

    /// P1's movement keys as the prompts name them: "arrow keys" when they
    /// are the arrows (the mirrored placing scheme puts them there), else
    /// the four caps in the order "WASD" spells a block.
    pub(crate) fn spell_moves(&self, tr: &Tr, caps: &crate::app::keycaps::KeyCaps) -> String {
        let [up, down, left, right] = self.moves;
        match self.moves == ARROWS.map(|(key, _)| key) {
            true => tr.ed_arrow_keys.to_string(),
            false => spell_block(caps, [up, left, down, right]),
        }
    }

    /// Whether `key` is one of P1's movement keys or a wall key, which win
    /// it from the editor's own keys.
    ///
    /// The cursor walks on P1's bindings, and the brushes and commands are
    /// keys of their own, so a binding moved onto one did two things at once:
    /// rebound to ESDF, E walked the cursor up and painted kelp on every step.
    /// Movement wins, being the binding the player chose and the one the
    /// prompt names, and so do the walls when they have moved onto P1's place
    /// keys. A brush they shadow is still reached with Tab and Space, and a
    /// command moves to its function key ([`EditorKeys::command`]).
    fn shadows(&self, key: KeyCode) -> bool {
        self.moves.contains(&key) || self.walls.iter().any(|(wall, _)| *wall == key)
    }

    /// A press the editor's own keys may answer: just pressed, and not one
    /// P1's movement or wall keys have taken ([`EditorKeys::shadows`]).
    pub(super) fn command_press<'a>(
        &'a self,
        keys: &'a ButtonInput<KeyCode>,
    ) -> impl Fn(KeyCode) -> bool + 'a {
        move |key| keys.just_pressed(key) && !self.shadows(key)
    }

    /// The key that loads `brush`, unless P1's movement or wall keys have
    /// taken it: a brush has no stand-in, and is reached with Tab and
    /// Space then. Read by the brush's press and by the palette and help
    /// card that name it, so a taken letter is never offered.
    pub(crate) fn brush(&self, brush: Brush) -> Option<KeyCode> {
        Some(brush.key()).filter(|&key| !self.shadows(key))
    }

    /// The key `command` is on: its letter, or its function key while P1's
    /// movement or wall keys have taken the letter. Read by the command
    /// itself, by the prompt that names it and by the pad's button for it,
    /// so the three cannot part.
    pub(crate) fn command(&self, command: Command) -> KeyCode {
        match self.shadows(command.letter()) {
            true => command.stand_in(),
            false => command.letter(),
        }
    }
}

/// The editor's commands on letter keys, besides the brushes: each has a
/// letter it is known by and a function key it moves to when P1's keys
/// take the letter. Bound to the IJKL scheme, the walls moved onto IJKL
/// and K, the gulls' key, toggled a wall instead while the prompt went on
/// saying "K gulls".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    /// The solver for a stage, the castle count for a beach.
    Check,
    /// The flock dial.
    Gulls,
    /// The open-ocean toggle.
    Wrap,
    /// The creature or hole under the cursor turned to face the next way.
    Turn,
    /// The last edit taken back, and with Shift made again.
    Undo,
}

impl Command {
    pub(crate) const ALL: [Command; 5] = [
        Command::Check,
        Command::Gulls,
        Command::Wrap,
        Command::Turn,
        Command::Undo,
    ];

    /// The letter the command is known by.
    fn letter(self) -> KeyCode {
        match self {
            Command::Check => KeyCode::KeyV,
            Command::Gulls => KeyCode::KeyK,
            Command::Wrap => KeyCode::KeyO,
            Command::Turn => KeyCode::KeyT,
            Command::Undo => KeyCode::KeyZ,
        }
    }

    /// Where the command goes when its letter is taken. No binding can
    /// take a function key (`binds::bindable`), so this one is always the
    /// command's own.
    fn stand_in(self) -> KeyCode {
        match self {
            Command::Check => KeyCode::F9,
            Command::Gulls => KeyCode::F7,
            Command::Wrap => KeyCode::F8,
            Command::Turn => KeyCode::F10,
            Command::Undo => KeyCode::F12,
        }
    }

    /// The prompt's name for the slot the key goes in.
    fn slot(self) -> &'static str {
        match self {
            Command::Check => "check",
            Command::Gulls => "gulls",
            Command::Wrap => "wrap",
            Command::Turn => "turn",
            Command::Undo => "undo",
        }
    }
}

/// The key that, with Shift, puts up the card of every key and button
/// ([`EditorCard::Help`]): F1 alone names the level. Function keys are
/// the editor's own, which no binding can take, and every one is spoken
/// for, so help shares F1 the way the sizes share F5.
pub(crate) const HELP_KEY: KeyCode = KeyCode::F1;

/// The key that moves the settings card's choice on to its next row, and
/// back with Shift. N is one of the game's own letters that no binding may
/// take (`binds::GLOBAL_KEYS`), so unlike the commands it never has to
/// move off its letter.
pub(crate) const NEXT_SETTING: KeyCode = KeyCode::KeyN;

/// The key `command` is on under these settings ([`EditorKeys::command`]):
/// what the pad's button for it presses.
pub(crate) fn command_key(settings: &GameSettings, command: Command) -> KeyCode {
    EditorKeys::of(settings).command(command)
}

/// The key that toggles the wall on `dir`'s side of the cursor's tile,
/// as [`EditorKeys`] reads it: what the pad's right stick presses.
pub(crate) fn wall_key(settings: &GameSettings, dir: Direction) -> Option<KeyCode> {
    EditorKeys::of(settings)
        .walls
        .iter()
        .find(|(_, side)| *side == dir)
        .map(|(key, _)| *key)
}

/// Four keys as a prompt names them, in the order "WASD" spells a block
/// (up, left, down, right): run together when each cap is one character,
/// "ESDF", else "Up/Left/Down/Right".
pub(crate) fn spell_block(caps: &crate::app::keycaps::KeyCaps, keys: [KeyCode; 4]) -> String {
    let labels = keys.map(|key| caps.label(key));
    match labels.iter().all(|label| label.chars().count() == 1) {
        true => labels.concat(),
        false => labels.join("/"),
    }
}

/// A group of four keys in the order "WASD" spells a block (up, left,
/// down, right), as [`spell_block`] takes them.
pub(super) fn block_order(group: [(KeyCode, Direction); 4]) -> [KeyCode; 4] {
    [
        Direction::Up,
        Direction::Left,
        Direction::Down,
        Direction::Right,
    ]
    .map(|dir| {
        group
            .iter()
            .find(|(_, d)| *d == dir)
            .map(|(key, _)| *key)
            .expect("a key for every way")
    })
}

/// `text` with the editor's `{moves}`, `{walls}`, `{place}`, `{remove}`,
/// `{help}` and command keys (`{check}`, `{gulls}`, `{wrap}`) filled in: the caps
/// of the keys [`EditorKeys`] reads,
/// or the language's own words while the walls and the playtest's keys
/// are the stock arrow keys and Space.
///
/// Run after [`crate::app::keycaps::KeyCaps::legend`]: the keys are
/// already in this keyboard's caps, and caps that happen to read "WASD"
/// would be respelled again as the QWERTY keys the legend means by it.
pub(crate) fn spell_editor_keys(
    text: &str,
    tr: &Tr,
    caps: &crate::app::keycaps::KeyCaps,
    settings: &GameSettings,
) -> String {
    let keys = EditorKeys::of(settings);
    let block = |group: [(KeyCode, Direction); 4]| -> String {
        match group == ARROWS {
            true => tr.ed_arrow_keys.to_string(),
            false => spell_block(caps, block_order(group)),
        }
    };
    let moves = keys.spell_moves(tr, caps);
    let remove = if keys.remove == KeyCode::Space {
        tr.ed_space_key.to_string()
    } else {
        caps.label(keys.remove)
    };
    let commands = Command::ALL.map(|command| (command.slot(), caps.label(keys.command(command))));
    let mut slots = vec![
        ("moves", moves),
        ("walls", block(keys.walls)),
        ("place", block(keys.place)),
        ("remove", remove),
        ("help", shifted(caps, HELP_KEY)),
    ];
    slots.extend(commands);
    let slots: Vec<(&str, &str)> = slots.iter().map(|(k, v)| (*k, v.as_str())).collect();
    fill(text, &slots)
}

/// A key held with Shift, as a prompt names it: "Shift+F1".
pub(crate) fn shifted(caps: &crate::app::keycaps::KeyCaps, key: KeyCode) -> String {
    format!("Shift+{}", caps.label(key))
}

/// Whether "+" (`up`) or "-" was pressed for the posts dial. The prompt
/// says "+/-" in every language, so the keys are the ones that say so:
/// the US pair beside Backspace, the keypad's, and whichever key this
/// keyboard's caps put the sign on.
pub(super) fn dial_press(
    pressed: &impl Fn(KeyCode) -> bool,
    keys: &ButtonInput<KeyCode>,
    caps: &crate::app::keycaps::KeyCaps,
    up: bool,
) -> bool {
    let (us, pad, sign) = if up {
        (KeyCode::Equal, KeyCode::NumpadAdd, '+')
    } else {
        (KeyCode::Minus, KeyCode::NumpadSubtract, '-')
    };
    pressed(us) || pressed(pad) || caps.pressed_caps(keys, sign).any(pressed)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{bench_app, sand, tap};
    use super::*;
    use crate::sim::TileKind;

    /// The prompt says "+/-" in every language, so the dial answers the
    /// keys that say so: the keypad's pair, and on a QWERTZ board the
    /// caps beside P and where QWERTY has "/", besides the US pair.
    #[test]
    fn the_posts_dial_answers_the_keys_that_say_plus_and_minus() {
        let mut app = bench_app();
        app.add_systems(Update, editor_commands);
        app.world_mut().resource_mut::<EditorState>().posts = 3;
        let posts_after = |app: &mut App, key: KeyCode| {
            tap(app, key);
            app.world().resource::<EditorState>().posts
        };
        assert_eq!(posts_after(&mut app, KeyCode::NumpadAdd), 4);
        assert_eq!(posts_after(&mut app, KeyCode::NumpadSubtract), 3);
        assert_eq!(
            posts_after(&mut app, KeyCode::Equal),
            4,
            "the US pair still works"
        );
        assert_eq!(posts_after(&mut app, KeyCode::Minus), 3);
        assert_eq!(
            posts_after(&mut app, KeyCode::BracketRight),
            3,
            "QWERTY's ]"
        );
        app.world_mut()
            .resource_mut::<crate::app::keycaps::KeyCaps>()
            .force(Some(crate::app::keycaps::Layout::Qwertz));
        assert_eq!(
            posts_after(&mut app, KeyCode::BracketRight),
            4,
            "QWERTZ's +"
        );
        assert_eq!(posts_after(&mut app, KeyCode::Slash), 3, "QWERTZ's -");
        assert_eq!(posts_after(&mut app, KeyCode::Digit6), 3, "a 6 on QWERTZ");
        // AZERTY's "-" is on the 6, forced or learned from a press.
        app.world_mut()
            .resource_mut::<crate::app::keycaps::KeyCaps>()
            .force(Some(crate::app::keycaps::Layout::Azerty));
        assert_eq!(posts_after(&mut app, KeyCode::Digit6), 2, "AZERTY's -");
        let mut learned = crate::app::keycaps::KeyCaps::default();
        learned.learn(KeyCode::KeyW, 'z');
        app.insert_resource(learned);
        assert_eq!(
            posts_after(&mut app, KeyCode::Digit6),
            1,
            "AZERTY's -, learned"
        );
        assert_eq!(
            posts_after(&mut app, KeyCode::Digit5),
            1,
            "the rest of the row"
        );
        app.world_mut().resource_mut::<EditorState>().posts = 3;
        app.world_mut()
            .resource_mut::<crate::app::keycaps::KeyCaps>()
            .force(Some(crate::app::keycaps::Layout::Qwertz));
        // The cap at nine still holds on every key that says "+".
        app.world_mut().resource_mut::<EditorState>().posts = 9;
        assert_eq!(posts_after(&mut app, KeyCode::BracketRight), 9);
        assert_eq!(posts_after(&mut app, KeyCode::NumpadAdd), 9);
    }

    /// The cursor walks on P1's bindings and the brushes are keys of their
    /// own. Rebound to ESDF, E walked the cursor up and painted kelp with
    /// the same press; movement keeps the key, and the brush is still on
    /// Tab and Space.
    #[test]
    fn a_rebound_movement_key_does_not_also_paint() {
        use crate::app::binds::Action;
        let mut settings = GameSettings::default();
        for (action, key) in [
            (Action::MoveUp, KeyCode::KeyE),
            (Action::MoveLeft, KeyCode::KeyS),
            (Action::MoveDown, KeyCode::KeyD),
            (Action::MoveRight, KeyCode::KeyF),
        ] {
            settings.binds[0].set(action, key);
        }
        let mut app = bench_app();
        app.insert_resource(settings);
        app.world_mut()
            .spawn((Cursor::seated(0), Transform::default()));
        app.add_systems(Update, editor_input);
        tap(&mut app, KeyCode::KeyE);
        assert_eq!(
            app.world().resource::<Sim>().0.tile_at(0, 0),
            TileKind::Empty
        );
        assert_ne!(app.world().resource::<EditorState>().brush, Brush::Weed);
        // A brush letter the bindings left alone still paints.
        tap(&mut app, KeyCode::KeyR);
        assert_eq!(
            app.world().resource::<Sim>().0.tile_at(0, 0),
            TileKind::Rock
        );
    }

    /// P1 moving on the arrow keys left the editor no wall key and the
    /// playtest no place key, both being the arrows. The walls move onto
    /// P1's place keys and a playtest reads P1's place and remove keys, as a
    /// puzzle does; the prompts name the keys that work.
    #[test]
    fn arrows_bound_to_movement_leave_walls_and_playtest_on_the_place_keys() {
        use crate::app::binds::Action;
        let mut settings = GameSettings::default();
        let swapped = [
            (Action::MoveUp, KeyCode::ArrowUp),
            (Action::MoveDown, KeyCode::ArrowDown),
            (Action::MoveLeft, KeyCode::ArrowLeft),
            (Action::MoveRight, KeyCode::ArrowRight),
            (Action::PlaceUp, KeyCode::KeyW),
            (Action::PlaceDown, KeyCode::KeyS),
            (Action::PlaceLeft, KeyCode::KeyA),
            (Action::PlaceRight, KeyCode::KeyD),
            (Action::Remove, KeyCode::KeyX),
        ];
        for (action, key) in swapped {
            settings.binds[0].set(action, key);
        }
        let caps = crate::app::keycaps::KeyCaps::default();
        let stock = GameSettings::default();
        use crate::app::i18n::EN;
        let walls = spell_editor_keys(EN.ed_prompt, &EN, &caps, &stock);
        assert!(walls.contains("arrow keys walls"), "{walls}");
        let walls = spell_editor_keys(EN.ed_prompt, &EN, &caps, &settings);
        assert!(walls.contains("| WASD walls |"), "{walls}");
        let test = spell_editor_keys(EN.ed_playtest_prompt, &EN, &caps, &stock);
        assert_eq!(
            test,
            "arrow keys place | Space remove | R restart | Esc end test"
        );
        let test = spell_editor_keys(EN.ed_playtest_prompt, &EN, &caps, &settings);
        assert_eq!(test, "WASD place | X remove | R restart | Esc end test");
        // The move keys too, in the order "WASD" spells a block: the stock
        // keys read as the language's own legend did, rebound keys by
        // their caps.
        let painting = EN.ed_prompt.replacen("WASD", "{moves}", 1);
        let moves = spell_editor_keys(&painting, &EN, &caps, &stock);
        assert!(
            moves.starts_with("WASD move | arrow keys walls |"),
            "{moves}"
        );
        let moves = spell_editor_keys(&painting, &EN, &caps, &settings);
        assert!(moves.starts_with("arrow keys move |"), "{moves}");

        let mut app = bench_app();
        let mut board = sand();
        board.set_tile(0, 0, TileKind::Castle(0));
        app.insert_resource(Sim(board));
        app.insert_resource(EditorState {
            posts: 3,
            name: "Stage".into(),
            ..EditorState::default()
        });
        app.insert_resource(settings);
        let mut cursor = Cursor::seated(0);
        (cursor.x, cursor.y) = (2, 2);
        app.world_mut().spawn((cursor, Transform::default()));
        let editing = |state: Res<EditorState>| !state.is_testing();
        app.add_systems(
            Update,
            (
                (editor_input, editor_commands).chain().run_if(editing),
                editor_test_input.run_if(editor_testing),
            )
                .chain(),
        );
        let wall = |app: &App| app.world().resource::<Sim>().0.wall_at(2, 2, Direction::Up);
        tap(&mut app, KeyCode::ArrowUp);
        assert!(!wall(&app), "the arrow is P1's move key");
        tap(&mut app, KeyCode::KeyW);
        assert!(wall(&app), "P1's place key builds the wall");
        tap(&mut app, KeyCode::KeyW);
        assert!(!wall(&app), "and takes it down");

        tap(&mut app, KeyCode::Enter);
        assert!(app.world().resource::<EditorState>().is_testing());
        let posts = |app: &App| app.world().resource::<Sim>().0.signpost_count(0);
        tap(&mut app, KeyCode::ArrowLeft);
        assert_eq!(posts(&app), 0, "the arrow is P1's move key");
        tap(&mut app, KeyCode::KeyA);
        assert_eq!(posts(&app), 1, "P1's place key");
        tap(&mut app, KeyCode::Space);
        assert_eq!(posts(&app), 1, "Space is not P1's remove key here");
        tap(&mut app, KeyCode::KeyX);
        assert_eq!(posts(&app), 0, "P1's remove key");
    }

    /// Every command keeps a key of its own under any bindings: with P1
    /// moving on the arrows and placing on IJKL, the walls moved onto IJKL
    /// and K toggled a wall where the prompt said "K gulls". The command
    /// moves to its function key, and the line that names it names that
    /// key: the prompt for the check and the turn, the settings card's row
    /// for the flock and the wrap.
    #[test]
    fn every_command_has_a_key_in_every_binding_scheme() {
        use crate::app::binds::Action;
        use crate::app::i18n::EN;
        let caps = crate::app::keycaps::KeyCaps::default();
        let mut ijkl = GameSettings::default();
        for (action, key) in [
            (Action::MoveUp, KeyCode::ArrowUp),
            (Action::MoveDown, KeyCode::ArrowDown),
            (Action::MoveLeft, KeyCode::ArrowLeft),
            (Action::MoveRight, KeyCode::ArrowRight),
        ] {
            ijkl.binds[0].set(action, key);
        }
        ijkl.commit = crate::app::settings::CommitScheme::Ijkl;
        let mut letters = GameSettings::default();
        for (action, key) in [
            (Action::MoveUp, KeyCode::KeyV),
            (Action::MoveDown, KeyCode::KeyK),
            (Action::MoveLeft, KeyCode::KeyO),
            (Action::MoveRight, KeyCode::KeyT),
        ] {
            letters.binds[0].set(action, key);
        }
        let mut undo_taken = GameSettings::default();
        undo_taken.binds[0].set(Action::MoveUp, KeyCode::KeyZ);
        let wasd = GameSettings {
            commit: crate::app::settings::CommitScheme::Wasd,
            ..GameSettings::default()
        };
        for settings in [
            GameSettings::default(),
            ijkl.clone(),
            wasd,
            letters,
            undo_taken,
        ] {
            let keys = EditorKeys::of(&settings);
            let prompt = spell_editor_keys(EN.ed_prompt, &EN, &caps, &settings);
            for command in Command::ALL {
                let key = keys.command(command);
                assert!(!keys.shadows(key), "{command:?} on {key:?}");
                let row =
                    |setting| card::Row::Setting(setting).key(LastDevice::Keyboard, &keys, &caps);
                match command {
                    Command::Check | Command::Turn | Command::Undo => {
                        let named = format!("{} ", caps.label(key));
                        assert!(prompt.contains(&named), "{command:?}: {prompt}");
                    }
                    Command::Gulls => assert_eq!(row(Setting::Gulls), caps.label(key)),
                    Command::Wrap => assert_eq!(row(Setting::Wrap), caps.label(key)),
                }
            }
            assert!(!prompt.contains('{'), "every slot filled: {prompt}");
        }
        let stock = spell_editor_keys(EN.ed_prompt, &EN, &caps, &GameSettings::default());
        assert!(
            stock.contains("T turn") && stock.contains("V check"),
            "{stock}"
        );
        // N, which chooses the card's row, is a key no binding can take.
        assert!(!crate::app::binds::bindable(NEXT_SETTING));

        // Through the keys themselves: K is the down wall, F7 the gulls.
        let mut app = bench_app();
        app.insert_resource(ijkl);
        let mut cursor = Cursor::seated(0);
        (cursor.x, cursor.y) = (2, 2);
        app.world_mut().spawn((cursor, Transform::default()));
        app.add_systems(Update, (editor_input, editor_commands).chain());
        let period = |app: &App| app.world().resource::<Sim>().0.rules().gull_period;
        tap(&mut app, KeyCode::KeyK);
        assert_eq!(period(&app), 0, "K is a wall key here");
        assert!(
            app.world()
                .resource::<Sim>()
                .0
                .wall_at(2, 2, Direction::Down)
        );
        tap(&mut app, KeyCode::F7);
        assert_eq!(period(&app), setting::GULL_PERIODS[1], "F7 steps the gulls");
        tap(&mut app, KeyCode::F8);
        assert!(
            !app.world().resource::<Sim>().0.wrap(),
            "O is free, so F8 is not"
        );
        tap(&mut app, KeyCode::KeyO);
        assert!(app.world().resource::<Sim>().0.wrap(), "O wraps");
    }
}
