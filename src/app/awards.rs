//! The round's awards: a line or two under the standings for something
//! other than winning. Who came back from furthest behind, whose lure paid
//! most, who planted the most arrows, who lost the most.
//!
//! Taken tick by tick rather than from the [`SimEvent`] stream, because the
//! events are gathered a frame at a time and frames fall differently on
//! every machine: a crab banked on the tick a lure ran out would count
//! towards it on one screen and not on the next, and a room full of
//! friends reads every one of those screens. A tick is the same tick
//! everywhere, so [`RoundTally::observe`] runs beside each `Board::tick`
//! and every peer, and every replay of the round, lands on the same card.
//!
//! [`SimEvent`]: crate::app::sim_events::SimEvent

use crate::app::i18n::{Tr, fill};
use crate::app::teams::TeamMode;
use crate::sim::{Board, MAX_PLAYERS, PlayerAction, PlayerId};
use bevy::prelude::*;

/// How far behind the leader a winner has to have been for it to be a
/// comeback rather than the ordinary back and forth of a round. A golden
/// crab is fifty, so this is less than one, and more than a handful of
/// commons.
pub const COMEBACK_MIN: u32 = 20;

/// What one seat did over the round.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct SeatTally {
    /// Banked score lost: raids, and the left claw under a claw call.
    pub lost: u32,
    /// Arrows that went into the sand.
    pub placed: u32,
    /// The most gained over one of this seat's own lures.
    pub best_lure: u32,
    /// The furthest this seat trailed the leader at any tick.
    pub worst_deficit: u32,
}

/// The board as it stood before or after one tick, as much of it as the
/// tally reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Reading {
    pub ticks: u64,
    pub scores: [u32; MAX_PLAYERS],
    /// Whose lure is running.
    pub lure: Option<PlayerId>,
}

impl Reading {
    pub fn of(board: &Board) -> Reading {
        Reading {
            ticks: board.ticks(),
            scores: *board.scores(),
            lure: board.lure().map(|(owner, _)| owner),
        }
    }
}

/// The running tally of a versus round.
#[derive(Resource, Clone, Default, PartialEq, Eq, Debug)]
pub struct RoundTally {
    pub seats: [SeatTally; MAX_PLAYERS],
    /// The lure in progress: whose, and what it has brought them so far.
    lure: Option<(PlayerId, u32)>,
    /// The tick the next observation should start from. A board that
    /// arrives anywhere else was not watched from the start.
    next_tick: u64,
    /// Every tick of this round, from its first, has been seen. A round
    /// resumed from a save or a pasted code starts mid-play, and so does
    /// the tally's view of it: half a round's awards are not awards.
    whole: bool,
}

impl RoundTally {
    /// Count one tick: `before` read just ahead of `board.tick(actions)`,
    /// `board` as that left it.
    pub fn observe(
        &mut self,
        before: Reading,
        board: &Board,
        actions: &[PlayerAction; MAX_PLAYERS],
    ) {
        let placed = std::array::from_fn(|seat| match actions[seat] {
            PlayerAction::Place { x, y, dir } => board.signpost_at(x, y).is_some_and(|post| {
                usize::from(post.owner) == seat && post.dir == dir && post.placed >= before.ticks
            }),
            PlayerAction::None | PlayerAction::Remove { .. } | PlayerAction::CallEvent(_) => false,
        });
        self.fold(before, Reading::of(board), placed);
    }

    /// The arithmetic of [`RoundTally::observe`], on readings alone.
    pub fn fold(&mut self, before: Reading, after: Reading, placed: [bool; MAX_PLAYERS]) {
        if before.ticks == 0 {
            // A round's first tick: whatever was counted before belongs to
            // some other round.
            *self = RoundTally {
                whole: true,
                ..RoundTally::default()
            };
        } else if before.ticks != self.next_tick {
            self.whole = false;
        }
        self.next_tick = after.ticks;

        // A lure is credited by whose it was when the tick began: the
        // tick that ends it still belongs to it.
        let running = self.lure.map(|(owner, _)| owner);
        if running != before.lure {
            self.close_lure();
            self.lure = before.lure.map(|owner| (owner, 0));
        }
        for (seat, tally) in self.seats.iter_mut().enumerate() {
            let (was, now) = (before.scores[seat], after.scores[seat]);
            tally.lost += was.saturating_sub(now);
            tally.placed += u32::from(placed[seat]);
            if let Some((owner, gained)) = &mut self.lure
                && usize::from(*owner) == seat
            {
                *gained += now.saturating_sub(was);
            }
        }
        let leader = after.scores.iter().copied().max().unwrap_or(0);
        for (tally, &score) in self.seats.iter_mut().zip(&after.scores) {
            tally.worst_deficit = tally.worst_deficit.max(leader - score);
        }
    }

    fn close_lure(&mut self) {
        if let Some((owner, gained)) = self.lure.take() {
            let best = &mut self.seats[usize::from(owner)].best_lure;
            *best = (*best).max(gained);
        }
    }

    /// Whether this tally saw `board`'s round whole, up to where it stands.
    pub fn covers(&self, board: &Board) -> bool {
        self.whole && self.next_tick == board.ticks()
    }

    /// The awards this round earned, best story first.
    ///
    /// `winners` is the round's result as the standings crown it. Seats past
    /// `seats` never feature, whatever their slots hold.
    pub fn awards(&self, seats: u8, mode: TeamMode, winners: &[bool; MAX_PLAYERS]) -> Vec<Award> {
        let mut settled = self.clone();
        settled.close_lure();
        let seats = &settled.seats[..usize::from(seats).min(MAX_PLAYERS)];
        let mut out = Vec::new();
        // A comeback is a winner's story, and a team's win is not one
        // seat's comeback: its members trailed and recovered together.
        if mode == TeamMode::Solo {
            let behind = |seat: usize| winners[seat].then_some(seats[seat].worst_deficit);
            out.extend(
                top(seats.len(), behind, COMEBACK_MIN).map(|(who, n)| Award {
                    kind: AwardKind::Comeback,
                    who,
                    n,
                }),
            );
        }
        for (kind, figure) in [
            (
                AwardKind::Lure,
                (|t: &SeatTally| t.best_lure) as fn(&SeatTally) -> u32,
            ),
            (AwardKind::Arrows, |t| t.placed),
            (AwardKind::Hit, |t| t.lost),
        ] {
            let of = |seat: usize| Some(figure(&seats[seat]));
            out.extend(top(seats.len(), of, 1).map(|(who, n)| Award { kind, who, n }));
        }
        out
    }
}

/// The seats sharing the highest figure, if it reaches `floor`.
fn top(seats: usize, figure: impl Fn(usize) -> Option<u32>, floor: u32) -> Option<(Vec<u8>, u32)> {
    let best = (0..seats).filter_map(&figure).max()?;
    (best >= floor).then(|| {
        let who = (0..seats)
            .filter(|&seat| figure(seat) == Some(best))
            .map(|seat| seat as u8)
            .collect();
        (who, best)
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AwardKind {
    Comeback,
    Lure,
    Arrows,
    Hit,
}

/// One line of the card: what for, who, and the figure.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Award {
    pub kind: AwardKind,
    /// Every seat that shares it, lowest seat first.
    pub who: Vec<u8>,
    pub n: u32,
}

impl Award {
    /// The line as it reads, with `name` for each seat's label.
    pub fn line(&self, tr: &Tr, name: impl Fn(u8) -> String) -> String {
        let template = match self.kind {
            AwardKind::Comeback => tr.award_comeback,
            AwardKind::Lure => tr.award_lure,
            AwardKind::Arrows => tr.award_arrows,
            AwardKind::Hit => tr.award_hit,
        };
        let who: Vec<String> = self.who.iter().map(|&seat| name(seat)).collect();
        fill(
            template,
            &[("p", &who.join(" & ")), ("n", &self.n.to_string())],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::i18n::EN;
    use crate::sim::{BotLevel, bot_action, generate_arena};

    fn reading(ticks: u64, scores: [u32; MAX_PLAYERS], lure: Option<PlayerId>) -> Reading {
        Reading {
            ticks,
            scores,
            lure,
        }
    }

    /// Fold a list of score readings, one per tick, from tick zero.
    fn tally_of(steps: &[([u32; MAX_PLAYERS], Option<PlayerId>)]) -> RoundTally {
        let mut tally = RoundTally::default();
        let mut before = reading(0, [0; MAX_PLAYERS], None);
        for (tick, &(scores, lure)) in steps.iter().enumerate() {
            let after = reading(tick as u64 + 1, scores, lure);
            tally.fold(before, after, [false; MAX_PLAYERS]);
            before = after;
        }
        tally
    }

    const SOLO_WIN_P2: [bool; MAX_PLAYERS] = [false, true, false, false, false, false];

    /// The winner who trailed by twenty or more is the comeback, and one
    /// who trailed by less is not.
    #[test]
    fn a_comeback_is_a_winner_who_was_well_behind() {
        let tally = tally_of(&[([30, 0, 0, 0, 0, 0], None), ([30, 40, 0, 0, 0, 0], None)]);
        let awards = tally.awards(2, TeamMode::Solo, &SOLO_WIN_P2);
        assert_eq!(
            awards[0],
            Award {
                kind: AwardKind::Comeback,
                who: vec![1],
                n: 30
            }
        );
        let close = tally_of(&[([10, 0, 0, 0, 0, 0], None), ([10, 40, 0, 0, 0, 0], None)]);
        let awards = close.awards(2, TeamMode::Solo, &SOLO_WIN_P2);
        assert!(
            awards.iter().all(|a| a.kind != AwardKind::Comeback),
            "{awards:?}"
        );
        // And never in teams, where the deficit was the team's.
        let awards = tally.awards(2, TeamMode::Pairs, &SOLO_WIN_P2);
        assert!(
            awards.iter().all(|a| a.kind != AwardKind::Comeback),
            "{awards:?}"
        );
    }

    /// A lure counts what its owner gained from the tick it began through
    /// the tick it ended, and only its owner's gains; the best of several
    /// is the one kept.
    #[test]
    fn a_lure_counts_its_owners_gains_while_it_runs() {
        let tally = tally_of(&[
            ([0, 0, 0, 0, 0, 0], Some(0)),
            ([5, 9, 0, 0, 0, 0], Some(0)),
            // The tick the lure was running at its start and not at its end.
            ([8, 9, 0, 0, 0, 0], None),
            ([20, 9, 0, 0, 0, 0], None),
            ([20, 9, 0, 0, 0, 0], Some(0)),
            ([22, 9, 0, 0, 0, 0], Some(0)),
        ]);
        let lure = tally
            .awards(
                2,
                TeamMode::Solo,
                &[true, false, false, false, false, false],
            )
            .into_iter()
            .find(|a| a.kind == AwardKind::Lure)
            .expect("a lure award");
        assert_eq!(lure.who, vec![0]);
        assert_eq!(
            lure.n, 8,
            "5 then 3, not the 12 banked after it, nor P2's 9"
        );
    }

    /// Score that goes down is score lost, and a tie names every seat.
    #[test]
    fn losses_add_up_and_ties_share_the_line() {
        let tally = tally_of(&[([10, 10, 10, 0, 0, 0], None), ([4, 4, 10, 0, 0, 0], None)]);
        let hit = tally
            .awards(
                3,
                TeamMode::Solo,
                &[false, false, true, false, false, false],
            )
            .into_iter()
            .find(|a| a.kind == AwardKind::Hit)
            .expect("a hit award");
        assert_eq!((hit.who.clone(), hit.n), (vec![0, 1], 6));
        let line = hit.line(&EN, |seat| format!("P{}", seat + 1));
        assert_eq!(line, "Hardest hit: P1 & P2 lost 6");
    }

    /// Nothing happened, nothing is said: a figure of zero is no award.
    #[test]
    fn a_quiet_round_earns_no_lines() {
        let tally = tally_of(&[([0; MAX_PLAYERS], None), ([0; MAX_PLAYERS], None)]);
        assert!(
            tally
                .awards(2, TeamMode::Solo, &[true, true, false, false, false, false])
                .is_empty()
        );
    }

    /// Only a round watched from its first tick is covered: a board picked
    /// up mid-round, or after a gap, is not, and the next round's first
    /// tick starts clean.
    #[test]
    fn only_a_round_seen_whole_is_covered() {
        let mut tally = RoundTally::default();
        let mut board = generate_arena(7, 2, 12, 9);
        let idle = [PlayerAction::None; MAX_PLAYERS];
        for _ in 0..30 {
            let before = Reading::of(&board);
            board.tick(&idle);
            tally.observe(before, &board, &idle);
        }
        assert!(tally.covers(&board));

        // A tick the tally never saw.
        board.tick(&idle);
        let before = Reading::of(&board);
        board.tick(&idle);
        tally.observe(before, &board, &idle);
        assert!(!tally.covers(&board), "a gap is not a whole round");

        tally.seats[0].placed = 99;
        let mut fresh = generate_arena(8, 2, 12, 9);
        let before = Reading::of(&fresh);
        fresh.tick(&idle);
        tally.observe(before, &fresh, &idle);
        assert!(tally.covers(&fresh), "a new round's first tick");
        assert_eq!(tally.seats[0].placed, 0, "and nothing carried over");
    }

    /// Over a real round played by bots, the tally agrees with the board:
    /// every arrow it counts went into the sand, and the score that went
    /// up and down nets to the final score.
    #[test]
    fn a_bot_round_tallies_what_the_board_did() {
        let mut board = generate_arena(11, 4, 12, 9);
        let mut tally = RoundTally::default();
        let mut gained = [0u32; MAX_PLAYERS];
        let mut attempted = [0u32; MAX_PLAYERS];
        for _ in 0..3000 {
            let mut actions = [PlayerAction::None; MAX_PLAYERS];
            for seat in 0..4u8 {
                actions[usize::from(seat)] = bot_action(&board, seat, BotLevel::Hard);
                if matches!(actions[usize::from(seat)], PlayerAction::Place { .. }) {
                    attempted[usize::from(seat)] += 1;
                }
            }
            let before = Reading::of(&board);
            board.tick(&actions);
            for (seat, gained) in gained.iter_mut().enumerate() {
                *gained += board.scores()[seat].saturating_sub(before.scores[seat]);
            }
            tally.observe(before, &board, &actions);
        }
        assert!(tally.covers(&board));
        for seat in 0..4 {
            let t = tally.seats[seat];
            assert_eq!(
                gained[seat] - t.lost,
                board.scores()[seat],
                "seat {seat}: what came in less what went out is the score"
            );
            assert!(t.placed <= attempted[seat], "seat {seat}: {t:?}");
        }
        assert!(
            tally.seats.iter().any(|t| t.placed > 0),
            "the bots planted arrows: {:?}",
            tally.seats
        );
    }
}
