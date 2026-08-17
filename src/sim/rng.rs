/// Minimal PCG32 (XSH-RR 64/32): the `Board`'s seeded, deterministic PRNG.
///
/// Implemented inline rather than pulled in as a dependency so the simulation
/// core stays dependency-free and the exact sequence is under our control
/// (replays and rollback depend on it never changing behind our back).
#[derive(Clone, Debug)]
pub struct Pcg32 {
    state: u64,
    inc: u64,
}

const MULTIPLIER: u64 = 6364136223846793005;

impl Pcg32 {
    pub fn new(seed: u64, stream: u64) -> Self {
        let mut rng = Pcg32 {
            state: 0,
            inc: (stream << 1) | 1,
        };
        rng.next_u32();
        rng.state = rng.state.wrapping_add(seed);
        rng.next_u32();
        rng
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(MULTIPLIER).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    pub(crate) fn hash_state(&self) -> (u64, u64) {
        (self.state, self.inc)
    }

    /// Restore a stream mid-sequence, as [`hash_state`](Pcg32::hash_state)
    /// read it. For snapshots: a board resumed from a fresh `new(seed, ..)`
    /// would replay draws it has already spent.
    pub(crate) fn from_state(state: u64, inc: u64) -> Pcg32 {
        Pcg32 { state, inc }
    }
}

#[cfg(test)]
mod tests {
    use super::Pcg32;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Pcg32::new(42, 7);
        let mut b = Pcg32::new(42, 7);
        for _ in 0..1000 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = Pcg32::new(1, 7);
        let mut b = Pcg32::new(2, 7);
        let same = (0..100).filter(|_| a.next_u32() == b.next_u32()).count();
        assert!(same < 5);
    }
}
