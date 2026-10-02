//! What a bot is allowed to know about a board, and a copy of it to think
//! ahead on.
//!
//! The bot protocol (`docs/bot-seats.md`) shows a bot everything a player
//! can see on the screen and nothing they cannot. Two things on a board
//! are hidden from a player: the PRNG, which decides where the next gull
//! lands and what the roulette spins, and each gull's takeoff countdown.
//! [`Board::lookahead`] is a copy with both replaced by a draw nobody can
//! steer, so a lookahead knows what a player reading the board knows.

use super::*;

impl Board {
    /// The timed tide event running now and the ticks it has left, if any.
    ///
    /// The roulette's cooldown is as long as an event, so only one runs at
    /// a time in play; a spectators' call can overlap one, and then the
    /// mania is named first, as the banner names it.
    pub fn running_event(&self) -> Option<(TideEvent, u32)> {
        if let Some((mania, ticks)) = self.tide.mania {
            let event = match mania {
                Mania::Crab => TideEvent::CrabMania,
                Mania::Gull => TideEvent::GullMania,
            };
            return Some((event, ticks));
        }
        if let Some((tempo, ticks)) = self.tide.tempo {
            let event = match tempo {
                Tempo::Fast => TideEvent::SpeedUp,
                Tempo::Slow => TideEvent::SlowDown,
            };
            return Some((event, ticks));
        }
        (self.tide.claw_call > 0).then_some((TideEvent::RightClaws, self.tide.claw_call))
    }

    /// Every standing signpost with its tile, in reading order.
    pub fn signposts(&self) -> impl Iterator<Item = (u8, u8, Signpost)> + '_ {
        self.signposts
            .iter()
            .enumerate()
            .filter_map(|(tile, slot)| slot.map(|post| (tile as u16, post)))
            .map(|(tile, post)| {
                let (x, y) = self.coords_u8(tile);
                (x, y, post)
            })
    }

    /// A copy of this board to run forward without learning anything a
    /// player could not.
    ///
    /// The PRNG is reseeded from `seed`, so what it decides next (a spawned
    /// crab's kind and claw, the roulette, a gull's next flight) is a fresh
    /// draw rather than the real one. The ambient gull spawner is switched
    /// off, because it lands on a tile the PRNG picks and a gull on a
    /// guessed tile is worse than none. Each gull's takeoff countdown is
    /// redrawn, since the screen never shows it. Everything already on the
    /// sand, and every schedule a player can count (spawners fire on
    /// `tick % period`), is kept exactly.
    pub fn lookahead(&self, seed: u64) -> Board {
        let mut copy = self.clone();
        copy.rng = Pcg32::new(seed, 0x0005_eaba_55ed);
        copy.rules.gull_period = 0;
        for i in 0..copy.gulls.len() {
            copy.gulls[i].takeoff_in = copy.roll_takeoff();
        }
        copy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::classic_arena;

    /// The lookahead never sees the real draw: two boards that differ only
    /// in their PRNG position look ahead identically.
    #[test]
    fn a_lookahead_cannot_read_the_dice() {
        let mut a = classic_arena(false, 2);
        for _ in 0..200 {
            a.tick_idle();
        }
        let mut b = a.clone();
        b.rng = Pcg32::new(99, 7);
        let (mut la, mut lb) = (a.lookahead(5), b.lookahead(5));
        for _ in 0..600 {
            la.tick_idle();
            lb.tick_idle();
        }
        assert_eq!(la.state_hash(), lb.state_hash());
    }

    /// What is already on the sand is kept: the copy starts on the same
    /// crabs, posts and scores, and plays no ambient gulls.
    #[test]
    fn a_lookahead_keeps_the_sand_and_drops_the_ambient_gulls() {
        let mut board = classic_arena(false, 2);
        for _ in 0..90 {
            board.tick_idle();
        }
        let copy = board.lookahead(1);
        assert_eq!(copy.crabs().len(), board.crabs().len());
        assert_eq!(copy.scores(), board.scores());
        assert_eq!(copy.gull_period(), 0);
        assert!(board.gull_period() > 0);
    }

    #[test]
    fn the_running_event_is_named_with_its_clock() {
        let mut board = classic_arena(false, 2);
        assert_eq!(board.running_event(), None);
        board.force_tide_event(TideEvent::SpeedUp, 0);
        assert_eq!(
            board.running_event(),
            Some((TideEvent::SpeedUp, EVENT_TICKS))
        );
        board.tick_idle();
        assert_eq!(
            board.running_event(),
            Some((TideEvent::SpeedUp, EVENT_TICKS - 1))
        );
    }
}
