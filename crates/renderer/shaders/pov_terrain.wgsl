// POV terrain (3d-phase-4-plan.md §6): Lambert sun + GPU directional shadow,
// CPU-baked low-frequency ambient occlusion, and distance fog. Per-fragment
// **detail normals** continue the terrain
// gradient-noise spectrum above its authoritative top octave (the same
// integer-hash scheme `compose_map.wgsl` ports), bending the shading normal
// only — vertices are never displaced, so the CPU heightfield stays the
// ground truth 3D-2's collision will stand on (ADR 0016/0017, design §3.2).
//
// Deliberately minimal otherwise — no specular, no textures. Everything here
// is derived presentation; nothing is read back (ADR 0017).

struct PovParams {
    // Camera-relative view-projection (the view translation is applied on the
    // CPU in f64 through the per-chunk offsets, §4 of the plan).
    view_proj: mat4x4<f32>,
    // Camera-relative stabilized directional-light projection.
    light_view_proj: mat4x4<f32>,
    // Normalized, pointing from the sun toward the ground.
    sun_dir: vec3<f32>,
    fog_start: f32,
    // The clear color, so geometry dissolves into sky.
    fog_color: vec3<f32>,
    fog_end: f32,
    // Hemisphere ambient: sky tint for up-facing normals...
    sky_ambient: vec3<f32>,
    _pad0: f32,
    // ...ground bounce for down-facing ones.
    ground_ambient: vec3<f32>,
    _pad1: f32,
    // Detail-normal octaves: (frac_x, frac_y, 1/wavelength, slope) each —
    // the fractional lattice offset of a region origin, the lattice scale in
    // chunk-local units, and the apparent-slope amplitude (the shell folds
    // the terrain spectrum's continuation and the normal exaggeration in).
    detail: array<vec4<f32>, 3>,
    // Water frame state (3d-phase-3-plan.md §4.3), consumed by
    // pov_water.wgsl; declared here so both modules share one uniform layout.
    water: vec4<f32>,
    // (inverse map size, enabled, constant bias, slope bias).
    shadow: vec4<f32>,
    // Live diagnostic toggles (1.0/0.0): x = directional shadow/AO,
    // y = per-fragment detail normals, z/w reserved.
    toggles: vec4<f32>,
}

// The chunk's region origin relative to the camera (f64 subtraction on the
// CPU, truncated to f32): every position the GPU sees stays small, so f32 is
// exact-enough at any world coordinate.
struct ChunkOffset {
    offset: vec3<f32>,
    _pad: f32,
    // Per-octave 64-bit base lattice indices of this chunk's origin for the
    // detail noise: (ix.lo, ix.hi, iy.lo, iy.hi). Integer bases keep lattice
    // hashing exact at any distance from the world origin — only the small
    // chunk-local fraction lives in f32 (the map's refinement anchoring).
    detail_base: array<vec4<u32>, 3>,
}

@group(0) @binding(0) var<uniform> params: PovParams;
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;
@group(1) @binding(0) var<uniform> chunk: ChunkOffset;

struct VsIn {
    // Chunk-local: x, y in [0, REGION_SIZE], z = elevation.
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // sRGB bytes, Unorm8x4 -> 0..1 here.
    @location(2) color: vec4<f32>,
    // Presentation bytes, Unorm8x4 -> 0..1: x = reserved neutral,
    // y = ambient occlusion, z = river, w = wetness.
    @location(3) light: vec4<f32>,
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) dist: f32,
    // Reserved, ambient occlusion, river, wetness.
    @location(3) light: vec4<f32>,
    // Chunk-local x, y for the detail-noise lattice.
    @location(4) local: vec2<f32>,
    // Camera-relative position, for the wet-specular view direction.
    @location(5) pos: vec3<f32>,
    @location(6) light_clip: vec4<f32>,
}

fn terrain_position(in: VsIn) -> vec3<f32> {
    return in.position + chunk.offset;
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
    let pos = terrain_position(in); // camera-relative world space
    var out: VsOut;
    out.clip = params.view_proj * vec4<f32>(pos, 1.0);
    out.normal = in.normal;
    out.color = in.color;
    out.dist = length(pos);
    out.light = in.light;
    out.local = in.position.xy;
    out.pos = pos;
    out.light_clip = params.light_view_proj * vec4<f32>(pos, 1.0);
    return out;
}

@vertex
fn vs_shadow(in: VsIn) -> @builtin(position) vec4<f32> {
    return params.light_view_proj * vec4<f32>(terrain_position(in), 1.0);
}

// --- 64-bit integer hashing over u32 pairs (lo, hi) ------------------------
// Transcription of world-core's splitmix64/mix (ADR 0003), the same port
// `compose_map.wgsl` carries; presentation-only consumer (ADR 0017), so this
// is convenience, not a parity surface.

const GAMMA: vec2<u32> = vec2<u32>(0x7F4A7C15u, 0x9E3779B9u);
const SM_C1: vec2<u32> = vec2<u32>(0x1CE4E5B9u, 0xBF58476Du);
const SM_C2: vec2<u32> = vec2<u32>(0x133111EBu, 0x94D049BBu);
const TERRAIN_BASIS: vec2<u32> = vec2<u32>(0x0FFEE712u, 0x7E11AD5Cu);
const ALGORITHM_VERSION: u32 = 2u;

fn mul32x32(a: u32, b: u32) -> vec2<u32> {
    let a0 = a & 0xffffu; let a1 = a >> 16u;
    let b0 = b & 0xffffu; let b1 = b >> 16u;
    let p00 = a0 * b0;
    let p01 = a0 * b1;
    let p10 = a1 * b0;
    let p11 = a1 * b1;
    let mid = p10 + (p00 >> 16u);
    let mid2 = p01 + (mid & 0xffffu);
    let lo = (p00 & 0xffffu) | (mid2 << 16u);
    let hi = p11 + (mid >> 16u) + (mid2 >> 16u);
    return vec2<u32>(lo, hi);
}

fn mul64(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    var r = mul32x32(a.x, b.x);
    r.y = r.y + a.x * b.y + a.y * b.x;
    return r;
}

fn add64(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    let lo = a.x + b.x;
    let carry = select(0u, 1u, lo < a.x);
    return vec2<u32>(lo, a.y + b.y + carry);
}

fn shr64(a: vec2<u32>, s: u32) -> vec2<u32> {
    // s in (0, 32).
    return vec2<u32>((a.x >> s) | (a.y << (32u - s)), a.y >> s);
}

fn splitmix64(x_in: vec2<u32>) -> vec2<u32> {
    let x = add64(x_in, GAMMA);
    var z = x;
    z = mul64(z ^ shr64(z, 30u), SM_C1);
    z = mul64(z ^ shr64(z, 27u), SM_C2);
    return z ^ shr64(z, 31u);
}

fn mix64(seed: vec2<u32>, value: vec2<u32>) -> vec2<u32> {
    return splitmix64(seed ^ mul64(value, GAMMA));
}

// Signed 64-bit add of a small signed 32-bit delta.
fn add64_i32(a: vec2<u32>, d: i32) -> vec2<u32> {
    let ext = vec2<u32>(u32(d), select(0u, 0xffffffffu, d < 0));
    return add64(a, ext);
}

// --- detail-normal noise (terrain gradient scheme, octaves >= OCTAVES) -----

fn gradient_seed(ix: vec2<u32>, iy: vec2<u32>, octave: u32) -> vec2<u32> {
    var h = TERRAIN_BASIS;
    h = mix64(h, vec2<u32>(ALGORITHM_VERSION, 0u));
    h = mix64(h, vec2<u32>(octave, 0u));
    h = mix64(h, ix);
    h = mix64(h, iy);
    return h;
}

const SQRT_HALF: f32 = 0.70710678;

fn gradient_dir(index: u32) -> vec2<f32> {
    switch index {
        case 0u: { return vec2<f32>(1.0, 0.0); }
        case 1u: { return vec2<f32>(-1.0, 0.0); }
        case 2u: { return vec2<f32>(0.0, 1.0); }
        case 3u: { return vec2<f32>(0.0, -1.0); }
        case 4u: { return vec2<f32>(SQRT_HALF, SQRT_HALF); }
        case 5u: { return vec2<f32>(-SQRT_HALF, SQRT_HALF); }
        case 6u: { return vec2<f32>(SQRT_HALF, -SQRT_HALF); }
        default: { return vec2<f32>(-SQRT_HALF, -SQRT_HALF); }
    }
}

fn fade(t: f32) -> f32 {
    return t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
}

fn fade_deriv(t: f32) -> f32 {
    return 30.0 * t * t * (t * (t - 2.0) + 1.0);
}

// The analytic lattice-space gradient (d/du, d/dv) of one detail octave's
// gradient noise at lattice position (u, v) — the value itself is unused:
// only its slope bends the normal.
fn detail_noise_grad(base_ix: vec2<u32>, base_iy: vec2<u32>, u: f32, v: f32, octave: u32) -> vec2<f32> {
    let cu = floor(u);
    let cv = floor(v);
    let fx = u - cu;
    let fy = v - cv;
    let ix = add64_i32(base_ix, i32(cu));
    let iy = add64_i32(base_iy, i32(cv));
    let ix1 = add64_i32(ix, 1);
    let iy1 = add64_i32(iy, 1);
    let g00 = gradient_dir(gradient_seed(ix, iy, octave).x & 7u);
    let g10 = gradient_dir(gradient_seed(ix1, iy, octave).x & 7u);
    let g01 = gradient_dir(gradient_seed(ix, iy1, octave).x & 7u);
    let g11 = gradient_dir(gradient_seed(ix1, iy1, octave).x & 7u);
    let n00 = g00.x * fx + g00.y * fy;
    let n10 = g10.x * (fx - 1.0) + g10.y * fy;
    let n01 = g01.x * fx + g01.y * (fy - 1.0);
    let n11 = g11.x * (fx - 1.0) + g11.y * (fy - 1.0);
    let uu = fade(fx);
    let vv = fade(fy);
    let du = fade_deriv(fx);
    let dv = fade_deriv(fy);
    // value = lerp(vv, nx0, nx1) with nx0 = lerp(uu, n00, n10) etc.; its
    // partial derivatives in fx, fy (product rule through the fades).
    let dx0 = g00.x + (g10.x - g00.x) * uu + (n10 - n00) * du;
    let dx1 = g01.x + (g11.x - g01.x) * uu + (n11 - n01) * du;
    let dy0 = g00.y + (g10.y - g00.y) * uu;
    let dy1 = g01.y + (g11.y - g01.y) * uu;
    let nx0 = n00 + (n10 - n00) * uu;
    let nx1 = n01 + (n11 - n01) * uu;
    let dx = dx0 + (dx1 - dx0) * vv;
    let dy = dy0 + (dy1 - dy0) * vv + (nx1 - nx0) * dv;
    // The same amplitude normalization the noise value carries.
    return vec2<f32>(dx, dy) * 1.41421356;
}

// Sun strength, tuned with the frame ambients so flat ground under the 20°
// sun stays near the 2D palette's value range (plan §13, color-space drift
// note): flat ground gets 1.2 · sin(20°) ≈ 0.41 direct (vs the old 0.67)
// plus the raised sky ambient; sun-facing slopes gain the difference — the
// slope contrast is the point of the low sun.
const SUN_STRENGTH: f32 = 1.2;

// Wet-ground specular (3d-phase-3-plan.md §5.2): tight-ish lobe, modest
// strength — the gloss is the part of the water response the 2D map cannot
// express, so it carries the whole "reads as water" job on the material pass.
const WET_GLINT_POWER: f32 = 40.0;
const WET_GLINT_STRENGTH: f32 = 0.5;

// Manual 3x3 PCF with a normal-dependent receiver bias. Outside the fitted
// volume is fully lit; the one-texel fade prevents a clamped-edge stripe.
fn shadow_visibility(light_clip: vec4<f32>, normal: vec3<f32>) -> f32 {
    if (params.shadow.y < 0.5 || abs(light_clip.w) < 1e-6) {
        return 1.0;
    }
    let ndc = light_clip.xyz / light_clip.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if (ndc.z <= 0.0 || ndc.z >= 1.0 || any(uv <= vec2<f32>(0.0)) || any(uv >= vec2<f32>(1.0))) {
        return 1.0;
    }
    let texel = params.shadow.x;
    let edge = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
    let edge_fade = smoothstep(texel, 2.0 * texel, edge);
    let slope = 1.0 - max(dot(normalize(normal), -params.sun_dir), 0.0);
    let reference = ndc.z - (params.shadow.z + params.shadow.w * slope);
    var visible = 0.0;
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            visible = visible + textureSampleCompareLevel(
                shadow_map,
                shadow_sampler,
                uv + vec2<f32>(f32(x), f32(y)) * texel,
                reference,
            );
        }
    }
    return mix(1.0, visible / 9.0, edge_fade);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Cheap sRGB decode (pow 2.2, noted in plan §5.6): the vertex bytes are
    // the 2D palette's sRGB values; the surface is Rgba8UnormSrgb-family, so
    // the hardware re-encodes on write.
    let albedo = pow(in.color.rgb, vec3<f32>(2.2));
    var n = normalize(in.normal);
    // Detail normals: sum the continued spectrum's apparent-slope gradient
    // and fold it into the interpolated normal (reconstruct the surface
    // gradient, add, renormalize). Shading only — geometry is untouched.
    // Behind the `N` diagnostic toggle: the lattice hashing below is the
    // heaviest per-fragment work this shader does, which matters on a
    // software rasterizer (llvmpipe), where fragment cost IS CPU cost.
    if (params.toggles.y > 0.5) {
        var dgrad = vec2<f32>(0.0);
        for (var k = 0u; k < 3u; k = k + 1u) {
            let d = params.detail[k];
            let u = in.local.x * d.z + d.x;
            let v = in.local.y * d.z + d.y;
            let base = chunk.detail_base[k];
            dgrad = dgrad + d.w * detail_noise_grad(base.xy, base.zw, u, v, 5u + k);
        }
        n = normalize(vec3<f32>(n.x - dgrad.x * n.z, n.y - dgrad.y * n.z, n.z));
    }
    // GPU visibility gates direct light; retained CPU AO only attenuates the
    // hemisphere fill. The `B` diagnostic neutralizes both.
    let sunvis = shadow_visibility(in.light_clip, n);
    let ao = mix(1.0, in.light.y, params.toggles.x);
    let sun = SUN_STRENGTH * max(dot(n, -params.sun_dir), 0.0) * sunvis;
    let ambient = mix(params.ground_ambient, params.sky_ambient, n.z * 0.5 + 0.5) * ao;
    // The 3D-3 wet response (3d-phase-3-plan.md §5.2): a specular sun glint
    // gated by wetness and GPU directional visibility. Rivers dominate;
    // wetlands get a weaker version. Albedo is untouched — the color half of
    // the response already rides composite_cell_color, and doubling it here
    // would desynchronize the 3D ground from the 2D map.
    let wet = max(in.light.z, 0.6 * in.light.w);
    let view = -in.pos / max(in.dist, 1e-3);
    let glint = pow(max(dot(reflect(params.sun_dir, n), view), 0.0), WET_GLINT_POWER);
    let lit = albedo * (vec3<f32>(sun) + ambient)
        + vec3<f32>(glint * wet * sunvis * WET_GLINT_STRENGTH);
    let fog = smoothstep(params.fog_start, params.fog_end, in.dist);
    return vec4<f32>(mix(lit, params.fog_color, fog), 1.0);
}
