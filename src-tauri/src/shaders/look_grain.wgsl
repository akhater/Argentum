// A provider look's grain lattice - ours (mods/look_grain.rs).
//
// Made once for a photo's seed and lattice size, then kept: each node holds
// layers A, B and C of normal noise, hashed from the node and the seed, then
// smoothed with [1, 7, 1] / sqrt(51). Texel (0, 0) is node (-2, -2), so the
// bicubic in `ag_grain_noise` (modules.wgsl) finds every node it reads, at the
// frame's edges too.

struct Lattice {
    seed: u32,
    width: u32,
    height: u32,
    _pad: u32,
}

@group(0) @binding(0) var<uniform> lattice: Lattice;
@group(0) @binding(1) var smoothed: texture_storage_2d<rgba16float, write>;

/// [1, 7, 1] / sqrt(51): its squares sum to 1, so a smoothed layer still measures 1.
const SIDE: f32 = 0.14002801;
const MIDDLE: f32 = 0.98019606;
const TAU: f32 = 6.2831853;

// paper_: Jarzynski and Olano, "Hash Functions for GPU Rendering" (JCGT 2020), pcg4d.
fn paper_pcg4d(seed: vec4<u32>) -> vec4<u32> {
    var v = seed * 1664525u + 1013904223u;
    v.x += v.y * v.w;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    v.w += v.y * v.z;
    v = v ^ (v >> vec4<u32>(16u));
    v.x += v.y * v.w;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    v.w += v.y * v.z;
    return v;
}

/// The three layers' normal noise at one node, by Box-Muller.
fn normals(node: vec2<i32>) -> vec3<f32> {
    let h = paper_pcg4d(vec4<u32>(bitcast<u32>(node.x), bitcast<u32>(node.y), lattice.seed, 0x9e3779b9u));
    let u = (vec4<f32>(h >> vec4<u32>(8u)) + 0.5) / 16777216.0;
    let r = sqrt(-2.0 * log(u.xz));
    return vec3<f32>(r.x * cos(TAU * u.y), r.x * sin(TAU * u.y), r.y * cos(TAU * u.w));
}

@compute @workgroup_size(8, 8, 1)
fn ag_grain_lattice(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= lattice.width || id.y >= lattice.height) {
        return;
    }
    var weight = array<f32, 3>(SIDE, MIDDLE, SIDE);
    let node = vec2<i32>(id.xy) - 2;
    var sum = vec3<f32>(0.0);
    for (var j = 0; j < 3; j = j + 1) {
        for (var i = 0; i < 3; i = i + 1) {
            sum += weight[i] * weight[j] * normals(node + vec2<i32>(i - 1, j - 1));
        }
    }
    textureStore(smoothed, id.xy, vec4<f32>(sum, 0.0));
}
