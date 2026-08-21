//! Signposts: placing them under the cap, wearing them out, and letting
//! them wash away.
//!
//! The cap and the expiry are the versus balance valves (spec 3.3): three
//! standing at once, a fourth evicting the oldest, and every one of them
//! fading after ten seconds so no fortification is permanent.

use super::*;

impl Board {
    /// Spec §3.3: signposts go on empty sand only, not on castles, rocks,
    /// spawners, or a tile that already has one. At the cap, the outcome
    /// depends on the board's `CapPolicy`: evict the player's oldest (versus)
    /// or reject the placement (puzzle inventory).
    /// Whether a placement at `(x, y)` would succeed, without mutating.
    /// Mirrors [`Board::place_signpost`] exactly; the UI uses it for instant
    /// denied feedback on a queued (not yet applied) action.
    pub fn can_place_signpost(&self, player: PlayerId, x: u8, y: u8) -> bool {
        if seat(player).is_none() || !self.in_bounds(i32::from(x), i32::from(y)) {
            return false;
        }
        let t = self.index(i32::from(x), i32::from(y)) as usize;
        if self.tiles[t] != TileKind::Empty {
            return false;
        }
        match self.signposts[t] {
            // Your own signpost re-points in place; a rival's blocks.
            Some(sp) => sp.owner == player,
            // Empty tile: at the cap, only the evicting rule still places.
            None => {
                self.signpost_count(player) < self.signpost_cap as usize
                    || self.cap_policy == CapPolicy::Evict
            }
        }
    }

    /// Whether a refusal at `(x, y)` is the *inventory* talking rather than
    /// the tile: the player has spent their posts and this board rejects
    /// rather than evicts.
    ///
    /// The one branch of [`Board::can_place_signpost`] a player can fix by
    /// picking up a signpost instead of by aiming somewhere else, and the
    /// UI says so differently. Read off the same rule so the two cannot
    /// drift: under `Evict` this is never true, because the placement
    /// succeeds and takes the oldest in trade.
    pub fn out_of_signposts(&self, player: PlayerId, x: u8, y: u8) -> bool {
        if self.cap_policy != CapPolicy::Reject
            || seat(player).is_none()
            || !self.in_bounds(i32::from(x), i32::from(y))
        {
            return false;
        }
        let t = self.index(i32::from(x), i32::from(y)) as usize;
        // The inventory has to be the *only* thing in the way. A rock with
        // a spent inventory refuses for two reasons at once, and "you have
        // none left" is the wrong one to say: a post in hand would not have
        // gone there either.
        self.tiles[t] == TileKind::Empty
            && self.signposts[t].is_none()
            && self.signpost_count(player) >= self.signpost_cap as usize
    }

    pub fn place_signpost(&mut self, player: PlayerId, x: u8, y: u8, dir: Direction) -> bool {
        if !self.can_place_signpost(player, x, y) {
            return false;
        }
        let t = self.index(i32::from(x), i32::from(y)) as usize;
        // Re-pointing your own signpost refreshes it to Full and makes it
        // your newest for cap eviction; the count is unchanged so the cap
        // never triggers.
        if self.signposts[t].is_none() && self.signpost_count(player) >= self.signpost_cap as usize
        {
            // CapPolicy::Evict (Reject was filtered above): drop the oldest.
            let oldest = self
                .signposts
                .iter()
                .enumerate()
                .filter_map(|(i, slot)| slot.filter(|sp| sp.owner == player).map(|sp| (sp.seq, i)))
                .min();
            let (_, i) = oldest.expect("at cap implies at least one signpost");
            self.signposts[i] = None;
        }
        self.stamp_signpost(t, player, dir);
        true
    }

    /// Write a fresh full-health signpost into slot `t`, taking the next
    /// sequence number (which makes it the player's newest for eviction).
    pub(super) fn stamp_signpost(&mut self, t: usize, player: PlayerId, dir: Direction) {
        let seq = self.signpost_seq;
        self.signpost_seq += 1;
        self.signposts[t] = Some(Signpost {
            dir,
            owner: player,
            health: SignpostHealth::Full,
            seq,
            placed: self.tick,
        });
    }

    /// Remaining life of a signpost as a 0..=1 fraction (always 1 under
    /// puzzle rules, where posts are permanent).
    pub fn signpost_fade(&self, sp: &Signpost) -> f32 {
        match self.cap_policy {
            CapPolicy::Reject => 1.0,
            CapPolicy::Evict => {
                let age = self.tick.saturating_sub(sp.placed) as f32;
                (1.0 - age / f32::from(SIGNPOST_LIFETIME as u16)).max(0.0)
            }
        }
    }

    pub(super) fn expire_signposts(&mut self) {
        if self.cap_policy != CapPolicy::Evict {
            return;
        }
        let now = self.tick;
        for slot in &mut self.signposts {
            if let Some(sp) = slot
                && now.saturating_sub(sp.placed) >= u64::from(SIGNPOST_LIFETIME)
            {
                *slot = None;
            }
        }
    }

    /// Where a player's most recent signpost stands and when they placed it:
    /// `(x, y, tick)`.
    ///
    /// This is the anchor for a bot's cursor (see [`crate::sim::bot_action`]):
    /// the last tile it reached, so the walk to the next one can be charged
    /// for. Reading it from the board keeps the bot a pure function of the
    /// state, so every peer of an online match derives the same move for an
    /// AI seat.
    pub fn newest_signpost_of(&self, player: PlayerId) -> Option<(u8, u8, u64)> {
        self.signposts
            .iter()
            .enumerate()
            .filter_map(|(tile, slot)| {
                let sp = slot.as_ref().filter(|sp| sp.owner == player)?;
                Some((tile as u16, sp.seq, sp.placed))
            })
            .max_by_key(|&(_, seq, _)| seq)
            .map(|(tile, _, placed)| {
                let (x, y) = self.coords(tile);
                (x as u8, y as u8, placed)
            })
    }

    /// How many signposts `player` currently has on the board.
    pub fn signpost_count(&self, player: PlayerId) -> usize {
        self.signposts
            .iter()
            .flatten()
            .filter(|sp| sp.owner == player)
            .count()
    }

    /// Players may only remove their own signposts.
    pub fn remove_signpost(&mut self, player: PlayerId, x: u8, y: u8) -> bool {
        if !self.in_bounds(i32::from(x), i32::from(y)) {
            return false;
        }
        let t = self.index(i32::from(x), i32::from(y)) as usize;
        match self.signposts[t] {
            Some(sp) if sp.owner == player => {
                self.signposts[t] = None;
                true
            }
            _ => false,
        }
    }

    pub fn signpost_at(&self, x: u8, y: u8) -> Option<Signpost> {
        assert!(self.in_bounds(i32::from(x), i32::from(y)));
        self.signposts[self.index(i32::from(x), i32::from(y)) as usize]
    }

    /// The current signpost cap rule, for serialization.
    pub fn signpost_rule(&self) -> (u8, CapPolicy) {
        (self.signpost_cap, self.cap_policy)
    }
}
