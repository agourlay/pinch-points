//! One `cycled`/`index` implementation for every left/right-cyclable
//! config enum, instead of a hand-copied pair per type.

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

    fn cycled(self, right: bool) -> Self {
        let n = Self::VARIANTS.len();
        Self::VARIANTS[(self.index() + if right { 1 } else { n - 1 }) % n]
    }
}

#[cfg(test)]
mod tests {
    use super::Cycle;
    use crate::app::i18n::Lang;
    use crate::app::match_setup::{GullPressure, MapChoice, RoundLength};
    use crate::sim::BotLevel;

    fn round_trips<T: Cycle + std::fmt::Debug>() {
        for &x in T::VARIANTS {
            assert_eq!(x.cycled(true).cycled(false), x, "{x:?}");
        }
        // A full lap forward returns home.
        let mut x = T::VARIANTS[0];
        for _ in 0..T::VARIANTS.len() {
            x = x.cycled(true);
        }
        assert_eq!(x, T::VARIANTS[0]);
    }

    #[test]
    fn every_config_enum_cycles_cleanly() {
        round_trips::<MapChoice>();
        round_trips::<GullPressure>();
        round_trips::<RoundLength>();
        round_trips::<Lang>();
        round_trips::<BotLevel>();
    }
}
