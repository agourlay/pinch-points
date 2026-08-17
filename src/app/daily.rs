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
}

impl Daily {
    /// Days since the epoch, UTC: the worldwide shared seed basis.
    pub fn today() -> u32 {
        (crate::app::clock::now_secs() / 86_400) as u32
    }

    pub fn seed() -> u64 {
        Self::seed_for(Self::today())
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
        // The live seed is derived from today's number, nothing else.
        assert_eq!(Daily::seed(), Daily::seed_for(Daily::today()));
    }
}
