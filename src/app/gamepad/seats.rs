//! Which pad is whose: the claims made by pressing Start during match
//! setup, the order pads were plugged in, and the rule that turns a seat
//! into a pad. Everything that reads a pad for a seat asks
//! [`Lineup::pads`], through [`PadSeats::pad_for`] in a round and
//! [`PadSeats::joined`] and [`PadSeats::seat_pads`] during setup, so the
//! rule is written down once and step 3 names the pad each seat then
//! plays on.

use crate::app::match_setup::{CustomBeaches, MatchConfig};
use crate::app::settings::{GameSettings, SeatInput};
use crate::app::{Screen, SeatController};
use crate::sim::MAX_PLAYERS;
use bevy::prelude::*;

/// Who holds which pad at this machine.
///
/// The seat rule, in [`Lineup::pads`]: a seat that named a controller in
/// settings plays on it, counted in plug order; the pad that worked the
/// setup plays P1 ([`PadSeats::pilot`]); a pad that pressed Start during
/// setup (a claim) keeps the seat it was given, the one it was dealt
/// above P1's, else the lowest person's seat above P1's that no claim
/// held; every other pad fills the seats left over, P2 upwards, and P1
/// last while no pad works the setup, claims or none: P1's pad is taken
/// only by its own Start. So a friend who joins is never renumbered by
/// the next one, and a seat a pad is dealt is that pad's, at setup and in
/// the round alike: keyboard and pad, or two pads, play two players
/// without anybody pressing Start.
///
/// A claim is a friend sitting down beside whoever works the menu, so P1
/// is never a claim's: a claim pushed out of the seats a claim can take
/// drives nothing, and during setup is let go so its Start seats it again
/// ([`PadSeats::let_go_of_the_seatless`]).
#[derive(Resource, Default)]
pub struct PadSeats {
    /// Claims, each with the seat it holds. Kept by seat rather than by
    /// the order they came in, so a seat turned over to the AI or back on
    /// step 3 moves nobody else: counted in order, every claim above it
    /// stepped down a seat.
    claims: Vec<(Entity, u8)>,
    /// Pads a seat named in settings, picked up with Start to bring that
    /// seat to the table (P2 named controller 2 while the AI held P2).
    /// They drive the seat that named them, so they hold no seat here.
    own: Vec<Entity>,
    /// Pads whose claim a new pad took over while they were gone
    /// ([`keep_pad_order`]), or that a change over their heads pushed out
    /// of their seat ([`PadSeats::let_go_of_the_seatless`]). They drive
    /// nothing until they press Start: dealt as a spare, a friend's pad
    /// steered P1 beside the keyboard.
    benched: Vec<Entity>,
    /// The pad working the setup, when a pad is: the person at the menu,
    /// who is P1 whoever else sits down. Taken from the hands last used as
    /// the setup opens ([`PadSeats::setup_opened`]) and moved by the
    /// presses there ([`follow_the_setup_hands`]): the keyboard's own keys
    /// put P1 back on the keyboard. A Steam Deck alone at the setup was
    /// dealt P2 by the pads-from-P2-up rule and steered P2 against an idle
    /// P1, with nobody playing the AI.
    pilot: Option<Entity>,
    /// Whether P2 pressed one of their own keys at the setup: the
    /// keyboard's second player is here, and P2's seat is theirs, so a pad
    /// nobody claimed is dealt the seat after it ([`Rows`]).
    p2_at_keys: bool,
    /// A pad already at the table pressed Start on step 3, which starts
    /// the match there ([`pad_claim_seats`]); taken by the setup's keys
    /// the same frame ([`PadSeats::take_start`]).
    start_asked: bool,
    order: PlugOrder,
    chairs: Chairs,
}

/// The seats' "plays with" rows as the seat rule reads them: settings'
/// own, with P2's "whichever pad" read as the keys once P2 has pressed
/// one of theirs at the setup ([`PadSeats::p2_at_keys`]). Read raw, a pad
/// plugged in was dealt P2 under the keyboard's second player, who then
/// played P2 with them, and the seat after went to the AI.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Rows([SeatInput; crate::app::binds::BOUND_SEATS]);

impl Rows {
    /// What a seat asked for, or `None` for the seats past the bound two,
    /// which have no keyboard of their own and so nothing to choose
    /// between.
    fn choice(self, player: u8) -> Option<SeatInput> {
        self.0.get(usize::from(player)).copied()
    }

    /// Whether a seat takes whichever pad the rule deals it.
    fn is_auto(self, player: u8) -> bool {
        matches!(self.choice(player), Some(SeatInput::Auto) | None)
    }
}

/// What each claim took to seat its player, by the claiming pad, so the
/// claim let go gives the same thing back. Without it a pad that dropped
/// out and came back during setup lost its claim but not the seat the
/// claim had added, and its next Start added another: a table with a seat
/// nobody played, and one more each time the battery flickered. A claim
/// that sat in a person's seat already there took nothing and has no
/// entry.
#[derive(Default)]
struct Chairs(Vec<(Entity, Chair)>);

/// What a claim took from the table to seat its player.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Chair {
    /// A seat the AI or a bot had, and what had it, which gets it back.
    Ai(u8, SeatController),
    /// A seat added to the table, which goes again.
    Added,
}

impl Chair {
    /// Give the chair back to the table it came from. A seat set to
    /// something else since is left as it was set.
    fn give_back(self, config: &mut MatchConfig, beaches: &CustomBeaches) {
        match self {
            Chair::Ai(seat, was) => {
                if seat < config.seats && config.controller(seat) == SeatController::Local {
                    config.controllers[usize::from(seat)] = was;
                }
                config.clamp_bots();
            }
            Chair::Added => crate::app::match_setup::reseat(config, config.seats - 1, beaches),
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
///
/// It is also what "controller N" in settings counts along, and what
/// step 3 numbers a pad by, never the claims: counted claims first, a
/// friend's Start made their pad P1's "controller 1" and P1's own pad
/// went to somebody else.
#[derive(Default)]
struct PlugOrder(Vec<Entity>);

/// The pads as the seat rule reads them: every pad in plug order (one
/// plugged in this very frame last), the claims with their seats, the
/// named pads picked up with Start, the pads benched until their next
/// Start, and the pad working the setup.
struct Lineup {
    order: Vec<Entity>,
    claims: Vec<(Entity, u8)>,
    own: Vec<Entity>,
    benched: Vec<Entity>,
    pilot: Option<Entity>,
}

impl Lineup {
    /// Controller `n` as settings counts them: the `n`-th pad plugged in.
    fn plugged(&self, n: u8) -> Option<Entity> {
        self.order.get(usize::from(n)).copied()
    }

    /// The first seat whose settings row names this pad, which makes it
    /// that seat's and nobody else's.
    fn naming(&self, rows: Rows, pad: Entity) -> Option<u8> {
        rows.0
            .iter()
            .position(|choice| matches!(choice, SeatInput::Pad(n) if self.plugged(*n) == Some(pad)))
            .map(|seat| seat as u8)
    }

    /// Whether some seat named this pad in settings ([`Lineup::naming`]).
    fn is_named(&self, rows: Rows, pad: Entity) -> bool {
        self.naming(rows, pad).is_some()
    }

    /// Whether the pad is a claim of either kind: a seat's, or a named pad
    /// picked up.
    fn is_claimed(&self, pad: Entity) -> bool {
        self.claims.iter().any(|&(claimed, _)| claimed == pad) || self.own.contains(&pad)
    }

    /// The pad that plays P1 for working the setup, if one does: only
    /// while P1's row leaves the pad to the rule, and never a pad another
    /// seat named, claimed or benched, which is a friend's.
    fn pilot(&self, rows: Rows, players: &[u8]) -> Option<Entity> {
        self.pilot.filter(|&pad| {
            players.contains(&0)
                && rows.is_auto(0)
                && self.order.contains(&pad)
                && !self.is_named(rows, pad)
                && !self.is_claimed(pad)
                && !self.benched.contains(&pad)
        })
    }

    /// The pad each seat in `players` (the seats with a cursor here)
    /// plays on, if any: the one seat rule (see [`PadSeats`]).
    ///
    /// A lone cursor is the one person at this machine, whatever seat the
    /// network dealt it, and plays by P1's settings row: the keyboard does
    /// (`keymap(&settings, 0, ..)` online), so the pad has to as well. Read
    /// by the dealt seat, a joiner in seat two whose P2 row said "keys" had
    /// a dead pad all match. It takes the pad that worked the setup, or
    /// the pad in the hands on the way in ([`let_claims_go`]), else the
    /// first claim, else the first pad not benched.
    fn pads(&self, rows: Rows, players: &[u8]) -> Vec<(u8, Option<Entity>)> {
        if let [only] = players {
            let pad = match rows.choice(0) {
                Some(SeatInput::Keys) => None,
                Some(SeatInput::Pad(n)) => self.plugged(n),
                Some(SeatInput::Auto) | None => self
                    .pilot(rows, &[0])
                    .into_iter()
                    .chain(self.claims.iter().map(|&(pad, _)| pad))
                    .chain(
                        self.order
                            .iter()
                            .copied()
                            .filter(|pad| !self.benched.contains(pad)),
                    )
                    .next(),
            };
            return vec![(*only, pad)];
        }
        let reach = claim_seats(rows, players);
        // Each claim on a seat in reach holds it. A claim past them (its
        // seat went to the AI, the table was dialled down under it, or a
        // named pad's row named another) waits first in line for a seat
        // in reach left over, and with none drives nothing: a claim is
        // never P1's.
        let mut held: Vec<Option<Entity>> = vec![None; reach.len()];
        let mut waiting = Vec::new();
        for &(pad, seat) in &self.claims {
            if self.is_named(rows, pad) {
                continue;
            }
            match reach.iter().position(|&reached| reached == seat) {
                Some(at) if held[at].is_none() => held[at] = Some(pad),
                Some(_) | None => waiting.push(pad),
            }
        }
        waiting.extend(
            self.own
                .iter()
                .copied()
                .filter(|&pad| !self.is_named(rows, pad)),
        );
        // The pad working the setup plays P1. The pads nobody claimed,
        // named or benched fill what is left over, and P1 last while no
        // pad works the setup: a Start moves nobody but its own pad, so
        // P1's dealt pad stays P1's until that very pad sits down as a
        // friend.
        let pilot = self.pilot(rows, players);
        let spare = self.order.iter().copied().filter(|&pad| {
            !self.is_claimed(pad)
                && !self.benched.contains(&pad)
                && !self.is_named(rows, pad)
                && Some(pad) != pilot
        });
        let mut open = reach
            .iter()
            .zip(&held)
            .filter(|(_, pad)| pad.is_none())
            .map(|(&seat, _)| seat);
        let mut dealt: Vec<(u8, Entity)> = Vec::new();
        for pad in waiting {
            let Some(seat) = open.next() else { break };
            dealt.push((seat, pad));
        }
        dealt.extend(pilot.map(|pad| (0, pad)));
        let p1_free = players.contains(&0) && rows.is_auto(0) && pilot.is_none();
        dealt.extend(open.chain(p1_free.then_some(0)).zip(spare));
        players
            .iter()
            .map(|&player| {
                let pad = match rows.choice(player) {
                    Some(SeatInput::Keys) => None,
                    Some(SeatInput::Pad(n)) => self.plugged(n),
                    Some(SeatInput::Auto) | None => reach
                        .iter()
                        .zip(&held)
                        .find(|(seat, _)| **seat == player)
                        .and_then(|(_, pad)| *pad)
                        .or_else(|| {
                            dealt
                                .iter()
                                .find(|(seat, _)| *seat == player)
                                .map(|(_, pad)| *pad)
                        }),
                };
                (player, pad)
            })
            .collect()
    }

    /// The pad `player` plays on, by [`Lineup::pads`].
    fn pad_of(&self, rows: Rows, players: &[u8], player: u8) -> Option<Entity> {
        self.pads(rows, players)
            .into_iter()
            .find(|(seat, _)| *seat == player)
            .and_then(|(_, pad)| pad)
    }

    /// The seat `pad` drives, by [`Lineup::pads`].
    fn seat_of(&self, rows: Rows, players: &[u8], pad: Entity) -> Option<u8> {
        self.pads(rows, players)
            .into_iter()
            .find(|(_, held)| *held == Some(pad))
            .map(|(seat, _)| seat)
    }

    /// What step 3 calls `pad`: "controller N", N its place in plug order,
    /// one-based, as settings counts them.
    fn number(&self, pad: Entity) -> Option<u8> {
        self.order
            .iter()
            .position(|&plugged| plugged == pad)
            .map(|at| at as u8 + 1)
    }
}

/// The seats a claim can take, lowest first, among `players`: every one
/// but P1, who is the person working the menus, and but a seat whose row
/// named a controller or the keys.
fn claim_seats(rows: Rows, players: &[u8]) -> Vec<u8> {
    let mut seats: Vec<u8> = players
        .iter()
        .copied()
        .filter(|&player| player != 0 && rows.is_auto(player))
        .collect();
    seats.sort_unstable();
    seats
}

impl PadSeats {
    /// The pads as the seat rule reads them, with `live` the pads plugged
    /// in now: one not yet in the plug order (plugged in this frame) still
    /// counts, after every pad that is.
    fn lineup(&self, live: impl IntoIterator<Item = Entity>) -> Lineup {
        let PadSeats {
            claims,
            own,
            benched,
            pilot,
            p2_at_keys: _,
            start_asked: _,
            order: PlugOrder(order),
            chairs: _,
        } = self;
        let mut all = order.clone();
        for pad in live {
            if !all.contains(&pad) {
                all.push(pad);
            }
        }
        Lineup {
            order: all,
            claims: claims.clone(),
            own: own.clone(),
            benched: benched.clone(),
            pilot: *pilot,
        }
    }

    /// The seats' rows as the seat rule reads them ([`Rows`]).
    fn rows(&self, settings: &GameSettings) -> Rows {
        let mut rows = settings.seat_input;
        if self.p2_at_keys && rows[1] == SeatInput::Auto {
            rows[1] = SeatInput::Keys;
        }
        Rows(rows)
    }

    /// Match setup has opened: P2's keys are not pressed yet, and P1 is
    /// the pad given if the setup was opened with one (the hands last
    /// used), else the keyboard.
    pub fn setup_opened(&mut self, pilot: Option<Entity>) {
        self.p2_at_keys = false;
        self.pilot = pilot.filter(|pad| !self.benched.contains(pad));
    }

    /// Whether P2 has pressed one of their own keys at this setup, which
    /// makes P2's seat the keyboard's second player's.
    pub fn p2_at_keys(&self) -> bool {
        self.p2_at_keys
    }

    /// P2 pressed one of their own keys at the setup.
    pub fn p2_sat_down(&mut self) {
        self.p2_at_keys = true;
    }

    /// The pad `player` plays on, among the seats with a cursor here
    /// (`players`, sorted, as [`seated`] gives them), if it has one.
    pub(super) fn pad_for<'a>(
        &self,
        pads: &'a Query<(Entity, &Gamepad)>,
        settings: &GameSettings,
        players: &[u8],
        player: u8,
    ) -> Option<&'a Gamepad> {
        let entity = self.pad_entity_for(pads, settings, players, player)?;
        pads.get(entity).ok().map(|(_, pad)| pad)
    }

    /// The same pad as [`PadSeats::pad_for`], as the entity a rumble is
    /// sent to. A pad unplugged mid-round still holds its seat, empty.
    pub(super) fn pad_entity_for(
        &self,
        pads: &Query<(Entity, &Gamepad)>,
        settings: &GameSettings,
        players: &[u8],
        player: u8,
    ) -> Option<Entity> {
        self.lineup(pads.iter().map(|(entity, _)| entity))
            .pad_of(self.rows(settings), players, player)
            .filter(|&entity| pads.contains(entity))
    }

    /// Every claim: the seats' claims in order, then the named pads picked
    /// up.
    fn claimed(&self) -> impl Iterator<Item = Entity> + '_ {
        self.claims
            .iter()
            .map(|&(pad, _)| pad)
            .chain(self.own.iter().copied())
    }

    /// The seat each claim drives among the person's seats `players` of a
    /// configured table, lowest first, skipping a claim that drives none.
    /// Read through the rule the round reads pads with, so step 3's rows
    /// and the seat each pad then drives are one answer. A seated player's
    /// named pad picked up with Start is a claim too, of that player's own
    /// seat.
    pub fn joined(&self, live: &[Entity], settings: &GameSettings, players: &[u8]) -> Vec<u8> {
        self.seat_pads(live, settings, players)
            .into_iter()
            .filter(|seat| seat.joined)
            .map(|seat| seat.seat)
            .collect()
    }

    /// Every person's seat among `players` that a pad plugged in here
    /// plays, with the pad's number and whether it pressed Start for the
    /// seat (a claim) or was only dealt it. What step 3 says each seat is
    /// played by.
    pub fn seat_pads(
        &self,
        live: &[Entity],
        settings: &GameSettings,
        players: &[u8],
    ) -> Vec<SeatPad> {
        let lineup = self.lineup(live.iter().copied());
        let mut out: Vec<SeatPad> = lineup
            .pads(self.rows(settings), players)
            .into_iter()
            .filter_map(|(seat, pad)| {
                let pad = pad.filter(|pad| live.contains(pad))?;
                Some(SeatPad {
                    seat,
                    number: lineup.number(pad)?,
                    joined: lineup.is_claimed(pad),
                })
            })
            .collect();
        out.sort_by_key(|seat| seat.seat);
        out
    }

    /// The person's seats among `players` of a configured table that
    /// nothing at this machine can play: past the keyboard's seats, with no
    /// pad plugged in to drive them.
    pub fn seats_with_no_controls(
        &self,
        live: &[Entity],
        settings: &GameSettings,
        players: &[u8],
    ) -> Vec<u8> {
        self.lineup(live.iter().copied())
            .pads(self.rows(settings), players)
            .into_iter()
            .filter(|(seat, pad)| {
                usize::from(*seat) >= crate::app::binds::BOUND_SEATS
                    && !pad.is_some_and(|pad| live.contains(&pad))
            })
            .map(|(seat, _)| seat)
            .collect()
    }

    /// Whether a Start on some pad plugged in here would seat somebody
    /// ([`PadSeats::start_seats_a_friend`]): the line under step 3's seats
    /// offers the join only then.
    pub fn a_start_would_seat_someone(
        &self,
        live: &[Entity],
        settings: &GameSettings,
        players: &[u8],
    ) -> bool {
        live.iter()
            .any(|&pad| self.start_seats_a_friend(live, settings, players, pad))
    }

    /// Whether `pad`'s Start would seat somebody: a pad no claim holds,
    /// that P1's settings row does not name, and that is not working the
    /// setup. Those three are at the table already (P1 is the person
    /// working the menu, a claim's player sat down), so on step 3 their
    /// Start starts the match instead ([`pad_claim_seats`]), and the
    /// prompt says which. A pad the rule only dealt a seat, P1's at a
    /// keyboard-run setup included, is a friend's until its Start: it
    /// seats them, taking the AI's chair when it has to.
    pub fn start_seats_a_friend(
        &self,
        live: &[Entity],
        settings: &GameSettings,
        players: &[u8],
        pad: Entity,
    ) -> bool {
        let lineup = self.lineup(live.iter().copied());
        let rows = self.rows(settings);
        live.contains(&pad)
            && !lineup.is_claimed(pad)
            && lineup.naming(rows, pad) != Some(0)
            && lineup.pilot(rows, players) != Some(pad)
    }

    /// Whether the Start of `last`, the pad last reached with, would seat
    /// somebody, or with no such pad plugged in, any pad's: what step 3's
    /// controller line says Start does for the hands reading it.
    pub fn start_joins_here(
        &self,
        live: &[Entity],
        settings: &GameSettings,
        players: &[u8],
        last: Option<Entity>,
    ) -> bool {
        match last.filter(|pad| live.contains(pad)) {
            Some(pad) => self.start_seats_a_friend(live, settings, players, pad),
            None => self.a_start_would_seat_someone(live, settings, players),
        }
    }

    /// Whether a pad at the table pressed Start on step 3 since this was
    /// last asked: the match is to start, as Enter starts it.
    pub fn take_start(&mut self) -> bool {
        std::mem::take(&mut self.start_asked)
    }

    /// Seat the player holding `pad`, which has just pressed Start: the
    /// seat a claim can take that the pad was already dealt, so a friend
    /// confirming the seat they play moves nobody, else the lowest one no
    /// claim holds, else the AI's lowest chair, else a seat added to the
    /// table. The chairs it took are kept to
    /// give back when the claim is let go. A pad a seat named in settings
    /// is that seat's claim, taking a chair only when the seat is not a
    /// person's yet; the pad working the setup is P1's, already here.
    fn claim(
        &mut self,
        pad: Entity,
        live: &[Entity],
        settings: &GameSettings,
        config: &mut MatchConfig,
        beaches: &CustomBeaches,
    ) {
        self.benched.retain(|&benched| benched != pad);
        let rows = self.rows(settings);
        let lineup = self.lineup(live.iter().copied());
        if lineup.pilot(rows, &config.human_seats()) == Some(pad) {
            return;
        }
        let named = lineup.naming(rows, pad);
        match named {
            // P1 picking up the pad they named: the person at the menu,
            // already here.
            Some(0) => return,
            // Another seat's named pad: a claim of that seat, which step 3
            // names as joined.
            Some(_) => self.own.push(pad),
            None => {
                if self.claims.len() >= MAX_PLAYERS - 1 {
                    return;
                }
            }
        }
        // One person more: a seat above P1's the pad now drives, made by
        // taking the AI's chair or adding one only while it drives none.
        // A table that grows is reseated, so the map still seats it.
        let mut taken = Vec::new();
        loop {
            if named.is_none() {
                let players = config.human_seats();
                let reach = claim_seats(rows, &players);
                let seats = self.lineup(live.iter().copied()).pads(rows, &players);
                let dealt_here = seats
                    .iter()
                    .find(|&&(seat, held)| held == Some(pad) && reach.contains(&seat))
                    .map(|&(seat, _)| seat);
                let free = dealt_here.or_else(|| {
                    reach
                        .iter()
                        .copied()
                        .find(|&seat| !self.claims.iter().any(|&(_, held)| held == seat))
                });
                if let Some(seat) = free {
                    self.claims.retain(|&(claimed, _)| claimed != pad);
                    self.claims.push((pad, seat));
                }
            }
            let drives = self
                .lineup(live.iter().copied())
                .seat_of(rows, &config.human_seats(), pad)
                .is_some_and(|seat| seat != 0);
            if drives {
                break;
            }
            // A named pad's own seat, else the AI's lowest.
            let ai_chair = match named {
                Some(seat) => (seat < config.seats
                    && config.controller(seat) != SeatController::Local)
                    .then_some(seat),
                None => {
                    (1..config.seats).find(|&seat| config.controller(seat) != SeatController::Local)
                }
            };
            let chair = if let Some(seat) = ai_chair {
                let was = config.controller(seat);
                config.controllers[usize::from(seat)] = SeatController::Local;
                Chair::Ai(seat, was)
            } else if usize::from(config.seats) < MAX_PLAYERS {
                let seats = config.seats + 1;
                crate::app::match_setup::reseat(config, seats, beaches);
                Chair::Added
            } else {
                // A full table: the Start takes nothing.
                for chair in taken.into_iter().rev() {
                    Chair::give_back(chair, config, beaches);
                }
                self.let_go(pad);
                return;
            };
            taken.push(chair);
        }
        self.chairs
            .0
            .extend(taken.into_iter().map(|chair| (pad, chair)));
    }

    /// Settle every claim among the person's seats `players`: one that
    /// drives a seat above P1's holds that seat from now on (a claim whose
    /// seat went to the AI is dealt the next one free, and keeps it), and
    /// one that drives none is let go, its chairs forgotten and its pad
    /// benched.
    ///
    /// A claim is pushed out by a change made over its head: its seat
    /// turned over to the AI on step 3, the table dialled down under it,
    /// its seat set to the keyboard or to a named pad in settings, or P2's
    /// own keys pressed. What pushed it out has the seat now, so its chairs
    /// are not given back. Held on to, it drove nothing and its Start did
    /// nothing either, since the pad was already a claim; let go, the Start
    /// seats it again. Benched until then, so the friend holding it is not
    /// dealt P1 as a pad nobody claimed would be.
    fn let_go_of_the_seatless(&mut self, live: &[Entity], settings: &GameSettings, players: &[u8]) {
        let seats = self
            .lineup(live.iter().copied())
            .pads(self.rows(settings), players);
        let drives = |pad: Entity| {
            seats
                .iter()
                .find(|&&(seat, held)| held == Some(pad) && seat != 0)
                .map(|&(seat, _)| seat)
        };
        for (pad, seat) in &mut self.claims {
            if let Some(driven) = drives(*pad) {
                *seat = driven;
            }
        }
        let seatless: Vec<Entity> = self
            .claimed()
            .filter(|&pad| drives(pad).is_none())
            .collect();
        for pad in seatless {
            self.let_go(pad);
            self.chairs.0.retain(|&(claimed, _)| claimed != pad);
            if !self.benched.contains(&pad) {
                self.benched.push(pad);
            }
        }
    }

    /// The people in seats `a` and `b` trade places, the pads driving them
    /// with them, at a table whose person's seats are `players` (as they
    /// were before the trade): a partner picked on step 3. Each seat's
    /// claim moves to the other seat, and a pad the rule only dealt one of
    /// them becomes a claim of the other, so it follows its player rather
    /// than staying with the seat for the rule to deal again. Neither seat
    /// is P1's, nor one a settings row names: those never move.
    pub fn swap_seats(
        &mut self,
        live: &[Entity],
        settings: &GameSettings,
        players: &[u8],
        (a, b): (u8, u8),
    ) {
        let lineup = self.lineup(live.iter().copied());
        let seats = lineup.pads(self.rows(settings), players);
        let pad_at = |seat: u8| {
            seats
                .iter()
                .find(|&&(at, _)| at == seat)
                .and_then(|&(_, pad)| pad)
        };
        let moving = [(pad_at(a), b), (pad_at(b), a)];
        for (_, seat) in &mut self.claims {
            if *seat == a {
                *seat = b;
            } else if *seat == b {
                *seat = a;
            }
        }
        for (pad, to) in moving {
            if let Some(pad) = pad
                && !lineup.is_claimed(pad)
            {
                self.claims.push((pad, to));
            }
        }
        self.table_set_by_hand();
    }

    /// The table was set by hand on the setup steps: the seats and who
    /// plays them are what the dials and step 3 say, whatever the claims
    /// took to grow it. A claim let go after this gives nothing back,
    /// which once took a seat the dial had already removed and left the
    /// table smaller than it was set.
    pub fn table_set_by_hand(&mut self) {
        self.chairs.0.clear();
    }

    /// Drop `pad`'s claim.
    fn let_go(&mut self, pad: Entity) {
        self.claims.retain(|&(claimed, _)| claimed != pad);
        self.own.retain(|&claimed| claimed != pad);
    }
}

/// One person's seat a pad plays, as step 3 reads it: the seat, the pad's
/// number in plug order, and whether it is the pad's claim (pressed
/// Start for it) rather than a pad the rule only dealt it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SeatPad {
    pub seat: u8,
    pub number: u8,
    pub joined: bool,
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
/// A claim let go gives back the chair it took (see [`Chairs`]), so the
/// table is the people still holding a pad. The other claims keep their
/// seats, unless the chair given back was the one a claim sat in: that
/// claim moves into the lowest seat left free for a claim. The same pad
/// back presses Start and sits down again.
///
/// Given back during setup only, where the claim was made and the table
/// is on show: the table is also an online host's terms, and a pad whose
/// battery died in the lobby rewrote them under the joiners (the map
/// picked there). Gone elsewhere, the seat is owed until match setup is
/// next opened.
///
/// A claim (or the pilot's role) taken over by a new pad mid-round
/// benches the pad that held it ([`PadSeats::benched`]) until it next
/// presses Start.
pub fn keep_pad_order(
    pads: Query<Entity, With<Gamepad>>,
    screen: Res<State<Screen>>,
    mut seats: ResMut<PadSeats>,
    config: Option<ResMut<MatchConfig>>,
    beaches: Option<Res<CustomBeaches>>,
    settings: Option<Res<GameSettings>>,
) {
    let rows = settings.as_deref().map(|settings| seats.rows(settings));
    let PadSeats {
        claims,
        own,
        benched,
        pilot,
        p2_at_keys: _,
        start_asked: _,
        order: PlugOrder(order),
        chairs: Chairs(chairs),
    } = &mut *seats;
    for pad in &pads {
        if order.contains(&pad) {
            continue;
        }
        // A claimed pad gone and a new one plugged in: the new one takes
        // over the claim, and the seat the claim held. Placed after every
        // claim instead, it left that seat empty for the rest of a series
        // and steered a keyboard player's seat besides. The pilot's pad
        // the same: only overwritten in the order, the new pad was no
        // pilot, P1 was dealt the last spare pad, and the friend on P2
        // and the newcomer swapped seats for the rest of the series.
        let dead = claims
            .iter_mut()
            .map(|(claimed, _)| claimed)
            .chain(own.iter_mut())
            .chain(pilot.as_mut())
            .find(|claimed| !pads.contains(**claimed));
        if let Some(claimed) = dead {
            let gone = std::mem::replace(claimed, pad);
            if !benched.contains(&gone) {
                benched.push(gone);
            }
            // The chairs change hands with the claim.
            for chair in chairs.iter_mut().filter(|(claimed, _)| *claimed == gone) {
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
    if in_a_round {
        return;
    }
    claims.retain(|&(claimed, _)| pads.contains(claimed));
    own.retain(|&claimed| pads.contains(claimed));
    benched.retain(|&gone| pads.contains(gone));
    order.retain(|&seen| pads.contains(seen));
    // The pad working the setup unplugged: the keyboard is P1's again.
    if pilot.is_some_and(|pad| !pads.contains(pad)) {
        *pilot = None;
    }
    if *screen.get() != Screen::MatchSetup {
        return;
    }
    let mut config = config;
    let shelf = CustomBeaches::default();
    let beaches = beaches.as_deref().unwrap_or(&shelf);
    let mut given_back = false;
    chairs.retain(|&(claimed, chair)| {
        if claims.iter().any(|&(pad, _)| pad == claimed) || own.contains(&claimed) {
            return true;
        }
        if let Some(config) = config.as_deref_mut() {
            chair.give_back(config, beaches);
            given_back = true;
        }
        false
    });
    // A claim whose seat went with the chair given back moves into the
    // lowest seat left free for a claim.
    if given_back && let (Some(config), Some(rows)) = (config.as_deref(), rows) {
        let reach = claim_seats(rows, &config.human_seats());
        for at in 0..claims.len() {
            if reach.contains(&claims[at].1) {
                continue;
            }
            let free = reach
                .iter()
                .copied()
                .find(|&seat| !claims.iter().any(|&(_, held)| held == seat));
            if let Some(seat) = free {
                claims[at].1 = seat;
            }
        }
    }
}

/// Let every claim go, on the way into a board the people at this
/// machine play without the couch's seating (a puzzle, co-op, the editor).
/// A lone cursor takes the first claim if there is one, and the first
/// claim after a couch match was a friend's pad: P1's puzzle followed the
/// friend. The benched pads are free again too, there being no claim left
/// to keep them from, and P2 at the keys goes with the couch it seated.
/// The chairs the claims took are owed until match setup is next opened,
/// where the friends press Start again.
///
/// P1's pad is the one in the hands that came in
/// ([`super::hands_at_the_menu`]),
/// as at the setup: with none, a lone cursor took the first pad plugged
/// in, so a docked Deck's own pad steered while the pad in the player's
/// hands worked every menu and then nothing on the board.
pub fn let_claims_go(
    mut seats: ResMut<PadSeats>,
    last: Res<super::LastDevice>,
    last_pad: Res<super::LastPad>,
) {
    release_the_couch(&mut seats);
    seats.pilot = super::hands_at_the_menu(*last, *last_pad);
}

/// The claims, the named pads picked up, the benched pads and P2 at the
/// keys, all let go.
fn release_the_couch(seats: &mut PadSeats) {
    let PadSeats {
        claims,
        own,
        benched,
        pilot: _,
        p2_at_keys,
        start_asked,
        order: _,
        chairs: _,
    } = seats;
    claims.clear();
    own.clear();
    benched.clear();
    *p2_at_keys = false;
    *start_asked = false;
}

/// Back on the main menu, the pad that worked the last setup works no
/// table: the pilot plays the rounds of the match it set up and its
/// rematches, and the next setup takes the hands last used afresh
/// ([`PadSeats::setup_opened`]). Kept on, a stale pilot was the lone
/// cursor's pad in the daily and online.
pub fn forget_the_pilot(mut seats: ResMut<PadSeats>) {
    seats.pilot = None;
}

/// The claims let go as [`let_claims_go`] does, the pilot kept (it is the
/// one person's own pad), on the way into a versus round one person here
/// plays alone: the daily, a match against the AI, a resumed round with
/// one human seat, an online round. Asked of the round's own cursors
/// ([`crate::app::cursor::seats_played_here`]), so it is the round the
/// cursors are spawned for. A couch round is left alone.
///
/// Online too: a claim is only ever made at match setup, by a friend
/// beside P1, so a claim online is left over from a couch match. Kept,
/// the lone cursor played on the pad lying on the couch, and the pad in
/// the player's hands was dead.
pub fn let_a_lone_players_claims_go(
    source: crate::app::session::RoundSource,
    mut seats: ResMut<PadSeats>,
    last: Res<super::LastDevice>,
    last_pad: Res<super::LastPad>,
) {
    if crate::app::cursor::seats_played_here(&source).len() == 1 {
        release_the_couch(&mut seats);
        // P1's pad is the one in the hands that came in, as on the way
        // into a puzzle ([`let_claims_go`]); with the keyboard's, the
        // setup's pilot stays. The daily from the menu has no pilot, and
        // took the first pad plugged in.
        if let Some(pad) = super::hands_at_the_menu(*last, *last_pad) {
            seats.pilot = Some(pad);
        }
    }
}

/// Who works the setup, from the presses on it: a key the keyboard's
/// player 1 or the menu uses makes the keyboard P1's ([`PadSeats::pilot`]
/// none), and a button on a pad nobody claimed, named or benched makes
/// that pad P1's, while no other pad is. A friend's Start is their own
/// (`pad_claim_seats`), and so are P2's own keys, which seat P2: neither
/// moves P1. Not while a name is typed, where the keys are letters (a
/// Deck's on-screen keyboard among them), nor under a doorway card.
///
/// Nor on step 3, whose rows name who plays each seat: the table it shows
/// is the table the match starts with. Followed there, the keyboard's
/// Enter took P1 off the pad that set the match up a frame before the
/// launch, and a friend's pad on P2 pressing Select or the d-pad took P1.
pub fn follow_the_setup_hands(
    mut keyboard: MessageReader<bevy::input::keyboard::KeyboardInput>,
    pads: Query<(Entity, &Gamepad)>,
    settings: Res<GameSettings>,
    menu: Res<crate::app::match_setup::MatchMenu>,
    bots: Res<crate::app::bot_seats::BotSeats>,
    mut seats: ResMut<PadSeats>,
) {
    let typed: Vec<KeyCode> = keyboard
        .read()
        .filter(|press| press.state == bevy::input::ButtonState::Pressed)
        .map(|press| press.key_code)
        .collect();
    let step_three = menu.step == crate::app::match_setup::Step::Players;
    if menu.naming.is_some() || bots.door_for.card_up() || step_three {
        return;
    }
    let p2_only = crate::app::match_setup::p2_only_keys(&settings);
    if typed.iter().any(|key| !p2_only.contains(key)) {
        if seats.pilot.is_some() {
            seats.pilot = None;
        }
        return;
    }
    let live: Vec<Entity> = pads.iter().map(|(entity, _)| entity).collect();
    if seats.pilot.is_some_and(|pilot| live.contains(&pilot)) {
        return;
    }
    let rows = seats.rows(&settings);
    let lineup = seats.lineup(live.iter().copied());
    let setup = super::Face::MatchSetup {
        step: crate::app::match_setup::Step::Players,
        join: true,
        partners: false,
    };
    let reached = pads.iter().find(|&(entity, pad)| {
        let joins = super::scheme::pressed_on(&setup, super::scheme::Does::Join, pad);
        pad.get_just_pressed().count() > usize::from(joins)
            && !lineup.is_claimed(entity)
            && !lineup.benched.contains(&entity)
            && !lineup.is_named(rows, entity)
    });
    if let Some((entity, _)) = reached {
        seats.pilot = Some(entity);
    }
}

/// The press-Start-to-join ceremony during match setup, on any of its
/// three steps: an unclaimed pad pressing Start puts one more person at
/// the table (see [`PadSeats::claim`]), in the lowest person's seat above
/// P1's that no claim holds yet, keeping it until the claim is let go. A
/// Start pressed on step 1 or 2 is that friend already seated when step 3
/// opens. Up to five: every seat but the keyboard's player 1.
/// Disconnected pads lose their claim, and the seat with it, in
/// [`keep_pad_order`].
///
/// On step 3 a Start that seats nobody ([`PadSeats::start_seats_a_friend`]:
/// the pad working the setup, P1's own, a friend already seated) starts
/// the match, as Start does on every other screen: a lone pad had no
/// Start at all there.
pub fn pad_claim_seats(
    pads: Query<(Entity, &Gamepad)>,
    mut seats: ResMut<PadSeats>,
    mut config: ResMut<MatchConfig>,
    beaches: Res<CustomBeaches>,
    settings: Res<GameSettings>,
    (bots, menu): (
        Res<crate::app::bot_seats::BotSeats>,
        Res<crate::app::match_setup::MatchMenu>,
    ),
) {
    // A doorway card is up, waiting for the bots the table was set with:
    // the table is settled until it comes down. A claim taken then gave a
    // couch bot's chair to the pad, and the card went on waiting for a bot
    // whose seat was now a person's.
    if bots.door_for.card_up() {
        return;
    }
    let live: Vec<Entity> = pads.iter().map(|(entity, _)| entity).collect();
    seats.let_go_of_the_seatless(&live, &settings, &config.human_seats());
    // Read off step 3's own table whichever step is showing or box is
    // open, so a friend can sit down while the game is still being set.
    let setup = super::Face::MatchSetup {
        step: crate::app::match_setup::Step::Players,
        join: true,
        partners: false,
    };
    let step_three = menu.step == crate::app::match_setup::Step::Players && menu.naming.is_none();
    for (entity, pad) in &pads {
        if !super::scheme::pressed_on(&setup, super::scheme::Does::Join, pad) {
            continue;
        }
        let seats_one = seats.start_seats_a_friend(&live, &settings, &config.human_seats(), entity);
        if step_three && !seats_one {
            seats.start_asked = true;
        } else if !seats.claimed().any(|c| c == entity) {
            seats.claim(entity, &live, &settings, &mut config, &beaches);
        }
    }
}

#[cfg(test)]
mod tests {
    /// Step 3's word for a seat controller `n` was dealt by the rule.
    fn dealt(n: u8) -> crate::app::match_setup::PlayedBy {
        crate::app::match_setup::PlayedBy::Pad {
            number: n,
            joined: false,
        }
    }

    /// Step 3's word for a seat controller `n` pressed Start for.
    fn joined(n: u8) -> crate::app::match_setup::PlayedBy {
        crate::app::match_setup::PlayedBy::Pad {
            number: n,
            joined: true,
        }
    }

    use super::*;
    use crate::app::settings::{GameSettings, SeatInput};
    use bevy::ecs::system::RunSystemOnce;

    fn with(choices: [SeatInput; 2]) -> GameSettings {
        GameSettings {
            seat_input: choices,
            ..GameSettings::default()
        }
    }

    /// `n` pads, in plug order, and a lineup of them with `claims` by slot.
    /// The `k`-th claim holds seat `k + 1`, the first seat above P1's.
    fn lineup(n: usize, claims: &[Option<usize>]) -> (Lineup, Vec<Entity>) {
        let mut world = World::new();
        let pads: Vec<Entity> = (0..n).map(|_| world.spawn_empty().id()).collect();
        let lineup = Lineup {
            order: pads.clone(),
            claims: claims
                .iter()
                .enumerate()
                .filter_map(|(k, c)| c.map(|i| (pads[i], k as u8 + 1)))
                .collect(),
            own: Vec::new(),
            benched: Vec::new(),
            pilot: None,
        };
        (lineup, pads)
    }

    /// Left alone, pads fill seats from P2 upwards and P1 last; a lone
    /// cursor (puzzle, online) takes the first pad whatever its seat.
    #[test]
    fn pads_fill_from_p2_up_and_p1_last() {
        let auto = Rows([SeatInput::Auto; 2]);
        let (four, pads) = lineup(4, &[]);
        assert_eq!(four.pad_of(auto, &[0, 1], 1), Some(pads[0]));
        assert_eq!(four.pad_of(auto, &[0, 1], 0), Some(pads[1]));
        assert_eq!(four.pad_of(auto, &[0, 1, 2, 3], 1), Some(pads[0]));
        assert_eq!(four.pad_of(auto, &[0, 1, 2, 3], 3), Some(pads[2]));
        assert_eq!(four.pad_of(auto, &[0, 1, 2, 3], 0), Some(pads[3]));
        assert_eq!(four.pad_of(auto, &[2], 2), Some(pads[0]), "online");
        assert_eq!(four.pad_of(auto, &[0, 1], 2), None, "no cursor, no pad");
    }

    /// Claims hold their slots' seats, a hole leaves its seat to the pads
    /// nobody claimed, and those deal out over what is left, P1 last: a
    /// friend's Start takes their own pad, never P1's.
    #[test]
    fn claims_hold_their_seats_and_spares_fill_the_rest() {
        let auto = Rows([SeatInput::Auto; 2]);
        let (lineup, pads) = lineup(4, &[Some(2), None, Some(3)]);
        let seats = lineup.pads(auto, &[0, 1, 2, 3]);
        assert_eq!(
            seats,
            vec![
                (0, Some(pads[1])),
                (1, Some(pads[2])),
                (2, Some(pads[0])),
                (3, Some(pads[3])),
            ],
            "P2 and P4 claimed, a spare in the hole at P3, the last on P1"
        );
    }

    /// Online the one cursor plays by P1's settings row whatever seat the
    /// network dealt it, as its keyboard does. Read by the dealt seat, a
    /// joiner in seat two whose P2 row said "keys" had a dead pad.
    #[test]
    fn a_lone_cursor_plays_by_the_first_row() {
        let (lineup, pads) = lineup(3, &[]);
        let pad_for_p1 = Rows([SeatInput::Auto, SeatInput::Keys]);
        assert_eq!(lineup.pad_of(pad_for_p1, &[1], 1), Some(pads[0]));
        let keys_for_p1 = Rows([SeatInput::Keys, SeatInput::Auto]);
        assert_eq!(lineup.pad_of(keys_for_p1, &[1], 1), None);
        let named = Rows([SeatInput::Pad(2), SeatInput::Keys]);
        assert_eq!(lineup.pad_of(named, &[3], 3), Some(pads[2]));
    }

    /// A seat that names a controller gets that one, and the seat that
    /// would have been dealt it does not.
    #[test]
    fn a_named_controller_belongs_to_the_seat_that_named_it() {
        let (lineup, pads) = lineup(2, &[]);
        let mine = Rows([SeatInput::Pad(0), SeatInput::Auto]);
        assert_eq!(lineup.pad_of(mine, &[0, 1], 0), Some(pads[0]));
        assert_eq!(
            lineup.pad_of(mine, &[0, 1], 1),
            Some(pads[1]),
            "P2 takes the next one free, not the named one"
        );
    }

    /// "Controller 1" is the first pad plugged in, whoever claimed first:
    /// a friend's claim does not move P1's named pad onto the friend's.
    #[test]
    fn a_named_controller_is_counted_in_plug_order() {
        let (lineup, pads) = lineup(2, &[Some(1)]);
        let mine = Rows([SeatInput::Pad(0), SeatInput::Auto]);
        assert_eq!(lineup.pad_of(mine, &[0, 1], 0), Some(pads[0]));
        assert_eq!(lineup.pad_of(mine, &[0, 1], 1), Some(pads[1]));
    }

    /// Keyboard-only means no pad, and the pad it would have had goes to
    /// whoever is next in line rather than going spare.
    #[test]
    fn keyboard_only_gives_its_pad_up() {
        let (lineup, pads) = lineup(1, &[]);
        let quiet = Rows([SeatInput::Auto, SeatInput::Keys]);
        assert_eq!(lineup.pad_of(quiet, &[0, 1], 1), None);
        assert_eq!(lineup.pad_of(quiet, &[0, 1], 0), Some(pads[0]));
    }

    /// Both seats naming the same controller is a thing a player can do by
    /// walking the dial past it, and it must not panic or hand it to a
    /// third seat; they simply share it.
    #[test]
    fn two_seats_may_name_the_same_controller() {
        let (lineup, pads) = lineup(2, &[]);
        let both = Rows([SeatInput::Pad(1), SeatInput::Pad(1)]);
        assert_eq!(lineup.pad_of(both, &[0, 1], 0), Some(pads[1]));
        assert_eq!(lineup.pad_of(both, &[0, 1], 1), Some(pads[1]));
        // And a third seat skips the named one.
        assert_eq!(lineup.pad_of(both, &[0, 1, 2], 2), Some(pads[0]));
    }

    /// The setup screen with `pads` controllers plugged in, run by the
    /// app's own systems: the pad order, the Start that claims, the
    /// setup screen's keys and footer, the settings screen's keys, and the
    /// claims let go on the way into a board. Screens change by
    /// `NextState`, so each one's way in runs.
    fn setup_table(config: MatchConfig, pads: usize) -> (App, Vec<Entity>) {
        use crate::app::match_setup::{MatchMenu, Step};
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.insert_resource(State::new(Screen::MatchSetup));
        app.init_resource::<PadSeats>();
        app.init_resource::<GameSettings>();
        app.init_resource::<crate::app::bot_seats::BotSeats>();
        app.init_resource::<CustomBeaches>();
        app.insert_resource(config);
        app.init_resource::<ButtonInput<KeyCode>>();
        app.add_message::<bevy::input::keyboard::KeyboardInput>();
        // On step 3, where Enter starts the match.
        app.insert_resource(MatchMenu {
            step: Step::Players,
            ..MatchMenu::default()
        });
        app.init_resource::<crate::app::tournament::Tournament>();
        app.init_resource::<crate::app::settings::screen::SettingsMenu>();
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.init_resource::<crate::app::progress::Progress>();
        app.init_resource::<crate::app::net::Online>();
        app.init_resource::<crate::app::Playback>();
        app.init_resource::<crate::app::Daily>();
        app.init_resource::<crate::app::Resuming>();
        app.init_resource::<crate::app::rematch::LocalTable>();
        app.insert_resource(crate::app::Sandbox(false));
        app.init_resource::<super::super::PadFace>();
        app.init_resource::<super::super::LastDevice>();
        app.init_resource::<super::super::LastPad>();
        app.init_resource::<super::super::Bridged>();
        app.init_resource::<super::super::PadPick>();
        app.init_resource::<Time>();
        crate::app::schedule::add_claim_release(&mut app);
        app.add_systems(
            OnEnter(Screen::MatchSetup),
            crate::app::match_setup::enter_match_setup,
        );
        app.add_systems(OnEnter(Screen::Menu), forget_the_pilot);
        // The face as `read_face` gives it on the screens here: the menu,
        // a setup step, and nothing a pad presses keys for elsewhere.
        let face = |screen: Res<State<Screen>>,
                    menu: Res<MatchMenu>,
                    (seats, settings, config, last_pad): (
            Res<PadSeats>,
            Res<GameSettings>,
            Res<MatchConfig>,
            Res<super::super::LastPad>,
        ),
                    pads: Query<Entity, With<Gamepad>>,
                    mut face: ResMut<super::super::PadFace>| {
            let live: Vec<Entity> = pads.iter().collect();
            face.0 = match screen.get() {
                Screen::Menu => super::super::Face::Menu,
                Screen::MatchSetup => super::super::Face::MatchSetup {
                    step: menu.step,
                    join: seats.start_joins_here(
                        &live,
                        &settings,
                        &config.human_seats(),
                        last_pad.0,
                    ),
                    partners: crate::app::match_setup::setup_teams(&settings, &config)
                        != crate::app::teams::TeamMode::Solo,
                },
                Screen::Settings => super::super::Face::Settings { dial: true },
                Screen::Puzzle
                | Screen::Versus
                | Screen::Editor
                | Screen::Lobby
                | Screen::Controls
                | Screen::Achievements
                | Screen::StageSelect
                | Screen::Replays
                | Screen::Interlude
                | Screen::Language
                | Screen::NewVersion => super::super::Face::Round,
            };
        };
        app.add_systems(
            Update,
            (
                keep_pad_order,
                face,
                super::super::track_last_device,
                super::super::pad_menu_bridge,
                super::super::pad_bridge_release,
                (
                    follow_the_setup_hands,
                    pad_claim_seats,
                    crate::app::match_setup::match_setup_input,
                )
                    .chain()
                    .run_if(in_state(Screen::MatchSetup)),
                crate::app::settings::screen::settings_input.run_if(in_state(Screen::Settings)),
            )
                .chain(),
        );
        let plugged = (0..pads)
            .map(|_| app.world_mut().spawn(Gamepad::default()).id())
            .collect();
        app.update();
        (app, plugged)
    }

    fn press_start(app: &mut App, pad: Entity) {
        press(app, pad, GamepadButton::Start);
    }

    /// One press of `button` on `pad`, down for a frame and up the next,
    /// the keys the bridge pressed for it let go.
    fn press(app: &mut App, pad: Entity, button: GamepadButton) {
        let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("pad");
        gamepad.digital_mut().press(button);
        app.update();
        // The frame is over: nothing is just pressed any more, as Bevy's
        // input systems have it at the next frame's start.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        let mut gamepad = app.world_mut().get_mut::<Gamepad>(pad).expect("pad");
        gamepad.digital_mut().clear();
        gamepad.digital_mut().release(button);
        gamepad.digital_mut().clear();
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
    }

    /// Back to the main menu, a press there on `pad` (the d-pad) or at
    /// the keyboard (`None`, a key the menu reads), and on into the
    /// setup: the hands the setup is opened with.
    fn into_the_setup_by(app: &mut App, pad: Option<Entity>) {
        use crate::app::match_setup::{MatchMenu, Step};
        go(app, Screen::Menu);
        match pad {
            Some(pad) => press(app, pad, GamepadButton::DPadDown),
            None => tap(app, KeyCode::ArrowDown),
        }
        go(app, Screen::MatchSetup);
        app.world_mut().resource_mut::<MatchMenu>().step = Step::Players;
        app.update();
    }

    /// A key pressed for one frame at the keyboard: the key down, and the
    /// keyboard's own message, which the hands are told by.
    fn tap(app: &mut App, key: KeyCode) {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        app.world_mut()
            .write_message(bevy::input::keyboard::KeyboardInput {
                key_code: key,
                logical_key: bevy::input::keyboard::Key::Unidentified(
                    bevy::input::keyboard::NativeKey::Unidentified,
                ),
                state: bevy::input::ButtonState::Pressed,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
        app.update();
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(key);
        keys.clear();
    }

    /// Go to `screen` as the game does, and let its way in run.
    fn go(app: &mut App, screen: Screen) {
        app.world_mut()
            .resource_mut::<NextState<Screen>>()
            .set(screen);
        app.update();
    }

    /// Turn step 1's players row one notch `right` or left, from step 1
    /// and back to step 3.
    fn dial_players(app: &mut App, right: bool) {
        use crate::app::match_setup::{MatchMenu, Step};
        let mut menu = app.world_mut().resource_mut::<MatchMenu>();
        menu.step = Step::Game;
        menu.game_row = 0;
        tap(
            app,
            if right {
                KeyCode::ArrowRight
            } else {
                KeyCode::ArrowLeft
            },
        );
        app.world_mut().resource_mut::<MatchMenu>().step = Step::Players;
    }

    /// Turn who plays `seat` one notch `right` or left on step 3.
    fn turn_seat(app: &mut App, seat: u8, right: bool) {
        app.world_mut()
            .resource_mut::<crate::app::match_setup::MatchMenu>()
            .seat = seat;
        tap(
            app,
            if right {
                KeyCode::ArrowRight
            } else {
                KeyCode::ArrowLeft
            },
        );
    }

    /// Step P2's "plays with" one notch right on the settings screen, from
    /// the setup screen and back.
    fn p2_input_one_notch(app: &mut App) {
        use crate::app::settings::screen::{Row, SettingsMenu};
        go(app, Screen::Settings);
        let at = Row::ALL
            .iter()
            .position(|r| *r == Row::InputP2)
            .expect("the row");
        app.world_mut().resource_mut::<SettingsMenu>().selected = at;
        tap(app, KeyCode::ArrowRight);
        go(app, Screen::MatchSetup);
    }

    /// The table as (seats, AI and bots, the seat each claim drives).
    fn told(app: &mut App) -> (u8, u8, Vec<u8>) {
        let world = app.world_mut();
        let live: Vec<Entity> = world
            .query_filtered::<Entity, With<Gamepad>>()
            .iter(world)
            .collect();
        let config = app.world().resource::<MatchConfig>();
        let settings = app.world().resource::<GameSettings>();
        let joined =
            app.world()
                .resource::<PadSeats>()
                .joined(&live, settings, &config.human_seats());
        (config.seats, config.ai_seats(), joined)
    }

    /// Who plays each seat, as step 3 says it.
    fn rows(app: &mut App) -> Vec<crate::app::match_setup::PlayedBy> {
        let world = app.world_mut();
        let live: Vec<Entity> = world
            .query_filtered::<Entity, With<Gamepad>>()
            .iter(world)
            .collect();
        let config = app.world().resource::<MatchConfig>();
        let settings = app.world().resource::<GameSettings>();
        let seats = app.world().resource::<PadSeats>();
        let pads = seats.seat_pads(&live, settings, &config.human_seats());
        (0..config.seats)
            .map(|seat| {
                crate::app::match_setup::played_by(
                    config,
                    seat,
                    &pads,
                    seats.p2_at_keys(),
                    settings,
                )
            })
            .collect()
    }

    /// The pad the round reads for `seat` at the configured table, by the
    /// same call the cursor, the commits and the rumble make.
    fn driving(app: &mut App, seat: u8) -> Option<Entity> {
        let humans = app.world().resource::<MatchConfig>().human_seats();
        app.world_mut()
            .run_system_once(
                move |pads: Query<(Entity, &Gamepad)>,
                      seats: Res<PadSeats>,
                      settings: Res<GameSettings>| {
                    seats.pad_entity_for(&pads, &settings, &humans, seat)
                },
            )
            .expect("runs")
    }

    /// Each seat the round reads drives the pad given, and P1, at the
    /// keyboard, drives none of them.
    fn drives(app: &mut App, expected: &[(u8, Entity)]) {
        for &(seat, pad) in expected {
            assert_eq!(driving(app, seat), Some(pad), "P{} drives it", seat + 1);
        }
        assert!(
            !expected
                .iter()
                .any(|&(_, pad)| driving(app, 0) == Some(pad)),
            "a claim steers P1"
        );
    }

    /// Two players on the dial and two friends pressing Start: the first
    /// takes P2, the one empty human seat, and the second a seat added
    /// for them, P3. Nobody's label changes when the next one joins. It
    /// used to grow the table twice and leave P2 driven by nobody.
    #[test]
    fn two_friends_join_a_table_of_two_as_p2_and_p3() {
        let (mut app, pads) = setup_table(MatchConfig::default(), 2);
        assert_eq!(told(&mut app), (2, 0, vec![]));
        press_start(&mut app, pads[0]);
        assert_eq!(told(&mut app), (2, 0, vec![1]), "P2's empty seat");
        press_start(&mut app, pads[1]);
        assert_eq!(told(&mut app), (3, 0, vec![1, 2]), "P2 kept, P3 added");
        drives(&mut app, &[(1, pads[0]), (2, pads[1])]);
    }

    /// A table already dialled to three people: the two who press Start
    /// fill P2 and P3, and the table stays the size it was.
    #[test]
    fn friends_fill_the_seats_dialled_before_any_are_added() {
        let (mut app, pads) = setup_table(MatchConfig::with_ai(3, 0), 2);
        press_start(&mut app, pads[0]);
        press_start(&mut app, pads[1]);
        assert_eq!(told(&mut app), (3, 0, vec![1, 2]));
        drives(&mut app, &[(1, pads[0]), (2, pads[1])]);
    }

    /// One player against the AI: the first friend takes the AI's chair as
    /// P2 and keeps that name when the second joins as P3.
    #[test]
    fn a_friend_takes_the_ais_chair_and_keeps_its_name() {
        let (mut app, pads) = setup_table(MatchConfig::with_ai(2, 1), 2);
        press_start(&mut app, pads[0]);
        assert_eq!(told(&mut app), (2, 0, vec![1]), "two people, no AI");
        press_start(&mut app, pads[1]);
        assert_eq!(told(&mut app), (3, 0, vec![1, 2]), "still P2, then P3");
        drives(&mut app, &[(1, pads[0]), (2, pads[1])]);
    }

    /// P1 set to play on controller 1, and a friend presses Start on the
    /// other pad first: the friend is P2, and controller 1 still drives P1.
    /// Counted claims first, the friend's pad became controller 1 and
    /// steered P1. P1 picking their own pad up takes nothing.
    #[test]
    fn a_friends_start_never_steers_p1s_named_pad() {
        let (mut app, pads) = setup_table(MatchConfig::default(), 2);
        app.insert_resource(with([SeatInput::Pad(0), SeatInput::Auto]));
        press_start(&mut app, pads[1]);
        assert_eq!(told(&mut app), (2, 0, vec![1]), "the friend is P2");
        assert_eq!(driving(&mut app, 0), Some(pads[0]), "P1's own pad");
        assert_eq!(driving(&mut app, 1), Some(pads[1]));
        press_start(&mut app, pads[0]);
        assert_eq!(told(&mut app), (2, 0, vec![1]), "P1 was already here");
        assert_eq!(driving(&mut app, 0), Some(pads[0]));
        assert_eq!(driving(&mut app, 1), Some(pads[1]));
    }

    /// P1 on controller 1 alone against the AI picks that controller up
    /// and presses Start: the pad is P1's, so the AI keeps its chair.
    #[test]
    fn a_seated_players_named_pad_takes_no_chair() {
        let (mut app, pads) = setup_table(MatchConfig::with_ai(2, 1), 1);
        app.insert_resource(with([SeatInput::Pad(0), SeatInput::Auto]));
        press_start(&mut app, pads[0]);
        assert_eq!(told(&mut app), (2, 1, vec![]), "the AI still plays");
        assert_eq!(driving(&mut app, 0), Some(pads[0]));
    }

    /// P2 named controller 2 while the AI held P2: that pad's Start brings
    /// P2 to the table, in the AI's chair, on that pad.
    #[test]
    fn a_named_pad_brings_its_seat_to_the_table() {
        let (mut app, pads) = setup_table(MatchConfig::with_ai(2, 1), 2);
        app.insert_resource(with([SeatInput::Auto, SeatInput::Pad(1)]));
        press_start(&mut app, pads[1]);
        assert_eq!(told(&mut app), (2, 0, vec![1]));
        assert_eq!(driving(&mut app, 1), Some(pads[1]));
        // Gone, it gives the chair back.
        app.world_mut().entity_mut(pads[1]).remove::<Gamepad>();
        app.update();
        assert_eq!(told(&mut app), (2, 1, vec![]));
    }

    /// P2 set to the keyboard alone, against the AI: a friend's Start needs
    /// a seat P2 does not hold, so P2 comes to the table at the keyboard
    /// and the friend sits at P3.
    #[test]
    fn a_keyboard_only_p2_sends_a_friend_to_p3() {
        let (mut app, pads) = setup_table(MatchConfig::with_ai(2, 1), 1);
        app.insert_resource(with([SeatInput::Auto, SeatInput::Keys]));
        press_start(&mut app, pads[0]);
        assert_eq!(told(&mut app), (3, 0, vec![2]));
        drives(&mut app, &[(2, pads[0])]);
        // And both chairs go back with the claim.
        app.world_mut().entity_mut(pads[0]).remove::<Gamepad>();
        app.update();
        assert_eq!(told(&mut app), (2, 1, vec![]));
    }

    /// A full table takes no more claims, and the Start takes nothing.
    #[test]
    fn a_full_table_takes_no_more_claims() {
        let (mut app, pads) = setup_table(MatchConfig::with_ai(4, 2), 6);
        press_start(&mut app, pads[0]);
        assert_eq!(told(&mut app), (4, 2, vec![1]), "P2 was a human seat");
        press_start(&mut app, pads[1]);
        assert_eq!(told(&mut app), (4, 1, vec![1, 2]), "the AI's chair");
        for &pad in &pads[2..] {
            press_start(&mut app, pad);
        }
        assert_eq!(told(&mut app), (6, 0, vec![1, 2, 3, 4, 5]));
        assert!(
            !app.world()
                .resource::<PadSeats>()
                .claimed()
                .any(|c| c == pads[5])
        );
        drives(
            &mut app,
            &[
                (1, pads[0]),
                (2, pads[1]),
                (3, pads[2]),
                (4, pads[3]),
                (5, pads[4]),
            ],
        );
    }

    /// A pad's Start while the bot doorway card is up takes nothing: the
    /// card is waiting for a bot in the AI's chair, and a claim that took
    /// that chair left it waiting for a seat a person now had.
    #[test]
    fn no_claim_while_the_bot_card_is_up() {
        use crate::app::bot_seats::{BotSeats, DoorFor};
        let (mut app, pads) = setup_table(MatchConfig::with_ai(2, 1), 1);
        app.world_mut().resource_mut::<BotSeats>().door_for = DoorFor::Couch;
        press_start(&mut app, pads[0]);
        assert_eq!(told(&mut app), (2, 1, vec![]), "the bot's chair kept");
        let seats = app.world().resource::<PadSeats>();
        assert!(seats.claimed().next().is_none(), "no claim");
    }

    /// Pads joining a table on the four-castle arena move it to a beach
    /// that seats them all: grown in place, the fifth player had no castle.
    #[test]
    fn a_table_grown_by_pads_is_moved_to_a_beach_that_seats_it() {
        use crate::app::match_setup::{MapChoice, holds};
        let (mut app, pads) = setup_table(
            MatchConfig {
                map: MapChoice::Classic,
                ..MatchConfig::with_ai(2, 0)
            },
            4,
        );
        for &pad in &pads {
            press_start(&mut app, pad);
        }
        let config = app.world().resource::<MatchConfig>();
        assert_eq!(config.seats, 5, "P2 filled, then P3 to P5 added");
        assert!(holds(config.map, 5), "five on {:?}", config.map);
    }

    /// A friend's Start on step 3 that adds a fifth seat moves the small
    /// beach picked on step 2 to the large one: step 3 says so, naming
    /// both, and the match is launched on the large beach at the seed its
    /// card is drawn with, the board that card shows.
    #[test]
    fn a_start_that_moves_the_map_says_so_and_launches_it() {
        use crate::app::i18n::EN;
        use crate::app::match_setup::{
            Card, MapChoice, MatchMenu, card_config, local_board, map_moved, moved_note,
            picked_card,
        };
        let (mut app, pads) = setup_table(
            MatchConfig {
                seats: 4,
                map: MapChoice::GenSmall,
                ..MatchConfig::default()
            },
            4,
        );
        app.world_mut().resource_mut::<MatchMenu>().seed = 0x5EED;
        for &pad in &pads[..3] {
            press_start(&mut app, pad);
        }
        let shelf = CustomBeaches::default();
        let config = app.world().resource::<MatchConfig>();
        assert_eq!((config.seats, config.map), (4, MapChoice::GenSmall));
        assert_eq!(moved_note(&EN, config, &shelf), None);
        press_start(&mut app, pads[3]);
        let config = app.world().resource::<MatchConfig>().clone();
        assert_eq!((config.seats, config.map), (5, MapChoice::GenLarge));
        assert_eq!(
            map_moved(&config, &shelf),
            Some((
                Card::Map(MapChoice::GenSmall),
                Card::Map(MapChoice::GenLarge)
            ))
        );
        let note = moved_note(&EN, &config, &shelf).expect("step 3 says it");
        assert!(note.contains("Small") && note.contains("Large") && note.contains('5'));
        tap(&mut app, KeyCode::Enter);
        let launched = app.world().resource::<MatchConfig>();
        assert!(launched.armed);
        assert_eq!(launched.seed, Some(0x5EED));
        let shown = card_config(&config, picked_card(&config, &shelf), &shelf).expect("a card");
        assert_eq!(shown.map, MapChoice::GenLarge, "the card the note names");
        let played = local_board(launched, &shelf, 0x5EED);
        let drawn = local_board(&shown, &shelf, 0x5EED);
        assert_eq!(played.state_hash(), drawn.state_hash());
    }

    /// A pad that drops out on the setup screen gives its seat back, and
    /// coming back and pressing Start takes it again: one seat, however
    /// often the battery flickers.
    #[test]
    fn a_pad_that_drops_and_rejoins_keeps_one_seat() {
        let (mut app, pads) = setup_table(MatchConfig::default(), 2);
        press_start(&mut app, pads[0]);
        press_start(&mut app, pads[1]);
        assert_eq!(told(&mut app), (3, 0, vec![1, 2]));
        for _ in 0..3 {
            app.world_mut().entity_mut(pads[1]).remove::<Gamepad>();
            app.update();
            assert_eq!(told(&mut app), (2, 0, vec![1]), "the added seat goes");
            app.world_mut()
                .entity_mut(pads[1])
                .insert(Gamepad::default());
            app.update();
            press_start(&mut app, pads[1]);
            assert_eq!(told(&mut app), (3, 0, vec![1, 2]));
        }

        // A claim that took the AI's chair hands it back to the AI.
        let (mut app, pads) = setup_table(MatchConfig::with_ai(3, 1), 2);
        press_start(&mut app, pads[0]);
        press_start(&mut app, pads[1]);
        assert_eq!(told(&mut app), (3, 0, vec![1, 2]));
        app.world_mut().entity_mut(pads[1]).remove::<Gamepad>();
        app.update();
        assert_eq!(told(&mut app), (3, 1, vec![1]), "the AI sits back down");
    }

    /// A friend who drops out leaves the others where they sat: the seat
    /// waits empty for the next Start, which fills it. A chair the friend
    /// took from the AI goes back to the AI, and nobody else moves.
    #[test]
    fn a_friend_who_leaves_does_not_renumber_the_rest() {
        let (mut app, pads) = setup_table(MatchConfig::with_ai(3, 0), 3);
        press_start(&mut app, pads[0]);
        press_start(&mut app, pads[1]);
        app.world_mut().entity_mut(pads[0]).remove::<Gamepad>();
        app.update();
        assert_eq!(told(&mut app), (3, 0, vec![2]), "still P3");
        assert_eq!(driving(&mut app, 2), Some(pads[1]));
        press_start(&mut app, pads[2]);
        assert_eq!(told(&mut app), (3, 0, vec![1, 2]), "the new one fills P2");
        drives(&mut app, &[(1, pads[2]), (2, pads[1])]);

        let (mut app, pads) = setup_table(MatchConfig::with_ai(2, 1), 2);
        press_start(&mut app, pads[0]);
        press_start(&mut app, pads[1]);
        assert_eq!(told(&mut app), (3, 0, vec![1, 2]));
        app.world_mut().entity_mut(pads[0]).remove::<Gamepad>();
        app.update();
        assert_eq!(
            told(&mut app),
            (3, 1, vec![2]),
            "the AI back in the chair it gave up, the friend still at P3"
        );
        assert_eq!(driving(&mut app, 2), Some(pads[1]));
    }

    /// Past the keyboard's two seats, a human seat with no pad to drive it
    /// is named for the setup screen to warn about.
    #[test]
    fn a_seat_with_no_controls_is_named() {
        let (mut app, pads) = setup_table(MatchConfig::with_ai(4, 0), 2);
        let idle = |app: &mut App| {
            let world = app.world_mut();
            let live: Vec<Entity> = world
                .query_filtered::<Entity, With<Gamepad>>()
                .iter(world)
                .collect();
            let settings = app.world().resource::<GameSettings>();
            app.world().resource::<PadSeats>().seats_with_no_controls(
                &live,
                settings,
                &[0, 1, 2, 3],
            )
        };
        // Two pads nobody claimed drive P2 and P3; P4 has nothing.
        assert_eq!(idle(&mut app), vec![3]);
        app.world_mut().entity_mut(pads[1]).remove::<Gamepad>();
        app.update();
        assert_eq!(idle(&mut app), vec![2, 3]);
    }

    /// The pad a lone cursor (P1's, the only seat here) plays on.
    fn lone(app: &mut App) -> Option<Entity> {
        app.world_mut()
            .run_system_once(
                |pads: Query<(Entity, &Gamepad)>,
                 seats: Res<PadSeats>,
                 settings: Res<GameSettings>| {
                    seats.pad_entity_for(&pads, &settings, &[0], 0)
                },
            )
            .expect("runs")
    }

    /// After a couch match, a puzzle's lone cursor follows the pad that
    /// worked the menu into it, not the first friend's claim: the claims
    /// are let go on the way into it.
    #[test]
    fn a_puzzle_after_a_couch_match_follows_the_first_pad() {
        let (mut app, pads) = setup_table(MatchConfig::default(), 2);
        press_start(&mut app, pads[1]);
        assert_eq!(lone(&mut app), Some(pads[1]), "the claim, at the setup");
        go(&mut app, Screen::Menu);
        press(&mut app, pads[0], GamepadButton::DPadDown);
        go(&mut app, Screen::Puzzle);
        assert_eq!(lone(&mut app), Some(pads[0]));
    }

    /// Online after a couch match, the lone cursor plays on the pad in the
    /// player's hands, not on the friend's claim left lying on the couch.
    #[test]
    fn an_online_round_after_a_couch_match_lets_the_claims_go() {
        let (mut app, pads) = setup_table(MatchConfig::default(), 2);
        press_start(&mut app, pads[1]);
        go(&mut app, Screen::Menu);
        press(&mut app, pads[0], GamepadButton::DPadDown);
        let transport = crate::transport::UdpTransport::host(0).expect("ephemeral port");
        let lockstep = crate::sim::Lockstep::new(1, vec![0, 1], crate::sim::DEFAULT_DELAY);
        app.insert_resource(crate::app::net::Online(Some(
            crate::app::net::OnlineSession::new(
                transport,
                lockstep,
                2,
                crate::transport::MatchTerms::default(),
            ),
        )));
        go(&mut app, Screen::Versus);
        assert_eq!(lone(&mut app), Some(pads[0]));
        assert!(app.world().resource::<PadSeats>().claims.is_empty());
    }

    /// A lone cursor plays on the pad in the player's hands, not on the
    /// first pad plugged in: a docked Deck's own pad is controller 1 and
    /// the player holds controller 2. In a puzzle, the editor and the
    /// daily alike, and the keyboard's hands leave the first pad.
    #[test]
    fn a_lone_cursor_follows_the_pad_in_the_hands() {
        let (mut app, pads) = setup_table(MatchConfig::default(), 2);
        for screen in [Screen::Puzzle, Screen::Editor] {
            go(&mut app, Screen::Menu);
            press(&mut app, pads[1], GamepadButton::DPadDown);
            go(&mut app, screen);
            assert_eq!(lone(&mut app), Some(pads[1]), "{screen:?}");
        }
        go(&mut app, Screen::Menu);
        press(&mut app, pads[1], GamepadButton::DPadDown);
        app.world_mut().resource_mut::<crate::app::Daily>().active = true;
        go(&mut app, Screen::Versus);
        assert_eq!(lone(&mut app), Some(pads[1]), "the daily");
        app.world_mut().resource_mut::<crate::app::Daily>().active = false;
        go(&mut app, Screen::Menu);
        tap(&mut app, KeyCode::ArrowDown);
        go(&mut app, Screen::Puzzle);
        assert_eq!(lone(&mut app), Some(pads[0]), "at the keyboard");
    }

    /// The daily is a versus round one person plays alone: on the way into
    /// it the claims go, as on the way into a puzzle, and its lone cursor
    /// follows the first pad rather than a friend's. A couch round keeps
    /// them.
    #[test]
    fn the_daily_after_a_couch_match_follows_the_first_pad() {
        let (mut app, pads) = setup_table(MatchConfig::default(), 2);
        press_start(&mut app, pads[1]);
        assert_eq!(told(&mut app), (2, 0, vec![1]));
        tap(&mut app, KeyCode::Enter);
        app.update();
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Versus
        );
        drives(&mut app, &[(1, pads[1])]);

        go(&mut app, Screen::Menu);
        press(&mut app, pads[0], GamepadButton::DPadDown);
        app.world_mut().resource_mut::<crate::app::Daily>().active = true;
        go(&mut app, Screen::Versus);
        assert_eq!(lone(&mut app), Some(pads[0]), "the daily's one player");
    }

    /// A lone pad at a keyboard-run setup where P1 is the only person is
    /// dealt P1, and is a friend's until its Start: the line offers the
    /// join its Start makes, taking the AI's chair. The pad that works the
    /// setup is P1's own, and its Start is not offered as a join.
    #[test]
    fn p1s_dealt_pad_is_offered_to_join() {
        let offered = |config: MatchConfig| {
            let (mut app, _) = setup_table(config, 1);
            app.update();
            let world = app.world_mut();
            let live: Vec<Entity> = world
                .query_filtered::<Entity, With<Gamepad>>()
                .iter(world)
                .collect();
            let players = app.world().resource::<MatchConfig>().human_seats();
            let settings = app.world().resource::<GameSettings>();
            app.world()
                .resource::<PadSeats>()
                .a_start_would_seat_someone(&live, settings, &players)
        };
        assert!(offered(MatchConfig::with_ai(2, 1)), "P1 against the AI");
        assert!(offered(MatchConfig::with_ai(2, 0)), "a seat for a friend");
        let (mut app, pads) = setup_table(MatchConfig::with_ai(2, 1), 1);
        into_the_setup_by(&mut app, Some(pads[0]));
        let world = app.world_mut();
        let live: Vec<Entity> = world
            .query_filtered::<Entity, With<Gamepad>>()
            .iter(world)
            .collect();
        let players = app.world().resource::<MatchConfig>().human_seats();
        let settings = app.world().resource::<GameSettings>();
        let seats = app.world().resource::<PadSeats>();
        assert!(
            !seats.a_start_would_seat_someone(&live, settings, &players),
            "the pad working the setup"
        );
    }

    /// P2 set in settings to play on controller 2, at a table of two: that
    /// pad's Start is P2 sitting down, which the footer names and the
    /// Enter that starts the match counts. It used to record nothing, and
    /// the match would not start against a P2 holding their own pad.
    #[test]
    fn a_seated_players_named_pad_joins_as_that_seat() {
        let (mut app, pads) = setup_table(MatchConfig::default(), 2);
        app.insert_resource(with([SeatInput::Auto, SeatInput::Pad(1)]));
        press_start(&mut app, pads[1]);
        assert_eq!(told(&mut app), (2, 0, vec![1]), "P2 is here");
        drives(&mut app, &[(1, pads[1])]);
        tap(&mut app, KeyCode::Enter);
        assert!(
            app.world().resource::<MatchConfig>().armed,
            "the match starts"
        );
    }

    /// P1 playing on controller 1, the only pad here, at a table of two:
    /// its Start is P1's own and seats nobody, so step 3 does not offer it
    /// to a friend, and P2's seat stays open for P2's keys.
    #[test]
    fn p1s_own_pad_is_not_offered_to_join() {
        use crate::app::match_setup::PlayedBy;
        let (mut app, pads) = setup_table(MatchConfig::default(), 1);
        app.insert_resource(with([SeatInput::Pad(0), SeatInput::Auto]));
        app.update();
        let offered = |app: &mut App| {
            let world = app.world_mut();
            let live: Vec<Entity> = world
                .query_filtered::<Entity, With<Gamepad>>()
                .iter(world)
                .collect();
            let settings = app.world().resource::<GameSettings>();
            let players = app.world().resource::<MatchConfig>().human_seats();
            app.world()
                .resource::<PadSeats>()
                .a_start_would_seat_someone(&live, settings, &players)
        };
        assert!(!offered(&mut app), "no pad to press Start on");
        // On step 1, where its Start does not start the match.
        use crate::app::match_setup::{MatchMenu, Step};
        app.world_mut().resource_mut::<MatchMenu>().step = Step::Game;
        press_start(&mut app, pads[0]);
        app.world_mut().resource_mut::<MatchMenu>().step = Step::Players;
        app.update();
        assert_eq!(told(&mut app), (2, 0, vec![]));
        assert_eq!(
            rows(&mut app),
            vec![dealt(1), PlayedBy::Open { keys: true }]
        );
        // A second pad plugged in is a friend's to press Start on.
        app.world_mut().spawn(Gamepad::default());
        app.update();
        assert!(offered(&mut app));
    }

    /// A friend in P2 when P2 is set to the keyboard alone in settings: the
    /// claim has no seat it may take, and P1 is the keyboard player's, so
    /// it drives nothing and is let go, and the friend's next Start seats
    /// them at P3. It used to steer P1 ("Controllers joined: P1"), with no
    /// opponent counted and a Start that did nothing.
    #[test]
    fn a_friend_pushed_out_of_p2_by_settings_sits_down_again() {
        let (mut app, pads) = setup_table(MatchConfig::with_ai(2, 1), 1);
        press_start(&mut app, pads[0]);
        assert_eq!(told(&mut app), (2, 0, vec![1]));
        p2_input_one_notch(&mut app);
        assert_eq!(
            app.world().resource::<GameSettings>().seat_input[1],
            SeatInput::Keys
        );
        assert_eq!(told(&mut app), (2, 0, vec![]), "P2 is the keyboard's");
        press_start(&mut app, pads[0]);
        assert_eq!(told(&mut app), (3, 0, vec![2]), "the friend at P3");
        drives(&mut app, &[(2, pads[0])]);
        assert_eq!(driving(&mut app, 0), None, "P1 on the keyboard");
    }

    /// The table dialled down under a friend's added seat: the friend's
    /// claim is let go, drives nobody (P1 least of all), and their next
    /// Start sits them down again. It used to stay a claim that steered P1,
    /// and a Start that did nothing.
    #[test]
    fn a_friend_dialled_off_the_table_sits_down_again() {
        let (mut app, pads) = setup_table(MatchConfig::default(), 2);
        press_start(&mut app, pads[0]);
        press_start(&mut app, pads[1]);
        assert_eq!(told(&mut app), (3, 0, vec![1, 2]));
        dial_players(&mut app, false);
        app.update();
        assert_eq!(told(&mut app), (2, 0, vec![1]), "the third seat went");
        drives(&mut app, &[(1, pads[0])]);
        assert_eq!(driving(&mut app, 0), None, "nor is P1 the friend's");
        press_start(&mut app, pads[1]);
        assert_eq!(told(&mut app), (3, 0, vec![1, 2]), "back at P3");
        drives(&mut app, &[(1, pads[0]), (2, pads[1])]);
    }

    /// A pad dropping out after the seats were set by hand gives back
    /// nothing below what step 3 says. A friend took the AI's chair, the
    /// AI was set back in the seat a second friend's Start had added, and
    /// the first friend leaving handed the AI a second chair: a table set
    /// for two people and one AI left P1 alone against two.
    #[test]
    fn a_table_set_by_the_dials_keeps_what_they_say() {
        let (mut app, pads) = setup_table(MatchConfig::with_ai(2, 1), 2);
        press_start(&mut app, pads[0]);
        press_start(&mut app, pads[1]);
        assert_eq!(told(&mut app), (3, 0, vec![1, 2]));
        turn_seat(&mut app, 2, true);
        app.update();
        assert_eq!(told(&mut app), (3, 1, vec![1]), "the AI has P3");
        app.world_mut().entity_mut(pads[0]).remove::<Gamepad>();
        app.update();
        assert_eq!(told(&mut app), (3, 1, vec![]), "as the dials say");
    }

    /// Mid-round, a friend's pad drops and another pad takes its claim and
    /// seat. The first pad back drives nothing, least of all P1 beside the
    /// keyboard, until it presses Start.
    #[test]
    fn a_pad_back_after_its_claim_was_taken_over_drives_nothing() {
        let (mut app, pads) = setup_table(MatchConfig::default(), 1);
        press_start(&mut app, pads[0]);
        tap(&mut app, KeyCode::Enter);
        app.update();
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Versus
        );
        app.world_mut().entity_mut(pads[0]).remove::<Gamepad>();
        let spare = app.world_mut().spawn(Gamepad::default()).id();
        app.update();
        drives(&mut app, &[(1, spare)]);
        app.world_mut()
            .entity_mut(pads[0])
            .insert(Gamepad::default());
        app.update();
        drives(&mut app, &[(1, spare)]);
        assert_eq!(driving(&mut app, 0), None, "P1 is the keyboard's");

        // Back on the setup screen, its Start seats it again.
        go(&mut app, Screen::MatchSetup);
        assert_eq!(driving(&mut app, 0), None, "still benched");
        press_start(&mut app, pads[0]);
        assert_eq!(told(&mut app), (3, 0, vec![1, 2]));
        drives(&mut app, &[(1, spare), (2, pads[0])]);
    }

    /// The d-pad presses W on the setup steps. With P2 rebound onto W, a
    /// pad's d-pad is still no P2 at the keys, nor is the keyboard's W,
    /// which walks the setup; P2's other keys still are.
    #[test]
    fn a_pads_bridged_key_does_not_seat_p2_at_the_keys() {
        use crate::app::binds::Action;
        let (mut app, pads) = setup_table(MatchConfig::default(), 1);
        {
            let mut settings = app.world_mut().resource_mut::<GameSettings>();
            settings.binds[0].set(Action::MoveUp, KeyCode::KeyT);
            settings.binds[1].set(Action::PlaceUp, KeyCode::KeyW);
        }
        into_the_setup_by(&mut app, Some(pads[0]));
        press(&mut app, pads[0], GamepadButton::DPadUp);
        assert!(!app.world().resource::<PadSeats>().p2_at_keys(), "the pad");
        tap(&mut app, KeyCode::KeyW);
        assert!(
            !app.world().resource::<PadSeats>().p2_at_keys(),
            "the menu's W"
        );
        tap(&mut app, KeyCode::KeyI);
        assert!(app.world().resource::<PadSeats>().p2_at_keys(), "P2's key");
    }

    /// The pilot's pad dies mid-round and another is plugged in: the new
    /// pad takes P1, and the friend's pad dealt P2 stays on P2. It used
    /// to swap the two for the rest of the series.
    #[test]
    fn a_new_pad_takes_over_a_dead_pilots_seat() {
        let (mut app, pads) = setup_table(MatchConfig::default(), 2);
        into_the_setup_by(&mut app, Some(pads[0]));
        tap(&mut app, KeyCode::Enter);
        app.update();
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Versus
        );
        assert_eq!(driving(&mut app, 0), Some(pads[0]));
        assert_eq!(driving(&mut app, 1), Some(pads[1]));
        app.world_mut().entity_mut(pads[0]).remove::<Gamepad>();
        let new = app.world_mut().spawn(Gamepad::default()).id();
        app.update();
        assert_eq!(driving(&mut app, 0), Some(new));
        assert_eq!(driving(&mut app, 1), Some(pads[1]));
    }

    /// The table is also an online host's terms: a pad that drops out in
    /// the lobby leaves them alone, and its seat is given back the next
    /// time the setup screen opens.
    #[test]
    fn a_pad_gone_in_the_lobby_changes_nothing_until_the_setup_screen() {
        let (mut app, pads) = setup_table(MatchConfig::with_ai(4, 2), 2);
        press_start(&mut app, pads[0]);
        press_start(&mut app, pads[1]);
        assert_eq!(told(&mut app), (4, 1, vec![1, 2]));
        // Hosting now, with the AI dialled in the lobby.
        go(&mut app, Screen::Lobby);
        app.world_mut().resource_mut::<MatchConfig>().lobby_bots = 1;
        app.world_mut().entity_mut(pads[1]).remove::<Gamepad>();
        app.update();
        let config = app.world().resource::<MatchConfig>();
        assert_eq!(
            (config.seats, config.ai_seats(), config.lobby_bots),
            (4, 1, 1),
            "the terms stand"
        );
        go(&mut app, Screen::MatchSetup);
        let config = app.world().resource::<MatchConfig>();
        assert_eq!(config.ai_seats(), 2, "owed, and given back to the AI there");
        assert!(app.world().resource::<PadSeats>().chairs.0.is_empty());
    }

    /// A friend's Start pressed while the game or the map is still being
    /// set is heard all the same: the friend is seated by the time step 3
    /// opens, on the pad they pressed it on. The pad nobody claimed is
    /// dealt the seat left over, P1's.
    #[test]
    fn a_start_on_an_early_step_is_seated_on_step_three() {
        use crate::app::match_setup::{MatchMenu, PlayedBy, Step};
        for step in [Step::Game, Step::Map] {
            let (mut app, pads) = setup_table(MatchConfig::with_ai(3, 1), 2);
            app.world_mut().resource_mut::<MatchMenu>().step = step;
            press_start(&mut app, pads[1]);
            assert_eq!(told(&mut app), (3, 1, vec![1]), "{step:?}");
            app.world_mut().resource_mut::<MatchMenu>().step = Step::Players;
            app.update();
            assert_eq!(
                rows(&mut app),
                vec![
                    dealt(1),
                    joined(2),
                    PlayedBy::Ai(crate::sim::BotLevel::Normal),
                ],
                "{step:?}: controller 2, the second plugged in"
            );
            drives(&mut app, &[(1, pads[1])]);
        }
    }

    /// Start match with seats nobody sat down in: each is filled with the
    /// AI at step 1's level, the row having said so, and the people who
    /// did sit down keep their seats and pads.
    #[test]
    fn open_seats_are_filled_with_the_ai_at_start() {
        use crate::app::SeatController;
        use crate::app::match_setup::PlayedBy;
        let mut config = MatchConfig {
            seats: 4,
            ..MatchConfig::default()
        };
        config.ai_level = crate::sim::BotLevel::Hard;
        let (mut app, pads) = setup_table(config, 1);
        press_start(&mut app, pads[0]);
        assert_eq!(
            rows(&mut app),
            vec![
                PlayedBy::Keys(0),
                joined(1),
                PlayedBy::Open { keys: false },
                PlayedBy::Open { keys: false },
            ]
        );
        tap(&mut app, KeyCode::Enter);
        let config = app.world().resource::<MatchConfig>();
        assert!(config.armed, "Start match works with seats open");
        assert_eq!(
            config.controllers[..4],
            [
                SeatController::Local,
                SeatController::Local,
                SeatController::Ai(crate::sim::BotLevel::Hard),
                SeatController::Ai(crate::sim::BotLevel::Hard),
            ]
        );
        drives(&mut app, &[(1, pads[0])]);
    }

    /// Three seats, two pads, one friend presses Start: the other pad is
    /// dealt P3, and P3 is that pad's seat, on step 3 and in the round. The
    /// AI does not sit there, and the pad does not fall through to steer
    /// P1 beside the keyboard.
    #[test]
    fn a_pad_dealt_a_seat_plays_it_and_never_p1() {
        use crate::app::SeatController;
        use crate::app::match_setup::PlayedBy;
        let (mut app, pads) = setup_table(MatchConfig::with_ai(3, 0), 2);
        press_start(&mut app, pads[0]);
        assert_eq!(
            rows(&mut app),
            vec![PlayedBy::Keys(0), joined(1), dealt(2)],
            "the friend who pressed Start joined, the other pad was dealt"
        );
        tap(&mut app, KeyCode::Enter);
        let config = app.world().resource::<MatchConfig>();
        assert!(config.armed);
        assert_eq!(config.controllers[..3], [SeatController::Local; 3]);
        drives(&mut app, &[(1, pads[0]), (2, pads[1])]);
        assert_eq!(driving(&mut app, 0), None, "P1 is the keyboard's");
    }

    /// The keyboard and one pad, the setup worked at the keyboard, and
    /// nobody presses Start: the pad is dealt P2 and plays it, so two
    /// people play with no setup at all. A key on steps 1 and 2 puts the
    /// keyboard at P1 too, whatever the menu was worked with.
    #[test]
    fn the_keyboard_and_a_pad_play_two_without_a_start() {
        use crate::app::SeatController;
        use crate::app::match_setup::PlayedBy;
        let (mut app, pads) = setup_table(MatchConfig::default(), 1);
        into_the_setup_by(&mut app, None);
        assert_eq!(rows(&mut app), vec![PlayedBy::Keys(0), dealt(1)]);
        tap(&mut app, KeyCode::Enter);
        let config = app.world().resource::<MatchConfig>();
        assert!(config.armed);
        assert_eq!(config.controller(1), SeatController::Local, "not the AI");
        drives(&mut app, &[(1, pads[0])]);
        assert_eq!(driving(&mut app, 0), None, "P1 is the keyboard's");

        // A key on step 3 moves nobody: the table it shows is the one
        // launched. On step 1 it puts the keyboard at P1.
        let (mut app, pads) = setup_table(MatchConfig::default(), 1);
        into_the_setup_by(&mut app, Some(pads[0]));
        tap(&mut app, KeyCode::KeyS);
        let pad_at_p1 = vec![dealt(1), PlayedBy::Open { keys: true }];
        assert_eq!(rows(&mut app), pad_at_p1);
        use crate::app::match_setup::{MatchMenu, Step};
        app.world_mut().resource_mut::<MatchMenu>().step = Step::Game;
        tap(&mut app, KeyCode::KeyS);
        assert_eq!(rows(&mut app), vec![PlayedBy::Keys(0), dealt(1)]);
    }

    /// A Steam Deck: one pad, the menu and setup worked on it, nobody
    /// presses Start. The pad is P1's, as step 3 says, and P2 is open, so
    /// the match starts against the AI with the pad steering P1, the seat
    /// this machine's trophies are counted for. It used to deal the pad
    /// P2, leave P1 idle at a keyboard nobody held, and put no AI in.
    /// The pad's own Start seats nobody and starts the match, where it
    /// did nothing at all, and P2's keys still take P2.
    #[test]
    fn a_lone_pad_working_the_setup_plays_p1_against_the_ai() {
        use crate::app::SeatController;
        use crate::app::match_setup::PlayedBy;
        let (mut app, pads) = setup_table(MatchConfig::default(), 1);
        into_the_setup_by(&mut app, Some(pads[0]));
        assert_eq!(
            rows(&mut app),
            vec![dealt(1), PlayedBy::Open { keys: true }]
        );
        // The pad's Start on step 3 seats nobody: it starts the match.
        press_start(&mut app, pads[0]);
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Versus
        );
        let config = app.world().resource::<MatchConfig>();
        assert_eq!(
            config.controllers[..2],
            [SeatController::Local, SeatController::Ai(config.ai_level)],
            "P1 a person (the seat trophies count), P2 the AI"
        );
        assert_eq!(driving(&mut app, 0), Some(pads[0]), "the pad steers P1");

        // P2's keys sit a second player down at the keyboard beside it.
        let (mut app, pads) = setup_table(MatchConfig::default(), 1);
        into_the_setup_by(&mut app, Some(pads[0]));
        tap(&mut app, KeyCode::KeyI);
        assert_eq!(rows(&mut app), vec![dealt(1), PlayedBy::Keys(1)]);
    }

    /// Two pads and nobody at Start, the setup worked on one of them: that
    /// one is P1, the other P2, whichever was plugged in first.
    #[test]
    fn the_pad_working_the_setup_is_p1_and_the_other_p2() {
        for (pilot, other) in [(1, 0), (0, 1)] {
            let (mut app, pads) = setup_table(MatchConfig::default(), 2);
            into_the_setup_by(&mut app, Some(pads[pilot]));
            assert_eq!(
                rows(&mut app),
                vec![dealt(pilot as u8 + 1), dealt(other as u8 + 1)]
            );
            assert_eq!(driving(&mut app, 0), Some(pads[pilot]));
            assert_eq!(driving(&mut app, 1), Some(pads[other]));
            // The friend's Start keeps them at P2, and P1 on the pilot.
            press_start(&mut app, pads[other]);
            assert_eq!(told(&mut app), (2, 0, vec![1]));
            assert_eq!(driving(&mut app, 0), Some(pads[pilot]));
            assert_eq!(driving(&mut app, 1), Some(pads[other]));
        }
    }

    /// P2's settings name controller 2, P1 is dealt controller 1, and P2
    /// presses Start on their own pad: a confirm that moves nobody. P1
    /// still plays controller 1; it used to drive nothing once P2 had
    /// pressed it.
    #[test]
    fn a_named_pads_start_leaves_p1_its_pad() {
        let (mut app, pads) = setup_table(MatchConfig::default(), 2);
        app.insert_resource(with([SeatInput::Auto, SeatInput::Pad(1)]));
        app.update();
        assert_eq!(driving(&mut app, 0), Some(pads[0]));
        assert_eq!(driving(&mut app, 1), Some(pads[1]));
        press_start(&mut app, pads[1]);
        assert_eq!(told(&mut app), (2, 0, vec![1]), "P2 is here");
        assert_eq!(driving(&mut app, 0), Some(pads[0]), "P1 keeps its pad");
        assert_eq!(driving(&mut app, 1), Some(pads[1]));
    }

    /// Two pads dealt P2 and P1, and P2's holder presses Start: P2 keeps
    /// that pad and P1 keeps the other, which no claim needed.
    #[test]
    fn a_dealt_pads_start_leaves_p1_its_pad() {
        let (mut app, pads) = setup_table(MatchConfig::default(), 2);
        assert_eq!(driving(&mut app, 1), Some(pads[0]));
        assert_eq!(driving(&mut app, 0), Some(pads[1]));
        press_start(&mut app, pads[0]);
        assert_eq!(told(&mut app), (2, 0, vec![1]));
        assert_eq!(driving(&mut app, 1), Some(pads[0]));
        assert_eq!(driving(&mut app, 0), Some(pads[1]), "P1 keeps its pad");
    }

    /// Three seats and one pad nobody claimed, and P2 presses one of their
    /// own keys: P2 is the keyboard's second player, so the pad is dealt
    /// P3, step 3 says so, and the match starts with three people and no
    /// AI. It used to name the pad on P2, under the keys, leave P3 to the
    /// AI and the pad's holder with no seat.
    #[test]
    fn p2s_keys_send_the_pad_to_p3() {
        use crate::app::SeatController;
        use crate::app::match_setup::PlayedBy;
        let (mut app, pads) = setup_table(MatchConfig::with_ai(3, 0), 1);
        tap(&mut app, KeyCode::KeyI);
        assert_eq!(
            rows(&mut app),
            vec![PlayedBy::Keys(0), PlayedBy::Keys(1), dealt(1)]
        );
        tap(&mut app, KeyCode::Enter);
        let config = app.world().resource::<MatchConfig>();
        assert!(config.armed);
        assert_eq!(config.controllers[..3], [SeatController::Local; 3]);
        assert_eq!(driving(&mut app, 1), None, "P2 on the keys alone");
        assert_eq!(driving(&mut app, 2), Some(pads[0]));
    }

    /// Two pads and nobody at Start: the second is dealt P1, and step 3
    /// says so rather than naming the keys.
    #[test]
    fn two_pads_play_two_and_step_three_names_p1s() {
        let (mut app, pads) = setup_table(MatchConfig::default(), 2);
        assert_eq!(rows(&mut app), vec![dealt(2), dealt(1)]);
        drives(&mut app, &[(1, pads[0])]);
        assert_eq!(driving(&mut app, 0), Some(pads[1]));
    }

    /// The second keyboard seat is taken by pressing one of P2's keys, and
    /// then the match starts with P2 at the keys. Without it, P2's seat is
    /// open and the AI takes it, so a table of two always has a match.
    #[test]
    fn p2s_keys_take_the_second_seat() {
        use crate::app::SeatController;
        use crate::app::match_setup::PlayedBy;
        let (mut app, _) = setup_table(MatchConfig::default(), 0);
        assert_eq!(rows(&mut app)[1], PlayedBy::Open { keys: true });
        tap(&mut app, KeyCode::KeyI);
        assert_eq!(rows(&mut app)[1], PlayedBy::Keys(1));
        tap(&mut app, KeyCode::Enter);
        let config = app.world().resource::<MatchConfig>();
        assert!(config.armed);
        assert_eq!(config.controller(1), SeatController::Local, "P2 plays");

        let (mut app, _) = setup_table(MatchConfig::default(), 0);
        tap(&mut app, KeyCode::Enter);
        let config = app.world().resource::<MatchConfig>();
        assert!(config.armed, "nobody at P2's keys: the AI plays P2");
        assert_eq!(config.controller(1), SeatController::Ai(config.ai_level));
    }

    /// Turning one seat over to the AI on step 3 moves nobody else: the
    /// friends above and below it keep their seats, and the friend whose
    /// seat it was gets up, to sit down again with Start when the seat is
    /// a person's again. Counted in the order they came, every claim above
    /// it stepped down a seat.
    #[test]
    fn turning_a_seat_to_the_ai_moves_nobody_else() {
        let (mut app, pads) = setup_table(
            MatchConfig {
                seats: 4,
                ..MatchConfig::default()
            },
            3,
        );
        for &pad in &pads {
            press_start(&mut app, pad);
        }
        assert_eq!(told(&mut app), (4, 0, vec![1, 2, 3]));
        turn_seat(&mut app, 2, true);
        app.update();
        assert_eq!(told(&mut app), (4, 1, vec![1, 3]), "P2 and P4 stay put");
        drives(&mut app, &[(1, pads[0]), (3, pads[2])]);
        turn_seat(&mut app, 2, false);
        app.update();
        assert_eq!(told(&mut app), (4, 0, vec![1, 3]), "P3 open again");
        press_start(&mut app, pads[1]);
        assert_eq!(told(&mut app), (4, 0, vec![1, 2, 3]));
        drives(&mut app, &[(1, pads[0]), (2, pads[1]), (3, pads[2])]);
    }

    /// How a test starts the match on step 3.
    #[derive(Clone, Copy, Debug)]
    enum Go {
        Enter,
        Pad(usize, GamepadButton),
    }

    /// The table step 3 shows is the table the match starts with: right
    /// after the start, each seat step 3 named a pad for is driven by that
    /// pad, each keyboard seat by none, each open seat is the AI's, and
    /// each seat goes by the name step 3 gave it. For the keyboard's Enter
    /// and a pad's Start and A, at tables of two and of four in pairs, with
    /// one to three pads, the setup opened by either hands, P2's row left
    /// to the rule, set to the keys (and pressed) or naming controller 2,
    /// and with a partner picked first or not. A Start that seats a friend
    /// first is pressed again.
    #[test]
    fn the_table_step_three_shows_is_the_table_launched() {
        use crate::app::match_setup::PlayedBy;
        use crate::app::teams::TeamMode;
        let buttons = [GamepadButton::Start, GamepadButton::South];
        let four = MatchConfig {
            seats: 4,
            ..MatchConfig::default()
        };
        let tables = [
            (MatchConfig::default(), TeamMode::Solo),
            (MatchConfig::with_ai(2, 1), TeamMode::Solo),
            (four, TeamMode::Pairs),
            (MatchConfig::with_ai(4, 1), TeamMode::Pairs),
        ];
        let p2_rows = [SeatInput::Auto, SeatInput::Keys, SeatInput::Pad(1)];
        let mut cases = 0;
        for (config, mode) in tables {
            for pads in 1..=3 {
                for p2 in p2_rows {
                    if p2 == SeatInput::Pad(1) && pads < 2 {
                        continue;
                    }
                    for pick in [false, true] {
                        if pick && mode == TeamMode::Solo {
                            continue;
                        }
                        for opener in [None, Some(0), Some(pads - 1)] {
                            let mut goes = vec![Go::Enter];
                            for pad in 0..pads {
                                goes.extend(buttons.map(|button| Go::Pad(pad, button)));
                            }
                            for go in goes {
                                let case = format!(
                                    "{} seats {mode:?}, {pads} pads, P2 {p2:?}, pick {pick}, \
                                     opener {opener:?}, {go:?}",
                                    config.seats
                                );
                                launched_as_shown(
                                    &case,
                                    (config.clone(), mode),
                                    (pads, p2),
                                    (pick, opener, go),
                                );
                                cases += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(cases > 500, "{cases} cases");

        /// One case of the test above.
        fn launched_as_shown(
            case: &str,
            (config, mode): (MatchConfig, TeamMode),
            (pads, p2): (usize, SeatInput),
            (pick, opener, go): (bool, Option<usize>, Go),
        ) {
            let (mut app, plugged) = teams_table(config, mode, pads);
            app.world_mut().resource_mut::<GameSettings>().seat_input[1] = p2;
            into_the_setup_by(&mut app, opener.map(|at| plugged[at]));
            // The friends sit down, all but the last pad, which is dealt.
            for &pad in &plugged[..pads - 1] {
                press_start(&mut app, pad);
            }
            if p2 == SeatInput::Keys {
                tap(&mut app, KeyCode::KeyI);
                app.update();
            }
            if pick {
                tap(&mut app, KeyCode::KeyP);
                app.update();
            }
            let mut shown = rows(&mut app);
            let mut named = names(&app);
            for _ in 0..2 {
                shown = rows(&mut app);
                named = names(&app);
                match go {
                    Go::Enter => tap(&mut app, KeyCode::Enter),
                    Go::Pad(at, button) => press(&mut app, plugged[at], button),
                }
                app.update();
                if app.world().resource::<MatchConfig>().armed {
                    break;
                }
            }
            assert!(
                app.world().resource::<MatchConfig>().armed,
                "{case}: started"
            );
            assert_eq!(names(&app), named, "{case}: the names step 3 gave");
            for (seat, by) in shown.iter().enumerate() {
                let seat = seat as u8;
                let controller = app.world().resource::<MatchConfig>().controller(seat);
                match *by {
                    PlayedBy::Pad { number, .. } => {
                        assert_eq!(
                            driving(&mut app, seat),
                            Some(plugged[usize::from(number) - 1]),
                            "{case}: P{} was shown on controller {number}: {shown:?}",
                            seat + 1
                        );
                    }
                    PlayedBy::Keys(_) => {
                        assert_eq!(controller, SeatController::Local, "{case}");
                        assert_eq!(
                            driving(&mut app, seat),
                            None,
                            "{case}: P{} was shown on the keys: {shown:?}",
                            seat + 1
                        );
                    }
                    PlayedBy::Open { .. } | PlayedBy::Ai(_) => assert!(
                        controller.ai().is_some(),
                        "{case}: P{} was the AI's: {shown:?}",
                        seat + 1
                    ),
                    PlayedBy::Bot => {
                        assert_eq!(controller, SeatController::Bot, "{case}")
                    }
                }
            }
        }
    }

    /// A table of `config` in `mode`, the four friends named, on step 3.
    fn teams_table(
        config: MatchConfig,
        mode: crate::app::teams::TeamMode,
        pads: usize,
    ) -> (App, Vec<Entity>) {
        let (mut app, plugged) = setup_table(config, pads);
        let mut settings = app.world_mut().resource_mut::<GameSettings>();
        settings.team_mode = mode;
        for (seat, name) in ["Anna", "Bo", "Cleo", "Dan", "Eve", "Finn"]
            .iter()
            .enumerate()
        {
            settings.names[seat] = (*name).to_string();
        }
        app.update();
        (app, plugged)
    }

    /// The names the seats go by at the table, partner picks and all.
    fn names(app: &App) -> Vec<String> {
        let config = app.world().resource::<MatchConfig>();
        let saved = &app.world().resource::<GameSettings>().names;
        config.seat_names(saved)[..usize::from(config.seats)].to_vec()
    }

    /// Pairs at four, three friends seated on pads: P1 picks each partner
    /// in turn with P, the one picked moving to seat 2 with their name and
    /// their pad, P1 never moving; the match starts with each pad on the
    /// seat step 3 showed it on.
    #[test]
    fn p_picks_p1s_partner_and_the_pads_follow() {
        use crate::app::teams::TeamMode;
        let (mut app, pads) = teams_table(
            MatchConfig {
                seats: 4,
                ..MatchConfig::default()
            },
            TeamMode::Pairs,
            3,
        );
        for &pad in &pads {
            press_start(&mut app, pad);
        }
        drives(&mut app, &[(1, pads[0]), (2, pads[1]), (3, pads[2])]);
        let mut partners = Vec::new();
        for _ in 0..3 {
            tap(&mut app, KeyCode::KeyP);
            partners.push(names(&app)[1].clone());
            assert_eq!(names(&app)[0], "Anna", "P1 stays");
        }
        assert_eq!(partners, ["Cleo", "Dan", "Bo"], "each friend in turn");
        tap(&mut app, KeyCode::KeyP);
        assert_eq!(names(&app), ["Anna", "Cleo", "Dan", "Bo"]);
        drives(&mut app, &[(1, pads[1]), (2, pads[2]), (3, pads[0])]);
        let shown = rows(&mut app);
        tap(&mut app, KeyCode::Enter);
        assert!(app.world().resource::<MatchConfig>().armed);
        assert_eq!(shown[1], joined(2), "Cleo's pad, controller 2");
        drives(&mut app, &[(1, pads[1]), (2, pads[2]), (3, pads[0])]);
    }

    /// The AI is a partner like anybody: P1 with a friend on a dealt pad
    /// and two AI seats picks the AI, which takes seat 2 while the friend
    /// and their pad go to seat 3; the AI seats stay the AI's.
    #[test]
    fn the_ai_can_be_picked_as_a_partner() {
        use crate::app::match_setup::PlayedBy;
        use crate::app::teams::TeamMode;
        let (mut app, pads) = teams_table(MatchConfig::with_ai(4, 2), TeamMode::Pairs, 1);
        into_the_setup_by(&mut app, None);
        assert_eq!(rows(&mut app)[1], dealt(1));
        tap(&mut app, KeyCode::KeyP);
        let level = app.world().resource::<MatchConfig>().ai_level;
        assert_eq!(
            rows(&mut app),
            vec![
                PlayedBy::Keys(0),
                PlayedBy::Ai(level),
                joined(1),
                PlayedBy::Ai(level)
            ]
        );
        assert_eq!(names(&app)[2], "Bo", "the friend's name went with them");
        tap(&mut app, KeyCode::Enter);
        drives(&mut app, &[(2, pads[0])]);
        let config = app.world().resource::<MatchConfig>();
        assert_eq!(config.human_seats(), vec![0, 2]);
    }

    /// The keyboard's second player is wired to seat 2: at four in pairs
    /// P1 and P2 stay a team, P does nothing, and the line under the seats
    /// says why, for the seat whose pick it holds back.
    #[test]
    fn the_keyboards_p2_is_never_moved() {
        use crate::app::match_setup::{MatchMenu, p2_pin, partner_line};
        use crate::app::teams::TeamMode;
        let (mut app, _) = teams_table(
            MatchConfig {
                seats: 4,
                ..MatchConfig::default()
            },
            TeamMode::Pairs,
            0,
        );
        tap(&mut app, KeyCode::KeyI);
        let before = names(&app);
        tap(&mut app, KeyCode::KeyP);
        assert_eq!(names(&app), before, "nobody moved");
        let line = |app: &App, seat: u8| {
            let config = app.world().resource::<MatchConfig>();
            let settings = app.world().resource::<GameSettings>();
            let seats = app.world().resource::<PadSeats>();
            let pin = p2_pin(config, settings, &[], seats.p2_at_keys());
            partner_line(
                &crate::app::i18n::EN,
                crate::app::gamepad::LastDevice::Keyboard,
                (config, settings),
                seat,
                pin,
            )
        };
        assert_eq!(
            line(&app, 0).as_deref(),
            Some("Bo plays on the keyboard, so their seat stays put")
        );
        app.world_mut().resource_mut::<MatchMenu>().seat = 2;
        tap(&mut app, KeyCode::KeyP);
        assert_eq!(names(&app), before, "Cleo cannot have P1 or P2 either");
        // At six, the other four still pair up as they like.
        let (mut app, _) = teams_table(MatchConfig::with_ai(6, 0), TeamMode::Pairs, 0);
        tap(&mut app, KeyCode::KeyI);
        app.world_mut().resource_mut::<MatchMenu>().seat = 2;
        tap(&mut app, KeyCode::KeyP);
        assert_eq!(names(&app), ["Anna", "Bo", "Cleo", "Eve", "Dan", "Finn"]);
    }

    /// Trios: the rows are listed team by team and W/S walk them in that
    /// order; a mode the table does not fit says what it waits for, and P
    /// moves nobody.
    #[test]
    fn the_rows_go_team_by_team() {
        use crate::app::match_setup::{MatchMenu, partner_line};
        use crate::app::teams::TeamMode;
        let (mut app, _) = teams_table(MatchConfig::with_ai(6, 5), TeamMode::Trios, 0);
        let mut walked = Vec::new();
        for _ in 0..6 {
            tap(&mut app, KeyCode::KeyS);
            walked.push(app.world().resource::<MatchMenu>().seat);
        }
        assert_eq!(walked, [2, 4, 1, 3, 5, 0]);
        let (mut app, _) = teams_table(MatchConfig::with_ai(5, 4), TeamMode::Trios, 0);
        let before = names(&app);
        tap(&mut app, KeyCode::KeyP);
        assert_eq!(names(&app), before);
        let config = app.world().resource::<MatchConfig>();
        let settings = app.world().resource::<GameSettings>();
        let said = partner_line(
            &crate::app::i18n::EN,
            crate::app::gamepad::LastDevice::Keyboard,
            (config, settings),
            0,
            None,
        );
        assert_eq!(said.as_deref(), Some("trios (free-for-all until 6)"));
    }

    /// P2's settings row names the keys or a controller, and P2's seat is
    /// turned over to the AI: the seat is still wired to P2, so no pick
    /// moves a friend into it, where their pad would drive nothing, and
    /// the line says the seat is kept.
    #[test]
    fn a_seat_wired_to_p2_stays_put_under_the_ai() {
        use crate::app::match_setup::{MatchMenu, p2_pin, partner_line};
        use crate::app::teams::TeamMode;
        for (wired, said) in [
            (
                SeatInput::Keys,
                "Bo's seat is kept for the keyboard, so it stays put",
            ),
            (
                SeatInput::Pad(1),
                "Bo's seat is kept for controller 2 set in Settings, so it stays put",
            ),
        ] {
            let (mut app, pads) = teams_table(
                MatchConfig {
                    seats: 4,
                    ..MatchConfig::default()
                },
                TeamMode::Pairs,
                2,
            );
            app.world_mut().resource_mut::<GameSettings>().seat_input[1] = wired;
            let level = app.world().resource::<MatchConfig>().ai_level;
            app.world_mut().resource_mut::<MatchConfig>().controllers[1] =
                SeatController::Ai(level);
            app.update();
            press_start(&mut app, pads[0]);
            app.update();
            assert_eq!(driving(&mut app, 2), Some(pads[0]), "{wired:?}: Cleo in P3");
            app.world_mut().resource_mut::<MatchMenu>().seat = 0;
            tap(&mut app, KeyCode::KeyP);
            app.update();
            let config = app.world().resource::<MatchConfig>();
            assert_eq!(config.controller(1), SeatController::Ai(level), "{wired:?}");
            assert_eq!(driving(&mut app, 2), Some(pads[0]), "{wired:?}: Cleo stays");
            let world = app.world_mut();
            let live: Vec<Entity> = world
                .query_filtered::<Entity, With<Gamepad>>()
                .iter(world)
                .collect();
            let config = app.world().resource::<MatchConfig>();
            let settings = app.world().resource::<GameSettings>();
            let seats = app.world().resource::<PadSeats>();
            let pin = p2_pin(
                config,
                settings,
                &seats.seat_pads(&live, settings, &config.human_seats()),
                seats.p2_at_keys(),
            );
            let line = partner_line(
                &crate::app::i18n::EN,
                crate::app::gamepad::LastDevice::Keyboard,
                (config, settings),
                0,
                pin,
            );
            assert_eq!(line.as_deref(), Some(said));
        }
    }

    /// P2's row left to whichever pad, P2's keys not pressed yet, and P2's
    /// seat open: it is kept for the keyboard's second player, so P moves
    /// no friend into it, where P2's keys would push them out.
    #[test]
    fn an_open_p2_seat_is_kept_for_the_keyboard() {
        use crate::app::match_setup::{MatchMenu, PlayedBy};
        use crate::app::teams::TeamMode;
        let (mut app, pads) = teams_table(
            MatchConfig {
                seats: 4,
                ..MatchConfig::default()
            },
            TeamMode::Pairs,
            1,
        );
        into_the_setup_by(&mut app, None);
        let level = app.world().resource::<MatchConfig>().ai_level;
        app.world_mut().resource_mut::<MatchConfig>().controllers[1] = SeatController::Ai(level);
        app.update();
        press_start(&mut app, pads[0]);
        app.world_mut().resource_mut::<MatchConfig>().controllers[1] = SeatController::Local;
        app.update();
        assert_eq!(rows(&mut app)[1], PlayedBy::Open { keys: true });
        assert_eq!(driving(&mut app, 2), Some(pads[0]), "Cleo in P3");
        app.world_mut().resource_mut::<MatchMenu>().seat = 0;
        tap(&mut app, KeyCode::KeyP);
        app.update();
        assert_eq!(rows(&mut app)[1], PlayedBy::Open { keys: true }, "kept");
        tap(&mut app, KeyCode::KeyI);
        app.update();
        assert_eq!(rows(&mut app)[1], PlayedBy::Keys(1));
        assert_eq!(driving(&mut app, 2), Some(pads[0]), "Cleo still plays");
    }

    /// A pick moves people for this match only: the names saved in
    /// settings stay on their own rows, and the seats go by the names of
    /// the people picked into them.
    #[test]
    fn a_pick_leaves_the_saved_names_alone() {
        use crate::app::teams::TeamMode;
        let (mut app, pads) = teams_table(
            MatchConfig {
                seats: 4,
                ..MatchConfig::default()
            },
            TeamMode::Pairs,
            3,
        );
        for &pad in &pads {
            press_start(&mut app, pad);
        }
        tap(&mut app, KeyCode::KeyP);
        app.update();
        let saved = &app.world().resource::<GameSettings>().names;
        assert_eq!(saved[..4], ["Anna", "Bo", "Cleo", "Dan"], "saved as typed");
        assert_eq!(names(&app), ["Anna", "Cleo", "Bo", "Dan"], "the table's");
    }

    /// A friend's pad picks partners with Y, through the bridge, as P does
    /// at the keyboard: the seat under the cursor gets its next partner.
    #[test]
    fn a_pads_y_picks_partners_through_the_bridge() {
        use crate::app::teams::TeamMode;
        let (mut app, pads) = teams_table(
            MatchConfig {
                seats: 4,
                ..MatchConfig::default()
            },
            TeamMode::Pairs,
            3,
        );
        for &pad in &pads {
            press_start(&mut app, pad);
        }
        app.update();
        press(&mut app, pads[1], GamepadButton::North);
        app.update();
        assert_eq!(names(&app), ["Anna", "Cleo", "Bo", "Dan"], "Y picked");
        drives(&mut app, &[(1, pads[1]), (2, pads[0]), (3, pads[2])]);
    }
}
