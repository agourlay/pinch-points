//! Z and Shift+Z: the bench's edits taken back and made again.
//!
//! Each edit leaves the level as it then stood behind it, as the text a
//! file would hold ([`snapshot`]): the one form that carries everything an
//! edit can change, the board and the level's own lines alike (its
//! signposts, kind, goal, tables, roll and solution). Every edit goes
//! through [`EditorState::edited`], which is where the history is
//! kept, so no edit can be missed by it.

use super::{EditorState, level_here};
use crate::sim::Board;
use std::collections::VecDeque;

/// How many edits back Z reaches. A level is a few hundred bytes of text,
/// so this is a few hundred kilobytes at the most.
pub(super) const DEPTH: usize = 100;

/// The name every snapshot is taken under: naming a level is not an edit
/// and is not taken back.
const SNAPSHOT_NAME: &str = "-";

/// The level on the bench as a snapshot: [`level_here`] under one name.
pub(super) fn snapshot(state: &EditorState, board: &Board) -> String {
    level_here(state, board, SNAPSHOT_NAME).to_text()
}

/// The bench's past and the futures undone from it.
#[derive(Default, Debug)]
pub(super) struct History {
    /// The bench as it stands, as of the last edit or since the level came
    /// onto it; `None` until then, when there is nothing to go back to.
    now: Option<String>,
    /// The levels it stood as before, the newest last.
    back: VecDeque<String>,
    /// The levels undone, the next one to redo last. Any new edit drops
    /// them: it starts another future.
    ahead: Vec<String>,
}

impl History {
    /// A history that begins with the bench as `now`: what a level brought
    /// onto the bench starts from, with nothing to undo.
    pub(super) fn starting_at(now: String) -> Self {
        History {
            now: Some(now),
            ..History::default()
        }
    }

    /// Whether the bench reads `text` as of the last edit: an edit that
    /// leaves it so changed nothing.
    pub(super) fn is_now(&self, text: &str) -> bool {
        self.now.as_deref() == Some(text)
    }

    /// The bench reads `now` without an edit having made it so: a check's
    /// way through, stored as the level's own. Nothing to undo, but the
    /// level an undo comes back to carries it.
    pub(super) fn amend(&mut self, now: String) {
        if self.now.is_some() {
            self.now = Some(now);
        }
    }

    /// The bench reads `after` once an edit is done. `merge`: the edit
    /// carries on the one before it, a stroke dragged on over another
    /// tile, which Z takes back whole rather than a tile at a time.
    pub(super) fn record(&mut self, after: String, merge: bool) {
        if self.is_now(&after) {
            return;
        }
        if let Some(before) = self.now.replace(after)
            && !merge
        {
            if self.back.len() == DEPTH {
                self.back.pop_front();
            }
            self.back.push_back(before);
        }
        self.ahead.clear();
    }

    /// The level before the last edit, which the bench goes back to.
    pub(super) fn undo(&mut self) -> Option<String> {
        let before = self.back.pop_back()?;
        if let Some(now) = self.now.replace(before.clone()) {
            self.ahead.push(now);
        }
        Some(before)
    }

    /// The level the last undo went back from.
    pub(super) fn redo(&mut self) -> Option<String> {
        let next = self.ahead.pop()?;
        if let Some(now) = self.now.replace(next.clone()) {
            self.back.push_back(now);
        }
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(n: usize) -> String {
        format!("level {n}")
    }

    /// Undo walks back through the edits and redo forward again, and an
    /// edit after an undo starts a new future.
    #[test]
    fn undo_and_redo_walk_the_edits() {
        let mut history = History::starting_at(text(0));
        assert_eq!(history.undo(), None, "nothing before the level came in");
        history.record(text(1), false);
        history.record(text(2), false);
        assert_eq!(history.undo(), Some(text(1)));
        assert_eq!(history.undo(), Some(text(0)));
        assert_eq!(history.undo(), None);
        assert_eq!(history.redo(), Some(text(1)));
        history.record(text(3), false);
        assert_eq!(history.redo(), None, "a new edit drops what was undone");
        assert_eq!(history.undo(), Some(text(1)));
    }

    /// A stroke is one step back, an edit that changed nothing is none,
    /// and the oldest edits go once the history is full.
    #[test]
    fn strokes_merge_and_the_depth_holds() {
        let mut history = History::starting_at(text(0));
        history.record(text(1), false);
        history.record(text(2), true);
        history.record(text(2), false);
        assert_eq!(history.undo(), Some(text(0)), "the stroke went whole");
        let mut full = History::starting_at(text(0));
        for n in 1..=DEPTH + 5 {
            full.record(text(n), false);
        }
        let mut steps = 0;
        while full.undo().is_some() {
            steps += 1;
        }
        assert_eq!(steps, DEPTH);
    }
}
