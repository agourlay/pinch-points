/// A grid direction in screen space: row 0 is the top of the board, so `Up`
/// decreases `y`. "Left of" and "right of" are from the walking agent's point
/// of view (facing `Up`, the agent's left is `Left`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Up,
    Right,
    Down,
    Left,
}

impl Direction {
    /// Every direction, clockwise from `Up`, for the callers that try all
    /// four, so the array is not restated at each of them.
    pub const ALL: [Direction; 4] = [
        Direction::Up,
        Direction::Right,
        Direction::Down,
        Direction::Left,
    ];

    /// The level-format letter (`U`/`D`/`L`/`R`); `from_letter` is its
    /// inverse.
    pub fn letter(self) -> char {
        match self {
            Direction::Up => 'U',
            Direction::Down => 'D',
            Direction::Left => 'L',
            Direction::Right => 'R',
        }
    }

    pub fn from_letter(letter: &str) -> Option<Direction> {
        match letter {
            "U" => Some(Direction::Up),
            "D" => Some(Direction::Down),
            "L" => Some(Direction::Left),
            "R" => Some(Direction::Right),
            _ => None,
        }
    }

    /// The direction to the agent's left.
    pub fn left(self) -> Self {
        match self {
            Direction::Up => Direction::Left,
            Direction::Left => Direction::Down,
            Direction::Down => Direction::Right,
            Direction::Right => Direction::Up,
        }
    }

    /// The direction to the agent's right.
    pub fn right(self) -> Self {
        match self {
            Direction::Up => Direction::Right,
            Direction::Right => Direction::Down,
            Direction::Down => Direction::Left,
            Direction::Left => Direction::Up,
        }
    }

    pub fn reverse(self) -> Self {
        match self {
            Direction::Up => Direction::Down,
            Direction::Down => Direction::Up,
            Direction::Left => Direction::Right,
            Direction::Right => Direction::Left,
        }
    }

    /// Tile offset `(dx, dy)` of one step in this direction.
    pub fn offset(self) -> (i32, i32) {
        match self {
            Direction::Up => (0, -1),
            Direction::Down => (0, 1),
            Direction::Left => (-1, 0),
            Direction::Right => (1, 0),
        }
    }

    pub(crate) fn id(self) -> u8 {
        match self {
            Direction::Up => 0,
            Direction::Right => 1,
            Direction::Down => 2,
            Direction::Left => 3,
        }
    }

    /// Inverse of [`Direction::id`]; masks to the low two bits, so any byte
    /// decodes to some direction (wire formats rely on this).
    pub(crate) fn from_id(id: u8) -> Direction {
        match id & 0b11 {
            0 => Direction::Up,
            1 => Direction::Right,
            2 => Direction::Down,
            _ => Direction::Left,
        }
    }

    /// Greedy direction of travel for a `(dx, dy)` displacement in screen
    /// space: the axis with the larger magnitude wins, ties go horizontal.
    /// `(0, 0)` yields `Left` by fall-through; callers exclude it.
    pub fn toward(dx: i32, dy: i32) -> Direction {
        if dx.abs() >= dy.abs() {
            if dx > 0 {
                Direction::Right
            } else {
                Direction::Left
            }
        } else if dy > 0 {
            Direction::Down
        } else {
            Direction::Up
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Direction;
    use super::Direction::*;

    #[test]
    fn letters_round_trip_for_every_direction() {
        for dir in [Up, Down, Left, Right] {
            assert_eq!(Direction::from_letter(&dir.letter().to_string()), Some(dir));
        }
        assert_eq!(Direction::from_letter("X"), None);
    }

    #[test]
    fn left_right_reverse_are_consistent() {
        for d in [Up, Right, Down, Left] {
            assert_eq!(d.left().right(), d);
            assert_eq!(d.right().left(), d);
            assert_eq!(d.left().left(), d.reverse());
            assert_eq!(d.right().right(), d.reverse());
            assert_eq!(d.reverse().reverse(), d);
        }
    }

    #[test]
    fn id_round_trips() {
        for d in [Up, Right, Down, Left] {
            assert_eq!(Direction::from_id(d.id()), d);
        }
        // Any byte decodes (wire safety).
        for byte in 0..=u8::MAX {
            let _ = Direction::from_id(byte);
        }
    }

    #[test]
    fn toward_picks_the_dominant_axis() {
        assert_eq!(Direction::toward(5, 2), Right);
        assert_eq!(Direction::toward(-5, 2), Left);
        assert_eq!(Direction::toward(1, 4), Down);
        assert_eq!(Direction::toward(1, -4), Up);
        assert_eq!(Direction::toward(3, 3), Right); // tie goes horizontal
        assert_eq!(Direction::toward(-3, -3), Left);
    }

    #[test]
    fn screen_space_handedness() {
        // Facing up the screen, the agent's left hand points to screen-left.
        assert_eq!(Up.left(), Left);
        assert_eq!(Down.left(), Right);
    }
}
