//! The brush: what the editor paints, the key that loads each one, and the
//! edits a press makes to the sand under the cursor.

use crate::sim::MAX_PLAYERS;
use crate::sim::{Board, CrabKind, Direction, Handedness, Spawner, TileKind};
use bevy::prelude::*;

pub(super) const SPAWNER_PERIOD: u32 = 60;

/// What the brush paints. One name for each thing the sand can hold, so
/// the palette, the key that selects it and the edit it makes are three
/// views of one list rather than three lists that have to agree.
///
/// Before this the editor had nine letters and no way to find out what any
/// of them did except to press it and watch: no palette, no selection, and
/// a prompt line reading `R C H L W P B G O tiles`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Brush {
    /// Bare sand, which is also the eraser: the only way to clear a tile
    /// without knowing what is already on it.
    #[default]
    Sand,
    Rock,
    Castle,
    Hole,
    Log,
    Weed,
    Pool,
    Crab,
    Gull,
}

impl Brush {
    pub const ALL: [Brush; 9] = [
        Brush::Sand,
        Brush::Rock,
        Brush::Castle,
        Brush::Hole,
        Brush::Log,
        Brush::Weed,
        Brush::Pool,
        Brush::Crab,
        Brush::Gull,
    ];

    /// The key that loads this brush, which is also the shortcut that
    /// paints it outright. Kept as they were where they could be, so a
    /// hand that already knows the editor does not have to learn it again.
    ///
    /// None of them may be W, A, S or D: those walk the cursor, and a key
    /// that does both walks the cursor *and* changes the brush. Weed was
    /// on W, so moving up quietly armed the kelp. `no_brush_takes_a_
    /// movement_key` holds the line.
    pub fn key(self) -> KeyCode {
        match self {
            Brush::Sand => KeyCode::KeyX,
            Brush::Rock => KeyCode::KeyR,
            Brush::Castle => KeyCode::KeyC,
            Brush::Hole => KeyCode::KeyH,
            Brush::Log => KeyCode::KeyL,
            Brush::Weed => KeyCode::KeyE,
            Brush::Pool => KeyCode::KeyP,
            Brush::Crab => KeyCode::KeyB,
            Brush::Gull => KeyCode::KeyG,
        }
    }

    /// The letter the palette shows beside the brush: the one on the key
    /// that loads it, always. The two lists once disagreed - Weed moved
    /// off W to E and the palette went on saying W - and a palette that
    /// names the wrong key is worse than one that names none, so
    /// `letters_name_their_keys` now holds them together.
    pub fn letter(self) -> &'static str {
        match self {
            Brush::Sand => "X",
            Brush::Rock => "R",
            Brush::Castle => "C",
            Brush::Hole => "H",
            Brush::Log => "L",
            Brush::Weed => "E",
            Brush::Pool => "P",
            Brush::Crab => "B",
            Brush::Gull => "G",
        }
    }

    pub fn label(self, tr: &crate::app::i18n::Tr) -> &'static str {
        tr.ed_brushes[Brush::ALL.iter().position(|b| *b == self).unwrap_or(0)]
    }

    pub fn icon(self, art: &crate::app::art::Art) -> Handle<Image> {
        match self {
            Brush::Sand => art.sand_a.clone(),
            Brush::Rock => art.rock.clone(),
            Brush::Castle => art.castle.clone(),
            Brush::Hole => art.hole.clone(),
            Brush::Log => art.log.clone(),
            Brush::Weed => art.kelp.clone(),
            Brush::Pool => art.pool.clone(),
            Brush::Crab => art.crab.clone(),
            Brush::Gull => art.gull.clone(),
        }
    }
}

/// Step the castle on a tile through the four seats and then away again,
/// so one key paints every owner. Anything else on the tile is replaced
/// (and its creatures swept) on the way in.
pub(super) fn cycle_castle(board: &mut Board, x: u8, y: u8) {
    let kind = match board.tile_at(x, y) {
        TileKind::Castle(owner) if usize::from(owner) + 1 < MAX_PLAYERS => {
            TileKind::Castle(owner + 1)
        }
        TileKind::Castle(_) => TileKind::Empty,
        TileKind::Empty
        | TileKind::Rock
        | TileKind::Spawner(_)
        | TileKind::Turnstile { .. }
        | TileKind::Kelp
        | TileKind::Pool => {
            board.remove_crabs_at(x, y);
            board.remove_gulls_at(x, y);
            TileKind::Castle(0)
        }
    };
    board.set_tile(x, y, kind);
}

/// Put `brush` on the tile under the cursor.
///
/// Every brush replaces whatever was there, rather than toggling it off
/// again: a palette that paints on the first press and erases on the second
/// is a palette that cannot be dragged across a row. `Sand` is the eraser,
/// and it is the only way to clear a tile without first knowing what is on
/// it.
pub(super) fn paint(board: &mut Board, x: u8, y: u8, brush: Brush) {
    // Two of them cycle rather than replace, because they carry a value the
    // player has to be able to reach: which seat a castle belongs to, and
    // which way a hole faces.
    match brush {
        Brush::Castle => return cycle_castle(board, x, y),
        Brush::Hole => {
            let kind = match board.tile_at(x, y) {
                TileKind::Spawner(s) => match next_dir(s.dir) {
                    Some(dir) => TileKind::Spawner(Spawner {
                        dir,
                        period: SPAWNER_PERIOD,
                    }),
                    None => TileKind::Empty,
                },
                TileKind::Empty
                | TileKind::Rock
                | TileKind::Castle(_)
                | TileKind::Turnstile { .. }
                | TileKind::Kelp
                | TileKind::Pool => {
                    board.remove_crabs_at(x, y);
                    board.remove_gulls_at(x, y);
                    TileKind::Spawner(Spawner {
                        dir: Direction::Right,
                        period: SPAWNER_PERIOD,
                    })
                }
            };
            return board.set_tile(x, y, kind);
        }
        Brush::Crab => return cycle_crab(board, x, y),
        Brush::Gull => {
            let tile = board.index_of(x, y);
            if board.gulls().iter().any(|g| g.tile == tile) {
                board.remove_gulls_at(x, y);
            } else if board.tile_at(x, y) != TileKind::Rock {
                board.spawn_gull(x, y, Direction::Right);
            }
            return;
        }
        Brush::Sand | Brush::Rock | Brush::Log | Brush::Weed | Brush::Pool => {}
    }
    // The rest are plain ground. Anything standing on the tile goes with
    // it, since a crab inside a rock is not a thing the sim has a rule for.
    board.remove_crabs_at(x, y);
    board.remove_gulls_at(x, y);
    board.set_tile(
        x,
        y,
        match brush {
            Brush::Rock => TileKind::Rock,
            Brush::Log => TileKind::Turnstile { next_right: true },
            Brush::Weed => TileKind::Kelp,
            Brush::Pool => TileKind::Pool,
            // The four that return above never reach here; sand is the
            // eraser and clears the tile.
            Brush::Sand | Brush::Castle | Brush::Hole | Brush::Crab | Brush::Gull => {
                TileKind::Empty
            }
        },
    );
}

fn next_dir(dir: Direction) -> Option<Direction> {
    match dir {
        Direction::Right => Some(Direction::Down),
        Direction::Down => Some(Direction::Left),
        Direction::Left => Some(Direction::Up),
        Direction::Up => None,
    }
}

/// Cycle the crab on a tile through kind/handedness combinations, ending at
/// none. Spawned crabs wall-resolve, so the cycle keys off identity fields.
fn cycle_crab(board: &mut Board, x: u8, y: u8) {
    const CYCLE: [(CrabKind, Handedness); 12] = [
        (CrabKind::Common, Handedness::Left),
        (CrabKind::Common, Handedness::Right),
        (CrabKind::Juvenile, Handedness::Left),
        (CrabKind::Juvenile, Handedness::Right),
        (CrabKind::Giant, Handedness::Left),
        (CrabKind::Giant, Handedness::Right),
        (CrabKind::Molting, Handedness::Left),
        (CrabKind::Molting, Handedness::Right),
        (CrabKind::Golden, Handedness::Left),
        (CrabKind::Golden, Handedness::Right),
        (CrabKind::Sparkling, Handedness::Left),
        (CrabKind::Sparkling, Handedness::Right),
    ];
    if board.tile_at(x, y) != TileKind::Empty {
        return; // crabs start on open sand only
    }
    let tile = board.index_of(x, y);
    let current = board
        .crabs()
        .iter()
        .find(|c| c.tile == tile)
        .map(|c| (c.kind, c.handed));
    board.remove_crabs_at(x, y);
    let next = match current {
        None => Some(CYCLE[0]),
        Some(cur) => CYCLE
            .iter()
            .position(|&e| e == cur)
            .and_then(|i| CYCLE.get(i + 1))
            .copied(),
    };
    if let Some((kind, handed)) = next {
        board.spawn_crab(x, y, Direction::Right, handed, kind);
    }
}

/// Which brush would have drawn what is on this tile. Creatures win over
/// the ground they stand on, because that is what the player put there
/// last and what another press would take away.
pub(super) fn standing_on(board: &Board, x: u8, y: u8) -> Brush {
    let tile = board.index_of(x, y);
    if board.gulls().iter().any(|g| g.tile == tile) {
        return Brush::Gull;
    }
    if board.crabs().iter().any(|c| c.tile == tile) {
        return Brush::Crab;
    }
    match board.tile_at(x, y) {
        TileKind::Rock => Brush::Rock,
        TileKind::Castle(_) => Brush::Castle,
        TileKind::Spawner(_) => Brush::Hole,
        TileKind::Turnstile { .. } => Brush::Log,
        TileKind::Kelp => Brush::Weed,
        TileKind::Pool => Brush::Pool,
        TileKind::Empty => Brush::Sand,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The editor moves on WASD and paints with letters, so a brush on one
    /// of those four does two things at once. It also must not collide
    /// with the editor's own keys, which are just as invisible a clash.
    #[test]
    fn no_brush_takes_a_movement_key() {
        use bevy::prelude::KeyCode;
        let movement = [KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD];
        let controls = [
            KeyCode::KeyK,   // gull period
            KeyCode::KeyO,   // wrapping edges
            KeyCode::KeyV,   // validate
            KeyCode::Tab,    // next brush
            KeyCode::Space,  // paint
            KeyCode::Enter,  // playtest
            KeyCode::Escape, // leave
            KeyCode::F6,     // puzzle or beach
        ];
        for brush in Brush::ALL {
            let key = brush.key();
            assert!(!movement.contains(&key), "{brush:?} is on a movement key");
            assert!(!controls.contains(&key), "{brush:?} is on a control key");
        }
        // And no two brushes share one.
        let mut keys: Vec<_> = Brush::ALL
            .iter()
            .map(|b| format!("{:?}", b.key()))
            .collect();
        keys.sort();
        let before = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), before, "two brushes on one key");
    }

    /// The palette names the key that loads each brush, so the letter it
    /// shows has to be the key's own: it once said "W" for kelp after the
    /// key had moved to E.
    #[test]
    fn letters_name_their_keys() {
        for brush in Brush::ALL {
            assert_eq!(
                format!("{:?}", brush.key()),
                format!("Key{}", brush.letter()),
                "{brush:?} shows a letter that is not its key"
            );
        }
    }

    fn sand() -> Board {
        Board::new(6, 5, 3)
    }

    /// One key paints all four owners and then clears the tile, and a
    /// castle never lands on top of a creature.
    #[test]
    fn the_castle_key_walks_the_seats_then_clears() {
        let mut board = sand();
        board.spawn_crab(2, 2, Direction::Right, Handedness::Left, CrabKind::Common);
        for owner in 0..MAX_PLAYERS as u8 {
            cycle_castle(&mut board, 2, 2);
            assert_eq!(board.tile_at(2, 2), TileKind::Castle(owner));
        }
        assert!(board.crabs().is_empty(), "the crab was swept aside");
        cycle_castle(&mut board, 2, 2);
        assert_eq!(board.tile_at(2, 2), TileKind::Empty, "and round to nothing");
    }

    /// A brush replaces what is on the tile, and `Sand` is what takes it
    /// away.
    ///
    /// It used to toggle: pressing the same key twice put the tile back to
    /// bare sand. That reads well with one key per terrain and badly with a
    /// palette, where dragging a brush along a row would rub out every
    /// other tile it crossed. `Sand` is now the eraser, and it is the only
    /// one that does not need to know what it is erasing.
    #[test]
    fn a_brush_replaces_and_sand_erases() {
        let mut board = sand();
        paint(&mut board, 1, 1, Brush::Weed);
        assert_eq!(board.tile_at(1, 1), TileKind::Kelp);
        paint(&mut board, 1, 1, Brush::Weed);
        assert_eq!(board.tile_at(1, 1), TileKind::Kelp, "and again is the same");
        paint(&mut board, 1, 1, Brush::Pool);
        assert_eq!(board.tile_at(1, 1), TileKind::Pool, "another replaces it");
        paint(&mut board, 1, 1, Brush::Sand);
        assert_eq!(board.tile_at(1, 1), TileKind::Empty, "and sand clears it");

        // Whatever was standing there goes with the ground: a crab inside a
        // rock is not a thing the sim has a rule for.
        board.spawn_crab(3, 3, Direction::Right, Handedness::Left, CrabKind::Common);
        paint(&mut board, 3, 3, Brush::Rock);
        assert_eq!(board.tile_at(3, 3), TileKind::Rock);
        assert!(board.crabs().is_empty());
    }

    /// Every brush has its own letter, and the palette, the keys and the
    /// labels are all the one list.
    #[test]
    fn the_palette_is_one_list() {
        use std::collections::HashSet;
        let keys: HashSet<KeyCode> = Brush::ALL.iter().map(|b| b.key()).collect();
        assert_eq!(keys.len(), Brush::ALL.len(), "two brushes share a key");
        let letters: HashSet<&str> = Brush::ALL.iter().map(|b| b.letter()).collect();
        assert_eq!(
            letters.len(),
            Brush::ALL.len(),
            "two brushes share a letter"
        );
        assert_eq!(
            crate::app::i18n::EN.ed_brushes.len(),
            Brush::ALL.len(),
            "the palette and its labels have drifted apart"
        );
    }

    /// The spawner key turns its hole through the four directions and then
    /// removes it, so the whole cycle is reachable from one key.
    #[test]
    fn spawner_directions_run_out_after_four() {
        let dirs: Vec<Direction> =
            std::iter::successors(Some(Direction::Right), |d| next_dir(*d)).collect();
        assert_eq!(
            dirs,
            [
                Direction::Right,
                Direction::Down,
                Direction::Left,
                Direction::Up
            ]
        );
        assert_eq!(next_dir(Direction::Up), None, "the cycle ends, and removes");
    }

    /// The crab key walks every kind in both handednesses and then leaves
    /// the tile bare; crabs refuse to stand on anything but open sand.
    #[test]
    fn the_crab_key_cycles_twelve_then_clears() {
        let mut board = sand();
        let mut seen = Vec::new();
        for _ in 0..12 {
            cycle_crab(&mut board, 3, 3);
            let crab = board.crabs().first().copied().expect("a crab is standing");
            seen.push((crab.kind, crab.handed));
        }
        seen.dedup();
        assert_eq!(seen.len(), 12, "every kind, both hands: {seen:?}");
        cycle_crab(&mut board, 3, 3);
        assert!(board.crabs().is_empty(), "the thirteenth press clears it");

        // Not on a rock.
        board.set_tile(4, 3, TileKind::Rock);
        cycle_crab(&mut board, 4, 3);
        assert!(board.crabs().is_empty(), "crabs start on open sand only");
    }
}
