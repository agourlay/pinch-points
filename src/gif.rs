//! A minimal animated-GIF writer: one shared palette, one LZW-compressed
//! frame per call, looping forever.
//!
//! Hand-rolled rather than pulled in as a dependency, for the same reason
//! the PRNG is (see [`Pcg32`](crate::sim::Pcg32)): the crate stays dependency-free
//! and the bytes we write are the bytes we chose. Only what the highlight
//! reel needs is implemented: no interlacing, no local palettes, no
//! transparency.

use crate::lzw::compress;

pub struct Gif {
    width: u16,
    height: u16,
    /// Bits per palette index; the table holds `1 << palette_bits` colours.
    palette_bits: u8,
    out: Vec<u8>,
}

impl Gif {
    /// Start a GIF. The palette is padded out to the next power of two (the
    /// format has no other sizes) and may hold at most 256 colours.
    pub fn new(width: u16, height: u16, palette: &[[u8; 3]]) -> Gif {
        assert!(!palette.is_empty() && palette.len() <= 256, "palette size");
        let palette_bits = (usize::BITS - (palette.len() - 1).leading_zeros()).clamp(1, 8) as u8;
        let entries = 1usize << palette_bits;
        let mut out = Vec::new();
        out.extend_from_slice(b"GIF89a");
        // Logical screen: size, then a packed byte saying "a global colour
        // table follows, with `entries` colours".
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.push(0b1000_0000 | (palette_bits - 1));
        out.push(0); // background colour index
        out.push(0); // square pixels
        for index in 0..entries {
            let color = palette.get(index).copied().unwrap_or([0, 0, 0]);
            out.extend_from_slice(&color);
        }
        // The Netscape extension, which is how a GIF says "loop forever".
        out.extend_from_slice(b"\x21\xFF\x0BNETSCAPE2.0\x03\x01\x00\x00\x00");
        Gif {
            width,
            height,
            palette_bits,
            out,
        }
    }

    /// Append a frame of palette indices, shown for `delay` hundredths of a
    /// second. `pixels` is row-major, `width * height` long.
    pub fn add_frame(&mut self, pixels: &[u8], delay: u16) {
        assert_eq!(
            pixels.len(),
            usize::from(self.width) * usize::from(self.height),
            "frame is not the GIF's size"
        );
        // Graphic control extension: just the frame delay.
        self.out.extend_from_slice(&[0x21, 0xF9, 0x04, 0x00]);
        self.out.extend_from_slice(&delay.to_le_bytes());
        self.out.extend_from_slice(&[0x00, 0x00]);
        // Image descriptor: full-frame, no local palette, not interlaced.
        self.out.push(0x2C);
        self.out.extend_from_slice(&0u16.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes());
        self.out.extend_from_slice(&self.width.to_le_bytes());
        self.out.extend_from_slice(&self.height.to_le_bytes());
        self.out.push(0);
        // GIF's LZW never uses fewer than two bits per code.
        let min_code_bits = self.palette_bits.max(2);
        self.out.push(min_code_bits);
        let compressed = compress(pixels, min_code_bits);
        // The data goes out in sub-blocks of at most 255 bytes, each led by
        // its own length, terminated by an empty one.
        for chunk in compressed.chunks(255) {
            self.out.push(chunk.len() as u8);
            self.out.extend_from_slice(chunk);
        }
        self.out.push(0);
    }

    pub fn finish(mut self) -> Vec<u8> {
        self.out.push(0x3B); // trailer
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_is_shaped_like_a_gif() {
        let palette = [[0, 0, 0], [255, 0, 0], [0, 255, 0]];
        let mut gif = Gif::new(4, 2, &palette);
        gif.add_frame(&[0, 1, 2, 0, 2, 1, 0, 1], 10);
        gif.add_frame(&[1; 8], 10);
        let bytes = gif.finish();
        assert_eq!(&bytes[..6], b"GIF89a");
        assert_eq!(&bytes[6..10], &[4, 0, 2, 0], "size, little-endian");
        // Three colours round up to a four-entry table: 2 bits per index.
        assert_eq!(bytes[10] & 0b0000_0111, 1);
        assert_eq!(*bytes.last().expect("trailer"), 0x3B);
        assert_eq!(
            bytes.iter().filter(|&&b| b == 0x2C).count(),
            2,
            "one image descriptor per frame"
        );
    }
}
