// Noise Reduction's maths, RapidRAW's: luma by a gated bilateral with an
// outlier pass, colour by a bilateral on the chroma. Shared by the pass that
// runs the global amounts before sharpening (denoise.wgsl,
// mods/denoise_gpu.rs) and the main shader, which runs a mask's own amount.
// Both declare the `input_texture` it reads.

// The taps' spacing, in pixels, on a picture `scale` from the reference
// (reference_scale in shared.wgsl). RapidRAW's, from inside
// apply_noise_reduction.
fn nr_res_factor(scale: f32) -> f32 {
    return clamp(sqrt(scale), 0.5, 2.0);
}

// Argentum: from RapidRAW cc8200a9, src-tauri/src/shaders/shader.wgsl - the
// luma taps are unrolled into vec4s instead of filled into arrays in a loop.
fn nr_luma_tap(
    coords_i: vec2<i32>,
    max_idx: vec2<i32>,
    dx: i32,
    dy: i32,
    extra: f32,
    jx: f32,
    jy: f32,
    is_raw: u32
) -> f32 {
    let ring = max(abs(dx), abs(dy));
    let ring_factor = select(0.5, 1.0, ring == 2);
    let grow = 1.0 + extra * ring_factor;
    let off_f = vec2<f32>(f32(dx) * grow + jx, f32(dy) * grow + jy);
    let off   = vec2<i32>(i32(round(off_f.x)), i32(round(off_f.y)));
    let coord = clamp(coords_i + off, vec2<i32>(0), max_idx);
    var s = textureLoad(input_texture, vec2<u32>(coord), 0).rgb;
    if (is_raw == 0u) { s = srgb_to_linear(s); }
    return get_luma(max(s, vec3<f32>(0.0)));
}

fn nr_accum_outlier(acc: ptr<function, vec2<f32>>, l: f32, init_w: f32, mean: f32, tol: f32) {
    if (init_w > 0.0001) {
        let d = l - mean;
        let r = abs(d) / tol;
        let bisq = max(0.0, 1.0 - r * r);
        let outlier_w = bisq * bisq;
        let w = init_w * outlier_w;
        (*acc).x += l * w;
        (*acc).y += w;
    }
}

fn nr_accum_outlier4(acc: ptr<function, vec2<f32>>, l: vec4<f32>, init_w: vec4<f32>, mean: f32, tol: f32) {
    nr_accum_outlier(acc, l.x, init_w.x, mean, tol);
    nr_accum_outlier(acc, l.y, init_w.y, mean, tol);
    nr_accum_outlier(acc, l.z, init_w.z, mean, tol);
    nr_accum_outlier(acc, l.w, init_w.w, mean, tol);
}

fn apply_noise_reduction(
    center_linear: vec3<f32>,
    coords_i: vec2<i32>,
    luma_amount: f32,
    color_amount: f32,
    res_factor: f32,
    is_raw: u32
) -> vec3<f32> {
    let luma_a  = clamp(luma_amount,  0.0, 1.0);
    let color_a = clamp(color_amount, 0.0, 1.0);
    if (luma_a < 0.001 && color_a < 0.001) {
        return center_linear;
    }

    let dims = vec2<i32>(textureDimensions(input_texture));
    let max_idx = dims - vec2<i32>(1);
    let center_safe   = max(center_linear, vec3<f32>(0.0));
    let center_luma   = get_luma(center_safe);
    let center_chroma = center_linear - vec3<f32>(center_luma);

    var new_luma   = center_luma;
    var new_chroma = center_chroma;

    if (luma_a > 0.001) {
        let l_curve = sqrt(luma_a);

        let stride_f = mix(1.0, 2.0, smoothstep(0.45, 0.95, luma_a)) * res_factor;
        let extra    = clamp(stride_f - 1.0, 0.0, 1.0);

        let l_spatial = mix(1.0, 1.5, l_curve);
        let l_spat_n  = -1.0 / max(2.0 * l_spatial * l_spatial, 1e-6);

        let h1 = hash(vec2<f32>(coords_i));
        let h2 = hash(vec2<f32>(coords_i) + vec2<f32>(17.31, 71.13));
        let jx = (h1 - 0.5) * 2.0 * extra;
        let jy = (h2 - 0.5) * 2.0 * extra;

        let l0 = vec4<f32>(
            nr_luma_tap(coords_i, max_idx, -2, -2, extra, jx, jy, is_raw),
            nr_luma_tap(coords_i, max_idx, -1, -2, extra, jx, jy, is_raw),
            nr_luma_tap(coords_i, max_idx,  0, -2, extra, jx, jy, is_raw),
            nr_luma_tap(coords_i, max_idx,  1, -2, extra, jx, jy, is_raw));
        let l1 = vec4<f32>(
            nr_luma_tap(coords_i, max_idx,  2, -2, extra, jx, jy, is_raw),
            nr_luma_tap(coords_i, max_idx, -2, -1, extra, jx, jy, is_raw),
            nr_luma_tap(coords_i, max_idx, -1, -1, extra, jx, jy, is_raw),
            nr_luma_tap(coords_i, max_idx,  0, -1, extra, jx, jy, is_raw));
        let l2 = vec4<f32>(
            nr_luma_tap(coords_i, max_idx,  1, -1, extra, jx, jy, is_raw),
            nr_luma_tap(coords_i, max_idx,  2, -1, extra, jx, jy, is_raw),
            nr_luma_tap(coords_i, max_idx, -2,  0, extra, jx, jy, is_raw),
            nr_luma_tap(coords_i, max_idx, -1,  0, extra, jx, jy, is_raw));
        let l3 = vec4<f32>(
            nr_luma_tap(coords_i, max_idx,  1,  0, extra, jx, jy, is_raw),
            nr_luma_tap(coords_i, max_idx,  2,  0, extra, jx, jy, is_raw),
            nr_luma_tap(coords_i, max_idx, -2,  1, extra, jx, jy, is_raw),
            nr_luma_tap(coords_i, max_idx, -1,  1, extra, jx, jy, is_raw));
        let l4 = vec4<f32>(
            nr_luma_tap(coords_i, max_idx,  0,  1, extra, jx, jy, is_raw),
            nr_luma_tap(coords_i, max_idx,  1,  1, extra, jx, jy, is_raw),
            nr_luma_tap(coords_i, max_idx,  2,  1, extra, jx, jy, is_raw),
            nr_luma_tap(coords_i, max_idx, -2,  2, extra, jx, jy, is_raw));
        let l5 = vec4<f32>(
            nr_luma_tap(coords_i, max_idx, -1,  2, extra, jx, jy, is_raw),
            nr_luma_tap(coords_i, max_idx,  0,  2, extra, jx, jy, is_raw),
            nr_luma_tap(coords_i, max_idx,  1,  2, extra, jx, jy, is_raw),
            nr_luma_tap(coords_i, max_idx,  2,  2, extra, jx, jy, is_raw));

        let e1 = exp(1.0 * l_spat_n);
        let e2 = exp(2.0 * l_spat_n);
        let e4 = exp(4.0 * l_spat_n);
        let e5 = exp(fma(l_spat_n, 4.0, l_spat_n));
        let e8 = exp(8.0 * l_spat_n);
        let sp0 = vec4<f32>(e8, e5, e4, e5);
        let sp1 = vec4<f32>(e8, e5, e2, e1);
        let sp2 = vec4<f32>(e2, e5, e4, e1);
        let sp3 = vec4<f32>(e1, e4, e5, e2);
        let sp4 = vec4<f32>(e1, e2, e5, e8);
        let sp5 = vec4<f32>(e5, e4, e5, e8);

        let lmin = min(center_luma, min(
            min(min(min(l0.x, l0.y), min(l0.z, l0.w)), min(min(l1.x, l1.y), min(l1.z, l1.w))),
            min(min(min(min(l2.x, l2.y), min(l2.z, l2.w)), min(min(l3.x, l3.y), min(l3.z, l3.w))),
                min(min(min(l4.x, l4.y), min(l4.z, l4.w)), min(min(l5.x, l5.y), min(l5.z, l5.w))))));
        let lmax = max(center_luma, max(
            max(max(max(l0.x, l0.y), max(l0.z, l0.w)), max(max(l1.x, l1.y), max(l1.z, l1.w))),
            max(max(max(max(l2.x, l2.y), max(l2.z, l2.w)), max(max(l3.x, l3.y), max(l3.z, l3.w))),
                max(max(max(l4.x, l4.y), max(l4.z, l4.w)), max(max(l5.x, l5.y), max(l5.z, l5.w))))));

        let luma_range    = lmax - lmin;
        let edge_strength = smoothstep(0.04, 0.20, luma_range);
        let edge_midpoint = (lmin + lmax) * 0.5;
        let center_side   = center_luma > edge_midpoint;

        let l_range_tol = mix(
            mix(0.025, 0.075, l_curve),
            mix(0.010, 0.025, l_curve),
            edge_strength
        );

        var samp_luma = array<f32, 25>(center_luma, l0.x, l0.y, l0.z, l0.w, l1.x, l1.y, l1.z, l1.w, l2.x, l2.y, l2.z, l2.w, l3.x, l3.y, l3.z, l3.w, l4.x, l4.y, l4.z, l4.w, l5.x, l5.y, l5.z, l5.w);
        var samp_spat = array<f32, 25>(1.0, sp0.x, sp0.y, sp0.z, sp0.w, sp1.x, sp1.y, sp1.z, sp1.w, sp2.x, sp2.y, sp2.z, sp2.w, sp3.x, sp3.y, sp3.z, sp3.w, sp4.x, sp4.y, sp4.z, sp4.w, sp5.x, sp5.y, sp5.z, sp5.w);
        var samp_gate: array<f32, 25>;
        var sum_a: f32 = 0.0;
        var w_a:   f32 = 0.0;
        for (var k: u32 = 0u; k < 25u; k = k + 1u) {
            let lk = samp_luma[k];
            let sk = samp_spat[k];
            let diff = abs(lk - center_luma);
            let g_range = 1.0 - smoothstep(l_range_tol * 0.6, l_range_tol, diff);
            let s_side  = lk > edge_midpoint;
            let g_side  = select(0.0, 1.0, s_side == center_side);
            let g_edge  = mix(1.0, g_side, edge_strength);
            let w = sk * g_range * g_edge;
            samp_gate[k] = w;
            sum_a += lk * w;
            w_a   += w;
        }
        let initial_mean = sum_a / max(w_a, 1e-4);
        let g_c = samp_gate[0];
        let g0 = vec4<f32>(samp_gate[1], samp_gate[2], samp_gate[3], samp_gate[4]);
        let g1 = vec4<f32>(samp_gate[5], samp_gate[6], samp_gate[7], samp_gate[8]);
        let g2 = vec4<f32>(samp_gate[9], samp_gate[10], samp_gate[11], samp_gate[12]);
        let g3 = vec4<f32>(samp_gate[13], samp_gate[14], samp_gate[15], samp_gate[16]);
        let g4 = vec4<f32>(samp_gate[17], samp_gate[18], samp_gate[19], samp_gate[20]);
        let g5 = vec4<f32>(samp_gate[21], samp_gate[22], samp_gate[23], samp_gate[24]);

        let outlier_tol = mix(0.07, 0.025, edge_strength);
        var acc_b = vec2<f32>(0.0);
        nr_accum_outlier(&acc_b, center_luma, g_c, initial_mean, outlier_tol);
        nr_accum_outlier4(&acc_b, l0, g0, initial_mean, outlier_tol);
        nr_accum_outlier4(&acc_b, l1, g1, initial_mean, outlier_tol);
        nr_accum_outlier4(&acc_b, l2, g2, initial_mean, outlier_tol);
        nr_accum_outlier4(&acc_b, l3, g3, initial_mean, outlier_tol);
        nr_accum_outlier4(&acc_b, l4, g4, initial_mean, outlier_tol);
        nr_accum_outlier4(&acc_b, l5, g5, initial_mean, outlier_tol);
        let robust_luma = select(initial_mean, acc_b.x / max(acc_b.y, 1e-6), acc_b.y > 0.01);

        let strength = luma_a * mix(1.0, 0.6, edge_strength);
        new_luma = mix(center_luma, robust_luma, strength);
    }

    if (color_a > 0.001) {
        let center_r_y = center_linear.r - center_luma;
        let center_b_y = center_linear.b - center_luma;
        let c_curve = sqrt(color_a);
        let stride_f = mix(2.0, 3.5, c_curve) * res_factor;

        let c_spatial = mix(2.0, 3.5, c_curve);
        let c_spat_n  = -1.0 / max(2.0 * c_spatial * c_spatial, 1e-6);

        let luma_tol = mix(0.12, 0.04, c_curve);
        let luma_n   = -1.0 / max(2.0 * luma_tol * luma_tol, 1e-6);

        let chroma_tol = mix(0.20, 0.08, c_curve);
        let chroma_n   = -1.0 / max(2.0 * chroma_tol * chroma_tol, 1e-6);

        let jh1 = hash(vec2<f32>(coords_i) + vec2<f32>(43.7, 91.1));
        let jh2 = hash(vec2<f32>(coords_i) + vec2<f32>(73.3, 17.9));
        let jx  = (jh1 - 0.5) * stride_f * 0.5;
        let jy  = (jh2 - 0.5) * stride_f * 0.5;

        var sum_r: f32 = center_r_y;
        var sum_b: f32 = center_b_y;
        var w_sum: f32 = 1.0;

        for (var dy: i32 = -2; dy <= 2; dy = dy + 1) {
            for (var dx: i32 = -2; dx <= 2; dx = dx + 1) {
                if (dx == 0 && dy == 0) { continue; }
                let off_f = vec2<f32>(f32(dx) * stride_f + jx, f32(dy) * stride_f + jy);
                let off   = vec2<i32>(i32(round(off_f.x)), i32(round(off_f.y)));
                let coord = clamp(coords_i + off, vec2<i32>(0), max_idx);
                var s = textureLoad(input_texture, vec2<u32>(coord), 0).rgb;

                if (is_raw == 0u) { s = srgb_to_linear(s); }

                let s_safe = max(s, vec3<f32>(0.0));
                let s_luma = get_luma(s_safe);
                let s_r_y  = s.r - s_luma;
                let s_b_y  = s.b - s_luma;

                let r2  = f32(dx * dx + dy * dy);
                let w_s = exp(r2 * c_spat_n);
                let dl  = s_luma - center_luma;
                let w_l = exp(dl * dl * luma_n);
                let dr  = s_r_y - center_r_y;
                let db  = s_b_y - center_b_y;
                let dc2 = dr * dr + db * db;
                let w_c = exp(dc2 * chroma_n);
                let w = w_s * w_l * w_c;

                sum_r += s_r_y * w;
                sum_b += s_b_y * w;
                w_sum += w;
            }
        }
        let filtered_r_y = sum_r / max(w_sum, 1e-6);
        let filtered_b_y = sum_b / max(w_sum, 1e-6);

        let new_r_y = mix(center_r_y, filtered_r_y, color_a);
        let new_b_y = mix(center_b_y, filtered_b_y, color_a);
        let new_g_y = -(LUMA_COEFF.r * new_r_y + LUMA_COEFF.b * new_b_y) / LUMA_COEFF.g;

        new_chroma = vec3<f32>(new_r_y, new_g_y, new_b_y);
    }

    return vec3<f32>(new_luma) + new_chroma;
}
