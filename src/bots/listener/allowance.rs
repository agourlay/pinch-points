//! The flood valve: what a bot may send before it is flooding.

use std::time::Instant;

/// The flood valve. A bot earns the right to send by being sent things by
/// a game: every tick (or hello, or end) written to it is worth `PER_LINE`
/// messages back (a reply and a few lookaheads), so a fast bot in a
/// fast-forward game, answering thousands of ticks a second, is never
/// mistaken for a flood. What the listener says back to the bot's own
/// requests (errors, replays, lookaheads) earns nothing, or a request that
/// draws an error would pay for itself. On top of that it may send
/// `PER_SECOND` a second of its own accord, and save up to `BURST`.
pub(super) const BURST: f64 = 400.0;
pub(super) const PER_SECOND: f64 = 50.0;
pub(super) const PER_LINE: f64 = 6.0;
/// The most a bot can have saved up, earned and refilled together.
pub(super) const CEILING: f64 = 2000.0;
/// Messages dropped for flooding before the connection goes, counted over
/// a sliding window: each drop is a strike, and strikes are forgiven at
/// `FORGIVE_PER_SECOND`. A bot that overruns once in a while is never
/// let go, and one that keeps on overrunning faster than it is forgiven
/// is, however slowly it does: one having `d` messages a second dropped
/// goes after 400 / (d - 40) seconds, ten at 80 a second, four at 140.
pub(super) const FLOOD_STRIKES: f64 = 400.0;
pub(super) const FORGIVE_PER_SECOND: f64 = 40.0;

/// What a bot may still send before it is flooding: see [`BURST`].
pub(super) struct Allowance {
    pub(super) level: f64,
    at: Instant,
    /// Messages dropped lately, less what time has forgiven: see
    /// [`FLOOD_STRIKES`]. Not cleared by a message that gets through, or a
    /// steady flood, let through a message every refill, would never add
    /// up to anything.
    strikes: f64,
}

impl Allowance {
    pub(super) fn new() -> Allowance {
        Allowance {
            level: BURST,
            at: Instant::now(),
            strikes: 0.0,
        }
    }

    /// Spend one message's worth, if there is one.
    pub(super) fn take(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.at).as_secs_f64();
        // The clock refills up to the burst; what was earned by being sent
        // lines may stand above it, up to the ceiling.
        let refilled = self.level + elapsed * PER_SECOND;
        self.level = refilled.min(self.level.max(BURST));
        self.strikes = (self.strikes - elapsed * FORGIVE_PER_SECOND).max(0.0);
        self.at = now;
        if self.level >= 1.0 {
            self.level -= 1.0;
            true
        } else {
            self.strikes += 1.0;
            false
        }
    }

    /// Dropped so much lately that it is flooding.
    pub(super) fn flooding(&self) -> bool {
        self.strikes > FLOOD_STRIKES
    }

    /// A line went out to the bot: it may answer it.
    pub(super) fn credit(&mut self) {
        self.level = (self.level + PER_LINE).min(CEILING);
    }
}
