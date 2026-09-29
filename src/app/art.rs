//! The procedural sprite registry, loaded once at app construction and
//! shared by every rendering module. White/light shapes are tinted at
//! spawn; gulls, rocks, holes, and terrain bake their palette.

use crate::app::cycle::Cycle;
use crate::app::i18n::{ALL_LANGS, Lang};
use bevy::prelude::*;

/// Procedurally generated sprite art (tools/gen_sprites.py, and
/// tools/gen_flags.py for the language chips). White shapes are tinted at
/// spawn; gulls, rocks, and holes bake their palette.
#[derive(Resource)]
pub struct Art {
    pub arrow: Handle<Image>,
    /// The same post after weather: splintered rather than merely faint.
    pub arrow_worn: Handle<Image>,
    pub crab: Handle<Image>,
    pub claw: Handle<Image>,
    pub gull: Handle<Image>,
    pub gull_fly: Handle<Image>,
    pub rock: Handle<Image>,
    pub hole: Handle<Image>,
    pub castle: Handle<Image>,
    pub sand_a: Handle<Image>,
    pub sand_b: Handle<Image>,
    pub shadow: Handle<Image>,
    pub plank: Handle<Image>,
    pub bracket: Handle<Image>,
    pub crown: Handle<Image>,
    pub kelp: Handle<Image>,
    pub pool: Handle<Image>,
    pub log: Handle<Image>,
    pub star: Handle<Image>,
    pub puff: Handle<Image>,
    pub foam: Handle<Image>,
    pub post: Handle<Image>,
    pub crab_b: Handle<Image>,
    pub wet: Handle<Image>,
    pub cloud: Handle<Image>,
    pub boat: Handle<Image>,
    /// The castle's growth: a curtain wall, its corner towers, its moat.
    pub keep_ring: Handle<Image>,
    pub turret: Handle<Image>,
    pub moat: Handle<Image>,
    /// What a gull leaves behind.
    pub feather: Handle<Image>,
    /// A vertical alpha ramp, opaque at the top: every soft gradient in the
    /// game is this one texture, tinted, stretched and turned.
    pub ramp: Handle<Image>,
    /// Clear in the middle, dark at the rim.
    pub vignette: Handle<Image>,
    /// A hollow circle: the shockwave shape every "here" effect swells in.
    pub ring: Handle<Image>,
    /// One flag chip per language, in [`ALL_LANGS`] order. Read through
    /// [`Art::flag`] rather than indexed directly.
    pub flags: [Handle<Image>; ALL_LANGS.len()],
}

impl Art {
    /// The flag chip for a language. Built and read in the same
    /// [`ALL_LANGS`] order, so the two cannot drift apart.
    pub fn flag(&self, lang: Lang) -> Handle<Image> {
        self.flags[lang.index()].clone()
    }

    /// Whether `id` is one of these sprites.
    fn holds(&self, id: AssetId<Image>) -> bool {
        let Self {
            arrow,
            arrow_worn,
            crab,
            claw,
            gull,
            gull_fly,
            rock,
            hole,
            castle,
            sand_a,
            sand_b,
            shadow,
            plank,
            bracket,
            crown,
            kelp,
            pool,
            log,
            star,
            puff,
            foam,
            post,
            crab_b,
            wet,
            cloud,
            boat,
            keep_ring,
            turret,
            moat,
            feather,
            ramp,
            vignette,
            ring,
            flags,
        } = self;
        [
            arrow, arrow_worn, crab, claw, gull, gull_fly, rock, hole, castle, sand_a, sand_b,
            shadow, plank, bracket, crown, kelp, pool, log, star, puff, foam, post, crab_b, wet,
            cloud, boat, keep_ring, turret, moat, feather, ramp, vignette, ring,
        ]
        .into_iter()
        .chain(flags)
        .any(|handle| handle.id() == id)
    }
}

/// Give every sprite a mip chain as it finishes loading.
///
/// The sprites are drawn at 192 pixels, a tile on a 4K screen, and most
/// screens draw them far smaller: 96 at 1080p, and under 40 for the XL
/// beach in a 720p window. The GPU shrinks a texture by sampling it, and a
/// texture with no smaller copies is sampled a few texels apart and the
/// rest skipped, so a crab's outline came out broken and crawled as it
/// walked. Measured against a proper downscale, a 192-pixel sprite drawn
/// at 56 pixels was twice as far off as the old 96-pixel one. With a chain
/// the GPU reads from the level nearest the size on screen, and the big
/// sprite is at least as good as the small one at every size.
///
/// Bevy uploads whatever levels an image carries, and its default sampler
/// already filters between them; nothing makes the levels, so this does,
/// once per sprite. The image it changes is a new asset version, and the
/// `Modified` event that follows is not the one this listens for.
pub fn mipmap_sprites(
    mut events: MessageReader<AssetEvent<Image>>,
    art: Res<Art>,
    mut images: ResMut<Assets<Image>>,
) {
    for event in events.read() {
        let AssetEvent::LoadedWithDependencies { id } = *event else {
            continue;
        };
        if art.holds(id)
            && let Some(mut image) = images.get_mut(id)
        {
            with_mips(&mut image);
        }
    }
}

/// Append a full mip chain to an 8-bit sRGB RGBA image, down to 1x1.
///
/// Each level is a 2x2 box filter of the one above, averaged in linear
/// light and weighted by alpha. Linear, because averaging sRGB values
/// darkens every edge between two colours. Weighted, because the colour
/// under a transparent texel is whatever the drawing tool left there,
/// usually black, and an unweighted average bleeds it into the outline as
/// a dark fringe.
///
/// Leaves an image alone that already has levels, holds no data, or is in
/// any other format.
fn with_mips(image: &mut Image) {
    use bevy::render::render_resource::TextureFormat;
    let descriptor = &image.texture_descriptor;
    if descriptor.mip_level_count != 1
        || descriptor.format != TextureFormat::Rgba8UnormSrgb
        || descriptor.size.depth_or_array_layers != 1
    {
        return;
    }
    let Some(data) = image.data.as_mut() else {
        return;
    };
    let (mut w, mut h) = (
        descriptor.size.width as usize,
        descriptor.size.height as usize,
    );
    let to_linear: [f32; 256] = std::array::from_fn(|v| srgb_to_linear(v as f32 / 255.0));
    let mut levels = 1;
    let mut above = 0; // where the level being halved starts in `data`
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = Vec::with_capacity(nw * nh * 4);
        for y in 0..nh {
            for x in 0..nw {
                let (mut rgb, mut alpha, mut count) = ([0.0f32; 3], 0.0f32, 0.0f32);
                for sy in (y * 2)..(y * 2 + 2).min(h) {
                    for sx in (x * 2)..(x * 2 + 2).min(w) {
                        let p = above + (sy * w + sx) * 4;
                        let a = f32::from(data[p + 3]) / 255.0;
                        for (c, sum) in rgb.iter_mut().enumerate() {
                            *sum += to_linear[usize::from(data[p + c])] * a;
                        }
                        alpha += a;
                        count += 1.0;
                    }
                }
                for sum in rgb {
                    let linear = if alpha > 0.0 { sum / alpha } else { 0.0 };
                    next.push((linear_to_srgb(linear) * 255.0).round() as u8);
                }
                next.push((alpha / count * 255.0).round() as u8);
            }
        }
        above = data.len();
        data.extend_from_slice(&next);
        (w, h) = (nw, nh);
        levels += 1;
    }
    image.texture_descriptor.mip_level_count = levels;
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

impl FromWorld for Art {
    /// Built at app construction so it exists before the first state
    /// transition (the menu's OnEnter needs it immediately).
    fn from_world(world: &mut World) -> Self {
        let assets = world.resource::<AssetServer>();
        Art {
            arrow: assets.load("sprites/arrow.png"),
            arrow_worn: assets.load("sprites/arrow_worn.png"),
            crab: assets.load("sprites/crab.png"),
            claw: assets.load("sprites/claw.png"),
            gull: assets.load("sprites/gull.png"),
            gull_fly: assets.load("sprites/gull_fly.png"),
            rock: assets.load("sprites/rock.png"),
            hole: assets.load("sprites/hole.png"),
            castle: assets.load("sprites/castle.png"),
            sand_a: assets.load("sprites/sand_a.png"),
            sand_b: assets.load("sprites/sand_b.png"),
            shadow: assets.load("sprites/shadow.png"),
            plank: assets.load("sprites/plank.png"),
            bracket: assets.load("sprites/bracket.png"),
            crown: assets.load("sprites/crown.png"),
            kelp: assets.load("sprites/kelp.png"),
            pool: assets.load("sprites/pool.png"),
            log: assets.load("sprites/log.png"),
            star: assets.load("sprites/star.png"),
            puff: assets.load("sprites/puff.png"),
            foam: assets.load("sprites/foam.png"),
            post: assets.load("sprites/post.png"),
            crab_b: assets.load("sprites/crab_b.png"),
            wet: assets.load("sprites/wet.png"),
            cloud: assets.load("sprites/cloud.png"),
            boat: assets.load("sprites/boat.png"),
            keep_ring: assets.load("sprites/keep_ring.png"),
            turret: assets.load("sprites/turret.png"),
            moat: assets.load("sprites/moat.png"),
            feather: assets.load("sprites/feather.png"),
            ramp: assets.load("sprites/ramp.png"),
            vignette: assets.load("sprites/vignette.png"),
            ring: assets.load("sprites/ring.png"),
            // Named by the language's settings key, so the set follows
            // ALL_LANGS without a second table to keep in step.
            flags: ALL_LANGS.map(|lang| assets.load(format!("sprites/flag_{}.png", lang.key()))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    fn image(w: u32, h: u32, pixels: &[[u8; 4]]) -> Image {
        Image::new(
            Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            pixels.concat(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        )
    }

    /// The chain runs to 1x1, every level half the last, laid out one
    /// after another the way the upload reads them. 192 is not a power of
    /// two, and 3 halves to 1: the odd row is simply not read.
    #[test]
    fn the_chain_halves_down_to_one_pixel() {
        let mut sprite = image(192, 192, &vec![[200, 100, 50, 255]; 192 * 192]);
        with_mips(&mut sprite);
        let sizes = [192usize, 96, 48, 24, 12, 6, 3, 1];
        assert_eq!(
            sprite.texture_descriptor.mip_level_count,
            sizes.len() as u32
        );
        let bytes: usize = sizes.iter().map(|s| s * s * 4).sum();
        assert_eq!(sprite.data.as_ref().map(Vec::len), Some(bytes));
        // A flat colour stays that colour all the way down.
        let data = sprite.data.unwrap();
        assert_eq!(&data[bytes - 4..], &[200, 100, 50, 255]);

        // A flag chip is 3:2, and each side stops halving at 1 on its own.
        let mut flag = image(96, 64, &vec![[255; 4]; 96 * 64]);
        with_mips(&mut flag);
        assert_eq!(flag.texture_descriptor.mip_level_count, 7, "96 wide");
    }

    /// A transparent texel's colour is not part of the picture. Averaged
    /// in, the black under the clear half of an outline edge would darken
    /// the half that shows.
    #[test]
    fn clear_texels_lend_no_colour() {
        let red = [255, 0, 0, 255];
        let clear = [0, 0, 0, 0];
        let mut edge = image(2, 2, &[red, clear, red, clear]);
        with_mips(&mut edge);
        let data = edge.data.unwrap();
        assert_eq!(&data[16..], &[255, 0, 0, 128], "full red, half covered");
    }

    /// Black and white average to a mid grey *in light*, which is sRGB
    /// 188, not the 128 that averaging the stored numbers gives.
    #[test]
    fn levels_are_averaged_in_linear_light() {
        let (black, white) = ([0, 0, 0, 255], [255, 255, 255, 255]);
        let mut checker = image(2, 2, &[black, white, white, black]);
        with_mips(&mut checker);
        let grey = checker.data.unwrap()[16];
        assert!((186..=189).contains(&grey), "{grey}");
    }

    /// Only the plain 8-bit sprites this game draws are touched: an image
    /// that already has levels keeps them.
    #[test]
    fn an_image_with_levels_is_left_alone() {
        let mut done = image(2, 2, &[[9; 4]; 4]);
        done.texture_descriptor.mip_level_count = 2;
        let before = done.data.clone();
        with_mips(&mut done);
        assert_eq!(done.data, before);
    }

    /// A language with no flag file loads nothing: the asset server logs a
    /// miss and the settings row draws an empty gap where the chip goes.
    /// Run tools/gen_flags.py after adding a language.
    #[test]
    fn every_language_has_a_flag_on_disk() {
        for lang in ALL_LANGS {
            let path = format!("assets/sprites/flag_{}.png", lang.key());
            assert!(
                std::path::Path::new(&path).exists(),
                "no flag for {lang:?}: {path} is missing"
            );
        }
    }
}
