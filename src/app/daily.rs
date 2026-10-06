//! The daily challenge: one beach a day, the same one for everybody.
//!
//! No server and no handshake - determinism means the seed *is* the
//! agreement, and the seed is the date.

use bevy::prelude::*;

/// The daily challenge: everyone in the world gets the same generated
/// arena for a given (UTC) day, thanks to determinism. `active` while the
/// current versus round is the daily.
#[derive(Resource, Default)]
pub struct Daily {
    pub active: bool,
    /// The day the round on the sand was seeded for, set when its board is
    /// built. The trophies count the round under this day rather than
    /// asking the clock again when it ends: a round begun at a minute to
    /// midnight was played on yesterday's beach, and counting it under
    /// today's put yesterday's score on a beach nobody had played yet.
    pub day: u32,
}

impl Daily {
    /// Days since the epoch, UTC: the worldwide shared seed basis.
    pub fn today() -> u32 {
        (crate::app::clock::now_secs() / 86_400) as u32
    }

    /// The arena seed for a given day number; pure so it can be tested.
    pub fn seed_for(day: u32) -> u64 {
        0xDA11_0000 ^ u64::from(day)
    }
}

#[cfg(test)]
mod tests {
    use super::Daily;

    #[test]
    fn daily_seed_is_stable_within_a_day_and_fresh_across_days() {
        assert_eq!(Daily::seed_for(20_662), Daily::seed_for(20_662));
        assert_ne!(Daily::seed_for(20_662), Daily::seed_for(20_663));
    }
}
