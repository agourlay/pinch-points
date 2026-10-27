//! The two-press question a menu asks before something it cannot undo:
//! wiping every key binding, deleting a kept round.
//!
//! The first press only asks, and names the key that answers. A second
//! press of that key, on the same thing, within [`SECOND_PRESS_SECS`] says
//! yes. Anything else pressed in between takes the question back, and a
//! question nobody answers lapses. One type for every screen that asks, so
//! the window and the rules are the same wherever a player meets them.

/// How long a first press waits for its second. Long enough to read the
/// question and press again, short enough that a press much later is a
/// new decision rather than the answer to an old one.
pub const SECOND_PRESS_SECS: f32 = 4.0;

/// A question standing since a moment, about `T`: the round a delete
/// would remove, or `()` where the screen has one thing to ask about.
///
/// Holding what was asked about is what makes the second press safe: a
/// shelf that changed under the cursor between the two presses (a file
/// dropped in, a re-read) asks again rather than deleting a round that
/// was never named.
#[derive(Clone, Debug, PartialEq)]
pub struct SecondPress<T = ()> {
    asked: Option<(f32, T)>,
}

impl<T> Default for SecondPress<T> {
    fn default() -> Self {
        Self { asked: None }
    }
}

impl<T: PartialEq> SecondPress<T> {
    /// Whether a question is standing.
    pub fn asked(&self) -> bool {
        self.asked.is_some()
    }

    /// Drop a question older than the window at `now`, in seconds since
    /// startup. Whether one lapsed, so the screen can take its words down.
    pub fn lapse(&mut self, now: f32) -> bool {
        let stale = self
            .asked
            .as_ref()
            .is_some_and(|(at, _)| now - at > SECOND_PRESS_SECS);
        if stale {
            self.asked = None;
        }
        stale
    }

    /// The key pressed at `now`, about `about`: `true` when it answers a
    /// question standing about the same thing, which is then settled, and
    /// `false` when it only asks.
    pub fn press(&mut self, now: f32, about: T) -> bool {
        match self.asked.take() {
            Some((_, standing)) if standing == about => true,
            _ => {
                self.asked = Some((now, about));
                false
            }
        }
    }

    /// Take a standing question back, as any other key does. Whether there
    /// was one, so the screen can take its words down.
    pub fn take_back(&mut self) -> bool {
        self.asked.take().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_press_soon_after_on_the_same_thing_says_yes() {
        let mut q = SecondPress::default();
        assert!(!q.press(0.0, 1), "the first press asks");
        assert!(q.asked());
        assert!(!q.lapse(SECOND_PRESS_SECS - 0.5));
        assert!(q.press(SECOND_PRESS_SECS - 0.5, 1), "the second answers");
        assert!(!q.asked(), "and settles it");
        // About something else, the second press only asks again.
        assert!(!q.press(0.0, 1));
        assert!(!q.press(1.0, 2), "a different thing is a new question");
        assert!(q.press(2.0, 2));
        // Lapsed, the next press asks afresh.
        assert!(!q.press(0.0, 3));
        assert!(q.lapse(SECOND_PRESS_SECS + 0.1));
        assert!(!q.press(SECOND_PRESS_SECS + 0.2, 3));
        // Taken back, likewise.
        assert!(q.take_back());
        assert!(!q.take_back(), "nothing left to take back");
        assert!(!q.press(10.0, 3));
    }
}
