//! Which pad is whose: the claims made by pressing Start on the setup
//! screen, the order pads were plugged in, and the rule that turns a seat
//! into a pad index. Everything that reads a pad for a seat asks
//! [`PadSeats::pad_for`], so the rule is written down once.

use crate::app::Screen;
use crate::app::settings::{GameSettings, SeatInput};
use crate::sim::MAX_PLAYERS;
use bevy::prelude::*;

/// Pad claims, in claim order, and every pad seen, in the order it was
/// first plugged in. Pad index `i` (the claimed pads, then the rest in
/// plug order) drives the `i`-th human seat **counting from the top**:
/// with cursors for P1 and P2, pad 0 is P2 and pad 1 is P1; with four
/// humans, pad 0 is P4. This makes keyboard+pad, two pads, and two
/// keyboards all work for two players with zero setup.
#[derive(Resource, Default)]
pub struct PadSeats(pub Vec<Entity>, PlugOrder, Chairs);

/// What each claim took to seat its player, by the claiming pad, so the
/// claim let go gives the same thing back. Without it a pad that dropped
/// out and came back on the setup screen lost its claim but not the seat
/// the claim had added, and its next Start added another: a table with a
/// seat nobody played, and one more each time the battery flickered.
#[derive(Default)]
struct Chairs(Vec<(Entity, Chair)>);

/// Where a claimed player sat down.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Chair {
    /// A seat the AI had, which it gets back.
    Ai,
    /// A seat added to the table, which goes again.
    Added,
    /// Nothing taken, so nothing given back: a human seat already at a
    /// full table, or the pad a seated player named for themselves.
    Taken,
}

impl Chair {
    /// Give the chair back to the table it came from.
    fn give_back(
        self,
        config: &mut crate::app::match_setup::MatchConfig,
        beaches: &crate::app::match_setup::CustomBeaches,
    ) {
        match self {
            Chair::Ai => {
                config.bots += 1;
                config.clamp_bots();
            }
            Chair::Added => crate::app::match_setup::reseat(config, config.seats - 1, beaches),
            Chair::Taken => {}
        }
    }
}

/// Every pad seen, in the order it was first plugged in, unplugged ones
/// included until the round is over. A pad that drops out keeps its place
/// for the rest of the round, so the pads after it do
/// not each move up a seat: counted live, P1's pad took over P2's cursor
/// the moment P2's battery died, and kept it after P2 plugged back in.
/// The same pad back (the engine gives it its old entity) finds its place
/// still there; a different one takes the first place left empty.
#[derive(Default)]
pub struct PlugOrder(Vec<Entity>);

impl PadSeats {
    /// The pad `player` plays on, among the seats with a cursor here
    /// (`players`, sorted, as [`seated`] gives them), if it has one.
    pub(super) fn pad_for<'a>(
        &self,
        pads: &'a Query<(Entity, &Gamepad)>,
        settings: &GameSettings,
        players: &[u8],
        player: u8,
    ) -> Option<&'a Gamepad> {
        pad_index_of(settings, players, player).and_then(|index| nth_pad(pads, self, index))
    }

    /// The same pad as [`PadSeats::pad_for`], as the entity a rumble is
    /// sent to.
    pub(super) fn pad_entity_for(
        &self,
        pads: &Query<(Entity, &Gamepad)>,
        settings: &GameSettings,
        players: &[u8],
        player: u8,
    ) -> Option<Entity> {
        pad_index_of(settings, players, player).and_then(|index| nth_pad_entity(pads, self, index))
    }
}

/// The seats with a cursor at this machine, sorted: the set
/// [`PadSeats::pad_for`] ranks a seat among.
pub(super) fn seated(players: impl Iterator<Item = u8>) -> Vec<u8> {
    let mut players: Vec<u8> = players.collect();
    players.sort_unstable();
    players
}

/// Keep the plug order, and let go of pads that are gone.
///
/// Only between rounds. Mid-round an unplugged pad's place (and claim)
/// holds its seat, empty, rather than handing the seat to the next pad
/// in line; afterwards both are let go, or a pad unplugged after a couch
/// match left every other pad dead for the rest of the session.
///
/// A claim let go gives back the seat it took (see [`Chairs`]), so the
/// table is the people still holding a pad. The same pad back presses
/// Start and sits down again.
///
/// Given back on the setup screen only, where the claim was made and the
/// table is on show: the table is also an online host's terms, and a pad
/// whose battery died in the lobby rewrote them under the joiners (the
/// AI count dialled there, the map picked there). Gone elsewhere, the
/// seat is owed until the setup screen is next opened.
pub fn keep_pad_order(
    pads: Query<Entity, With<Gamepad>>,
    screen: Res<State<Screen>>,
    mut seats: ResMut<PadSeats>,
    config: Option<ResMut<crate::app::match_setup::MatchConfig>>,
    beaches: Option<Res<crate::app::match_setup::CustomBeaches>>,
) {
    let PadSeats(claims, PlugOrder(order), Chairs(chairs)) = &mut *seats;
    for pad in &pads {
        if order.contains(&pad) {
            continue;
        }
        // A claimed pad gone and a new one plugged in: the new one takes
        // over the claim, and the seat the claim held. Placed after every
        // claim instead, it left that seat empty for the rest of a series
        // and steered a keyboard player's seat besides.
        if let Some(at) = claims.iter().position(|&claimed| !pads.contains(claimed)) {
            let gone = std::mem::replace(&mut claims[at], pad);
            // The seat changes hands with the claim.
            if let Some(chair) = chairs.iter_mut().find(|(claimed, _)| *claimed == gone) {
                chair.0 = pad;
            }
            if let Some(place) = order.iter().position(|&seen| seen == gone) {
                order[place] = pad;
                continue;
            }
        }
        match order.iter().position(|&seen| !pads.contains(seen)) {
            Some(empty) => order[empty] = pad,
            None => order.push(pad),
        }
    }
    let in_a_round = matches!(
        screen.get(),
        Screen::Puzzle | Screen::Versus | Screen::Interlude
    );
    if !in_a_round {
        claims.retain(|&claimed| pads.contains(claimed));
        order.retain(|&seen| pads.contains(seen));
        let mut config = config;
        let shelf = crate::app::match_setup::CustomBeaches::default();
        let beaches = beaches.as_deref().unwrap_or(&shelf);
        let settle = *screen.get() == Screen::MatchSetup;
        chairs.retain(|&(claimed, chair)| {
            if claims.contains(&claimed) || !settle {
                return true;
            }
            if let Some(config) = config.as_deref_mut() {
                chair.give_back(config, beaches);
            }
            false
        });
    }
}

/// The pad entity at claim/connection index `index`.
///
/// Split from [`nth_pad`] because reading a pad wants the component and
/// rumbling one wants the entity, and the rule for *which* pad an index
/// means is written down once.
fn nth_pad_entity(
    pads: &Query<(Entity, &Gamepad)>,
    claims: &PadSeats,
    index: usize,
) -> Option<Entity> {
    let PadSeats(claimed, PlugOrder(order), _) = claims;
    // A pad not yet in the plug order (plugged in this frame) still
    // counts, after every pad that is.
    let fresh = pads
        .iter()
        .map(|(entity, _)| entity)
        .filter(|pad| !order.contains(pad));
    let entity = claimed
        .iter()
        .copied()
        .chain(
            order
                .iter()
                .copied()
                .chain(fresh)
                .filter(|pad| !claimed.contains(pad)),
        )
        .nth(index)?;
    pads.contains(entity).then_some(entity)
}

/// The pad at claim/connection index `index`.
fn nth_pad<'a>(
    pads: &'a Query<(Entity, &Gamepad)>,
    claims: &PadSeats,
    index: usize,
) -> Option<&'a Gamepad> {
    let entity = nth_pad_entity(pads, claims, index)?;
    pads.get(entity).ok().map(|(_, pad)| pad)
}

/// Pad index for `player` among the live cursor set.
///
/// A seat that named a controller in settings gets that one, and nobody
/// else does. The rest rank from the top down (the highest seat takes the
/// lowest pad nobody claimed), which is the rule that makes keyboard+pad,
/// two pads and two keyboards all work with no setup at all. Online has a
/// single local cursor, which therefore takes the first free pad whatever
/// its seat.
fn pad_index_of(settings: &GameSettings, players: &[u8], player: u8) -> Option<usize> {
    // A lone cursor is the one person at this machine, whatever seat the
    // network dealt it, and plays by P1's settings row: the keyboard does
    // (`keymap(&settings, 0, ..)` online), so the pad has to as well. Read
    // by the dealt seat, a joiner in seat two whose P2 row said "keys" had
    // a dead pad all match.
    if let [only] = players
        && *only == player
    {
        return match seat_choice(settings, 0) {
            Some(SeatInput::Keys) => None,
            Some(SeatInput::Pad(n)) => Some(usize::from(n)),
            Some(SeatInput::Auto) | None => Some(0),
        };
    }
    match seat_choice(settings, player) {
        Some(SeatInput::Keys) => return None,
        Some(SeatInput::Pad(n)) => return Some(usize::from(n)),
        Some(SeatInput::Auto) | None => {}
    }
    // Auto seats, highest first, over the pads no seat asked for by name.
    let mut autos: Vec<u8> = players
        .iter()
        .copied()
        .filter(|&p| {
            !matches!(
                seat_choice(settings, p),
                Some(SeatInput::Pad(_) | SeatInput::Keys)
            )
        })
        .collect();
    autos.sort_unstable();
    let rank = autos.iter().rev().position(|&p| p == player)?;
    (0..).filter(|index| !named(settings, *index)).nth(rank)
}

/// The seat pad claim `claim` drives at a configured table of `humans`
/// people (seats `0..humans`, where a configured match puts its cursors),
/// by [`pad_index_of`]: claims come first in the pad order, so claim `i`
/// is pad index `i`. `None` when no seat there plays on it.
///
/// What the setup screen's footer names a claim by, and what
/// [`pad_claim_seats`] asks before it gives a claim a chair, so the line
/// that says who joined and the seat the pad then drives are one answer.
pub fn seat_of_claim(settings: &GameSettings, humans: u8, claim: usize) -> Option<u8> {
    let players: Vec<u8> = (0..humans).collect();
    players
        .iter()
        .copied()
        .find(|&player| pad_index_of(settings, &players, player) == Some(claim))
}

/// Whether pad claim `claim` is the pad a seat already at the table named
/// in settings ("P1 plays with: controller 1"): that person was already
/// counted, and the Start was them picking their own pad up, not a friend
/// sitting down. Read through [`seat_of_claim`], so it is the seat the pad
/// will in fact drive.
fn claim_is_a_seated_players_own(settings: &GameSettings, humans: u8, claim: usize) -> bool {
    seat_of_claim(settings, humans, claim).is_some_and(|seat| {
        matches!(seat_choice(settings, seat), Some(SeatInput::Pad(n)) if usize::from(n) == claim)
    })
}

/// What a seat asked for, or `None` for the seats past the bound two,
/// which have no keyboard of their own and so nothing to choose between.
fn seat_choice(settings: &GameSettings, player: u8) -> Option<SeatInput> {
    settings.seat_input.get(usize::from(player)).copied()
}

/// Whether some seat named this pad index outright.
fn named(settings: &GameSettings, index: usize) -> bool {
    settings
        .seat_input
        .iter()
        .any(|choice| matches!(choice, SeatInput::Pad(n) if usize::from(*n) == index))
}

/// Seats the keyboard always owns; the join ceremony grows the table
/// above them.
const KEYBOARD_SEATS: usize = 2;
/// How many pads the ceremony can seat: everything the keyboard leaves.
const PAD_SEATS: usize = MAX_PLAYERS - KEYBOARD_SEATS;

/// The press-Start-to-join ceremony on the match-setup screen: an
/// unclaimed pad pressing Start puts one more person at the table, in a
/// seat the AI had if it had any, else in a new one. Claims drive the
/// human seats from the top down (see [`PadSeats`]), the first claim the
/// highest. Up to four: the keyboard's two seats and the pads' four make
/// the six there are. Disconnected pads lose their claim, and the seat with
/// it, in [`keep_pad_order`].
pub fn pad_claim_seats(
    pads: Query<(Entity, &Gamepad)>,
    mut seats: ResMut<PadSeats>,
    mut config: ResMut<crate::app::match_setup::MatchConfig>,
    beaches: Res<crate::app::match_setup::CustomBeaches>,
    settings: Res<GameSettings>,
    bots: Res<crate::app::bot_seats::BotSeats>,
) {
    // A doorway card is up, waiting for the bots the table was dialled
    // with: the table is settled until it comes down. A claim taken then
    // gave a couch bot's chair to the pad, and the card went on waiting
    // for a bot whose seat was now a person's.
    if bots.door_for.card_up() {
        return;
    }
    let PadSeats(claims, _, Chairs(chairs)) = &mut *seats;
    for (entity, pad) in &pads {
        if pad.just_pressed(GamepadButton::Start)
            && !claims.contains(&entity)
            && claims.len() < PAD_SEATS
        {
            let claim = claims.len();
            claims.push(entity);
            // A seated player picking up the pad they named takes no chair:
            // the AI's went to a human seat no pad then drove.
            let humans = config.humans();
            if claim_is_a_seated_players_own(&settings, humans, claim) {
                chairs.push((entity, Chair::Taken));
                continue;
            }
            // One person more, and only one: the AI gives up a chair if it
            // holds one, or the table grows. Counting a seat for every
            // claim above the keyboard's two instead took the AI away from
            // a player alone against it and left P2 a human nobody played.
            // A table that grows is reseated, so the map still seats it.
            let chair = if config.bots > 0 {
                config.bots -= 1;
                Chair::Ai
            } else if usize::from(config.seats) < MAX_PLAYERS {
                let seats = config.seats + 1;
                crate::app::match_setup::reseat(&mut config, seats, &beaches);
                Chair::Added
            } else {
                Chair::Taken
            };
            chairs.push((entity, chair));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::settings::{GameSettings, SeatInput};

    fn with(choices: [SeatInput; 2]) -> GameSettings {
        GameSettings {
            seat_input: choices,
            ..GameSettings::default()
        }
    }

    /// Left alone, pads fill seats from the top down; a lone cursor (puzzle,
    /// online) takes the first pad whatever its seat number.
    #[test]
    fn pads_map_top_down() {
        let auto = with([SeatInput::Auto; 2]);
        assert_eq!(pad_index_of(&auto, &[0, 1], 1), Some(0));
        assert_eq!(pad_index_of(&auto, &[0, 1], 0), Some(1));
        assert_eq!(pad_index_of(&auto, &[0, 1, 2, 3], 3), Some(0));
        assert_eq!(pad_index_of(&auto, &[0, 1, 2, 3], 0), Some(3));
        assert_eq!(pad_index_of(&auto, &[2], 2), Some(0), "online single seat");
        assert_eq!(pad_index_of(&auto, &[0, 1], 2), None, "no cursor, no pad");
    }

    /// Online the one cursor plays by P1's settings row whatever seat the
    /// network dealt it, as its keyboard does. Read by the dealt seat, a
    /// joiner in seat two whose P2 row said "keys" had a dead pad.
    #[test]
    fn a_lone_cursor_plays_by_the_first_row() {
        let pad_for_p1 = with([SeatInput::Auto, SeatInput::Keys]);
        assert_eq!(pad_index_of(&pad_for_p1, &[1], 1), Some(0));
        let keys_for_p1 = with([SeatInput::Keys, SeatInput::Auto]);
        assert_eq!(pad_index_of(&keys_for_p1, &[1], 1), None);
        let named = with([SeatInput::Pad(2), SeatInput::Keys]);
        assert_eq!(pad_index_of(&named, &[3], 3), Some(2));
    }

    /// A seat that names a controller gets that one, and the seat that
    /// would have inherited it under the top-down rule does not.
    #[test]
    fn a_named_controller_belongs_to_the_seat_that_named_it() {
        let mine = with([SeatInput::Pad(0), SeatInput::Auto]);
        assert_eq!(pad_index_of(&mine, &[0, 1], 0), Some(0), "P1 asked for it");
        assert_eq!(
            pad_index_of(&mine, &[0, 1], 1),
            Some(1),
            "P2 takes the next one free, not the claimed one"
        );
    }

    /// Keyboard-only means no pad, and the pad it would have had goes to
    /// whoever is next in line rather than going spare.
    #[test]
    fn keyboard_only_gives_its_pad_up() {
        let quiet = with([SeatInput::Auto, SeatInput::Keys]);
        assert_eq!(pad_index_of(&quiet, &[0, 1], 1), None);
        assert_eq!(pad_index_of(&quiet, &[0, 1], 0), Some(0));
    }

    /// Both seats naming the same controller is a thing a player can do by
    /// walking the dial past it, and it must not panic or hand it to a
    /// third seat; they simply share it.
    #[test]
    fn two_seats_may_name_the_same_controller() {
        let both = with([SeatInput::Pad(1), SeatInput::Pad(1)]);
        assert_eq!(pad_index_of(&both, &[0, 1], 0), Some(1));
        assert_eq!(pad_index_of(&both, &[0, 1], 1), Some(1));
        // And a third seat skips the claimed one.
        assert_eq!(pad_index_of(&both, &[0, 1, 2], 2), Some(0));
    }

    /// A pad's Start puts one person more at the table, in the AI's chair
    /// while it has one, and four pads can claim.
    #[test]
    fn a_claim_takes_a_seat_from_the_ai() {
        use crate::app::match_setup::MatchConfig;
        let mut app = App::new();
        app.init_resource::<PadSeats>();
        app.init_resource::<GameSettings>();
        app.init_resource::<crate::app::bot_seats::BotSeats>();
        app.insert_resource(MatchConfig {
            seats: 4,
            bots: 2,
            ..MatchConfig::default()
        });
        app.init_resource::<crate::app::match_setup::CustomBeaches>();
        for _ in 0..6 {
            app.world_mut().spawn(Gamepad::default());
        }
        app.add_systems(Update, pad_claim_seats);
        let pads: Vec<Entity> = app
            .world_mut()
            .query_filtered::<Entity, With<Gamepad>>()
            .iter(app.world())
            .collect();
        let start = |app: &mut App, pad: Entity| {
            let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("pad");
            gamepad.digital_mut().press(GamepadButton::Start);
            app.update();
            let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("pad");
            gamepad.digital_mut().release(GamepadButton::Start);
            gamepad.digital_mut().clear();
        };
        start(&mut app, pads[0]);
        let config = app.world().resource::<MatchConfig>();
        assert_eq!(
            (config.seats, config.bots),
            (4, 1),
            "seat three is a person"
        );
        for &pad in &pads[1..] {
            start(&mut app, pad);
        }
        assert_eq!(app.world().resource::<PadSeats>().0.len(), PAD_SEATS);
        let config = app.world().resource::<MatchConfig>();
        assert_eq!((config.seats, config.bots), (6, 0));

        // One player against the AI: a friend's Start takes the AI's chair,
        // rather than a third seat beside an empty second.
        app.insert_resource(MatchConfig {
            seats: 2,
            bots: 1,
            ..MatchConfig::default()
        });
        app.world_mut().resource_mut::<PadSeats>().0.clear();
        start(&mut app, pads[0]);
        let config = app.world().resource::<MatchConfig>();
        assert_eq!((config.seats, config.bots), (2, 0), "two people, no AI");
    }

    /// The setup screen with `pads` controllers plugged in and claims
    /// kept, for the claim tests below.
    fn setup_table(
        config: crate::app::match_setup::MatchConfig,
        pads: usize,
    ) -> (App, Vec<Entity>) {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.insert_resource(State::new(Screen::MatchSetup));
        app.init_resource::<PadSeats>();
        app.init_resource::<GameSettings>();
        app.init_resource::<crate::app::bot_seats::BotSeats>();
        app.init_resource::<crate::app::match_setup::CustomBeaches>();
        app.insert_resource(config);
        let plugged = (0..pads)
            .map(|_| app.world_mut().spawn(Gamepad::default()).id())
            .collect();
        app.add_systems(Update, (keep_pad_order, pad_claim_seats).chain());
        (app, plugged)
    }

    fn press_start(app: &mut App, pad: Entity) {
        let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("pad");
        gamepad.digital_mut().press(GamepadButton::Start);
        app.update();
        let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("pad");
        gamepad.digital_mut().release(GamepadButton::Start);
        gamepad.digital_mut().clear();
    }

    /// P1 set to play on controller 1, alone against the AI, picks that
    /// controller up and presses Start: the pad is P1's, so the footer's
    /// seat for it is P1 and the AI keeps its chair. Read top-down, the
    /// footer said "P2 joined" and the AI's chair went to a P2 no pad drove.
    #[test]
    fn a_seated_players_named_pad_takes_no_chair() {
        use crate::app::match_setup::MatchConfig;
        let (mut app, pads) = setup_table(
            MatchConfig {
                seats: 2,
                bots: 1,
                ..MatchConfig::default()
            },
            1,
        );
        app.insert_resource(with([SeatInput::Pad(0), SeatInput::Auto]));
        press_start(&mut app, pads[0]);
        let config = app.world().resource::<MatchConfig>();
        assert_eq!((config.seats, config.bots), (2, 1), "the AI still plays");
        let settings = app.world().resource::<GameSettings>();
        assert_eq!(seat_of_claim(settings, 1, 0), Some(0), "the pad is P1's");
        // Left on auto, the same Start is a friend in the AI's chair.
        let auto = with([SeatInput::Auto; 2]);
        assert_eq!(seat_of_claim(&auto, 2, 0), Some(1), "the pad is P2's");
    }

    /// A pad's Start while the bot doorway card is up takes nothing: the
    /// card is waiting for a bot in the AI's chair, and a claim that took
    /// that chair left it waiting for a seat a person now had.
    #[test]
    fn no_claim_while_the_bot_card_is_up() {
        use crate::app::bot_seats::{BotSeats, DoorFor};
        use crate::app::match_setup::MatchConfig;
        let (mut app, pads) = setup_table(
            MatchConfig {
                seats: 2,
                bots: 1,
                ..MatchConfig::default()
            },
            1,
        );
        app.world_mut().resource_mut::<BotSeats>().door_for = DoorFor::Couch;
        press_start(&mut app, pads[0]);
        let config = app.world().resource::<MatchConfig>();
        assert_eq!((config.seats, config.bots), (2, 1), "the bot's chair kept");
        assert!(app.world().resource::<PadSeats>().0.is_empty(), "no claim");
    }

    /// Pads joining a table on the four-castle arena move it to a beach
    /// that seats them all: grown in place, the fifth player had no castle.
    #[test]
    fn a_table_grown_by_pads_is_moved_to_a_beach_that_seats_it() {
        use crate::app::match_setup::{MapChoice, MatchConfig, holds};
        let (mut app, pads) = setup_table(
            MatchConfig {
                seats: 2,
                bots: 0,
                map: MapChoice::Classic,
                ..MatchConfig::default()
            },
            3,
        );
        for &pad in &pads {
            press_start(&mut app, pad);
        }
        let config = app.world().resource::<MatchConfig>();
        assert_eq!(config.seats, 5);
        assert!(holds(config.map, 5), "five on {:?}", config.map);
    }

    /// A pad that drops out on the setup screen gives its seat back, and
    /// coming back and pressing Start takes it again: one seat, however
    /// often the battery flickers.
    #[test]
    fn a_pad_that_drops_and_rejoins_keeps_one_seat() {
        use crate::app::match_setup::MatchConfig;
        let (mut app, pads) = setup_table(
            MatchConfig {
                seats: 2,
                bots: 0,
                ..MatchConfig::default()
            },
            2,
        );
        press_start(&mut app, pads[0]);
        assert_eq!(app.world().resource::<MatchConfig>().seats, 3);
        for _ in 0..3 {
            app.world_mut().entity_mut(pads[0]).remove::<Gamepad>();
            app.update();
            let config = app.world().resource::<MatchConfig>();
            assert_eq!(config.seats, 2, "the added seat goes with the claim");
            assert!(app.world().resource::<PadSeats>().0.is_empty());
            app.world_mut()
                .entity_mut(pads[0])
                .insert(Gamepad::default());
            app.update();
            press_start(&mut app, pads[0]);
            assert_eq!(app.world().resource::<MatchConfig>().seats, 3);
        }

        // A claim that took the AI's chair hands it back to the AI.
        let (mut app, pads) = setup_table(
            MatchConfig {
                seats: 3,
                bots: 1,
                ..MatchConfig::default()
            },
            1,
        );
        press_start(&mut app, pads[0]);
        let config = app.world().resource::<MatchConfig>();
        assert_eq!((config.seats, config.bots), (3, 0));
        app.world_mut().entity_mut(pads[0]).remove::<Gamepad>();
        app.update();
        let config = app.world().resource::<MatchConfig>();
        assert_eq!((config.seats, config.bots), (3, 1), "the AI sits back down");
    }

    /// The table is also an online host's terms: a pad that drops out in
    /// the lobby leaves them alone, and its seat is given back the next
    /// time the setup screen opens.
    #[test]
    fn a_pad_gone_in_the_lobby_changes_nothing_until_the_setup_screen() {
        use crate::app::match_setup::MatchConfig;
        let (mut app, pads) = setup_table(
            MatchConfig {
                seats: 4,
                bots: 1,
                ..MatchConfig::default()
            },
            1,
        );
        press_start(&mut app, pads[0]);
        // Hosting now, with the AI dialled up in the lobby.
        app.insert_resource(State::new(Screen::Lobby));
        app.world_mut().resource_mut::<MatchConfig>().bots = 1;
        app.world_mut().entity_mut(pads[0]).remove::<Gamepad>();
        app.update();
        let config = app.world().resource::<MatchConfig>();
        assert_eq!((config.seats, config.bots), (4, 1), "the terms stand");
        app.insert_resource(State::new(Screen::MatchSetup));
        app.update();
        let config = app.world().resource::<MatchConfig>();
        assert_eq!(config.bots, 2, "owed, and given back to the AI there");
        assert!(app.world().resource::<PadSeats>().2.0.is_empty());
    }
}
