//! Step 3's seats: who plays each, the second player's keys, partners,
//! and the name typed into a seat.

use super::*;

/// The team mode step 3's table plays: the one set on step 1 where the
/// table fits it, else free-for-all ([`TeamMode::played_by`]). Partners
/// are picked on step 3 under it, and the match is scored by it.
///
/// [`TeamMode::played_by`]: crate::app::teams::TeamMode::played_by
pub fn setup_teams(settings: &GameSettings, config: &MatchConfig) -> crate::app::teams::TeamMode {
    settings.team_mode.played_by(config.seats)
}

/// What a seat can be set to on step 3, in the order A/D steps through
/// them: a person, the AI at each level, a bot.
pub const SEAT_KINDS: [SeatController; 5] = [
    SeatController::Local,
    SeatController::Ai(BotLevel::Easy),
    SeatController::Ai(BotLevel::Normal),
    SeatController::Ai(BotLevel::Hard),
    SeatController::Bot,
];

/// Whether A/D turns who plays `seat`: every seat but player 1's, which
/// stays a person, whoever is working the menu being player 1. What
/// [`turn_seat`] does, and what step 3's hint and prompt say.
pub fn seat_turns(seat: u8) -> bool {
    seat != 0
}

/// Step who plays `seat` one notch, where [`seat_turns`] says it turns.
pub fn turn_seat(config: &mut MatchConfig, seat: u8, turn: Turn) {
    if !seat_turns(seat) || seat >= config.seats {
        return;
    }
    let now = config.controller(seat);
    let at = SEAT_KINDS.iter().position(|c| *c == now).unwrap_or(0);
    let next = crate::app::cycle::dial(at as u8, turn, 1, 0..=SEAT_KINDS.len() as u8 - 1);
    config.controllers[usize::from(seat)] = SEAT_KINDS[usize::from(next)];
}

/// Who plays a seat, as step 3 says it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlayedBy {
    /// A keyboard seat's keys: P1's, or P2's once one was pressed.
    Keys(u8),
    /// Controller `number`: the pad the seat rule gives the seat
    /// ([`PadSeats`]). The round reads the same rule, so this pad is the
    /// one that plays it. `joined` when its player pressed Start for the
    /// seat ([`SeatPad::joined`]), not when the rule only dealt it.
    Pad {
        number: u8,
        joined: bool,
    },
    /// A person's seat nobody has sat down in. `keys`: P2's keys would
    /// take it too. The match fills it with the AI ([`open_seats`]).
    Open {
        keys: bool,
    },
    Ai(BotLevel),
    Bot,
}

/// Whether P2's keyboard keys can play the second seat: not when P2's
/// settings row names a controller.
fn p2_keys(settings: &GameSettings) -> bool {
    !matches!(settings.seat_input[1], SeatInput::Pad(_))
}

/// Who plays `seat`, with `pads` the pads seated by the one seat rule
/// ([`PadSeats::seat_pads`]) and `p2_here` whether P2 pressed one of
/// their own keys ([`PadSeats::p2_at_keys`]).
pub fn played_by(
    config: &MatchConfig,
    seat: u8,
    pads: &[SeatPad],
    p2_here: bool,
    settings: &GameSettings,
) -> PlayedBy {
    match config.controller(seat) {
        SeatController::Ai(level) => return PlayedBy::Ai(level),
        SeatController::Bot => return PlayedBy::Bot,
        SeatController::Local | SeatController::Remote => {}
    }
    // A seat the rule gives a pad is that pad's, claimed or dealt: the
    // round drives it with that pad, so the AI never sits there. Player 1
    // is at the menu, on the keys unless the rule gives P1 a pad.
    if let Some(pad) = pads.iter().find(|pad| pad.seat == seat) {
        return PlayedBy::Pad {
            number: pad.number,
            joined: pad.joined,
        };
    }
    if seat == 0 {
        return PlayedBy::Keys(0);
    }
    let keys = seat == 1 && p2_keys(settings);
    match keys && p2_here {
        true => PlayedBy::Keys(1),
        false => PlayedBy::Open { keys },
    }
}

/// The person's seats nobody has sat down in, which the match is started
/// with the AI in at step 1's level.
pub fn open_seats(
    config: &MatchConfig,
    pads: &[SeatPad],
    p2_here: bool,
    settings: &GameSettings,
) -> Vec<u8> {
    (0..config.seats)
        .filter(|&seat| {
            matches!(
                played_by(config, seat, pads, p2_here, settings),
                PlayedBy::Open { .. }
            )
        })
        .collect()
}

/// What step 3's "played by" cell says, an open seat's Start named as a
/// pad of `family` has it.
pub fn played_by_text(tr: &crate::app::i18n::Tr, by: PlayedBy, family: PadFamily) -> String {
    let start = || Pad::Start.name(tr, family);
    match by {
        PlayedBy::Keys(seat) => fill(
            tr.setup_by_keys,
            &[("p", &crate::app::seat_label(tr, seat))],
        ),
        PlayedBy::Pad { number, .. } => fill(tr.setup_by_pad, &[("n", &number.to_string())]),
        PlayedBy::Open { keys: false } => fill(tr.setup_by_open, &[("start", start())]),
        PlayedBy::Open { keys: true } => fill(
            &crate::app::name_the_pair(tr, tr.setup_by_open_keys),
            &[("start", start())],
        ),
        PlayedBy::Ai(level) => fill(tr.setup_by_ai, &[("l", tr.bot_levels[level.index()])]),
        PlayedBy::Bot => tr.setup_by_bot.to_string(),
    }
}

/// The status at the end of a seat's row: ready at the keys or on a pad
/// the rule dealt the seat, joined on a pad whose player pressed Start for
/// it, the AI to come in an open seat, and nothing for the AI's own.
pub fn status_text(tr: &crate::app::i18n::Tr, by: PlayedBy) -> &'static str {
    match by {
        PlayedBy::Keys(_) | PlayedBy::Pad { joined: false, .. } => tr.setup_status[0],
        PlayedBy::Pad { joined: true, .. } => tr.setup_status[1],
        PlayedBy::Open { .. } => tr.setup_status[2],
        PlayedBy::Ai(_) | PlayedBy::Bot => "",
    }
}

/// The line under step 3's seats: which seats are open and what fills
/// them, else how a friend joins when a pad here could (its Start named
/// as a pad of `family` has it), else nothing. The second half says
/// whether it is the warning.
pub fn seats_line(
    tr: &crate::app::i18n::Tr,
    open: &[u8],
    level: BotLevel,
    (pad_to_join, family): (bool, PadFamily),
) -> (String, bool) {
    if !open.is_empty() {
        let list: Vec<String> = open
            .iter()
            .map(|&seat| crate::app::seat_label(tr, seat))
            .collect();
        return (
            fill(
                tr.setup_open_line,
                &[
                    ("list", &list.join(", ")),
                    ("l", tr.bot_levels[level.index()]),
                ],
            ),
            true,
        );
    }
    match pad_to_join {
        true => (
            fill(
                tr.setup_join_hint,
                &[("start", Pad::Start.name(tr, family))],
            ),
            false,
        ),
        false => (String::new(), false),
    }
}

/// The hint under step 3's seats: what A/D does to the seat under the
/// cursor, by the keys or the pad's d-pad, as the step's own table names
/// it. Nothing on P1's row, which A/D leaves alone ([`turn_seat`]).
pub fn seat_hint(tr: &crate::app::i18n::Tr, device: LastDevice, seat: u8) -> String {
    if !seat_turns(seat) {
        return String::new();
    }
    let keys = match device {
        LastDevice::Keyboard => "A/D".to_string(),
        LastDevice::Pad(family) => crate::app::gamepad::scheme::Pad::Left
            .name(tr, family)
            .to_string(),
    };
    fill(tr.setup_seat_hint, &[("k", &keys)])
}

/// A seat with no name yet, saying the key that opens it for typing:
/// "Tab" in every language, which a pad's player is told as step 3's
/// button for it.
pub fn unnamed(tr: &crate::app::i18n::Tr, device: LastDevice) -> String {
    use crate::app::gamepad::scheme::{Does, button_name};
    let face = crate::app::gamepad::Face::MatchSetup {
        step: Step::Players,
        join: true,
        partners: false,
    };
    match (
        device,
        button_name(&face, Does::Rename, tr, device.family()),
    ) {
        (LastDevice::Pad(_), Some(button)) => tr.match_name_empty.replacen("Tab", &button, 1),
        _ => tr.match_name_empty.to_string(),
    }
}

/// Whether this frame's keys include one of P2's own: a key bound to the
/// second keyboard seat that neither player 1 nor the menu also uses, so
/// the person working the menu cannot join as P2 by accident.
///
/// Read under the commit rule the round itself plays with
/// ([`GameSettings::commit_in_play`], keyboard shared): a versus round puts
/// P1's placement on the arrows whatever the preset, so IJKL is P2's here
/// too, and the one-hand preset must not hide the keys P2 will play with.
///
/// Not a key a pad's button holds down ([`Bridged`]): a pad's button is
/// no one at the keyboard, though it presses a key. The setup's own keys
/// (the d-pad's W/A/S/D among them) are already left out of
/// [`p2_only_keys`]; this keeps any other a pad's table presses out too.
pub fn p2_pressed(keys: &ButtonInput<KeyCode>, bridged: &Bridged, settings: &GameSettings) -> bool {
    p2_only_keys(settings)
        .into_iter()
        .any(|key| keys.just_pressed(key) && !bridged.holds(key))
}

/// P2's own keys, as [`p2_pressed`] reads them: bound to the second
/// keyboard seat, and used by neither player 1 nor the menu. Every other
/// key is the hands of whoever works the setup
/// ([`crate::app::gamepad::follow_the_setup_hands`]). The menu's keys
/// include the ones it walks with ([`menu_ui::NAV_KEYS`]): with P1 moved
/// off W and P2 bound to it, walking up the steps sat P2 down.
pub fn p2_only_keys(settings: &GameSettings) -> Vec<KeyCode> {
    use crate::app::cursor::keymap;
    let menu = [
        KeyCode::Enter,
        KeyCode::NumpadEnter,
        KeyCode::Escape,
        KeyCode::Tab,
        KeyCode::Backspace,
    ]
    .into_iter()
    .chain(menu_ui::NAV_KEYS)
    .collect::<Vec<_>>();
    let commit = settings.commit_in_play(true);
    let p1: Vec<KeyCode> = keymap(settings, 0, commit).keys().collect();
    keymap(settings, 1, commit)
        .keys()
        .filter(|key| !p1.contains(key) && !menu.contains(key))
        .collect()
}

/// The seats a bot holds in the match as set up.
pub fn bot_seat_list(config: &MatchConfig) -> Vec<u8> {
    (0..config.seats)
        .filter(|&seat| config.controller(seat) == SeatController::Bot)
        .collect()
}

/// Step 3: W/S choose a seat, in the order the rows are shown
/// ([`seat_rows`]), A/D change who plays it, P picks its partners under a
/// team mode (`partner`), Tab names it, Enter starts the match, and so
/// does `pad_start`, a seated pad's Start
/// ([`crate::app::gamepad::pad_claim_seats`]); Esc back to the map.
pub(super) fn players_keys(
    (keys, pad_start, partner): (&ButtonInput<KeyCode>, bool, bool),
    menu: &mut MatchMenu,
    config: &mut MatchConfig,
    settings: &GameSettings,
    beaches: &CustomBeaches,
    start: &mut MatchStart,
    pads: &mut SetupPads,
) {
    if keys.just_pressed(KeyCode::Escape) {
        open_map_step(menu, config, beaches);
        return;
    }
    menu.seat = menu.seat.min(config.seats - 1);
    // Tab opens a name for typing, not Enter, which starts the match.
    if keys.just_pressed(KeyCode::Tab) {
        menu.naming = Some(menu.seat);
        return;
    }
    if menu_ui::enter(keys) || pad_start {
        start_match(config, menu, start, pads, settings, beaches);
        return;
    }
    if partner {
        pick_partners(menu, config, settings, pads);
        return;
    }
    let rows: Vec<u8> = seat_rows(setup_teams(settings, config), config.seats)
        .into_iter()
        .filter(|&seat| seat < config.seats)
        .collect();
    let at = rows.iter().position(|&seat| seat == menu.seat).unwrap_or(0);
    menu.seat = rows[menu_ui::nav(keys, at, rows.len())];
    if let Some(turn) = menu_ui::left_right(keys) {
        turn_seat(config, menu.seat, turn);
        pads.seats.table_set_by_hand();
    }
}

/// Step 3's rows in the order they are shown, every seat a table can
/// have: under a team mode team by team, as the line-up lists them
/// ([`crate::app::lineup::seat_order`]), else in seat order.
pub fn seat_rows(mode: crate::app::teams::TeamMode, seats: u8) -> Vec<u8> {
    match mode {
        crate::app::teams::TeamMode::Solo => (0..MAX_PLAYERS as u8).collect(),
        crate::app::teams::TeamMode::Pairs | crate::app::teams::TeamMode::Trios => {
            crate::app::lineup::seat_order(mode, seats)
                .into_iter()
                .map(|(seat, _)| seat)
                .collect()
        }
    }
}

/// Why P2's seat stays where it is when partners are picked: the seat is
/// wired to the keyboard's second keys, or to the controller settings name
/// for P2, so a person moved into it would be driven by those, not by
/// their own pad. Whoever plays it (P2, the AI, nobody yet) stays.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum P2Pin {
    /// P2 plays it at the keyboard.
    Keys,
    /// P2 plays it on controller `n`, counted from 0 as settings count
    /// them.
    Pad(u8),
    /// Nobody plays it at the keys yet (the AI, or nobody), and it is
    /// kept for them.
    KeptForKeys,
    /// Controller `n` does not play it (the AI does, or the pad is not
    /// plugged in), and it is kept for that pad.
    KeptForPad(u8),
}

/// P2's pin at `config`'s table ([`P2Pin`]), read off step 3's own row
/// for the seat ([`played_by`], with `pads` and `p2_here` as it takes
/// them), so the line and the rows agree. A row naming the keys or a
/// controller pins the seat whoever plays it. A row left to whichever pad
/// pins it once P2's keys were pressed, and while the seat is a person's
/// nobody sits in: the keyboard's second player takes it with one key,
/// and would push a friend picked into it out.
pub fn p2_pin(
    config: &MatchConfig,
    settings: &GameSettings,
    pads: &[SeatPad],
    p2_here: bool,
) -> Option<P2Pin> {
    if config.seats < 2 {
        return None;
    }
    let by = played_by(config, 1, pads, p2_here, settings);
    let seated = matches!(by, PlayedBy::Keys(_) | PlayedBy::Pad { .. });
    let keys = match seated {
        true => P2Pin::Keys,
        false => P2Pin::KeptForKeys,
    };
    match settings.seat_input[1] {
        SeatInput::Keys => Some(keys),
        SeatInput::Pad(n) if seated => Some(P2Pin::Pad(n)),
        SeatInput::Pad(n) => Some(P2Pin::KeptForPad(n)),
        SeatInput::Auto if p2_here => Some(keys),
        SeatInput::Auto => (by == PlayedBy::Open { keys: true }).then_some(P2Pin::KeptForKeys),
    }
}

/// The seats a couch pick never moves, besides P1's: P2's when pinned.
fn pinned_seats(pin: Option<P2Pin>) -> Vec<u8> {
    pin.map(|_| 1).into_iter().collect()
}

/// The partner pick for the seat under step 3's cursor: the next team it
/// can be given ([`crate::app::partners::next_pick`]), made by swapping
/// seats ([`swap_seats`]). The cursor follows the picker when the pick
/// moves them to a seat beside a partner who cannot move.
fn pick_partners(
    menu: &mut MatchMenu,
    config: &mut MatchConfig,
    settings: &GameSettings,
    pads: &mut SetupPads,
) {
    let mode = setup_teams(settings, config);
    let pin = p2_pin(
        config,
        settings,
        &pads.seated(config, settings),
        pads.seats.p2_at_keys(),
    );
    let pinned = pinned_seats(pin);
    let Some(pick) = crate::app::partners::next_pick(
        mode,
        config.seats,
        menu.seat,
        (&[0], &pinned),
        &config.people.0,
    ) else {
        return;
    };
    for &pair in &pick.swaps {
        swap_seats(config, settings, pads, pair);
    }
    crate::app::partners::reseated(&mut config.people.0, &pick);
    menu.seat = pick.moved(menu.seat);
}

/// The people in seats `a` and `b` trade places: who plays each (a
/// person, the AI, a bot), the name it goes by ([`MatchConfig::people`]),
/// and the pad that drives it ([`PadSeats::swap_seats`]).
pub fn swap_seats(
    config: &mut MatchConfig,
    settings: &GameSettings,
    pads: &mut SetupPads,
    (a, b): (u8, u8),
) {
    let live: Vec<Entity> = pads.live.iter().collect();
    pads.seats
        .swap_seats(&live, settings, &config.human_seats(), (a, b));
    let (a, b) = (usize::from(a), usize::from(b));
    config.controllers.swap(a, b);
    config.filled_at_start.swap(a, b);
}

/// The line under step 3's seats about teams, for the seat under the
/// cursor, if there is one to say: under a team mode the partner key and
/// whose partners it picks, or why P2 cannot be moved when that holds a
/// pick back; under a mode the table does not fit, the scoring row's own
/// words for it ([`crate::app::teams::row_value`]).
pub fn partner_line(
    tr: &crate::app::i18n::Tr,
    device: LastDevice,
    (config, settings): (&MatchConfig, &GameSettings),
    seat: u8,
    pin: Option<P2Pin>,
) -> Option<String> {
    use crate::app::teams::TeamMode;
    let set = settings.team_mode;
    let mode = setup_teams(settings, config);
    let names = config.seat_names(&settings.names);
    let name = |seat: u8| crate::app::name_or_label(&names, tr, seat);
    if mode == TeamMode::Solo {
        return (set != TeamMode::Solo)
            .then(|| crate::app::teams::row_value(tr, set, config.seats, true));
    }
    let (_, held) = crate::app::partners::picks(mode, config.seats, seat, &[0], &pinned_seats(pin));
    if held && let Some(pin) = pin {
        let n = |n: u8| (n + 1).to_string();
        return Some(match pin {
            P2Pin::Keys => fill(tr.setup_pinned_keys, &[("p", &name(1))]),
            P2Pin::Pad(pad) => fill(tr.setup_pinned_pad, &[("p", &name(1)), ("n", &n(pad))]),
            P2Pin::KeptForKeys => fill(tr.setup_kept_keys, &[("p", &name(1))]),
            P2Pin::KeptForPad(pad) => fill(tr.setup_kept_pad, &[("p", &name(1)), ("n", &n(pad))]),
        });
    }
    let face = crate::app::gamepad::Face::MatchSetup {
        step: Step::Players,
        join: false,
        partners: true,
    };
    let key = crate::app::gamepad::scheme::spelled(
        tr,
        device,
        &face,
        crate::app::gamepad::scheme::Does::Partners,
        &crate::app::gamepad::scheme::PARTNER_CAP.to_string(),
    );
    let hint = match mode {
        TeamMode::Pairs => tr.setup_partner_hint,
        TeamMode::Solo | TeamMode::Trios => tr.setup_teammates_hint,
    };
    Some(fill(hint, &[("k", &key), ("p", &name(seat))]))
}

/// The keyboard belongs to one name, the one saved on settings row `row`: characters land in it, Backspace
/// rubs one out, Enter or Esc hands the keyboard back. Bevy's `text` field
/// is used rather than the key codes so the player's own layout, and their
/// shift key, decide what a keystroke means.
pub(super) fn type_a_name(
    row: u8,
    typed: &mut MessageReader<bevy::input::keyboard::KeyboardInput>,
    keys: &ButtonInput<KeyCode>,
    settings: &mut GameSettings,
    menu: &mut MatchMenu,
) {
    use crate::app::typing::{Keystroke, keystrokes};
    let ends = [
        KeyCode::Enter,
        KeyCode::NumpadEnter,
        KeyCode::Escape,
        KeyCode::Tab,
    ];
    for stroke in keystrokes(typed, &ends) {
        match stroke {
            Keystroke::Erase => settings.pop_name_char(row),
            Keystroke::Char(ch) => settings.push_name_char(row, ch),
            // The finish is decided below from just_pressed, one branch
            // for every way out.
            Keystroke::Done(_) => {}
        }
    }
    // Every way out of the name box puts it away and nothing else: Enter
    // does not also start the match here, or a player finishing a name
    // would find the round already running.
    let done = menu_ui::enter(keys)
        || keys.just_pressed(KeyCode::Escape)
        || keys.just_pressed(KeyCode::Tab);
    if done {
        settings.tidy_name(row);
        menu.naming = None;
    }
}
