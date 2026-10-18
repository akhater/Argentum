// The pixel pass: every per-pixel tool of one run of the registry, on one
// pixel after another, in one dispatch.
//
// Written by src/pixels/mod.rs from this template: the shared helpers, each
// tool's WGSL, and one call per tool, in registry order, go at the three
// marked lines below. The structure is the `main` of the app's shader.wgsl
// @ c786c146, where the adjustments follow one another on one pixel; there
// the calls are written by hand, here the registry writes them.

@group(0) @binding(0) var ag_input: texture_2d<f32>;
@group(0) @binding(1) var<storage, read> ag_params: array<vec4<f32>>;
@group(0) @binding(2) var ag_gf_tonal: texture_2d<f32>;
@group(0) @binding(3) var ag_gf_coarse: texture_2d<f32>;
@group(0) @binding(4) var ag_frame: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(5) var ag_linear: texture_storage_2d<rgba32float, write>;
@group(0) @binding(6) var<storage, read> ag_columns: array<u32>;
@group(0) @binding(7) var<storage, read> ag_rows: array<u32>;

/// What a tool's function is given besides the colour so far.
struct Px {
    /// The pixel's place in the pass's input picture.
    coord: vec2<u32>,
    /// Its colour as the pass received it, before any tool here.
    input: vec3<f32>,
    /// Where the tool's parameters start in `ag_params`.
    p: u32,
}

//@common

//@tools

fn ag_chain(coord: vec2<u32>) -> vec3<f32> {
    let input = textureLoad(ag_input, coord, 0).rgb;
    var c = input;
//@calls
    return c;
}

// The view's frame: each frame pixel reads the input pixel the CPU picked
// for its column and row (frame.rs), so cut and zoom are the CPU's exactly,
// and is encoded as the app's final `linear_to_srgb`.
@compute @workgroup_size(8, 8, 1)
fn ag_frame_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(ag_frame);
    if (id.x >= size.x || id.y >= size.y) {
        return;
    }
    let c = ag_chain(vec2<u32>(ag_columns[id.x], ag_rows[id.y]));
    textureStore(ag_frame, id.xy, vec4<f32>(linear_to_srgb(c), 1.0));
}

// The whole picture, linear light, for a render that is not a view.
@compute @workgroup_size(8, 8, 1)
fn ag_linear_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(ag_linear);
    if (id.x >= size.x || id.y >= size.y) {
        return;
    }
    textureStore(ag_linear, id.xy, vec4<f32>(ag_chain(id.xy), 1.0));
}
