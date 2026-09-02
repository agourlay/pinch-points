//! GIF-flavoured LZW, both directions.
//!
//! Written for the highlight reel's GIF writer (see [`crate::gif`]) and kept
//! general because share codes want the same thing: a round of Pinch Points
//! is thirty kilobytes of mostly-repeated text, and a code nobody can carry
//! is not a share code. The decoder lived in the GIF tests for a year,
//! proving the encoder round-tripped; share codes are the first thing to
//! decode bytes a *stranger* wrote, so it moved out here and learned to
//! refuse rather than panic.
//!
//! Hand-rolled rather than pulled in as a dependency, for the same reason
//! the PRNG is (see [`Pcg32`](crate::sim::Pcg32)).

use std::collections::HashMap;
use std::collections::hash_map::Entry;

/// Codes wider than this are not allowed by the format; the dictionary is
/// cleared when it fills up.
pub const MAX_CODE_BITS: u8 = 12;

/// Bit-packer for LZW codes: GIF packs them least-significant-bit first,
/// running across byte boundaries.
struct BitWriter {
    out: Vec<u8>,
    bits: u32,
    count: u8,
}

impl BitWriter {
    fn new() -> BitWriter {
        BitWriter {
            out: Vec::new(),
            bits: 0,
            count: 0,
        }
    }

    fn write(&mut self, code: u16, width: u8) {
        self.bits |= u32::from(code) << self.count;
        self.count += width;
        while self.count >= 8 {
            self.out.push((self.bits & 0xFF) as u8);
            self.bits >>= 8;
            self.count -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 {
            self.out.push((self.bits & 0xFF) as u8);
        }
        self.out
    }
}

/// The dictionary starts with one code per symbol value plus the clear and
/// end-of-information codes, grows one code per emitted pair, and is reset
/// when it can grow no further.
///
/// `min_code_bits` is how many bits a raw symbol takes: 8 for arbitrary
/// bytes, fewer for a small GIF palette.
pub fn compress(symbols: &[u8], min_code_bits: u8) -> Vec<u8> {
    let clear = 1u16 << min_code_bits;
    let end = clear + 1;
    let mut bits = BitWriter::new();
    let mut width = min_code_bits + 1;
    let mut dict: HashMap<(u16, u8), u16> = HashMap::new();
    let mut next = end + 1;
    bits.write(clear, width);
    let mut symbols = symbols.iter().copied();
    let Some(first) = symbols.next() else {
        bits.write(end, width);
        return bits.finish();
    };
    let mut prefix = u16::from(first);
    for symbol in symbols {
        // One hash per pair: the miss and the insert it leads to are the
        // same `entry`.
        match dict.entry((prefix, symbol)) {
            Entry::Occupied(known) => {
                prefix = *known.get();
                continue;
            }
            Entry::Vacant(slot) => {
                bits.write(prefix, width);
                if next < 1 << MAX_CODE_BITS {
                    slot.insert(next);
                    next += 1;
                    if next > (1u16 << width) && width < MAX_CODE_BITS {
                        width += 1;
                    }
                    prefix = u16::from(symbol);
                    continue;
                }
            }
        }
        // Full: start over, exactly as the decoder will on seeing this.
        bits.write(clear, width);
        dict.clear();
        next = end + 1;
        width = min_code_bits + 1;
        prefix = u16::from(symbol);
    }
    bits.write(prefix, width);
    bits.write(end, width);
    bits.finish()
}

/// How much a single stream may expand to. A few hundred bytes of LZW can
/// name megabytes of output, and the streams reaching this decoder are typed
/// in by hand from somewhere else, and a share code that is nonsense should
/// cost a rejection, not the machine's memory.
const MAX_OUTPUT: usize = 1 << 22;

/// Decode a stream, the way a GIF viewer does, including the one-entry lag
/// that makes the two sides' code widths line up.
///
/// `None` for a stream this decoder cannot follow: a code naming a
/// dictionary entry that was never built, or output past the size cap.
/// Trailing bits too short to make a code are the encoder's padding and end
/// the stream, as do a missing end code and running off the data.
pub fn decompress(data: &[u8], min_code_bits: u8) -> Option<Vec<u8>> {
    decode(data, min_code_bits).map(|(out, _)| out)
}

/// [`decompress`], and how many times the stream told the decoder to start
/// over, so a test can prove it exercised the dictionary-full path rather
/// than merely believing it did.
pub(crate) fn decode(data: &[u8], min_code_bits: u8) -> Option<(Vec<u8>, usize)> {
    let clear = 1u16 << min_code_bits;
    let end = clear + 1;
    let mut width = min_code_bits + 1;
    let mut table: Vec<Vec<u8>> = Vec::new();
    let reset = |table: &mut Vec<Vec<u8>>| {
        table.clear();
        for index in 0..=u16::from(u8::MAX) {
            table.push(vec![index as u8]);
            if index == clear - 1 {
                break;
            }
        }
        table.push(Vec::new()); // clear
        table.push(Vec::new()); // end
    };
    reset(&mut table);
    let (mut acc, mut count, mut at) = (0u32, 0u8, 0usize);
    let mut out = Vec::new();
    let mut prev: Option<u16> = None;
    // The leading clear code is bookkeeping, not a restart.
    let mut restarts = 0usize;
    let mut started = false;
    loop {
        while count < width && at < data.len() {
            acc |= u32::from(data[at]) << count;
            count += 8;
            at += 1;
        }
        if count < width {
            break;
        }
        let code = (acc & ((1u32 << width) - 1)) as u16;
        acc >>= width;
        count -= width;
        if code == clear {
            if started {
                restarts += 1;
            }
            started = true;
            reset(&mut table);
            width = min_code_bits + 1;
            prev = None;
            continue;
        }
        if code == end {
            break;
        }
        let entry = if usize::from(code) < table.len() {
            table[usize::from(code)].clone()
        } else {
            // The one code a stream may name before it exists: the entry
            // being built right now. Anything further ahead is not a stream
            // this encoder wrote.
            let prefix = table.get(usize::from(prev?))?;
            let mut entry = prefix.clone();
            entry.push(*entry.first()?);
            entry
        };
        let first = *entry.first()?;
        if out.len() + entry.len() > MAX_OUTPUT {
            return None;
        }
        out.extend_from_slice(&entry);
        if let Some(prev) = prev {
            let mut new = table.get(usize::from(prev))?.clone();
            new.push(first);
            table.push(new);
            if table.len() >= 1 << width && width < MAX_CODE_BITS {
                width += 1;
            }
        }
        prev = Some(code);
    }
    Some((out, restarts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        // Empty, single, flat, and repetitive: the everyday shapes.
        let cases: Vec<Vec<u8>> = vec![
            vec![],
            vec![3],
            vec![0; 500],
            (0..4000u32).map(|i| (i % 16) as u8).collect(),
            (0..9000u32).map(|i| ((i * i) % 16) as u8).collect(),
        ];
        for symbols in cases {
            let encoded = compress(&symbols, 4);
            let decoded = decompress(&encoded, 4);
            assert_eq!(decoded.as_ref(), Some(&symbols), "{} bytes", symbols.len());
        }
        // And the same at the full byte width, as share codes use.
        let text = b"the tide is out, the tide is out, the tide is out".to_vec();
        assert_eq!(decompress(&compress(&text, 8), 8), Some(text));
    }

    /// The dictionary-full path: at 4096 entries both sides must start over,
    /// independently and in step. It is the subtlest branch in the encoder
    /// and the easiest to believe you have covered: the repetitive cases
    /// above never get past 400 entries, because LZW learns long phrases
    /// from them and stops adding new ones. Only high-entropy data fills the
    /// table, so this feeds it noise from the sim's own PRNG (no
    /// dependency, and the same bytes every run) and *asserts the restart
    /// happened*, so the coverage cannot lapse again unnoticed.
    #[test]
    fn survives_a_full_dictionary() {
        let mut rng = crate::sim::Pcg32::new(0x1ADE_C0DE, 0x9971);
        // One reel frame's worth of noise crosses 4096 entries; three of
        // them make the encoder restart repeatedly.
        let symbols: Vec<u8> = (0..3 * 144 * 112)
            .map(|_| (rng.next_u32() % 16) as u8)
            .collect();
        let encoded = compress(&symbols, 4);
        let (decoded, restarts) = decode(&encoded, 4).expect("our own stream decodes");
        assert!(
            restarts >= 2,
            "expected the dictionary to fill and reset; it reset {restarts} times"
        );
        assert_eq!(decoded, symbols, "the two sides restarted out of step");
    }

    /// Every palette size the GIF writer will round up to, at both ends of
    /// its range: a two-colour image still uses GIF's minimum two-bit codes,
    /// and a full 256-entry table uses eight.
    #[test]
    fn round_trips_at_every_code_width() {
        let mut rng = crate::sim::Pcg32::new(7, 3);
        for bits in 1..=8u8 {
            let alphabet = 1u32 << bits;
            let symbols: Vec<u8> = (0..5000)
                .map(|_| (rng.next_u32() % alphabet) as u8)
                .collect();
            let min_code_bits = bits.max(2);
            let encoded = compress(&symbols, min_code_bits);
            let decoded = decompress(&encoded, min_code_bits);
            assert_eq!(decoded.as_ref(), Some(&symbols), "{bits}-bit alphabet");
        }
    }

    /// The decoder reads bytes a stranger typed in, so it has to refuse
    /// rather than panic or run away with the machine.
    #[test]
    fn nonsense_is_refused_rather_than_followed() {
        // A code far past anything the dictionary can hold yet.
        assert_eq!(decompress(&[0xFF, 0xFF, 0xFF, 0xFF], 8), None);
        // Whatever it is handed, it either answers or declines - never both,
        // and never for long.
        for seed in 0..400u32 {
            let junk: Vec<u8> = (0..40u32)
                .map(|i| (seed.wrapping_mul(2_654_435_761).wrapping_add(i * 97) >> 5) as u8)
                .collect();
            let _ = decompress(&junk, 8);
        }
    }
}
