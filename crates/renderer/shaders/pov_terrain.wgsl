// Vertex-lit POV terrain (3d-phase-1-plan.md §5.6): Lambert sun + hemisphere
// ambient + distance fog, over per-vertex material color.
//
// Deliberately minimal — no specular, no shadows, no textures, and no GPU
// displacement: the refinement octaves of `compose_map.wgsl` are *not* ported
// here, because displacing vertices away from the CPU-authoritative
// heightfield would contradict the ground truth 3D-2's collision will stand
// on (ADR 0016/0017, design §3.2). Everything here is derived presentation;
// nothing is read back.

struct PovParams {
    // Camera-relative view-projection (the view translation is applied on the
    // CPU in f64 through the per-chunk offsets, §4 of the plan).
    view_proj: mat4x4<f32>,
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
}

// The chunk's region origin relative to the camera (f64 subtraction on the
// CPU, truncated to f32): every position the GPU sees stays small, so f32 is
// exact-enough at any world coordinate.
struct ChunkOffset {
    offset: vec3<f32>,
    _pad: f32,
}

@group(0) @binding(0) var<uniform> params: PovParams;
@group(1) @binding(0) var<uniform> chunk: ChunkOffset;

struct VsIn {
    // Chunk-local: x, y in [0, REGION_SIZE], z = elevation.
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // sRGB bytes, Unorm8x4 -> 0..1 here.
    @location(2) color: vec4<f32>,
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) dist: f32,
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
    let pos = in.position + chunk.offset; // camera-relative world space
    var out: VsOut;
    out.clip = params.view_proj * vec4<f32>(pos, 1.0);
    out.normal = in.normal;
    out.color = in.color;
    out.dist = length(pos);
    return out;
}

// Sun strength, tuned with the frame ambients so mid-day flat ground roughly
// matches the 2D palette's value range (plan §13, color-space drift note).
const SUN_STRENGTH: f32 = 0.75;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Cheap sRGB decode (pow 2.2, noted in plan §5.6): the vertex bytes are
    // the 2D palette's sRGB values; the surface is Rgba8UnormSrgb-family, so
    // the hardware re-encodes on write.
    let albedo = pow(in.color.rgb, vec3<f32>(2.2));
    let n = normalize(in.normal);
    let sun = SUN_STRENGTH * max(dot(n, -params.sun_dir), 0.0);
    let ambient = mix(params.ground_ambient, params.sky_ambient, n.z * 0.5 + 0.5);
    let lit = albedo * (vec3<f32>(sun) + ambient);
    let fog = smoothstep(params.fog_start, params.fog_end, in.dist);
    return vec4<f32>(mix(lit, params.fog_color, fog), 1.0);
}
