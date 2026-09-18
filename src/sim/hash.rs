/// FNV-1a 64-bit, used to fingerprint the full simulation state for the
/// cross-platform determinism test and future desync detection. Not a general
/// hasher: field order in `Board::state_hash` is part of the format.
pub(crate) struct Fnv(u64);

impl Fnv {
    pub fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    pub fn u8(&mut self, b: u8) {
        self.0 ^= u64::from(b);
        self.0 = self.0.wrapping_mul(0x100_0000_01b3);
    }

    pub fn u16(&mut self, v: u16) {
        for b in v.to_le_bytes() {
            self.u8(b);
        }
    }

    pub fn u32(&mut self, v: u32) {
        for b in v.to_le_bytes() {
            self.u8(b);
        }
    }

    pub fn u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.u8(b);
        }
    }

    pub fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }

    pub fn finish(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plain FNV-1a, against the reference values everybody else's is
    /// checked against.
    ///
    /// The determinism anchor is a number written down in a test and
    /// compared across machines, so what computes it has to be a thing
    /// somebody else could compute too. A drifted constant would still
    /// agree with itself on every machine and be wrong about all of them.
    #[test]
    fn the_fingerprint_is_plain_fnv_1a() {
        let feed = |bytes: &[u8]| {
            let mut fnv = Fnv::new();
            for &b in bytes {
                fnv.u8(b);
            }
            fnv.finish()
        };
        assert_eq!(feed(b""), 0xcbf2_9ce4_8422_2325, "the offset basis");
        assert_eq!(feed(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(feed(b"foobar"), 0x8594_4171_f739_67e8);
    }

    /// A number goes in little end first, and a wider one is exactly its
    /// own bytes: the fingerprint travels between machines, so the byte
    /// order cannot be the one this machine happens to keep.
    #[test]
    fn a_number_is_fed_in_little_end_first() {
        let bytes = |feed: &dyn Fn(&mut Fnv)| {
            let mut fnv = Fnv::new();
            feed(&mut fnv);
            fnv.finish()
        };
        let by_hand = |raw: &[u8]| {
            bytes(&|fnv| {
                for &b in raw {
                    fnv.u8(b);
                }
            })
        };
        assert_eq!(bytes(&|f| f.u16(0x0102)), by_hand(&[0x02, 0x01]));
        assert_eq!(
            bytes(&|f| f.u32(0x0102_0304)),
            by_hand(&[0x04, 0x03, 0x02, 0x01])
        );
        assert_eq!(
            bytes(&|f| f.u64(0x0102_0304_0506_0708)),
            by_hand(&[0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01])
        );
        // A flag is one byte, and the two of them are not the same byte.
        assert_eq!(bytes(&|f| f.bool(true)), by_hand(&[1]));
        assert_eq!(bytes(&|f| f.bool(false)), by_hand(&[0]));
        assert_ne!(bytes(&|f| f.bool(true)), bytes(&|f| f.bool(false)));
    }
}
