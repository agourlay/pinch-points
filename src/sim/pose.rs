use crate::sim::direction::Direction;

/// Where a creature stands on the grid: `progress` subunits past the centre
/// of `tile`, heading in `dir`. Crabs and gulls both carry one of these as
/// last tick's position, which only the render layer reads (spec §7.4) to
/// slide a sprite between two ticks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pose {
    pub tile: u16,
    pub dir: Direction,
    pub progress: u16,
}
