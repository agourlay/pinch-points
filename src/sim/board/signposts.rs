//! Signposts: placing them under the cap, wearing them out, and letting
//! them wash away.
//!
//! The cap and the expiry are the versus balance valves (spec 3.3): three
//! standing at once, a fourth evicting the oldest, and every one of them
//! fading after ten seconds so no fortification is permanent.

use super::*;

/// Why the sim turned an action down. Read off the same branches that
/// decide it, so a bot hearing a reason is hearing the rule that applied.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refusal {
    /// The action named a seat that does not exist.
    NoSeat,
    OffBoard,
    Rock,
    Castle,
    Spawner,
    Turnstile,
    Kelp,
    Pool,
    /// Somebody else's signpost stands there.
    RivalPost,
    /// Every post is spent and this board refuses rather than evicts.
    OutOfPosts,
    /// A removal aimed at a tile with no signpost on it.
    NoPost,
}

impl Refusal {
    /// The tile's own objection to a signpost, if it has one: only sand
    /// takes one.
    fn of_tile(tile: TileKind) -> Option<Refusal> {
        match tile {
            TileKind::Empty => None,
            TileKind::Rock => Some(Refusal::Rock),
            TileKind::Castle(_) => Some(Refusal::Castle),
            TileKind::Spawner(_) => Some(Refusal::Spawner),
            TileKind::Turnstile { .. } => Some(Refusal::Turnstile),
            TileKind::Kelp => Some(Refusal::Kelp),
            TileKind::Pool => Some(Refusal::Pool),
        }
    }

    /// The protocol's word for it (see `docs/bot-protocol.md`).
    pub fn token(self) -> &'static str {
        match self {
            Refusal::NoSeat => "no_seat",
            Refusal::OffBoard => "off_board",
            Refusal::Rock => "rock",
            Refusal::Castle => "castle",
            Refusal::Spawner => "spawner",
            Refusal::Turnstile => "turnstile",
            Refusal::Kelp => "kelp",
            Refusal::Pool => "pool",
            Refusal::RivalPost => "rival_post",
            Refusal::OutOfPosts => "out_of_posts",
            Refusal::NoPost => "no_post",
        }
    }
}

impl Board {
    /// Whether a placement at `(x, y)` would succeed, without mutating.
    /// Mirrors [`Board::place_signpost`] exactly; the UI uses it for instant
    /// denied feedback on a queued (not yet applied) action.
    pub fn can_place_signpost(&self, player: PlayerId, x: u8, y: u8) -> bool {
        self.placement_refusal(player, x, y).is_none()
    }

    /// Why a placement at `(x, y)` would be refused, or `None` when it
    /// would stand. The rule itself: [`Board::can_place_signpost`] is this
    /// with the reason thrown away, so the two cannot disagree, and a bot
    /// is told the same reason the sim acted on.
    pub fn placement_refusal(&self, player: PlayerId, x: u8, y: u8) -> Option<Refusal> {
        if seat(player).is_none() {
            return Some(Refusal::NoSeat);
        }
        if !self.in_bounds(i32::from(x), i32::from(y)) {
            return Some(Refusal::OffBoard);
        }
        let t = self.index(i32::from(x), i32::from(y)) as usize;
        if let Some(refusal) = Refusal::of_tile(self.grid.tiles[t]) {
            return Some(refusal);
        }
        match self.signposts[t] {
            // Your own signpost re-points in place; a rival's blocks.
            Some(sp) => (sp.owner != player).then_some(Refusal::RivalPost),
            // Empty tile: at the cap, only the evicting rule still places.
            None => (self.signpost_count(player) >= usize::from(self.signpost_cap_of(player))
                && self.rules.cap_policy != CapPolicy::Evict)
                .then_some(Refusal::OutOfPosts),
        }
    }

    /// Why a removal at `(x, y)` would do nothing, or `None` when it would
    /// take a post up. The same rule as [`Board::remove_signpost`].
    pub fn removal_refusal(&self, player: PlayerId, x: u8, y: u8) -> Option<Refusal> {
        if !self.in_bounds(i32::from(x), i32::from(y)) {
            return Some(Refusal::OffBoard);
        }
        let t = self.index(i32::from(x), i32::from(y)) as usize;
        match self.signposts[t] {
            Some(sp) if sp.owner == player => None,
            Some(_) => Some(Refusal::RivalPost),
            None => Some(Refusal::NoPost),
        }
    }

    /// Whether a refusal at `(x, y)` is the *inventory* talking rather than
    /// the tile: the player has spent their posts and this board rejects
    /// rather than evicts.
    ///
    /// The one branch of [`Board::can_place_signpost`] a player can fix by
    /// picking up a signpost instead of by aiming somewhere else, which the
    /// UI says differently. Read off the same rule so the two cannot drift:
    /// under `Evict` it is never true, the placement succeeding and taking
    /// the oldest in trade.
    pub fn out_of_signposts(&self, player: PlayerId, x: u8, y: u8) -> bool {
        // The inventory has to be the *only* thing in the way: a rock with
        // a spent inventory refuses for two reasons, and a post in hand
        // would not have gone there either. The refusal names the tile
        // first, so it says "out of posts" only when nothing else would.
        self.placement_refusal(player, x, y) == Some(Refusal::OutOfPosts)
    }

    /// Spec §3.3: signposts go on empty sand only, not on castles, rocks,
    /// spawners, or a tile that already has one. At the cap, the outcome
    /// depends on the board's `CapPolicy`: evict the player's oldest (versus)
    /// or reject the placement (puzzle inventory).
    pub fn place_signpost(&mut self, player: PlayerId, x: u8, y: u8, dir: Direction) -> bool {
        if !self.can_place_signpost(player, x, y) {
            return false;
        }
        let t = self.index(i32::from(x), i32::from(y)) as usize;
        // Re-pointing your own signpost refreshes it to Full and makes it
        // your newest for cap eviction; the count is unchanged so the cap
        // never triggers.
        if self.signposts[t].is_none()
            && self.signpost_count(player) >= usize::from(self.signpost_cap_of(player))
        {
            // CapPolicy::Evict (Reject was filtered above): drop the oldest.
            // At the cap there is one to evict, unless the cap is zero,
            // which every parser refuses under Evict; a board built by
            // hand that way gets a refusal here rather than a panic.
            let Some(i) = self.oldest_signpost(player) else {
                return false;
            };
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
        self.hands[usize::from(player)] = Some((t as u16, self.tick));
    }

    /// Remaining life of a signpost as a 0..=1 fraction (always 1 under
    /// puzzle rules, where posts are permanent).
    pub fn signpost_fade(&self, sp: &Signpost) -> f32 {
        match self.rules.cap_policy {
            CapPolicy::Reject => 1.0,
            CapPolicy::Evict => {
                let age = self.tick.saturating_sub(sp.placed) as f32;
                (1.0 - age / f32::from(SIGNPOST_LIFETIME as u16)).max(0.0)
            }
        }
    }

    pub(super) fn expire_signposts(&mut self) {
        if self.rules.cap_policy != CapPolicy::Evict {
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

    /// Where a player last put a signpost down and when: `(x, y, tick)`,
    /// whether or not the post still stands.
    ///
    /// The anchor for a bot's cursor (see [`crate::sim::bot_action`]): the
    /// last tile its hand reached, so the walk to the next can be charged
    /// for. Read from the board, so the bot stays a pure function of the
    /// state and every peer derives the same move for an AI seat. Read off
    /// the posts instead, a hand whose posts had all gone (Fresh Sand, a
    /// gull, its own pickup) was nowhere, and placed anywhere at once.
    pub fn last_placement_of(&self, player: PlayerId) -> Option<(u8, u8, u64)> {
        let (tile, at) = (*self.hands.get(usize::from(player))?)?;
        let (x, y) = self.coords_u8(tile);
        Some((x, y, at))
    }

    /// The slot holding `player`'s oldest signpost: the first planted or
    /// re-pointed, which is the one the cap takes and the one that runs out
    /// first.
    fn oldest_signpost(&self, player: PlayerId) -> Option<usize> {
        self.signposts
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| slot.filter(|sp| sp.owner == player).map(|sp| (sp.seq, i)))
            .min()
            .map(|(_, i)| i)
    }

    /// The signpost `player` would lose to the next one they plant, if the
    /// next one costs them one: under the evicting rule at the cap, their
    /// oldest. `None` with room to spare, and always under puzzle rules,
    /// where a full inventory refuses instead.
    ///
    /// The UI marks this post as the one about to go. Asked of the same
    /// slot the eviction takes, so the mark cannot point at the wrong one.
    pub fn next_to_go(&self, player: PlayerId) -> Option<(u8, u8)> {
        if self.rules.cap_policy != CapPolicy::Evict
            || self.signpost_count(player) < usize::from(self.signpost_cap_of(player))
        {
            return None;
        }
        self.oldest_signpost(player)
            .map(|i| self.coords_u8(i as u16))
    }

    /// How much life each of `player`'s signposts has left, oldest first,
    /// into `out` (cleared first): the order the cap takes them in, and
    /// under versus rules the order they run out in.
    pub fn signpost_lives(&self, player: PlayerId, out: &mut Vec<f32>) {
        out.clear();
        let mut posts: Vec<&Signpost> = self
            .signposts
            .iter()
            .flatten()
            .filter(|sp| sp.owner == player)
            .collect();
        posts.sort_by_key(|sp| sp.seq);
        out.extend(posts.into_iter().map(|sp| self.signpost_fade(sp)));
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

    /// [`Board::signpost_at`] for coordinates the sim did not choose: an
    /// action's, which a bot's reply or a peer's input can aim anywhere a
    /// byte reaches. Off the board there is no post, rather than a panic.
    pub fn try_signpost_at(&self, x: u8, y: u8) -> Option<Signpost> {
        if !self.in_bounds(i32::from(x), i32::from(y)) {
            return None;
        }
        self.signpost_at(x, y)
    }

    /// `action` as this board can take it: a placement or a removal aimed
    /// off the board is no action at all. The tick refuses one anyway, so
    /// the board plays the same either way; what changes is that nothing
    /// reading the action beside the tick ever meets a tile that is not
    /// there.
    pub fn on_board(&self, action: PlayerAction) -> PlayerAction {
        match action {
            PlayerAction::Place { x, y, .. } | PlayerAction::Remove { x, y }
                if !self.in_bounds(i32::from(x), i32::from(y)) =>
            {
                PlayerAction::None
            }
            PlayerAction::None
            | PlayerAction::Place { .. }
            | PlayerAction::Remove { .. }
            | PlayerAction::CallEvent(_) => action,
        }
    }

    /// The current signpost cap rule, for serialization.
    pub fn signpost_rule(&self) -> (u8, CapPolicy) {
        (self.rules.signpost_cap, self.rules.cap_policy)
    }
}
