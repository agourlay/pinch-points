// The beach's sand, in two kinds. Round the board (mode 0): two tones
// drifting into each other in broad dunes, ridged by the wind, peppered
// with grain. On the board (mode 1): the play's own checkerboard of a
// light and a darker tile, so the grid still reads, with the same wind
// ripples, damp patches where the tide lay longest, and finer grain, all
// in world space so no two tiles are the same.
//
// Self-contained like water.wgsl: no imports.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(2) uv: vec2<f32>,
}

// Keep in step with `SandUniform` in src/app/materials.rs.
struct Sand {
    low: vec4<f32>,
    high: vec4<f32>,
    grain_dark: vec4<f32>,
    grain_pale: vec4<f32>,
    // 0 round the board, 1 the board itself (`low` and `high` then the
    // darker and the lighter tile).
    mode: u32,
}

// A tile in world units. Keep in step with `layout::TILE`.
const TILE: f32 = 64.0;

// How much of the grain shows, for grain `cell` world units across drawn
// at `pixel` world units a pixel: all of it while a speck covers a couple
// of pixels, fading out as one shrinks under a pixel. A speck smaller
// than a pixel lands on a different pixel every frame the camera shakes,
// and a zoomed-out board fizzed through every raid.
fn grain_shown(cell: f32, pixel: f32) -> f32 {
    return 1.0 - smoothstep(0.45, 0.9, pixel / cell);
}

// Sand `c` with its grain: a dark speck on `dark` of the cells `cell`
// world units across and a pale one on `pale` of them, each mixed toward
// the uniform's grain colour by that colour's alpha, and faded out as a
// speck shrinks under a pixel. The beach and the board both draw theirs
// with this, from their own colours and densities.
fn grain(c: vec3<f32>, p: vec2<f32>, cell: f32, pixel: f32, dark: f32, pale: f32) -> vec3<f32> {
    let g = hash1(p / cell);
    let shown = grain_shown(cell, pixel);
    if g > 1.0 - dark {
        return mix(c, sand.grain_dark.rgb, sand.grain_dark.a * shown);
    } else if g < pale {
        return mix(c, sand.grain_pale.rgb, sand.grain_pale.a * shown);
    }
    return c;
}

@group(2) @binding(0) var<uniform> sand: Sand;
@group(2) @binding(1) var noise_tex: texture_2d<f32>;
@group(2) @binding(2) var noise_sampler: sampler;

// How many noise cells the noise texture spans. Keep in step with
// `NOISE_CELLS` in src/app/materials.rs.
const NOISE_CELLS: f32 = 16.0;

// Smooth noise at `p`, one cell `cell` world units across, read from the
// texture built once at start-up rather than worked out per pixel: red
// and green one octave each, blue two.
fn smooth_noise(p: vec2<f32>, cell: f32) -> vec4<f32> {
    return textureSample(noise_tex, noise_sampler, p / (cell * NOISE_CELLS));
}

// An integer hash of a cell (pcg2d): the classic fract(sin(dot)) one
// takes arguments in the tens of thousands of radians out on the beach,
// where GPU sin is coarse, and drew the grain as dotted chains of twins.
fn pcg2d(seed: vec2<u32>) -> vec2<u32> {
    var v = seed * 1664525u + 1013904223u;
    v.x += v.y * 1664525u;
    v.y += v.x * 1664525u;
    v = v ^ (v >> vec2<u32>(16u));
    v.x += v.y * 1664525u;
    v.y += v.x * 1664525u;
    v = v ^ (v >> vec2<u32>(16u));
    return v;
}

fn hash2(cell: vec2<f32>) -> vec2<f32> {
    let bits = bitcast<vec2<u32>>(vec2<i32>(floor(cell)));
    return vec2<f32>(pcg2d(bits)) / 4294967295.0;
}

fn hash1(cell: vec2<f32>) -> f32 {
    return hash2(cell).x;
}

// The board's sand. The quad covers the board exactly, so a tile is a
// cell of uv; the board's size in tiles comes off the screen-space slopes
// (world units per uv, over a tile), with uv's y running down the board
// as the sim's does.
fn board(p: vec2<f32>, uv: vec2<f32>, size: vec2<f32>, pixel: f32) -> vec4<f32> {
    // Boards are whole tiles, so the count is rounded: the slopes carry a
    // little float noise, and scaled by a far column's index it put a seam
    // a pixel off on a big board on a big display.
    let tile = floor(uv * round(size / TILE));
    let light = (i32(tile.x) + i32(tile.y)) % 2 == 0;
    var c = select(sand.low.rgb, sand.high.rgb, light);
    // A gentle drift of tone, so a row of light tiles is not one colour.
    c = c * (0.97 + 0.06 * smooth_noise(p, 230.0).r);
    // Wind ripples, fainter than out on the open beach: the play is read
    // over them.
    let warp = smooth_noise(p + vec2<f32>(600.0, 240.0), 120.0).g * 30.0;
    let ripple = sin((p.y + warp + p.x * 0.22) / 6.0);
    c = c * (0.985 + 0.02 * ripple);
    // Damp where the tide lay longest: darker, in soft-edged patches.
    let damp = smoothstep(0.62, 0.74, smooth_noise(p + vec2<f32>(1650.0, 600.0), 150.0).b);
    c = c * (1.0 - 0.07 * damp);
    // Finer and denser grain than the open beach, as the tiles had. The
    // board's grain colours are black and white at a fraction (see
    // `SandUniform::board`), so a speck is the sand's own shade made darker
    // or paler: it darkens with a damp patch and sits right on either tile.
    return vec4<f32>(grain(c, p, 2.2, pixel, 0.018, 0.016), 1.0);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = in.world_position.xy;
    // The quad's size in world units, taken here where the slopes are
    // defined for every pixel.
    let size = abs(vec2<f32>(dpdx(p.x) / dpdx(in.uv.x), dpdy(p.y) / dpdy(in.uv.y)));
    // World units a pixel, for the grain.
    let pixel = max(abs(dpdx(p.x)), abs(dpdy(p.y)));
    if sand.mode == 1u {
        return board(p, in.uv, size, pixel);
    }
    // Broad dunes: where the sand lies in the light and where in its own
    // lee.
    let dunes = smoothstep(0.25, 0.75, smooth_noise(p, 420.0).b);
    var c = mix(sand.low.rgb, sand.high.rgb, dunes);
    // Wind ripples, bent by a slow warp so they never draw as stripes.
    let warp = smooth_noise(p + vec2<f32>(480.0, 160.0), 160.0).g * 40.0;
    let ripple = sin((p.y + warp + p.x * 0.18) / 7.5);
    c = c * (0.975 + 0.035 * ripple);
    // Grain: a dark and a pale speck here and there, a few world units
    // across, so the sand has a texture at the board's own scale.
    return vec4<f32>(grain(c, p, 3.0, pixel, 0.012, 0.008), 1.0);
}
