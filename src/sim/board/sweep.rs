//! Crab Monopoly's sweep: the crabs it carries home through the air
//! ([`Rules::sweep_flight`]), and the record of each one it banked, kept
//! for the render layer, which cannot tell a crab banked off open sand
//! from one eaten.

use super::*;

/// A crab on its way home in a Crab Monopoly sweep.
#[derive(Clone, Copy, Debug)]
pub struct Sweeping {
    /// The crab as it stood when it was picked up.
    pub crab: Crab,
    /// Whose castle it is flying to, and whose score it banks into.
    pub owner: PlayerId,
    /// The tick it leaves the sand.
    pub launch: u64,
    /// The tick it lands and banks.
    pub lands: u64,
    /// Whether Right Claws was running when the sweep fired, which is what
    /// the crab banks under where [`Rules::sweep_keeps_worth`] holds.
    pub claw_call: bool,
}

/// One crab the tide banked, for the render layer to find.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Swept {
    pub(super) crab: u32,
    pub(super) owner: PlayerId,
    /// The tick it happened on. The reader runs between frames and a frame
    /// can span several ticks - most likely the frame straight after a
    /// sweep, which is the busiest one the renderer ever draws - so the
    /// record has to outlive its own tick to be read at all.
    pub(super) at: u64,
    /// What it paid, as credited: under the claw call it banked under,
    /// which for a crab in flight is not the one running when it lands.
    /// Kept so nobody outside has to work that out again.
    pub(super) points: i32,
}

/// How many ticks a [`Swept`] record is kept before it is pruned: long
/// enough that a frame running several ticks behind still finds it, short
/// enough that the list is a sweep or two and never a round's worth.
///
/// A frame covers more ticks than it looks. The fixed clock runs up to a
/// quarter second's worth of steps in one frame after a hitch, eight at
/// thirty a second; a replay at 4x feeds four ticks a step, and an online
/// peer catching up two frames a step. Kept for eight, a crab banked on a
/// replay's busiest frame was gone from here by the time it was read, and
/// drawn as eaten. The worst case is 36 ticks, nine steps (the eight and
/// the overstep the clock carries into the frame) of four, and this keeps
/// half as many again and more: a test in the shell holds it to that
/// margin against the shell's own numbers.
pub const SWEEP_MEMORY: u64 = 64;

/// Crab Monopoly's crabs, those in the air and those just landed. Two
/// lists with one subject, kept together so a copy or a reset cannot take
/// one and forget the other.
#[derive(Clone, Debug, Default)]
pub(super) struct Sweeps {
    /// Crabs Crab Monopoly is carrying home, in the order they were swept:
    /// one sweep's crabs land in that order, but a second Monopoly fired
    /// while the first is in the air adds its crabs after the first's,
    /// and some of them land before the first's last ones do (see
    /// [`Rules::sweep_flight`]).
    pub(super) in_air: Vec<Sweeping>,
    /// Crabs the tide put straight into a keep, and who got them.
    ///
    /// `Monopoly` banks crabs where they stand rather than walking them
    /// home, so afterwards nothing on the board says what became of them:
    /// they are simply gone, from tiles that are not castles. The render
    /// layer tells a bank from a death by reading the tile a departed crab
    /// left, so a sweep of eighty-two crabs read as eighty-two deaths,
    /// with no sign that anybody scored.
    ///
    /// Nothing in here is ever read back by the sim, which is why it stays
    /// out of the fingerprint: it cannot make two peers play differently,
    /// and hashing it would only make this build disagree with builds it
    /// is playing identically to.
    pub(super) landed: Vec<Swept>,
}

impl Sweeps {
    /// Send a crab on its way home.
    pub(super) fn launch(&mut self, flight: Sweeping) {
        self.in_air.push(flight);
    }

    /// Drop the landing records too old for any frame still to read:
    /// kept for a few ticks rather than cleared outright, because the only
    /// reader runs between frames and a frame can cover more than one tick.
    pub(super) fn forget_before(&mut self, tick: u64) {
        self.landed.retain(|swept| swept.at + SWEEP_MEMORY > tick);
    }

    /// The landing record for `crab`, if it is still kept.
    fn lookup(&self, crab: u32) -> Option<&Swept> {
        self.landed.iter().find(|swept| swept.crab == crab)
    }

    /// Become a copy of `other`, keeping both buffers (see
    /// [`Board::copy_from`]).
    pub(super) fn copy_from(&mut self, other: &Self) {
        let Self { in_air, landed } = other;
        refill(&mut self.in_air, in_air);
        refill(&mut self.landed, landed);
    }
}

impl Board {
    /// Bank the swept crabs due this tick, as they reach their castle.
    ///
    /// Everything still in the air lands on the last tick of a round: the
    /// sim freezes at the wave, and a crab caught mid-flight by it would
    /// be points the event promised and never paid.
    ///
    /// Run after this tick's timers have counted down, so without
    /// [`Rules::sweep_keeps_worth`] a crab landing on Right Claws' last
    /// tick banks as if the call were already over. With it the clock is
    /// never read here: each crab banks under the call written on it when
    /// the sweep fired.
    pub(super) fn land_sweeps(&mut self) {
        if self.sweeps.in_air.is_empty() {
            return;
        }
        let tick = self.tick;
        let last = self
            .rules
            .round_length
            .is_some_and(|len| tick + 1 >= u64::from(len));
        let mut i = 0;
        while i < self.sweeps.in_air.len() {
            if !last && self.sweeps.in_air[i].lands > tick {
                i += 1;
                continue;
            }
            let Sweeping {
                crab,
                owner,
                claw_call,
                ..
            } = self.sweeps.in_air.remove(i);
            let claw_call = match self.rules.sweep_keeps_worth {
                true => claw_call,
                false => self.in_claw_call(),
            };
            self.bank_swept(owner, &crab, claw_call);
        }
    }

    /// Bank a crab the tide carried rather than walked, and write it down.
    ///
    /// Written down because it cannot be worked out later: the crab is
    /// gone from a tile that is not a castle, which from the outside is
    /// exactly what being eaten looks like. See [`Self::swept_home`].
    pub(super) fn bank_swept(&mut self, owner: PlayerId, crab: &Crab, claw_call: bool) {
        let points = self.credit_bank_under(owner, crab, claw_call);
        self.sweeps.landed.push(Swept {
            crab: crab.id,
            owner,
            at: self.tick,
            points,
        });
    }

    /// Crabs Crab Monopoly is carrying home, in the order they were swept,
    /// which is the order they land only while one sweep is in the air.
    pub fn sweeping(&self) -> &[Sweeping] {
        &self.sweeps.in_air
    }

    /// The seat a crab was banked *for* by the tide, if it was one of
    /// those rather than one that walked home.
    ///
    /// The render layer's question, and the only reason [`Board`] keeps the
    /// record: a crab that vanished off open sand looks eaten from the
    /// outside.
    pub fn swept_home(&self, crab: u32) -> Option<PlayerId> {
        self.sweeps.lookup(crab).map(|swept| swept.owner)
    }

    /// What a crab the tide banked paid its owner, before the floor at
    /// nothing, alongside [`Self::swept_home`].
    ///
    /// The one answer to it: a crab in flight banks under the claw call
    /// written on it when the sweep fired (see
    /// [`Rules::sweep_keeps_worth`]), not the one running when it lands,
    /// and a reader pricing it off the clock gets the wrong sign.
    pub fn swept_paid(&self, crab: u32) -> Option<i32> {
        self.sweeps.lookup(crab).map(|swept| swept.points)
    }
}
