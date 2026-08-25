//! One `cycled`/`index` implementation for every left/right-cyclable
//! config enum, instead of a hand-copied pair per type.

/// Which way a dial was turned. Its own type rather than a bool: `right:
/// true` at a call site says which argument is which only to a reader who
/// already knows, and every dial in the game takes one of these.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Turn {
    Left,
    Right,
}

impl Turn {
    /// The step's sign, for the dials that add to a number rather than
    /// cycle an enum: rightward is +1.
    pub fn signum(self) -> i32 {
        match self {
            Turn::Left => -1,
            Turn::Right => 1,
        }
    }
}

/// A finite enum a settings row can step through with left/right.
pub trait Cycle: Sized + Copy + PartialEq + 'static {
    /// Every variant, in display order.
    const VARIANTS: &'static [Self];

    fn index(self) -> usize {
        Self::VARIANTS.iter().position(|&x| x == self).unwrap_or(0)
    }

    /// The variant a wire byte (or a saved index) names. Out-of-range wraps
    /// rather than panicking: a peer on another build must not crash us.
    fn from_index(index: usize) -> Self {
        Self::VARIANTS[index % Self::VARIANTS.len()]
    }

    fn cycled(self, turn: Turn) -> Self {
        let n = Self::VARIANTS.len();
        let step = match turn {
            Turn::Right => 1,
            Turn::Left => n - 1, // one backwards, modulo n
        };
        Self::VARIANTS[(self.index() + step) % n]
    }
}

/// One notch on a numeric dial: `value` moved `step` toward `right`, held
/// inside `range`. Eight rows once inlined this with their own casts and
/// their own copy of the bounds, and the lenient settings parser restated
/// the bounds a third time; the ranges now live beside the parser as named
/// constants and the card turns them through here.
pub fn dial(value: u8, turn: Turn, step: u8, range: std::ops::RangeInclusive<u8>) -> u8 {
    let moved = i32::from(value) + i32::from(step) * turn.signum();
    moved.clamp(i32::from(*range.start()), i32::from(*range.end())) as u8
}

#[cfg(test)]
mod tests {
    use super::{Cycle, Turn};
    use crate::app::i18n::Lang;
    use crate::app::match_setup::{GullPressure, MapChoice, RoundLength};
    use crate::sim::BotLevel;

    fn round_trips<T: Cycle + std::fmt::Debug>() {
        for &x in T::VARIANTS {
            assert_eq!(x.cycled(Turn::Right).cycled(Turn::Left), x, "{x:?}");
        }
        // A full lap forward returns home.
        let mut x = T::VARIANTS[0];
        for _ in 0..T::VARIANTS.len() {
            x = x.cycled(Turn::Right);
        }
        assert_eq!(x, T::VARIANTS[0]);
    }

    #[test]
    fn every_config_enum_cycles_cleanly() {
        round_trips::<MapChoice>();
        round_trips::<GullPressure>();
        round_trips::<RoundLength>();
        round_trips::<Lang>();
        round_trips::<Option<crate::app::keycaps::Layout>>();
        round_trips::<BotLevel>();
    }
}
