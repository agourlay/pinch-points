// Every body of water in the game: the rock pools on the board, the sea
// along the bottom of the beach, and the open sea out to the horizon on
// the menu's postcard.
//
// Self-contained on purpose: no imports, so it does not depend on the
// engine's shader module paths, which moved between releases. The inputs
// below are a subset of the 2D mesh pipeline's vertex output, and the
// globals are the view's own binding.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(2) uv: vec2<f32>,
}

struct Globals {
    time: f32,
    delta_time: f32,
    frame_count: u32,
}

@group(0) @binding(1) var<uniform> globals: Globals;

// Keep in step with `WaterUniform` in src/app/materials.rs.
struct Water {
    shallow: vec4<f32>,
    deep: vec4<f32>,
    // rgb: a mania's colour; a: how far the water takes it on.
    tint: vec4<f32>,
    // 0 sea, 1 pool layer, 2 glint, 3 ocean.
    mode: u32,
    caustics: f32,
    // The wet sand band along the sea's top, and over what depth the
    // water goes from shallow to deep.
    wet: f32,
    depth_span: f32,
    // 1, or 0 under reduced motion: the water holds still.
    motion: f32,
}

@group(2) @binding(0) var<uniform> water: Water;
@group(2) @binding(1) var mask: texture_2d<f32>;
@group(2) @binding(2) var mask_sampler: sampler;

// The water's clock: the view's, stopped under reduced motion.
fn now() -> f32 {
    return globals.time * water.motion;
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

fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash1(i);
    let b = hash1(i + vec2<f32>(1.0, 0.0));
    let c = hash1(i + vec2<f32>(0.0, 1.0));
    let d = hash1(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// The web of light a rippled surface throws on a shallow floor: the
// edges between drifting cells (F2 - F1 of a moving Voronoi field).
fn caustic(p: vec2<f32>, t: f32) -> f32 {
    let cell = floor(p);
    let f = fract(p);
    var f1 = 8.0;
    var f2 = 8.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let o = vec2<f32>(f32(x), f32(y));
            let h = hash2(cell + o);
            let c = o + 0.5 + 0.38 * sin(vec2<f32>(t, t * 0.8) + 6.2831 * h) - f;
            let d = length(c);
            if d < f1 {
                f2 = f1;
                f1 = d;
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    let edge = f2 - f1;
    let line = clamp(1.0 - edge / 0.14, 0.0, 1.0);
    return line * line * line;
}

fn light(world: vec2<f32>) -> f32 {
    let t = now();
    let a = caustic(world / 34.0, t * 0.9);
    let b = caustic(world / 21.0 + vec2<f32>(7.3, 2.1), t * 1.2 + 1.7);
    return 0.6 * a + 0.4 * b;
}

// The waterline both seas share: how far from the edge of the beach the
// water starts, `wet` past it plus the waves running up the sand and back,
// never all at once.
fn shoreline(world: vec2<f32>) -> f32 {
    let t = now();
    let run = 5.0 * sin(t * 0.8 + world.x / 170.0) + 3.0 * sin(t * 1.3 + world.x / 61.0);
    return water.wet + 11.0 + run + (value_noise(vec2<f32>(world.x / 37.0, 1.0)) - 0.5) * 6.0;
}

// Wet sand `d` short of the waterline (d < 0), darkest where the last wave
// was. Linear light, like every colour that reaches this target.
fn wet_sand(d: f32) -> vec4<f32> {
    let w = clamp(1.0 + d / water.wet, 0.0, 1.0);
    return vec4<f32>(0.147, 0.089, 0.040, 0.32 * w * w);
}

// The foam `d` out from the waterline, and the bubbles it drags back.
fn foam_at(world: vec2<f32>, d: f32) -> f32 {
    let fd = (d - 1.5) / 2.6;
    var foam = exp(-fd * fd);
    // The bubbles only within reach of the line, where they can show.
    if d < 24.0 {
        // Squared by hand: pow of a negative base is undefined in WGSL,
        // and Metal's fast pow turns it into NaN.
        let behind = (d - 7.0) / 4.0;
        foam += smoothstep(0.5, 0.64, value_noise(world / 3.5)) * exp(-behind * behind) * 0.55;
    }
    return clamp(foam, 0.0, 1.0) * 0.9;
}

// How much a crest at `depth` (in crest spacings) bends along the shore:
// smooth sines only, since a noise row per crest broke each one where the
// rows met.
fn crest_bend(world: vec2<f32>, depth: f32) -> f32 {
    return 0.9 * sin(world.x / 150.0 + depth * 0.7) + 0.5 * sin(world.x / 57.0 - depth * 1.3);
}

fn sea(world: vec2<f32>, uv: vec2<f32>, height: f32) -> vec4<f32> {
    // How far down from the quad's top edge, world units (`height` is the
    // quad's, measured off the screen-space slopes in `fragment`). The top
    // of the quad is the sand the waves run up; the water starts below.
    // The waterline never comes higher than `wet` from the top, so the wet
    // sand's fade is whole and the quad's own edge never shows.
    let d = uv.y * height - shoreline(world);
    if d < 0.0 {
        return wet_sand(d);
    }
    let k = clamp(d / water.depth_span, 0.0, 1.0);
    var c = mix(water.shallow.rgb, water.deep.rgb, k);
    c = c + vec3<f32>(0.92, 1.0, 0.98) * light(world) * water.caustics * (1.0 - 0.7 * k);
    // Crests running in, fainter further out. d grows out to sea, so the
    // phase gains time to bring a crest in.
    let crest = sin(d / 9.0 + now() * 1.3 + crest_bend(world, d / 56.0));
    c = c + vec3<f32>(1.0) * smoothstep(0.93, 1.0, crest) * 0.16 * (1.0 - k);
    let foam = foam_at(world, d);
    c = mix(c, water.tint.rgb, water.tint.a);
    c = mix(c, vec3<f32>(0.97, 0.99, 0.98), foam);
    return vec4<f32>(c, max(mix(water.shallow.a, water.deep.a, k), foam));
}

// The postcard's open sea, seen from the beach: the waterline along the
// quad's foot, where waves run up the sand as they do in a round, and the
// water out to the horizon along its top. Crests bunch up and thin out
// toward the horizon, which haze swallows; the sun glitters on the far
// water. `height` is the quad's, measured off the screen-space slopes
// (see `fragment`), and `wet` is the sand at its foot.
fn ocean(world: vec2<f32>, uv: vec2<f32>, height: f32) -> vec4<f32> {
    let shore = shoreline(world);
    let d = (1.0 - uv.y) * height - shore;
    if d < 0.0 {
        return wet_sand(d);
    }
    let t = now();
    // 0 at the waterline, 1 at the horizon.
    let far = clamp(d / max(height - shore, 1.0), 0.0, 1.0);
    var c = mix(water.shallow.rgb, water.deep.rgb, smoothstep(0.0, 0.45, far));
    // The light on the floor, only in the shallows where it shows: past
    // them its weight is nothing and the two caustic passes were the most
    // expensive thing on every postcard screen.
    if far < 0.3 {
        c = c + vec3<f32>(0.92, 1.0, 0.98) * light(world) * water.caustics * (1.0 - smoothstep(0.0, 0.3, far));
    }
    // Crests: evenly spaced on the water, so bunched toward the horizon
    // in the picture, rolling in toward the beach. Kept off the shallows,
    // where the light on the floor already reads and a crest drawn that
    // close is a broad smear.
    let depth = 1.0 / (1.04 - far);
    let crest = sin(depth * 4.0 + t * 1.1 + crest_bend(world, depth));
    let crests = smoothstep(0.95, 1.0, crest) * smoothstep(0.12, 0.3, far) * pow(1.0 - far, 0.8);
    c = c + vec3<f32>(1.0) * crests * 0.3;
    // The sun on the far water: fine specks that come and go.
    if far > 0.5 {
        let speck = hash1(world / vec2<f32>(2.5, 1.2) + vec2<f32>(floor(t * 2.5) * 17.0, 0.0));
        let band = smoothstep(0.5, 0.75, far) * (1.0 - smoothstep(0.88, 1.0, far));
        c = c + vec3<f32>(1.0, 0.97, 0.9) * step(0.995, speck) * band * 0.45;
    }
    // Haze where the sea meets the sky: the horizon band's colour,
    // srgb(0.72, 0.84, 0.92), in linear light.
    c = mix(c, vec3<f32>(0.477, 0.674, 0.828), smoothstep(0.8, 1.0, far) * 0.55);
    c = mix(c, vec3<f32>(0.97, 0.99, 0.98), foam_at(world, d));
    return vec4<f32>(c, 1.0);
}

fn pool(world: vec2<f32>, uv: vec2<f32>) -> vec4<f32> {
    let shape = textureSample(mask, mask_sampler, uv).a;
    // Most of a pool's quad is the sand round the puddle: there the light
    // on the floor, two Voronoi passes, would be worked out to be drawn at
    // no alpha at all.
    if shape <= 0.0 {
        return vec4<f32>(0.0);
    }
    var c = water.shallow.rgb + vec3<f32>(0.92, 1.0, 0.98) * light(world) * water.caustics;
    return vec4<f32>(c, water.shallow.a * shape);
}

fn glint(world: vec2<f32>, uv: vec2<f32>) -> vec4<f32> {
    let p = (uv - vec2<f32>(0.5)) * 2.0;
    let r = dot(p, p);
    let shimmer = 0.75 + 0.25 * sin(now() * 2.1 + (world.x + world.y) * 0.05);
    return vec4<f32>(1.0, 1.0, 1.0, exp(-r * 3.0) * 0.55 * shimmer * (1.0 - smoothstep(0.6, 1.0, r)));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let world = in.world_position.xy;
    // The quad's height in world units: how far the world moves for a
    // step of uv, both linear across it. Taken here, outside the switch,
    // where the slopes are defined for every pixel.
    let height = abs(dpdy(in.world_position.y) / dpdy(in.uv.y));
    switch water.mode {
        case 0u: { return sea(world, in.uv, height); }
        case 1u: { return pool(world, in.uv); }
        case 3u: { return ocean(world, in.uv, height); }
        default: { return glint(world, in.uv); }
    }
}
