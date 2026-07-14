// GPU composition of the debug map from the region-tile atlas
// (phase-6-plan.md §6.5) — the first render-graph node.
//
// Fullscreen pass over the map viewport: screen position → virtual map cell →
// window region → atlas slot → channel texels → false color (matching the
// canonical presenter in `viewer_host::map` and per-cell palette values in
// `world_runtime::mapcolor`), plus optional refinement octaves that continue
// the terrain gradient-noise spectrum above FIELD_RES per *screen* pixel,
// using the same integer-hash gradient scheme as `world-core`
// (`splitmix64`/`mix` re-implemented over u32 pairs below).
//
// ADR 0017: everything computed here is derived presentation. Nothing is read
// back, hashed, persisted, or consumed by gameplay — the renderer exposes no
// readback API. Refinement adds zero-mean detail around the authoritative
// sample (gradient noise is zero-mean), so CPU and GPU presentations agree at
// tile resolution.

struct RefineOctave {
    // 64-bit base lattice indices of the view's NW corner (u64 as lo/hi).
    base_ix: vec2<u32>,
    base_iy: vec2<u32>,
    // Fractional lattice position of the NW corner (x, y).
    frac: vec2<f32>,
    // 1 / wavelength, in units of map cells.
    inv_wavelength_cells: f32,
    // Display amplitude of this octave, world units.
    amplitude: f32,
    // The terrain octave index this continues (>= OCTAVES).
    octave: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

struct MapParams {
    // Window geometry.
    half_regions: i32,
    resolution: i32,
    side_cells: f32,     // (2*half+1) * resolution
    atlas_tiles_x: u32,  // atlas slots per row
    // Presentation.
    channel: u32,
    refine_octave_count: u32,
    zoom: f32,
    grid_thickness_cells: f32,
    refine: array<RefineOctave, 3>,
}

@group(0) @binding(0) var<uniform> params: MapParams;
// Window slot lookup: (2h+1)² entries, row-major from the NW region; -1 = not
// resident in the atlas.
@group(0) @binding(1) var<storage, read> slots: array<i32>;
// Channel planes: rgba32float atlases.
//   plane0 = (elevation, hardness, temperature, moisture)
//   plane1 = (river, wetness, soil_depth, fertility)
//   plane2 = (vegetation, canopy, herbivore, predator)
//   plane3 = (diversity, presence_mask, 0, 0)
@group(0) @binding(2) var plane0: texture_2d<f32>;
@group(0) @binding(3) var plane1: texture_2d<f32>;
@group(0) @binding(4) var plane2: texture_2d<f32>;
@group(0) @binding(5) var plane3: texture_2d<f32>;
// Integer plane: rg16uint (biome id, dominant species index).
@group(0) @binding(6) var ints: texture_2d<u32>;
// CPU-drawn sparse overlay (ordered grid, routes, rings, organisms, markers),
// map-cell resolution.
@group(0) @binding(7) var overlay: texture_2d<f32>;

// --- 64-bit integer hashing over u32 pairs (lo, hi) ------------------------
// Transcription of world-core's splitmix64/mix (ADR 0003); presentation-only
// consumer (ADR 0017), so this port is convenience, not a parity surface.

const GAMMA: vec2<u32> = vec2<u32>(0x7F4A7C15u, 0x9E3779B9u);
const SM_C1: vec2<u32> = vec2<u32>(0x1CE4E5B9u, 0xBF58476Du);
const SM_C2: vec2<u32> = vec2<u32>(0x133111EBu, 0x94D049BBu);
const TERRAIN_BASIS: vec2<u32> = vec2<u32>(0x0FFEE712u, 0x7E11AD5Cu);
const HABITAT_BASIS: vec2<u32> = vec2<u32>(0x5E39D264u, 0x48A17B0Cu);
const SPECIES_BASIS: vec2<u32> = vec2<u32>(0xA11CE5E5u, 0x5EEDC0DEu);
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

fn xor64(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    return a ^ b;
}

fn shr64(a: vec2<u32>, s: u32) -> vec2<u32> {
    // s in (0, 32).
    return vec2<u32>((a.x >> s) | (a.y << (32u - s)), a.y >> s);
}

fn splitmix64(x_in: vec2<u32>) -> vec2<u32> {
    let x = add64(x_in, GAMMA);
    var z = x;
    z = mul64(xor64(z, shr64(z, 30u)), SM_C1);
    z = mul64(xor64(z, shr64(z, 27u)), SM_C2);
    return xor64(z, shr64(z, 31u));
}

fn mix64(seed: vec2<u32>, value: vec2<u32>) -> vec2<u32> {
    return splitmix64(xor64(seed, mul64(value, GAMMA)));
}

fn u64_from_u32(v: u32) -> vec2<u32> {
    return vec2<u32>(v, 0u);
}

// --- refinement noise (terrain gradient scheme above FIELD_RES) ------------

fn gradient_seed(ix: vec2<u32>, iy: vec2<u32>, octave: u32) -> vec2<u32> {
    var h = TERRAIN_BASIS;
    h = mix64(h, u64_from_u32(ALGORITHM_VERSION));
    h = mix64(h, u64_from_u32(octave));
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

// Signed 64-bit add of a small signed 32-bit delta.
fn add64_i32(a: vec2<u32>, d: i32) -> vec2<u32> {
    let ext = vec2<u32>(u32(d), select(0u, 0xffffffffu, d < 0));
    return add64(a, ext);
}

// One refinement octave's gradient noise at map-cell position (px, py).
fn refine_noise(oct: RefineOctave, px: f32, py: f32) -> f32 {
    let u = oct.frac.x + px * oct.inv_wavelength_cells;
    // Map rows grow south while world y grows north.
    let v = oct.frac.y - py * oct.inv_wavelength_cells;
    let cu = floor(u);
    let cv = floor(v);
    let fx = u - cu;
    let fy = v - cv;
    let ix = add64_i32(oct.base_ix, i32(cu));
    let iy = add64_i32(oct.base_iy, i32(cv));
    let ix1 = add64_i32(ix, 1);
    let iy1 = add64_i32(iy, 1);
    let g00 = gradient_dir(gradient_seed(ix, iy, oct.octave).x & 7u);
    let g10 = gradient_dir(gradient_seed(ix1, iy, oct.octave).x & 7u);
    let g01 = gradient_dir(gradient_seed(ix, iy1, oct.octave).x & 7u);
    let g11 = gradient_dir(gradient_seed(ix1, iy1, oct.octave).x & 7u);
    let n00 = g00.x * fx + g00.y * fy;
    let n10 = g10.x * (fx - 1.0) + g10.y * fy;
    let n01 = g01.x * fx + g01.y * (fy - 1.0);
    let n11 = g11.x * (fx - 1.0) + g11.y * (fy - 1.0);
    let uu = fade(fx);
    let vv = fade(fy);
    let nx0 = n00 + (n10 - n00) * uu;
    let nx1 = n01 + (n11 - n01) * uu;
    return (nx0 + (nx1 - nx0) * vv) * 1.41421356;
}

fn refinement_delta(px: f32, py: f32) -> f32 {
    var delta = 0.0;
    for (var i = 0u; i < params.refine_octave_count; i = i + 1u) {
        delta = delta + params.refine[i].amplitude * refine_noise(params.refine[i], px, py);
    }
    return delta;
}

// --- palettes (matching viewer_host::map/world_runtime::mapcolor) ----------

fn lerp_rgb(a: vec3<f32>, b: vec3<f32>, t: f32) -> vec3<f32> {
    return mix(a, b, clamp(t, 0.0, 1.0));
}

fn rgb8(r: f32, g: f32, b: f32) -> vec3<f32> {
    return vec3<f32>(r, g, b) / 255.0;
}

// CPU palettes and sparse overlays compose in sRGB byte space. The render
// target is itself sRGB, so decode exactly once after every encoded-space
// palette/overlay operation; the attachment then re-encodes to the same
// visible bytes as the canonical CPU texture path.
fn srgb_to_linear_channel(encoded: f32) -> f32 {
    if encoded <= 0.04045 {
        return encoded / 12.92;
    }
    return pow((encoded + 0.055) / 1.055, 2.4);
}

fn srgb_to_linear(encoded: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        srgb_to_linear_channel(encoded.r),
        srgb_to_linear_channel(encoded.g),
        srgb_to_linear_channel(encoded.b),
    );
}

const SEA_LEVEL: f32 = 0.0;

fn elevation_color(e: f32) -> vec3<f32> {
    if e < SEA_LEVEL {
        return lerp_rgb(rgb8(8.0, 16.0, 64.0), rgb8(70.0, 130.0, 190.0),
                        clamp(1.0 + e / 600.0, 0.0, 1.0));
    }
    let t = clamp(e / 900.0, 0.0, 1.0);
    if t < 0.5 {
        return lerp_rgb(rgb8(70.0, 120.0, 60.0), rgb8(140.0, 120.0, 80.0), t * 2.0);
    }
    return lerp_rgb(rgb8(140.0, 120.0, 80.0), rgb8(245.0, 245.0, 245.0), (t - 0.5) * 2.0);
}

fn temperature_color(t: f32) -> vec3<f32> {
    return lerp_rgb(rgb8(40.0, 60.0, 200.0), rgb8(220.0, 60.0, 40.0), (t + 15.0) / 50.0);
}

fn moisture_color(m: f32) -> vec3<f32> {
    return lerp_rgb(rgb8(150.0, 110.0, 70.0), rgb8(40.0, 90.0, 200.0), m);
}

fn river_color(r: f32) -> vec3<f32> {
    return lerp_rgb(rgb8(20.0, 20.0, 26.0), rgb8(80.0, 170.0, 255.0), r);
}

fn wetness_color(w: f32) -> vec3<f32> {
    return lerp_rgb(rgb8(120.0, 100.0, 70.0), rgb8(30.0, 120.0, 160.0), w);
}

fn soil_color(depth: f32, fertility: f32) -> vec3<f32> {
    let hue = lerp_rgb(rgb8(190.0, 170.0, 130.0), rgb8(80.0, 60.0, 30.0), fertility);
    return hue * (0.35 + 0.65 * depth);
}

fn vegetation_color(v: f32) -> vec3<f32> {
    return lerp_rgb(rgb8(190.0, 175.0, 130.0), rgb8(20.0, 110.0, 40.0), v);
}

fn herbivore_color(h: f32) -> vec3<f32> {
    return lerp_rgb(rgb8(20.0, 24.0, 20.0), rgb8(210.0, 200.0, 60.0),
                    clamp(h * 8.0, 0.0, 1.0));
}

fn predator_color(p: f32) -> vec3<f32> {
    return lerp_rgb(rgb8(22.0, 18.0, 20.0), rgb8(220.0, 70.0, 60.0),
                    clamp(p * 40.0, 0.0, 1.0));
}

fn diversity_color(d: f32) -> vec3<f32> {
    return lerp_rgb(rgb8(30.0, 20.0, 45.0), rgb8(90.0, 220.0, 200.0), d);
}

fn biome_color(id: u32) -> vec3<f32> {
    switch id {
        case 0u: { return rgb8(24.0, 44.0, 110.0); }   // Ocean
        case 1u: { return rgb8(58.0, 120.0, 216.0); }  // River
        case 2u: { return rgb8(70.0, 120.0, 110.0); }  // Wetland
        case 3u: { return rgb8(225.0, 200.0, 140.0); } // Desert
        case 4u: { return rgb8(150.0, 180.0, 90.0); }  // Grassland
        case 5u: { return rgb8(170.0, 160.0, 100.0); } // Shrubland
        case 6u: { return rgb8(45.0, 120.0, 55.0); }   // TemperateForest
        case 7u: { return rgb8(15.0, 95.0, 45.0); }    // Rainforest
        case 8u: { return rgb8(60.0, 100.0, 80.0); }   // Taiga
        case 9u: { return rgb8(160.0, 160.0, 140.0); } // Tundra
        case 10u: { return rgb8(130.0, 125.0, 120.0); } // Bare
        default: { return rgb8(235.0, 240.0, 248.0); } // Ice
    }
}

// Species tint: signature(seed) -> species_seed -> splitmix64 -> vivid color
// (matching `viewer_host::map::species_color` + the world-core seed folds).
fn species_color(temperature: f32, moisture: f32, fertility: f32, biome: u32, index: u32) -> vec3<f32> {
    // HabitatSignature banding (habitat.rs `band`).
    let tband = min(u32(clamp((temperature + 20.0) / 60.0, 0.0, 1.0) * 6.0), 5u);
    let mband = min(u32(clamp(moisture, 0.0, 1.0) * 5.0), 4u);
    let fband = min(u32(clamp(fertility, 0.0, 1.0) * 4.0), 3u);
    var h = HABITAT_BASIS;
    h = mix64(h, u64_from_u32(ALGORITHM_VERSION));
    h = mix64(h, u64_from_u32(biome));
    h = mix64(h, u64_from_u32(tband));
    h = mix64(h, u64_from_u32(mband));
    h = mix64(h, u64_from_u32(fband));
    var s = SPECIES_BASIS;
    s = mix64(s, h);
    s = mix64(s, u64_from_u32(index));
    let c = splitmix64(s);
    let r = 96u + (c.x & 0x7Fu);
    let g = 96u + ((c.x >> 20u) & 0x7Fu);
    let b = 96u + ((c.y >> 8u) & 0x7Fu);
    return rgb8(f32(r), f32(g), f32(b));
}

fn missing_color(cx: u32, cy: u32) -> vec3<f32> {
    if ((cx / 4u + cy / 4u) % 2u) == 0u {
        return rgb8(24.0, 24.0, 28.0);
    }
    return rgb8(32.0, 32.0, 38.0);
}

// --- vertex: fullscreen triangle -------------------------------------------

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VsOut {
    // Oversized triangle covering the viewport; uv in [0, 1] over the quad.
    var out: VsOut;
    let x = f32(i32(index & 1u) * 4 - 1);
    let y = f32(i32(index >> 1u) * 4 - 1);
    out.position = vec4<f32>(x, -y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (y + 1.0) * 0.5);
    return out;
}

// --- fragment ---------------------------------------------------------------

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let side = params.side_cells;
    // Canonical center zoom: identical source-space transform for the field,
    // refinement, and sparse overlay. Row 0 remains north.
    let center = side * 0.5;
    let zoom = max(params.zoom, 1.0);
    let px = (in.uv.x * side - center) / zoom + center;
    let py = (in.uv.y * side - center) / zoom + center;
    let res = u32(params.resolution);
    let cellx = u32(px);
    let celly = u32(py);
    let col = cellx / res;
    let row = celly / res;
    let span = u32(2 * params.half_regions + 1);
    var rgb = rgb8(18.0, 18.0, 22.0);

    if col < span && row < span {
        let cx = cellx % res;
        let cy = res - 1u - (celly % res);
        let slot = slots[row * span + col];
        if slot < 0 {
            rgb = missing_color(cx, cy);
        } else {
            let tile = vec2<u32>(u32(slot) % params.atlas_tiles_x,
                                 u32(slot) / params.atlas_tiles_x);
            let texel = vec2<i32>(tile * res + vec2<u32>(cx, cy));
            let p0 = textureLoad(plane0, texel, 0); // elev, hard, temp, moist
            let p1 = textureLoad(plane1, texel, 0); // river, wet, depth, fert
            let p2 = textureLoad(plane2, texel, 0); // veg, canopy, herb, pred
            let p3 = textureLoad(plane3, texel, 0); // diversity, presence
            let ip = textureLoad(ints, texel, 0);   // biome, dominant
            let presence = u32(p3.y);
            // Presence bits: 1 elevation..river.., bit order matches the
            // shell's packing; bit 13 = biome, bit 14 = dominant.
            let have_base = (presence & 0x1u) != 0u;      // elevation
            let have_biome = (presence & 0x2000u) != 0u;
            let have_l8 = (presence & 0x4000u) != 0u;

            var elev = p0.x;
            if params.channel == 1u || params.channel == 0u {
                if params.refine_octave_count > 0u && have_base {
                    elev = elev + refinement_delta(px, py);
                }
            }

            switch params.channel {
                case 0u: { // composite
                    if have_base && have_biome {
                        if elev < SEA_LEVEL {
                            rgb = elevation_color(elev);
                        } else {
                            var c = biome_color(ip.x);
                            c = lerp_rgb(c, rgb8(58.0, 120.0, 216.0), p1.x * 0.8);
                            c = lerp_rgb(c, rgb8(35.0, 60.0, 70.0), p1.y * 0.25);
                            c = lerp_rgb(c, rgb8(130.0, 125.0, 120.0),
                                         clamp((elev - 500.0) / 400.0, 0.0, 1.0));
                            if have_l8 {
                                c = lerp_rgb(c, species_color(p0.z, p0.w, p1.w, ip.x, ip.y), 0.18);
                            }
                            rgb = c;
                        }
                    } else {
                        rgb = missing_color(cx, cy);
                    }
                }
                case 1u: { if have_base { rgb = elevation_color(elev); } else { rgb = missing_color(cx, cy); } }
                case 2u: { if (presence & 0x4u) != 0u { rgb = temperature_color(p0.z); } else { rgb = missing_color(cx, cy); } }
                case 3u: { if (presence & 0x8u) != 0u { rgb = moisture_color(p0.w); } else { rgb = missing_color(cx, cy); } }
                case 4u: { if (presence & 0x10u) != 0u { rgb = river_color(p1.x); } else { rgb = missing_color(cx, cy); } }
                case 5u: { if (presence & 0x20u) != 0u { rgb = wetness_color(p1.y); } else { rgb = missing_color(cx, cy); } }
                case 6u: { if (presence & 0xC0u) == 0xC0u { rgb = soil_color(p1.z, p1.w); } else { rgb = missing_color(cx, cy); } }
                case 7u: { if have_biome { rgb = biome_color(ip.x); } else { rgb = missing_color(cx, cy); } }
                case 8u: { if (presence & 0x100u) != 0u { rgb = vegetation_color(p2.x); } else { rgb = missing_color(cx, cy); } }
                case 9u: { if (presence & 0x400u) != 0u { rgb = herbivore_color(p2.z); } else { rgb = missing_color(cx, cy); } }
                case 10u: { if (presence & 0x800u) != 0u { rgb = predator_color(p2.w); } else { rgb = missing_color(cx, cy); } }
                case 11u: { if (presence & 0x1000u) != 0u { rgb = diversity_color(p3.x); } else { rgb = missing_color(cx, cy); } }
                default: { rgb = missing_color(cx, cy); }
            }

        }
    }

    // CPU-drawn sparse overlay on top (nearest texel — cell-crisp like the
    // CPU path).
    let osize = textureDimensions(overlay);
    let opix = vec2<i32>(i32(px), i32(py));
    let over = textureLoad(overlay, clamp(opix, vec2<i32>(0), vec2<i32>(osize) - 1), 0);
    rgb = mix(rgb, over.rgb, over.a);

    return vec4<f32>(srgb_to_linear(rgb), 1.0);
}
