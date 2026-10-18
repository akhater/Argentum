// ============================================================================
// rt_capture_sharpen - RawTherapee's capture sharpening, as the engine runs it.
//
// Copied from src-tauri/src/shaders/sharpen.wgsl @ 2bcb953d, capture
// sharpening only:
//
//   rt_  contrast mask     RawTherapee rtengine/rt_algo.cc, buildBlendMask
//                          @ 1c2b3f33772eaf491b7bd4d66323ac36d13aa344
//   rt_  capture sharpen   RawTherapee rtengine/capturesharpening.cc,
//                          CaptureDeconvSharpening
//                          @ c6d04960e14fed89e531cbbdf680021d131b4685
//
// Left out: darktable's unsharp mask, each photo mask's own Sharpen, the mask
// view, the passthrough for a region and the sRGB path (the engine's input is
// always linear). The arithmetic of every pass that is here is unchanged, with
// one difference at the end: `compose` writes the factor each pixel's
// luminance moved by, and the host multiplies its f32 pixels by it - the
// app's compose does `rgb * ratio` itself and stores the result in half
// floats.
//
// One tile at a time: the host walks the image in tiles with a border, and
// every buffer below is one extended tile (inner region plus border), indexed
// row-major. Radii arrive already in render pixels.
// ============================================================================

struct Params {
    img_w: u32,
    img_h: u32,
    origin_x: i32,
    origin_y: i32,
    ext_w: u32,
    ext_h: u32,
    inner_x0: u32,
    inner_y0: u32,
    inner_w: u32,
    inner_h: u32,
    flags: u32,
    contrast: f32,
    mask_sigma: f32,
    cap_sigma: f32,
    cap_slope: f32,
    cap_amount: f32,
    centre_x: f32,
    centre_y: f32,
    clip: f32,
    _pad0: u32,
}

const FLAG_CAPTURE: u32 = 1u;
const FLAG_ITER_CHECK: u32 = 8u;
const FLAG_MASK: u32 = 16u;
// The mask keeps off near-clipped highlights.
const FLAG_CLIP: u32 = 64u;

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var ratio_out: texture_storage_2d<r32float, write>;
@group(0) @binding(3) var<storage, read_write> y0: array<f32>;
@group(0) @binding(4) var<storage, read_write> est: array<f32>;
@group(0) @binding(5) var<storage, read_write> tmp: array<f32>;
@group(0) @binding(6) var<storage, read_write> aux: array<f32>;
@group(0) @binding(7) var<storage, read_write> blend: array<f32>;
@group(0) @binding(8) var<storage, read_write> stopped: array<atomic<u32>>;
@group(0) @binding(9) var<storage, read_write> counter: array<u32>;

// RawTherapee's largest deconvolution kernel is 13x13, radius 6; the app's
// shader allows 12 for darktable's sharpen, and so does this copy.
const MAX_RADIUS: i32 = 12;
// RawTherapee's iteration check works on 32px tiles.
const STOP_BLOCK: u32 = 32u;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn in_ext(id: vec3<u32>) -> bool {
    return id.x < p.ext_w && id.y < p.ext_h;
}

fn idx(x: u32, y: u32) -> u32 {
    return y * p.ext_w + x;
}

/// The source pixel at a global coordinate, clamped to the image. Linear.
fn src_lin(gx: i32, gy: i32) -> vec4<f32> {
    let c = vec2<i32>(
        clamp(gx, 0, i32(p.img_w) - 1),
        clamp(gy, 0, i32(p.img_h) - 1),
    );
    return textureLoad(src, c, 0);
}

/// Y from linear sRGB. RawTherapee's xyz_rgb, second row.
fn luma(c: vec3<f32>) -> f32 {
    return max(dot(c, vec3<f32>(0.212671, 0.715160, 0.072169)), 0.0);
}

/// CIE L* (0..100) from Y with white at 1.
fn lstar(y: f32) -> f32 {
    let e = 216.0 / 24389.0;
    let k = 24389.0 / 27.0;
    let f = select((k * y + 16.0) / 116.0, pow(max(y, 0.0), 1.0 / 3.0), y > e);
    return 116.0 * f - 16.0;
}

fn global_of(x: u32, y: u32) -> vec2<i32> {
    return vec2<i32>(p.origin_x + i32(x), p.origin_y + i32(y));
}

fn is_inner(x: u32, y: u32) -> bool {
    return x >= p.inner_x0 && y >= p.inner_y0
        && x < p.inner_x0 + p.inner_w && y < p.inner_y0 + p.inner_h;
}

fn radius_for(sigma: f32, extent: f32) -> i32 {
    return clamp(i32(ceil(sigma * extent)), 1, MAX_RADIUS);
}

fn gauss(d: f32, sigma: f32) -> f32 {
    return exp(-(d * d) / (2.0 * sigma * sigma));
}

/// The capture sharpening radius at a pixel. RawTherapee raises it towards
/// the corners - "corner boost" - linearly in the distance from the centre,
/// up to 2. It does this per 32px tile; per pixel is the same line without
/// the steps.
fn capture_sigma_at(x: u32, y: u32) -> f32 {
    if (p.cap_slope == 0.0) {
        return p.cap_sigma;
    }
    let g = vec2<f32>(global_of(x, y));
    let d = distance(g, vec2<f32>(p.centre_x, p.centre_y));
    return max(p.cap_sigma + p.cap_slope * d, 0.05);
}

// ---------------------------------------------------------------------------
// rt_ contrast mask - RawTherapee rt_algo.cc
// ---------------------------------------------------------------------------

/// calcBlendFactor: a sigmoid in ]0;1] with its inflexion at the threshold.
fn rt_blend_factor(val: f32, threshold: f32) -> f32 {
    let x = -16.0 + (16.0 / threshold) * val;
    return 0.5 * (1.0 + x / sqrt(1.0 + x * x));
}

/// Whether a near-clipped pixel is within RawTherapee's dilation pattern.
///
/// buildClipMask* marks every photosite at or above 0.95 of white and zeroes
/// a 5x5 neighbourhood less its corners around it, so deconvolution does not
/// ring against a blown highlight. The image is demosaiced here, so "clipped"
/// is any channel of the pixel at the level the host passes in.
fn rt_near_clip(gx: i32, gy: i32) -> bool {
    for (var dy = -2; dy <= 2; dy = dy + 1) {
        for (var dx = -2; dx <= 2; dx = dx + 1) {
            if (abs(dx) == 2 && abs(dy) == 2) {
                continue;
            }
            let c = src_lin(gx + dx, gy + dy).rgb;
            if (max(c.r, max(c.g, c.b)) >= p.clip) {
                return true;
            }
        }
    }
    return false;
}

fn l_at(gx: i32, gy: i32) -> f32 {
    return lstar(luma(src_lin(gx, gy).rgb));
}

/// RawTherapee measures contrast on L scaled to 0..32768 and multiplies by
/// 0.0625 / 327.68; on plain L* that is 0.0625.
fn rt_local_contrast(g: vec2<i32>) -> f32 {
    let dh1 = l_at(g.x + 1, g.y) - l_at(g.x - 1, g.y);
    let dv1 = l_at(g.x, g.y + 1) - l_at(g.x, g.y - 1);
    let dh2 = l_at(g.x + 2, g.y) - l_at(g.x - 2, g.y);
    let dv2 = l_at(g.x, g.y + 2) - l_at(g.x, g.y - 2);
    return sqrt(dh1 * dh1 + dv1 * dv1 + dh2 * dh2 + dv2 * dv2) * 0.0625;
}

// ---------------------------------------------------------------------------
// Passes
// ---------------------------------------------------------------------------

/// Luminance in, estimate started, the unblurred mask computed.
@compute @workgroup_size(16, 16, 1)
fn prep(@builtin(global_invocation_id) id: vec3<u32>) {
    if (!in_ext(id)) { return; }
    let g = global_of(id.x, id.y);
    let i = idx(id.x, id.y);
    let y = luma(src_lin(g.x, g.y).rgb);
    y0[i] = y;
    est[i] = y;

    if ((p.flags & FLAG_MASK) == 0u) {
        // A contrast threshold of 0 sharpens everywhere, as in RawTherapee,
        // where it also skips the clip mask.
        blend[i] = 1.0;
        return;
    }

    var b = rt_blend_factor(rt_local_contrast(g), p.contrast);
    if ((p.flags & FLAG_CLIP) != 0u && rt_near_clip(g.x, g.y)) {
        b = 0.0;
    }
    blend[i] = b;
}

/// The mask is blurred to smooth its transitions: RawTherapee uses sigma 2.
@compute @workgroup_size(16, 16, 1)
fn mask_h(@builtin(global_invocation_id) id: vec3<u32>) {
    if (!in_ext(id)) { return; }
    let s = p.mask_sigma;
    let r = radius_for(s, 3.0);
    var sum = 0.0;
    var wsum = 0.0;
    for (var k = -r; k <= r; k = k + 1) {
        let x = u32(clamp(i32(id.x) + k, 0, i32(p.ext_w) - 1));
        let w = gauss(f32(k), s);
        sum += blend[idx(x, id.y)] * w;
        wsum += w;
    }
    tmp[idx(id.x, id.y)] = sum / wsum;
}

@compute @workgroup_size(16, 16, 1)
fn mask_v(@builtin(global_invocation_id) id: vec3<u32>) {
    if (!in_ext(id)) { return; }
    let s = p.mask_sigma;
    let r = radius_for(s, 3.0);
    var sum = 0.0;
    var wsum = 0.0;
    for (var k = -r; k <= r; k = k + 1) {
        let y = u32(clamp(i32(id.y) + k, 0, i32(p.ext_h) - 1));
        let w = gauss(f32(k), s);
        sum += tmp[idx(id.x, y)] * w;
        wsum += w;
    }
    blend[idx(id.x, id.y)] = sum / wsum;
}

/// One iteration has begun. The iteration check needs to know which.
@compute @workgroup_size(1, 1, 1)
fn tick() {
    counter[0] = counter[0] + 1u;
}

// rt_ capture sharpening: Richardson-Lucy deconvolution with a gaussian PSF.
//
// RawTherapee's gauss*div2 then gauss*mult2, per iteration:
//     ratio    = observed / blur(estimate)
//     estimate = estimate * blur(ratio)
// as four separable passes.

@compute @workgroup_size(16, 16, 1)
fn rl_blur_est_h(@builtin(global_invocation_id) id: vec3<u32>) {
    if (!in_ext(id)) { return; }
    let s = capture_sigma_at(id.x, id.y);
    let r = radius_for(s, 3.0);
    var sum = 0.0;
    var wsum = 0.0;
    for (var k = -r; k <= r; k = k + 1) {
        let x = u32(clamp(i32(id.x) + k, 0, i32(p.ext_w) - 1));
        let w = gauss(f32(k), s);
        sum += est[idx(x, id.y)] * w;
        wsum += w;
    }
    tmp[idx(id.x, id.y)] = sum / wsum;
}

@compute @workgroup_size(16, 16, 1)
fn rl_ratio_v(@builtin(global_invocation_id) id: vec3<u32>) {
    if (!in_ext(id)) { return; }
    let s = capture_sigma_at(id.x, id.y);
    let r = radius_for(s, 3.0);
    var sum = 0.0;
    var wsum = 0.0;
    for (var k = -r; k <= r; k = k + 1) {
        let y = u32(clamp(i32(id.y) + k, 0, i32(p.ext_h) - 1));
        let w = gauss(f32(k), s);
        sum += tmp[idx(id.x, y)] * w;
        wsum += w;
    }
    let i = idx(id.x, id.y);
    aux[i] = y0[i] / max(sum / wsum, 0.00001);
}

@compute @workgroup_size(16, 16, 1)
fn rl_blur_ratio_h(@builtin(global_invocation_id) id: vec3<u32>) {
    if (!in_ext(id)) { return; }
    let s = capture_sigma_at(id.x, id.y);
    let r = radius_for(s, 3.0);
    var sum = 0.0;
    var wsum = 0.0;
    for (var k = -r; k <= r; k = k + 1) {
        let x = u32(clamp(i32(id.x) + k, 0, i32(p.ext_w) - 1));
        let w = gauss(f32(k), s);
        sum += aux[idx(x, id.y)] * w;
        wsum += w;
    }
    tmp[idx(id.x, id.y)] = sum / wsum;
}

@compute @workgroup_size(16, 16, 1)
fn rl_update_v(@builtin(global_invocation_id) id: vec3<u32>) {
    if (!in_ext(id)) { return; }
    let i = idx(id.x, id.y);
    let blocks_w = (p.ext_w + STOP_BLOCK - 1u) / STOP_BLOCK;
    let b = (id.y / STOP_BLOCK) * blocks_w + id.x / STOP_BLOCK;
    let k_now = counter[0];

    // A block RawTherapee would have stopped iterating keeps the estimate it
    // stopped at. Stopped "at" an earlier iteration, never this one: every
    // pixel of the block finishes the iteration in which the check failed,
    // as RawTherapee's tile does.
    let at = atomicLoad(&stopped[b]);
    if (at != 0u && at < k_now) {
        return;
    }

    let s = capture_sigma_at(id.x, id.y);
    let r = radius_for(s, 3.0);
    var sum = 0.0;
    var wsum = 0.0;
    for (var k = -r; k <= r; k = k + 1) {
        let y = u32(clamp(i32(id.y) + k, 0, i32(p.ext_h) - 1));
        let w = gauss(f32(k), s);
        sum += tmp[idx(id.x, y)] * w;
        wsum += w;
    }
    let next = est[i] * (sum / wsum);
    est[i] = next;

    // checkForStop: the estimate has fallen below half of what was observed
    // where the mask is fully on - deconvolution has started to ring.
    if ((p.flags & FLAG_ITER_CHECK) != 0u && is_inner(id.x, id.y)) {
        if (next < y0[i] * blend[i] * 0.5) {
            atomicMax(&stopped[b], k_now);
        }
    }
}

/// luminance = intp(blend, estimate, luminance), with an amount on top.
@compute @workgroup_size(16, 16, 1)
fn capture_mix(@builtin(global_invocation_id) id: vec3<u32>) {
    if (!in_ext(id)) { return; }
    let i = idx(id.x, id.y);
    let w = clamp(blend[i] * p.cap_amount, 0.0, 1.0);
    est[i] = mix(y0[i], max(est[i], 0.0), w);
}

/// How much each pixel's luminance moved - RawTherapee's YNew / YOld - for
/// the host to scale every channel by, so hue and saturation are untouched.
/// Exactly 1 where nothing moved, which the host copies as it was.
@compute @workgroup_size(16, 16, 1)
fn compose(@builtin(global_invocation_id) id: vec3<u32>) {
    if (!in_ext(id) || !is_inner(id.x, id.y)) { return; }
    let g = global_of(id.x, id.y);
    let i = idx(id.x, id.y);
    let old_y = y0[i];
    var ratio = 1.0;
    if (est[i] != old_y && old_y > 0.00001) {
        ratio = est[i] / old_y;
    }
    textureStore(ratio_out, g, vec4<f32>(ratio, 0.0, 0.0, 1.0));
}
