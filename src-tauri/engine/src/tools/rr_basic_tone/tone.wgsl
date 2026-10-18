// The tone sliders on one pixel: RapidRAW's, from
// src-tauri/src/shaders/shader.wgsl @ c786c146. `shape_detail`,
// `apply_tonal_adjustments` and `apply_highlights_adjustment` are copied as
// written; `sample_gf_tex` reads the pixel pass's input for the picture's
// size, where the app reads its own input texture; and `rr_basic_tone` is
// the part of the app's `main` that measures a pixel's detail and calls
// them.

const GF_LUMA_FLOOR: f32 = 1.0e-4;
const GF_REINJECT_SIGMA: f32 = 0.8;

fn sample_gf_tex(tex: texture_2d<f32>, abs_coord: vec2<u32>) -> vec4<f32> {
    let full = vec2<f32>(textureDimensions(ag_input));
    let low_i = vec2<i32>(textureDimensions(tex));
    let p = (vec2<f32>(abs_coord) + 0.5) / full * vec2<f32>(low_i) - 0.5;
    let fl = floor(p);
    let w = p - fl;
    let max_idx = low_i - vec2<i32>(1);
    let c0 = clamp(vec2<i32>(fl), vec2<i32>(0), max_idx);
    let c1 = clamp(vec2<i32>(fl) + vec2<i32>(1), vec2<i32>(0), max_idx);
    let v00 = textureLoad(tex, c0, 0);
    let v10 = textureLoad(tex, vec2<i32>(c1.x, c0.y), 0);
    let v01 = textureLoad(tex, vec2<i32>(c0.x, c1.y), 0);
    let v11 = textureLoad(tex, c1, 0);
    return mix(mix(v00, v10, w.x), mix(v01, v11, w.x), w.y);
}

fn shape_detail(d: f32, sigma: f32) -> f32 {
    let x = d / sigma;
    return d / (1.0 + x * x);
}

fn apply_tonal_adjustments(
    color: vec3<f32>,
    local_log_detail: f32,
    mid_log_detail: f32,
    is_raw: u32,
    con: f32,
    sh: f32,
    wh: f32,
    bl: f32
) -> vec3<f32> {
    var rgb = color;

    if (wh != 0.0) {
        let w_luma = max(get_luma(max(rgb, vec3<f32>(0.0))), 0.0001);
        let w_base = max(w_luma * exp2(-clamp(local_log_detail + mid_log_detail, -3.0, 3.0)), 0.0001);
        let t_w = pow(w_base, 0.4545);
        let w_pivot = 0.45;
        let u = max(t_w - w_pivot, 0.0);

        var t_new = t_w;
        if (wh > 0.0) {
            t_new = t_w + wh * 0.25 * (u * u) / (1.0 + u * u);
        } else if (u > 0.0) {
            t_new = w_pivot + u / (1.0 + (-wh) * 0.3 * u);
        }
        let w_ramp = smoothstep(0.25, 0.9, t_w);
        t_new = mix(t_w, t_new, w_ramp);

        let w_amount = abs(t_new - t_w) * w_ramp;
        rgb *= pow(t_new, 2.2) / w_base;

        let w_log_d = local_log_detail * 0.4545;
        let w_safe_detail = exp2(w_log_d / (1.0 + abs(w_log_d) * 0.4));
        let w_corr = pow(w_safe_detail, 1.0 + w_amount * 1.0) / w_safe_detail;
        rgb *= pow(w_corr, 2.2);
        rgb *= exp2(shape_detail(mid_log_detail, GF_REINJECT_SIGMA) * w_amount * 4.0);
    }

    let pixel_luma = get_luma(max(rgb, vec3<f32>(0.0)));

    let safe_pixel_luma = max(pixel_luma, 0.0001);

    if (sh != 0.0 || bl != 0.0) {
        let t_pixel = pow(safe_pixel_luma, 0.4545);
        let t_detail = exp2(local_log_detail * 0.4545);
        let t_blurred = t_pixel / max(t_detail, 0.0001);

        let shadow_lift = sh * t_pixel * pow(max(1.0 - t_pixel, 0.0), 4.5);
        let black_lift = bl * t_pixel * pow(max(1.0 - t_pixel, 0.0), 12.0);
        let lift_amount = max(shadow_lift + black_lift, 0.0);

        let t_pixel_curved = max(t_pixel + shadow_lift + black_lift, 0.0);

        let shadow_pivot = 0.2;
        let stretch_focus = 1.0 - smoothstep(0.1, 0.85, t_pixel);
        let detail_focus = 1.0 - smoothstep(0.05, 0.55, t_pixel);
        let stretch_factor = 1.0 + (lift_amount * stretch_focus * 4.4);
        let contrasted_t = shadow_pivot + (t_pixel_curved - shadow_pivot) * stretch_factor;

        let final_t = max(contrasted_t, 0.0);
        let curved_luma = pow(final_t, 2.2);

        let luma_ratio = curved_luma / safe_pixel_luma;
        rgb *= luma_ratio;

        let log_d = log2(max(t_detail, 0.0001));
        let safe_detail = exp2(log_d / (1.0 + abs(log_d) * 0.4));

        let noise_protection = smoothstep(0.0, 0.1, t_blurred);

        let detail_amp = 1.0 + (lift_amount * detail_focus * 1.5 * noise_protection);

        let enhanced_detail = pow(safe_detail, detail_amp);
        let detail_correction = enhanced_detail / safe_detail;

        let linear_correction = pow(detail_correction, 2.2);
        rgb *= linear_correction;

        let lift_structure = lift_amount * detail_focus * noise_protection * 6.0;
        rgb *= exp2(shape_detail(mid_log_detail, GF_REINJECT_SIGMA) * lift_structure);

        if (luma_ratio > 1.0) {
            let recovered_luma = get_luma(rgb);
            let boost_amount = clamp((luma_ratio - 1.0) * 0.05, 0.0, 0.12);
            rgb = mix(rgb, vec3<f32>(recovered_luma), boost_amount);
        }
    }

    if (con != 0.0) {
        let safe_rgb = max(rgb, vec3<f32>(0.0));
        let g = 2.2;
        let perceptual = pow(safe_rgb, vec3<f32>(1.0 / g));
        let clamped_perceptual = clamp(perceptual, vec3<f32>(0.0), vec3<f32>(1.0));
        let strength = pow(2.0, con * 1.25);
        let condition = clamped_perceptual < vec3<f32>(0.5);
        let high_part = 1.0 - 0.5 * pow(2.0 * (1.0 - clamped_perceptual), vec3<f32>(strength));
        let low_part = 0.5 * pow(2.0 * clamped_perceptual, vec3<f32>(strength));
        let curved_perceptual = select(high_part, low_part, condition);
        let contrast_adjusted_rgb = pow(curved_perceptual, vec3<f32>(g));
        let mix_factor = smoothstep(vec3<f32>(1.0), vec3<f32>(1.01), safe_rgb);
        rgb = mix(contrast_adjusted_rgb, rgb, mix_factor);
    }
    return rgb;
}

fn apply_highlights_adjustment(
    color_in: vec3<f32>,
    local_log_detail: f32,
    mid_log_detail: f32,
    is_raw: u32,
    highlights_adj: f32
) -> vec3<f32> {
    if (abs(highlights_adj) < 0.001) {
        return color_in;
    }

    let pixel_luma = get_luma(max(color_in, vec3<f32>(0.0)));
    if (pixel_luma < 0.0001) {
        return color_in;
    }

    const l_pivot: f32 = 0.06;
    if (pixel_luma <= l_pivot) {
        return color_in;
    }

    let luma_base = pixel_luma * exp2(-local_log_detail);
    let detail_ratio = pixel_luma / max(luma_base, 1e-4);

    let safe_detail = clamp(detail_ratio, 0.65, 1.55);

    let log_detail = log2(safe_detail);
    let soft_log_detail = log_detail / (1.0 + abs(log_detail) * 0.40);

    var target_luma: f32 = pixel_luma;

    if (highlights_adj < 0.0) {
        let k = -highlights_adj;
        let delta_base = max(luma_base - l_pivot, 0.0);

        let compression_strength = k * 2.2;
        let compressed_delta = delta_base / (1.0 + compression_strength * (delta_base / (1.0 + delta_base * 0.35)));
        let target_base = l_pivot + compressed_delta;

        let neg_soft_detail = local_log_detail / (1.0 + abs(local_log_detail) * 0.80);
        let restoration_gain = 1.0 + k * 0.3;
        let structure_gain = shape_detail(mid_log_detail, GF_REINJECT_SIGMA) * k * 0.6;
        let recovered_detail = exp2(neg_soft_detail * restoration_gain + structure_gain);

        let recovered_target = target_base * recovered_detail;

        let blend = smoothstep(l_pivot, l_pivot + 0.22, pixel_luma);
        target_luma = mix(pixel_luma, recovered_target, blend);

    } else {
        let boost = highlights_adj * 0.70;
        let delta_base = max(luma_base - l_pivot, 0.0);
        let boosted_delta = delta_base * (1.0 + boost / (1.0 + delta_base * 0.25));
        let target_base = l_pivot + boosted_delta;

        let detail_boost = exp2(soft_log_detail * (1.0 + highlights_adj * 0.20));
        let boosted_target = target_base * detail_boost;

        let blend = smoothstep(l_pivot, l_pivot + 0.22, pixel_luma);
        target_luma = mix(pixel_luma, boosted_target, blend);
    }

    let luma_ratio = target_luma / pixel_luma;
    var final_color = color_in * luma_ratio;

    if (highlights_adj < 0.0 && pixel_luma > 1.0) {
        let blowout = smoothstep(1.0, 3.5, pixel_luma) * (-highlights_adj) * 0.40;
        final_color = mix(final_color, vec3<f32>(target_luma), blowout);
    }

    return final_color;
}

// The detail, as the app's `main` measures it: the log luminance of the
// pixel as the pass received it - before exposure and white balance - less
// the line the guided filter fitted there (`tonal_log_detail`), and that
// line less the coarse scale's (`tonal_mid_detail`). Clarity and structure
// would add to the first; this engine has neither yet.
fn rr_basic_tone(c: vec3<f32>, px: Px) -> vec3<f32> {
    // Contrast, shadows, whites, blacks; then highlights.
    let a = ag_params[px.p];
    let highlights = ag_params[px.p + 1u].x;
    var local_detail = 0.0;
    var mid_detail = 0.0;
    if (a.y != 0.0 || a.w != 0.0 || highlights != 0.0 || a.z != 0.0) {
        let gf_i = log2(max(get_luma(max(px.input, vec3<f32>(0.0))), GF_LUMA_FLOOR));
        let tonal = sample_gf_tex(ag_gf_tonal, px.coord).xy;
        let fitted = tonal.x * gf_i + tonal.y;
        let broad = sample_gf_tex(ag_gf_coarse, px.coord).xy;
        local_detail = gf_i - fitted;
        mid_detail = fitted - (broad.x * gf_i + broad.y);
    }
    let toned = apply_tonal_adjustments(c, local_detail, mid_detail, 1u, a.x, a.y, a.z, a.w);
    return apply_highlights_adjustment(toned, local_detail, mid_detail, 1u, highlights);
}
