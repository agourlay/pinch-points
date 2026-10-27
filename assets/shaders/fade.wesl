// A flat colour at a strength each quad carries for itself: the
// footprints crabs leave in the sand, hundreds of them fading at once.
//
// The colour is the material's, shared by every quad; the strength is the
// quad's `MeshTag`, an f32's bits, read from the 2D mesh pipeline's
// per-instance data. A sprite fading the same way changes its colour, and
// Bevy draws every colour a sprite takes with a material of its own, so
// each print made a new material every frame of its life.
//
// Self-contained on purpose, like water.wgsl: no imports. The inputs are a
// subset of the 2D mesh pipeline's vertex output, and `Mesh2d` is that
// pipeline's per-instance struct, laid out as the engine lays it out.
//
// WESL rather than WGSL for one conditional, the engine's own: where the
// device has no storage buffers the per-instance data is a uniform array
// in batches, and `FadeMaterial::specialize` passes the batch size the
// mesh pipeline was built with as `PER_OBJECT_BUFFER_BATCH_SIZE`, as
// Bevy's mesh2d bindings read it. Declared as a storage array there, the
// pipeline fails validation at the first print and the game closes.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(5) @interpolate(flat) instance_index: u32,
}

struct Mesh2d {
    world_from_local: mat3x4<f32>,
    local_from_world_transpose_a: mat2x4<f32>,
    local_from_world_transpose_b: f32,
    flags: u32,
    material_bind_group_slot: u32,
    tag: u32,
    metadata_index: u32,
}

@if(PER_OBJECT_BUFFER_BATCH_SIZE)
@group(1) @binding(0) var<uniform> mesh: array<Mesh2d, u32(constants::PER_OBJECT_BUFFER_BATCH_SIZE)>;
@else
@group(1) @binding(0) var<storage> mesh: array<Mesh2d>;

// Linear rgb; its alpha is not read.
@group(2) @binding(0) var<uniform> color: vec4<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let strength = bitcast<f32>(mesh[in.instance_index].tag);
    return vec4<f32>(color.rgb, strength);
}
