//! The board's two shaders: the water (assets/shaders/water.wgsl), one
//! material for every body of water in a round so the rock pools and the
//! sea catch the same moving light, and the dry sand round the board
//! (assets/shaders/sand.wgsl).
//!
//! A [`WaterMaterial`] is drawn on a quad. Which water it is comes from
//! [`WaterMode`]; every board shares the few materials held in [`Waters`].

use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};

/// Which water a quad is. Its number is the shader's `mode`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaterMode {
    /// The sea along the bottom of the beach, the sand its waves run up
    /// along the quad's top, deepening away from shore.
    Sea = 0,
    /// One layer of a rock pool, cut to the puddle's shape.
    Pool = 1,
    /// The sun caught on a pool's surface.
    Glint = 2,
    /// The menu postcard's open sea, from the waterline to the horizon.
    Ocean = 3,
}

/// The shader's uniform. Field order and types are the WGSL `Water`
/// struct's; change both together.
#[derive(ShaderType, Clone, Copy, Debug)]
pub struct WaterUniform {
    pub shallow: LinearRgba,
    pub deep: LinearRgba,
    /// A mania's colour on the sea, and in its alpha how far the water
    /// takes it on; none in calm water.
    pub tint: LinearRgba,
    pub mode: u32,
    /// How strongly the moving light shows, 0 for none.
    pub caustics: f32,
    /// How tall the band of wet sand along the sea's top is.
    pub wet: f32,
    /// Over what depth of water the sea goes from shallow to deep, world
    /// units.
    pub depth_span: f32,
    /// 1, or 0 under reduced motion, which stops the water's clock: the
    /// light, the waves and the glint hold still.
    pub motion: f32,
}

impl WaterUniform {
    pub fn new(mode: WaterMode, shallow: Color, deep: Color) -> Self {
        WaterUniform {
            shallow: shallow.into(),
            deep: deep.into(),
            tint: LinearRgba::NONE,
            mode: mode as u32,
            caustics: CAUSTICS,
            wet: 0.0,
            depth_span: 1.0,
            motion: 1.0,
        }
    }
}

/// How strongly the light plays on the water. Kept low: the pools sit
/// under the crabs, and moving light pulls the eye far harder than a
/// still picture of it suggests.
const CAUSTICS: f32 = 0.16;

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct WaterMaterial {
    #[uniform(0)]
    pub water: WaterUniform,
    /// The shape a pool layer is cut to; the other modes ignore it.
    #[texture(1)]
    #[sampler(2)]
    pub mask: Handle<Image>,
}

impl Material2d for WaterMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/water.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

/// The colours of a rock pool's water, shallow rim then deep middle: the
/// flat layers' own, which tools/gen_sprites.py also bakes into the moat
/// round a big castle and the editor's pool brush. Brightened, the board's
/// pools no longer matched either, a pool beside a moat most of all.
pub const POOL_SHALLOW: Color = Color::srgb(0.44, 0.69, 0.80);
pub const POOL_DEEP: Color = Color::srgb(0.33, 0.57, 0.72);

/// The open sea on the menu's postcard: the round's sea, its waterline
/// and shallows and wet sand, deepening to the postcard's old horizon
/// blue. Built on the round's, so retuning one retunes both.
fn ocean_uniform() -> WaterUniform {
    let mut water = crate::app::board_render::sea_uniform();
    water.mode = WaterMode::Ocean as u32;
    water.deep = Color::srgb(0.16, 0.36, 0.52).into();
    water.caustics = CAUSTICS * 0.8;
    water
}

/// The beach's own colour, under everything: the sand shader's shade in
/// a dune's lee, which is also what shows for the frames the shaders take
/// to compile on the first board after launch, rather than the engine's
/// grey with the margin's scenery floating on it.
pub const SAND: Color = Color::srgb(0.78, 0.65, 0.46);

/// The quad and the shared materials every board's water is drawn with.
#[derive(Clone, Default)]
pub struct Waters {
    /// A 1x1 quad, sized by its transform's scale.
    pub quad: Handle<Mesh>,
    pub pool_shallow: Handle<WaterMaterial>,
    pub pool_deep: Handle<WaterMaterial>,
    pub glint: Handle<WaterMaterial>,
    /// The sea's, its mania colour rewritten as manias come and go.
    pub sea: Handle<WaterMaterial>,
    /// The postcard's open sea.
    pub ocean: Handle<WaterMaterial>,
    /// The dry beach under everything.
    pub sand: Handle<SandMaterial>,
    /// The board's own sand, one quad over the whole board.
    pub board_sand: Handle<SandMaterial>,
}

impl Waters {
    /// Built with the rest of [`crate::app::art::Art`], cutting the pools
    /// to the puddle sprite's shape.
    pub fn new(world: &mut World, puddle: &Handle<Image>) -> Self {
        let quad = world
            .resource_mut::<Assets<Mesh>>()
            .add(Rectangle::new(1.0, 1.0));
        let noise = world.resource_mut::<Assets<Image>>().add(sand_noise());
        let mut materials = world.resource_mut::<Assets<WaterMaterial>>();
        let mut layer = |mode, colour| {
            materials.add(WaterMaterial {
                water: WaterUniform::new(mode, colour, colour),
                mask: puddle.clone(),
            })
        };
        Waters {
            quad,
            pool_shallow: layer(WaterMode::Pool, POOL_SHALLOW),
            pool_deep: layer(WaterMode::Pool, POOL_DEEP),
            glint: layer(WaterMode::Glint, Color::WHITE),
            sea: materials.add(WaterMaterial {
                water: crate::app::board_render::sea_uniform(),
                mask: Handle::default(),
            }),
            ocean: materials.add(WaterMaterial {
                water: ocean_uniform(),
                mask: Handle::default(),
            }),
            sand: world
                .resource_mut::<Assets<SandMaterial>>()
                .add(SandMaterial {
                    sand: SandUniform::beach(),
                    noise: noise.clone(),
                }),
            board_sand: world
                .resource_mut::<Assets<SandMaterial>>()
                .add(SandMaterial {
                    sand: SandUniform::board(),
                    noise,
                }),
        }
    }
}

/// Stop the water under reduced motion and start it again when the
/// setting is turned off, in every shared material. Shaders read the
/// engine's clock directly, so without this the water moved for players
/// who asked nothing to.
pub(super) fn still_water(
    settings: Res<crate::app::settings::GameSettings>,
    art: Res<crate::app::art::Art>,
    mut materials: ResMut<Assets<WaterMaterial>>,
) {
    if !settings.is_changed() {
        return;
    }
    let motion = if settings.reduced_motion { 0.0 } else { 1.0 };
    let Waters {
        quad: _,
        pool_shallow,
        pool_deep,
        glint,
        sea,
        ocean,
        sand: _,
        board_sand: _,
    } = &art.water;
    for handle in [pool_shallow, pool_deep, glint, sea, ocean] {
        if materials
            .get(handle)
            .is_some_and(|m| m.water.motion != motion)
            && let Some(mut material) = materials.get_mut(handle)
        {
            material.water.motion = motion;
        }
    }
}

/// Register the material. Before `Art` is built, which makes its
/// [`Waters`].
pub(super) fn install(app: &mut App) {
    app.add_plugins((
        Material2dPlugin::<WaterMaterial>::default(),
        Material2dPlugin::<SandMaterial>::default(),
    ))
    .insert_resource(ClearColor(SAND))
    .add_systems(Update, still_water);
    // The shaders run off the view's clock, which Bevy wraps every hour:
    // a long LAN evening saw every wave and glint jump at once on the
    // hour. Nothing else reads the wrapped clock. A day keeps it to the
    // nearest hundredth of a second, finer than a frame.
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .set_wrap_period(std::time::Duration::from_secs(24 * 60 * 60));
}

/// The sand shader's uniform; the WGSL `Sand` struct's fields, in order.
#[derive(ShaderType, Clone, Copy, Debug)]
pub struct SandUniform {
    /// The sand in a dune's lee, and on its sunlit side.
    pub low: LinearRgba,
    pub high: LinearRgba,
    /// The specks of grain, each with how strongly it shows in its alpha.
    pub grain_dark: LinearRgba,
    pub grain_pale: LinearRgba,
    /// 0 for the beach round the board, 1 for the board's own sand, where
    /// `low` and `high` are its darker and lighter tile.
    pub mode: u32,
}

impl SandUniform {
    /// Warmer and lighter than the dull flat colour it replaced, and still
    /// a shade darker and more saturated than the board's own sand, so the
    /// play stays the brightest thing on screen.
    fn beach() -> SandUniform {
        SandUniform {
            low: SAND.into(),
            high: Color::srgb(0.85, 0.73, 0.53).into(),
            grain_dark: Color::srgba(0.45, 0.36, 0.24, 0.35).into(),
            grain_pale: Color::srgba(0.98, 0.94, 0.84, 0.4).into(),
            mode: 0,
        }
    }

    /// The board's sand: the two tiles' own colours, which the sand
    /// sprites were drawn in, and grain that follows the sand's shade as
    /// the sprites' specks did on each tile. The dark speck is the sand
    /// times 0.785 in linear light, which is the sprites' "a twelfth
    /// darker" (211/230 in sRGB); the pale one a third of the way to white,
    /// as strong as theirs. In linear light a factor of 0.92, as it was,
    /// darkened a speck by a third of what the tiles did, and halfway to
    /// white made the grain read as white flecks.
    fn board() -> SandUniform {
        SandUniform {
            low: Color::srgb(0.898, 0.808, 0.631).into(),
            high: Color::srgb(0.925, 0.847, 0.690).into(),
            grain_dark: LinearRgba::new(0.0, 0.0, 0.0, 0.215),
            grain_pale: LinearRgba::new(1.0, 1.0, 1.0, 0.33),
            mode: 1,
        }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct SandMaterial {
    #[uniform(0)]
    pub sand: SandUniform,
    /// The smooth noise the sand's dunes, damp patches, ripple warp and
    /// tone drift are read from (see [`sand_noise`]).
    #[texture(1)]
    #[sampler(2)]
    pub noise: Handle<Image>,
}

/// How many texels a side the sand's noise texture has, and how many
/// noise cells it spans, so a cell is `NOISE_PX / NOISE_CELLS` texels.
/// Keep `NOISE_CELLS` in step with sand.wgsl's.
const NOISE_PX: u32 = 256;
const NOISE_CELLS: u32 = 16;

/// The smooth noise the sand shader reads instead of working it out at
/// every pixel: three channels of tileable value noise, built once.
///
/// The sand was two octaves of value noise and two more single ones per
/// pixel, about twenty integer hashes, on every board pixel and every
/// beach pixel of every frame: on the integrated graphics a LAN evening
/// brings, a fifth of a frame or more. Sampled from here it is two reads
/// on the beach and three on the board.
/// It tiles every `NOISE_CELLS` noise cells, a few thousand world units at
/// the scales the shader reads it at, which is wider than any board.
///
/// Red and green are one octave each (tone drift, ripple warp); blue is
/// two (dunes, damp patches), weighted as sand.wgsl's `fbm` was.
pub fn sand_noise() -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let octave = |cells: u32, seed: u64| -> Vec<f32> {
        // A lattice of random heights, wrapped, eased between.
        let mut state = seed;
        let mut next = || {
            // splitmix64
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            ((z ^ (z >> 31)) >> 40) as f32 / (1u64 << 24) as f32
        };
        let lattice: Vec<f32> = (0..cells * cells).map(|_| next()).collect();
        let at = |x: u32, y: u32| lattice[((y % cells) * cells + (x % cells)) as usize];
        let per = NOISE_PX as f32 / cells as f32;
        (0..NOISE_PX * NOISE_PX)
            .map(|i| {
                let (px, py) = ((i % NOISE_PX) as f32 + 0.5, (i / NOISE_PX) as f32 + 0.5);
                let (fx, fy) = (px / per, py / per);
                let (x0, y0) = (fx.floor() as u32, fy.floor() as u32);
                let ease = |t: f32| t * t * (3.0 - 2.0 * t);
                let (u, v) = (ease(fx.fract()), ease(fy.fract()));
                let top = at(x0, y0) + (at(x0 + 1, y0) - at(x0, y0)) * u;
                let bottom = at(x0, y0 + 1) + (at(x0 + 1, y0 + 1) - at(x0, y0 + 1)) * u;
                top + (bottom - top) * v
            })
            .collect()
    };
    let red = octave(NOISE_CELLS, 1);
    let green = octave(NOISE_CELLS, 2);
    let (blue_a, blue_b) = (octave(NOISE_CELLS, 3), octave(NOISE_CELLS * 2, 4));
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let data = (0..(NOISE_PX * NOISE_PX) as usize)
        .flat_map(|i| {
            let blue = (2.0 * blue_a[i] + blue_b[i]) / 3.0;
            [byte(red[i]), byte(green[i]), byte(blue), 255]
        })
        .collect();
    let mut image = Image::new(
        Extent3d {
            width: NOISE_PX,
            height: NOISE_PX,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        // Linear: these are numbers, not colours.
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

impl Material2d for SandMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/sand.wgsl".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sand's noise wraps without a seam: the step from the last
    /// column to the first, and the last row to the first, is no bigger
    /// than steps inside the texture, or the beach would draw a straight
    /// line every few thousand world units.
    #[test]
    fn the_sand_noise_tiles_without_a_seam() {
        let image = sand_noise();
        let data = image.data.as_ref().expect("built with its data");
        let n = NOISE_PX as usize;
        let at = |x: usize, y: usize, c: usize| i16::from(data[(y * n + x) * 4 + c]);
        for c in 0..3 {
            let mut inner = 0;
            let mut seam = 0;
            for i in 0..n {
                inner = inner.max((at(n / 2, i, c) - at(n / 2 - 1, i, c)).abs());
                inner = inner.max((at(i, n / 2, c) - at(i, n / 2 - 1, c)).abs());
                seam = seam.max((at(0, i, c) - at(n - 1, i, c)).abs());
                seam = seam.max((at(i, 0, c) - at(i, n - 1, c)).abs());
            }
            assert!(
                seam <= inner + 2,
                "channel {c}: seam {seam} against {inner} inside"
            );
        }
    }
}
