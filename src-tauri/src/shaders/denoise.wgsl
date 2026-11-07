// Noise Reduction before sharpening, ours: RapidRAW's maths
// (noise_reduction.wgsl) over the whole input, one pixel per invocation.
// mods/denoise_gpu.rs runs it.

struct DenoiseParams {
    luma: f32,
    color: f32,
    // Render pixels per full-resolution pixel (mods/input_stage.rs).
    px_scale: f32,
    is_raw: u32,
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var output_texture: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var<uniform> params: DenoiseParams;

@compute @workgroup_size(8, 8, 1)
fn denoise(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = textureDimensions(input_texture);
    if (id.x >= dims.x || id.y >= dims.y) { return; }

    let texel = textureLoad(input_texture, id.xy, 0);
    var color = texel.rgb;
    if (params.is_raw == 0u) { color = srgb_to_linear(color); }

    // The full-resolution picture's neighbourhood, in this render's pixels:
    // the same at every zoom and in export, as sharpening's radii are.
    let full_short = f32(min(dims.x, dims.y)) / params.px_scale;
    let res_factor = nr_res_factor(reference_scale(full_short)) * params.px_scale;
    color = apply_noise_reduction(
        color, vec2<i32>(id.xy),
        params.luma, params.color, res_factor, params.is_raw
    );

    // Back to the encoding it came in, values above white kept.
    if (params.is_raw == 0u) { color = linear_to_srgb_extended(color); }
    textureStore(output_texture, id.xy, vec4<f32>(color, texel.a));
}
