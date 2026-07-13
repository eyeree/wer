// POV water (3d-phase-3-plan.md): the sea plane and the river overlay.
//
// Two entry-point pairs share this module:
// - `vs_sea`/`fs_sea` — a single camera-centered translucent quad at the
//   camera-relative plane height `params.water.y` (the shell computes
//   SEA_LEVEL − camera.z in f64; the renderer never learns SEA_LEVEL).
//   The quad reaches `fog_end`, where every fragment is pure fog color, so
//   its rim is invisible by construction (plan §4.1).
// - `vs_overlay`/`fs_overlay` — river strips: a subset of the core terrain
//   triangles drawn again through the terrain vertex buffers, lifted by
//   RIVER_LIFT and shaded as water, alpha feathered on the per-vertex river
//   intensity baked into `light.z` (plan §6).
//
// The wobble is normal perturbation only — no vertex is ever displaced —
// and is display-only animation (frame time reaches nothing but this
// shader; design §5.1 allows exactly this). Everything here is derived
// presentation; nothing is read back (ADR 0017).

struct PovParams {
    view_proj: mat4x4<f32>,
    sun_dir: vec3<f32>,
    fog_start: f32,
    fog_color: vec3<f32>,
    fog_end: f32,
    sky_ambient: vec3<f32>,
    _pad0: f32,
    ground_ambient: vec3<f32>,
    _pad1: f32,
    detail: array<vec4<f32>, 3>,
    // (time s wrapped at WOBBLE_PERIOD, camera-relative sea-plane z,
    //  wobble anchor fraction x, y) — plan §4.3. Layout-identical to
    //  pov_terrain.wgsl's PovParams and the Rust PovParamsRaw.
    water: vec4<f32>,
    // Live diagnostic toggles — unused here (the water toggle skips these
    // passes CPU-side), declared for uniform-layout parity with
    // pov_terrain.wgsl.
    toggles: vec4<f32>,
}

struct ChunkOffset {
    offset: vec3<f32>,
    _pad: f32,
    detail_base: array<vec4<u32>, 3>,
}

@group(0) @binding(0) var<uniform> params: PovParams;
@group(1) @binding(0) var<uniform> chunk: ChunkOffset;

const TAU: f32 = 6.28318530718;

// The wobble tiling contract (plan §4.3): wave vectors are integer cycle
// counts per WOBBLE_TILE = 64 world units, so the field is periodic over 64
// — the shell's `camera mod 64` anchor jump and the 256-unit (= 4 tiles)
// chunk-local overlay anchoring are both seamless, and the sea and a river
// mouth wobble in the same phase. Time factors are integer cycles per
// WOBBLE_PERIOD = 32 s (renderer::pov::WOBBLE_PERIOD — the shell wraps its
// clock there), so the wrap is seamless too.
const WOBBLE_TILE: f32 = 64.0;
const WOBBLE_PERIOD: f32 = 32.0;

// Apparent-slope gradient of the summed wobble waves at anchored position
// `p`, time `t`. Each wave contributes `amp · cos(phase)` of slope along its
// unit direction; amplitudes are small (plan §4.3: ~0.02–0.05).
fn wobble_grad(p: vec2<f32>, t: f32) -> vec2<f32> {
    var g = vec2<f32>(0.0);
    // (cycles_x, cycles_y, time cycles per period, slope amplitude).
    var waves = array<vec4<f32>, 4>(
        vec4<f32>(2.0, 0.0, 3.0, 0.035),
        vec4<f32>(1.0, 3.0, 5.0, 0.030),
        vec4<f32>(5.0, -2.0, 7.0, 0.025),
        vec4<f32>(-4.0, 7.0, 11.0, 0.020),
    );
    for (var w = 0u; w < 4u; w = w + 1u) {
        let k = waves[w].xy;
        let phase = TAU * (dot(p, k) / WOBBLE_TILE + waves[w].z * t / WOBBLE_PERIOD);
        g = g + waves[w].w * cos(phase) * normalize(k);
    }
    return g;
}

// Schlick Fresnel against the wobbled normal; `abs` so the surface still
// reads from below (walking the sea floor is allowed; the underwater view
// is untuned but must not vanish — plan §12).
fn fresnel(cos_theta: f32) -> f32 {
    let c = 1.0 - abs(cos_theta);
    return 0.02 + 0.98 * c * c * c * c * c;
}

// --- the sea plane (plan §4) ------------------------------------------------

// Linear-light water colors (the sRGB anchors decoded with the same cheap
// pow-2.2 the terrain shader uses): DEEP_WATER beside elevation_color's
// deep-ramp anchor [8, 16, 64]; RIVER_SHALLOW a darker cousin of the
// composite river blue so ribbons read as surfaces, not paint.
const DEEP_WATER: vec3<f32> = vec3<f32>(0.002, 0.012, 0.070);
// A thin translucent film, not blue paint: the sea floor (a sediment ramp
// with the cyan absorption cast baked in, `pov_sediment_color`) carries the
// depth read; the surface contributes tint, wobble, and glint. Alpha still
// rises toward grazing angles, where you could not see down anyway.
const SEA_ALPHA_LOW: f32 = 0.30;
const SEA_ALPHA_HIGH: f32 = 0.65;
// Tight sparkle rather than a smeared white sheet (the old 60/0.6 lobe
// blew out over wobbled normals).
const SEA_GLINT_POWER: f32 = 90.0;
const SEA_GLINT_STRENGTH: f32 = 0.35;

// Distance fade of the wobble slope, so distant water does not shimmer on
// llvmpipe's pixel grid (plan §4.3).
fn wobble_fade(dist: f32) -> f32 {
    return 1.0 / (1.0 + dist * 0.02);
}

struct SeaOut {
    @builtin(position) clip: vec4<f32>,
    // Camera-relative position (the camera is the render-space origin).
    @location(0) pos: vec3<f32>,
}

@vertex
fn vs_sea(@builtin(vertex_index) vi: u32) -> SeaOut {
    // Camera-centered quad, triangle-strip corners from the vertex index.
    let sx = select(-1.0, 1.0, (vi & 1u) == 1u);
    let sy = select(-1.0, 1.0, (vi & 2u) == 2u);
    let pos = vec3<f32>(sx * params.fog_end, sy * params.fog_end, params.water.y);
    var out: SeaOut;
    out.clip = params.view_proj * vec4<f32>(pos, 1.0);
    out.pos = pos;
    return out;
}

@fragment
fn fs_sea(in: SeaOut) -> @location(0) vec4<f32> {
    let dist = length(in.pos);
    let view = -in.pos / max(dist, 1e-3);
    let g = wobble_grad(in.pos.xy + params.water.zw, params.water.x) * wobble_fade(dist);
    let n = normalize(vec3<f32>(-g.x, -g.y, 1.0));
    let f = fresnel(dot(view, n));
    // Deep blue toward the sky (fog) color at grazing angles; the depth cue
    // beneath is the terrain's sediment ramp showing through (plan §1.1).
    var color = mix(DEEP_WATER, params.fog_color, f);
    // Sun glint — unshadowed: open water has no baked visibility (plan §12).
    let glint = pow(max(dot(reflect(params.sun_dir, n), view), 0.0), SEA_GLINT_POWER);
    color = color + vec3<f32>(glint * SEA_GLINT_STRENGTH);
    let fog = smoothstep(params.fog_start, params.fog_end, dist);
    let alpha = mix(SEA_ALPHA_LOW, SEA_ALPHA_HIGH, f);
    return vec4<f32>(mix(color, params.fog_color, fog), alpha);
}

// --- the river overlay (plan §6) --------------------------------------------

// Lift above the terrain surface, world units — high enough that
// Depth32Float never z-fights inside the fog range, low enough to read as
// the water surface (plan §6.4).
const RIVER_LIFT: f32 = 0.2;
// The feather band: alpha runs 0 → RIVER_ALPHA as the baked river intensity
// (light.z) crosses MIN → FULL. MIN must match the mesher's triangle
// selection threshold (platform-native pov::RIVER_OVERLAY_MIN), so the
// selection edge sits exactly where alpha reaches zero.
const RIVER_OVERLAY_MIN: f32 = 0.12;
const RIVER_OVERLAY_FULL: f32 = 0.30;
const RIVER_ALPHA: f32 = 0.45;
const RIVER_SHALLOW: vec3<f32> = vec3<f32>(0.016, 0.10, 0.30);
const RIVER_GLINT_POWER: f32 = 40.0;
const RIVER_GLINT_STRENGTH: f32 = 0.5;

// The terrain vertex layout (pov_terrain.wgsl VsIn) — the overlay draws the
// same buffers through its own index list.
struct OverlayIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) light: vec4<f32>,
}

struct OverlayOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) pos: vec3<f32>,
    // Baked river intensity (light.z) and sun visibility (light.x).
    @location(2) river: f32,
    @location(3) sunvis: f32,
    // Chunk-local x, y: ≡ world mod WOBBLE_TILE (REGION_SIZE = 4 tiles), so
    // the overlay wobble is seamless across chunks and in phase with the sea.
    @location(4) local: vec2<f32>,
}

@vertex
fn vs_overlay(in: OverlayIn) -> OverlayOut {
    let pos = in.position + chunk.offset + vec3<f32>(0.0, 0.0, RIVER_LIFT);
    var out: OverlayOut;
    out.clip = params.view_proj * vec4<f32>(pos, 1.0);
    out.normal = in.normal;
    out.pos = pos;
    out.river = in.light.z;
    out.sunvis = in.light.x;
    out.local = in.position.xy;
    return out;
}

@fragment
fn fs_overlay(in: OverlayOut) -> @location(0) vec4<f32> {
    let dist = length(in.pos);
    let view = -in.pos / max(dist, 1e-3);
    // Fold the wobble into the terrain normal (the overlay is conformal).
    let g = wobble_grad(in.local, params.water.x) * wobble_fade(dist);
    var n = normalize(in.normal);
    n = normalize(vec3<f32>(n.x - g.x * n.z, n.y - g.y * n.z, n.z));
    let f = fresnel(dot(view, n));
    var color = mix(RIVER_SHALLOW, params.fog_color, f);
    // Glint gated by the baked sun visibility: shadowed water doesn't sparkle.
    let glint = pow(max(dot(reflect(params.sun_dir, n), view), 0.0), RIVER_GLINT_POWER);
    color = color + vec3<f32>(glint * RIVER_GLINT_STRENGTH * in.sunvis);
    let fog = smoothstep(params.fog_start, params.fog_end, dist);
    // Feathered on the baked intensity: the ribbon edge fades to zero at the
    // mesher's selection threshold, so no hard boundary exists (plan §6.2).
    let alpha = RIVER_ALPHA * smoothstep(RIVER_OVERLAY_MIN, RIVER_OVERLAY_FULL, in.river);
    return vec4<f32>(mix(color, params.fog_color, fog), alpha);
}
