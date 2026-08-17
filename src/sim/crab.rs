use crate::sim::direction::Direction;

/// Which claw is oversized, i.e. which side this crab tries first when its
/// forward path is blocked. The one-bit divergence from ChuChu Rocket that
/// turns herding puzzles into sorting puzzles (spec §2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Handedness {
    Left,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrabKind {
    Common,
    Juvenile,
    Giant,
    /// Banking one starts the 10-second lure that draws every loose crab to
    /// the banking castle.
    Molting,
    /// The jackpot (ChuChu Rocket's gold mouse, re-shelled): vanishingly
    /// rare, quick, and worth a small castle tier by itself.
    Golden,
    /// The "?" mouse, re-shelled: banking it spins the tide-event roulette
    /// on boards where events are enabled.
    Sparkling,
}

impl CrabKind {
    pub const ALL: [CrabKind; 6] = [
        CrabKind::Common,
        CrabKind::Juvenile,
        CrabKind::Giant,
        CrabKind::Molting,
        CrabKind::Golden,
        CrabKind::Sparkling,
    ];

    /// The level-format token; `from_token` is its inverse.
    pub fn token(self) -> &'static str {
        match self {
            CrabKind::Common => "common",
            CrabKind::Juvenile => "juvenile",
            CrabKind::Giant => "giant",
            CrabKind::Molting => "molting",
            CrabKind::Golden => "golden",
            CrabKind::Sparkling => "sparkling",
        }
    }

    pub fn from_token(token: &str) -> Option<CrabKind> {
        Self::ALL.iter().copied().find(|k| k.token() == token)
    }

    /// Walking speed in subunits per tick (one tile = 256 subunits, spec §4.2).
    pub fn speed(self) -> u16 {
        match self {
            CrabKind::Common | CrabKind::Molting | CrabKind::Sparkling => 12,
            CrabKind::Juvenile => 18, // 1.5×
            CrabKind::Giant => 7,     // 0.6× of 12 is 7.2; integer sim rounds down
            CrabKind::Golden => 15,   // 1.25×: catchable, but it makes you work
        }
    }

    /// Score awarded when banked. Every kind banks differently: commons are
    /// the bread and butter, juveniles pay a little extra for being hard to
    /// route, molting crabs are prized (and start the lure), giants are the
    /// jackpot worth protecting.
    pub fn value(self) -> u32 {
        match self {
            CrabKind::Common => 1,
            CrabKind::Juvenile => 2,
            CrabKind::Molting => 5,
            CrabKind::Giant => 10,
            CrabKind::Golden => 50,
            CrabKind::Sparkling => 1, // the event is the prize
        }
    }

    pub(crate) fn id(self) -> u8 {
        match self {
            CrabKind::Common => 0,
            CrabKind::Juvenile => 1,
            CrabKind::Giant => 2,
            CrabKind::Molting => 3,
            CrabKind::Golden => 4,
            CrabKind::Sparkling => 5,
        }
    }
}

/// One crab. Position is integer-only: the crab is `progress` subunits past the
/// centre of `tile`, moving toward the centre of the next tile in `dir`.
/// `prev_*` hold last tick's position for render-side interpolation (spec §7.4)
/// and are never read by the simulation itself.
#[derive(Clone, Copy, Debug)]
pub struct Crab {
    /// Stable identity for the render layer's sprite mapping; unique within a
    /// match, assigned by the `Board` at spawn.
    pub id: u32,
    pub tile: u16,
    pub dir: Direction,
    pub progress: u16,
    pub prev_tile: u16,
    pub prev_progress: u16,
    pub prev_dir: Direction,
    pub handed: Handedness,
    pub kind: CrabKind,
}

impl Handedness {
    pub(crate) fn id(self) -> u8 {
        match self {
            Handedness::Left => 0,
            Handedness::Right => 1,
        }
    }

    /// The level-format token (`L`/`R`); `from_token` is its inverse.
    pub fn token(self) -> &'static str {
        match self {
            Handedness::Left => "L",
            Handedness::Right => "R",
        }
    }

    pub fn from_token(token: &str) -> Option<Handedness> {
        match token {
            "L" => Some(Handedness::Left),
            "R" => Some(Handedness::Right),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CrabKind, Handedness};

    #[test]
    fn tokens_round_trip_for_every_kind() {
        for kind in CrabKind::ALL {
            assert_eq!(CrabKind::from_token(kind.token()), Some(kind));
        }
        assert_eq!(CrabKind::from_token("hermit"), None);
        for handed in [Handedness::Left, Handedness::Right] {
            assert_eq!(Handedness::from_token(handed.token()), Some(handed));
        }
    }

    #[test]
    fn golden_crab_is_the_jackpot() {
        assert_eq!(CrabKind::Golden.value(), 50);
        assert!(CrabKind::Golden.speed() > CrabKind::Common.speed());
        assert!(CrabKind::Golden.speed() < CrabKind::Juvenile.speed());
    }
}
