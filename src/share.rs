//! Share codes: a beach, a level, or a whole round as one line of text.
//!
//! A code is `PP<kind><body>`, where the body is the payload compressed
//! ([`crate::lzw`]) and written in an alphabet chosen to survive being read
//! aloud, retyped, and passed through a chat window that likes to capitalise
//! things. The last character is a checksum, so a code with a typo in it is
//! refused rather than half-loaded.
//!
//! The alphabet is Crockford's base32: ten digits and twenty-two letters,
//! with `I`, `L`, `O` and `U` left out: the first three because they are
//! the ones people confuse with `1` and `0`, and the last because leaving
//! it out means no code ever spells anything unfortunate. Decoding maps the
//! confusable characters back, so a code someone typed as `IL0` still reads.

use crate::lzw;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// What a code carries. One letter each, in the code's third character, so a
/// code says what it is before anything tries to load it. Pasting a level
/// where a round was wanted should say so, not fail obscurely.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A beach mid-play: the board exactly as it stands, and the table
    /// it is being played at. Pasting one puts you where its sender was.
    Beach,
    /// A level, as the level format's own text.
    Level,
    /// A recorded round, as the replay format's own text.
    Round,
}

impl Kind {
    fn letter(self) -> char {
        match self {
            Kind::Beach => 'B',
            Kind::Level => 'L',
            Kind::Round => 'R',
        }
    }

    fn from_letter(letter: char) -> Option<Kind> {
        match letter {
            'B' => Some(Kind::Beach),
            'L' => Some(Kind::Level),
            'R' => Some(Kind::Round),
            _ => None,
        }
    }
}

/// Group size for readability. A code arrives as one run of characters and
/// is shown in fives, which is how people read a licence key.
const GROUP: usize = 5;

/// Write a payload as a share code, hyphenated into readable groups.
pub fn encode(kind: Kind, payload: &[u8]) -> String {
    let packed = lzw::compress(payload, 8);
    let mut body = base32_encode(&packed);
    body.push(ALPHABET[usize::from(checksum(&body))] as char);
    let mut out = format!("PP{}", kind.letter());
    for (i, ch) in body.chars().enumerate() {
        if i > 0 && i.is_multiple_of(GROUP) {
            out.push('-');
        }
        out.push(ch);
    }
    out
}

/// Read a share code back, or `None` if it is not one: wrong prefix, a
/// character that is not in the alphabet, or a checksum that does not match
/// what was typed.
///
/// Deliberately forgiving about everything that does not change the
/// meaning (case, spaces, hyphens, line breaks, and the four letters the
/// alphabet leaves out) because a code is something a person retypes from
/// another screen, or pastes off a clipboard that carries whatever the
/// textarea or the mail client wrapped it in.
pub fn decode(code: &str) -> Option<(Kind, Vec<u8>)> {
    let mut chars = code
        .chars()
        .filter(|ch| !matches!(ch, '-' | ' ' | '\t' | '\r' | '\n'));
    let (p, q) = (chars.next()?, chars.next()?);
    if !p.eq_ignore_ascii_case(&'P') || !q.eq_ignore_ascii_case(&'P') {
        return None;
    }
    let kind = Kind::from_letter(chars.next()?.to_ascii_uppercase())?;
    let body: String = chars.map(tidy).collect();
    let (digits, check) = body.split_at_checked(body.len().checked_sub(1)?)?;
    if check.chars().next()? != ALPHABET[usize::from(checksum(digits))] as char {
        return None;
    }
    let packed = base32_decode(digits)?;
    let payload = lzw::decompress(&packed, 8)?;
    Some((kind, payload))
}

/// The characters people substitute, mapped back to what they meant.
fn tidy(ch: char) -> char {
    match ch.to_ascii_uppercase() {
        'I' | 'L' => '1',
        'O' => '0',
        'U' => 'V',
        other => other,
    }
}

/// A checksum over the body. Not a hash: it is here to catch a mistyped
/// character, a swapped pair, or a dropped group, and one character of code
/// is the right price for that.
///
/// Two details carry the guarantee that *every* single wrong character is
/// caught. The sum is over each character's **alphabet index**, not its byte:
/// the byte values are not contiguous, and `'0'` and `'P'` happen to sit 32
/// apart, so a swap between them would vanish under the modulus. And the
/// weights are **odd**, which makes them invertible modulo 32, so one wrong
/// character always moves the sum, where an even weight can be cancelled
/// by a difference that shares its factors of two.
///
/// The cost of that choice, and it is the right way round: adjacent
/// transpositions move the sum by twice the difference of the two indices,
/// so the one pair this misses is two characters exactly 16 apart in the
/// alphabet. Both guarantees at once are beyond a single base-32 check
/// character by a weighted sum: full substitution cover needs odd weights,
/// and the difference of two odd weights is always even. Mistyping one
/// character is the common error; transposing a pair that happens to be 16
/// apart is not.
fn checksum(body: &str) -> u8 {
    let mut sum = 0u32;
    for (i, ch) in body.chars().enumerate() {
        let value = index_of(ch).map_or(0, u32::from);
        let weight = 2 * (i as u32 % 16) + 1;
        sum = sum.wrapping_add(value.wrapping_mul(weight));
    }
    (sum % 32) as u8
}

/// Where `ch` sits in the alphabet, if it is in it at all.
fn index_of(ch: char) -> Option<u8> {
    // Compared as a byte only once it is one: `as u8` truncates, and a
    // character past ASCII whose low byte happened to spell a digit was
    // being read as that digit.
    let byte = u8::try_from(ch).ok()?;
    ALPHABET
        .iter()
        .position(|&a| a == byte)
        .map(|index| index as u8)
}

fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    let (mut acc, mut bits) = (0u32, 0u8);
    for &byte in bytes {
        acc = (acc << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((acc >> bits) & 0x1F) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((acc << (5 - bits)) & 0x1F) as usize] as char);
    }
    out
}

fn base32_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u8);
    for ch in text.chars() {
        acc = (acc << 5) | u32::from(index_of(ch)?);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xFF) as u8);
        }
    }
    // The tail bits are the encoder's padding, not data.
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip_at_every_size() {
        for payload in [
            vec![],
            vec![0u8],
            b"wrap: on\n".to_vec(),
            (0..500u32).map(|i| (i % 251) as u8).collect(),
        ] {
            for kind in [Kind::Beach, Kind::Level, Kind::Round] {
                let code = encode(kind, &payload);
                assert_eq!(decode(&code), Some((kind, payload.clone())), "{code}");
            }
        }
    }

    /// A code is something a person reads off one screen and types into
    /// another, so everything that survives that has to survive this.
    #[test]
    fn a_retyped_code_still_reads() {
        let code = encode(Kind::Level, b"a small beach");
        let expected = decode(&code).expect("the code as written");
        for variant in [
            code.to_lowercase(),
            code.replace('-', ""),
            code.replace('-', " "),
            format!("  {code}  ").replace(' ', ""),
        ] {
            assert_eq!(decode(&variant), Some(expected.clone()), "{variant}");
        }
        // And the letters the alphabet leaves out map back to what they
        // looked like, so a code read aloud survives the trip.
        let muddled: String = code
            .chars()
            .map(|ch| match ch {
                '1' => 'I',
                '0' => 'O',
                other => other,
            })
            .collect();
        assert_eq!(decode(&muddled), Some(expected), "{muddled}");
    }

    /// A code more often arrives on the clipboard than through the
    /// keyboard, and a clipboard carries whatever wrapped it: the trailing
    /// newline of a copied line, the CRLF of a Windows mail client, the
    /// break a textarea folded it at. None of those change what the code
    /// says, so none of them may refuse it.
    #[test]
    fn a_pasted_code_still_reads() {
        let code = encode(Kind::Level, b"a small beach");
        let expected = decode(&code).expect("the code as written");
        for variant in [
            format!("{code}\n"),
            format!("{code}\r\n"),
            format!("\n{code}\n"),
            code.replace('-', "\n"),
            code.replace('-', "\r\n"),
        ] {
            assert_eq!(decode(&variant), Some(expected.clone()), "{variant:?}");
        }
    }

    /// The checksum earns its character: a typo is refused, not half-loaded.
    ///
    /// Every position of the code is mistyped as every other letter of the
    /// alphabet: the body, and the checksum character itself. Testing only
    /// the last character would have missed that the old checksum let a
    /// wrong body character through whenever the two bytes sat 32 apart.
    #[test]
    fn a_typo_is_refused() {
        for payload in [b"seed 12345".as_slice(), b"a", b"wrap: on\n"] {
            let code = encode(Kind::Beach, payload);
            assert!(decode(&code).is_some(), "the code itself is good");
            let chars: Vec<char> = code.chars().collect();
            for (at, &original) in chars.iter().enumerate() {
                // The `PP<kind>` prefix is not covered by the checksum; it
                // is checked outright by `decode`.
                if at < 3 || original == '-' {
                    continue;
                }
                for replacement in ALPHABET.iter().map(|&b| b as char) {
                    if replacement == original {
                        continue;
                    }
                    let mut typo = chars.clone();
                    typo[at] = replacement;
                    let typo: String = typo.into_iter().collect();
                    assert_eq!(decode(&typo), None, "{original}->{replacement} in {code}");
                }
            }
        }
    }

    /// A swapped pair, which a plain unweighted sum would wave through:
    /// every adjacent pair except the documented blind spot, two characters
    /// exactly 16 apart in the alphabet.
    #[test]
    fn a_swapped_pair_is_refused() {
        let code = encode(Kind::Beach, b"seed 12345");
        let body: Vec<char> = code.chars().filter(|c| *c != '-').collect();
        let mut swaps = 0;
        for at in 3..body.len() - 1 {
            let (a, b) = (body[at], body[at + 1]);
            let apart = index_of(a).unwrap().abs_diff(index_of(b).unwrap());
            if a == b || apart == 16 {
                continue;
            }
            let mut swapped = body.clone();
            swapped.swap(at, at + 1);
            let swapped: String = swapped.into_iter().collect();
            assert_eq!(decode(&swapped), None, "swap at {at} of {code}");
            swaps += 1;
        }
        assert!(swaps > 0, "the sample code had no adjacent pair to swap");
    }

    /// A character past ASCII whose low byte spells an alphabet letter is
    /// not that letter: the lookup used to truncate and read it as one.
    #[test]
    fn a_character_past_ascii_is_not_in_the_alphabet() {
        for &letter in ALPHABET {
            assert!(index_of(char::from(letter)).is_some());
            let lookalike = char::from_u32(u32::from(letter) + 0x100).expect("a char");
            assert_eq!(
                index_of(lookalike),
                None,
                "{lookalike:?} read as {letter:?}"
            );
        }
        let code = encode(Kind::Beach, b"seed 12345");
        let first = code.chars().next().expect("a code");
        let lookalike = char::from_u32(u32::from(first) + 0x100).expect("a char");
        let forged = code.replacen(first, &lookalike.to_string(), 1);
        assert_eq!(decode(&forged), None);
    }

    #[test]
    fn what_is_not_a_code_is_refused() {
        for junk in [
            "",
            "PP",
            "PPX-12345",
            "hello",
            "PPB", // no body at all
            "12345-67890",
        ] {
            assert_eq!(decode(junk), None, "{junk}");
        }
    }

    /// The point of compressing: a round is thirty kilobytes of mostly the
    /// same few characters, and a code nobody can carry is not a share code.
    #[test]
    fn a_repetitive_payload_shrinks() {
        let round = "000000 000000 000000 000000\n".repeat(400);
        let code = encode(Kind::Round, round.as_bytes());
        assert!(
            code.len() < round.len() / 8,
            "{} characters for {} bytes",
            code.len(),
            round.len()
        );
        assert_eq!(
            decode(&code).map(|(_, bytes)| bytes),
            Some(round.into_bytes())
        );
    }
}
