// Rigid instanced POV organisms (3d-phase-4-plan.md §6.9). The color and
// depth-only entry points call the same position helper, keeping visible and
// caster silhouettes identical. All positions are reconstructed relative to
// a split high/low camera; no absolute large f32 coordinate reaches a matrix.

struct OrganismParams {
    view_proj: mat4x4<f32>,
    light_view_proj: mat4x4<f32>,
    camera_hi: vec4<f32>,
    camera_lo: vec4<f32>,
    sun_dir: vec3<f32>,
    fog_start: f32,
    fog_color: vec3<f32>,
    fog_end: f32,
    sky_ambient: vec3<f32>,
    _pad0: f32,
    ground_ambient: vec3<f32>,
    _pad1: f32,
    // (inverse shadow resolution, enabled, wrapped time, producer tint).
    shadow: vec4<f32>,
}

@group(0) @binding(0) var<uniform> params: OrganismParams;
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // xyz = split position high; w = sin(yaw).
    @location(2) position_hi_yaw_sin: vec4<f32>,
    // xyz = split position low; w = cos(yaw).
    @location(3) position_lo_yaw_cos: vec4<f32>,
    // xyz = non-uniform scale; w = optional bob amplitude.
    @location(4) scale_bob_amplitude: vec4<f32>,
    // Expressed sRGB plus producer flag in alpha.
    @location(5) color: vec4<f32>,
    // x = normalized ground AO; remaining bytes reserved.
    @location(6) ambient_flags: vec4<f32>,
    @location(7) bob_phase: f32,
}

const TAU: f32 = 6.28318530718;
const SUN_STRENGTH: f32 = 1.2;

fn rotate_yaw(v: vec3<f32>, yaw_sin: f32, yaw_cos: f32) -> vec3<f32> {
    return vec3<f32>(
        yaw_cos * v.x - yaw_sin * v.y,
        yaw_sin * v.x + yaw_cos * v.y,
        v.z,
    );
}

fn organism_position(in: VsIn) -> vec3<f32> {
    let center = (in.position_hi_yaw_sin.xyz - params.camera_hi.xyz)
        + (in.position_lo_yaw_cos.xyz - params.camera_lo.xyz);
    let local = rotate_yaw(
        in.position * in.scale_bob_amplitude.xyz,
        in.position_hi_yaw_sin.w,
        in.position_lo_yaw_cos.w,
    );
    // The host keeps amplitude zero unless the optional activity gate ships.
    let bob = in.scale_bob_amplitude.w
        * (0.5 + 0.5 * sin(TAU * (params.shadow.z * 0.25 + in.bob_phase)));
    return center + local + vec3<f32>(0.0, 0.0, bob);
}

fn organism_normal(in: VsIn) -> vec3<f32> {
    // Inverse non-uniform scale is load-bearing for slab/pillar forms.
    let inverse_scale = 1.0 / max(in.scale_bob_amplitude.xyz, vec3<f32>(1e-6));
    return normalize(rotate_yaw(
        in.normal * inverse_scale,
        in.position_hi_yaw_sin.w,
        in.position_lo_yaw_cos.w,
    ));
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) ground_ao: f32,
    @location(3) pos: vec3<f32>,
    @location(4) light_clip: vec4<f32>,
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
    let pos = organism_position(in);
    var out: VsOut;
    out.clip = params.view_proj * vec4<f32>(pos, 1.0);
    out.normal = organism_normal(in);
    out.color = in.color;
    out.ground_ao = in.ambient_flags.x;
    out.pos = pos;
    out.light_clip = params.light_view_proj * vec4<f32>(pos, 1.0);
    return out;
}

@vertex
fn vs_shadow(in: VsIn) -> @builtin(position) vec4<f32> {
    return params.light_view_proj * vec4<f32>(organism_position(in), 1.0);
}

fn shadow_visibility(light_clip: vec4<f32>, normal: vec3<f32>) -> f32 {
    if (params.shadow.y < 0.5 || params.shadow.x <= 0.0 || abs(light_clip.w) < 1e-6) {
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
    // Keep receiver constants layout-compatible with terrain. The organism
    // uniform uses the same tuned literals in shader-owned form here.
    let reference = ndc.z - (0.00035 + 0.0015 * slope);
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
    let n = normalize(in.normal);
    var albedo = pow(in.color.rgb, vec3<f32>(2.2));
    // A subtle shader-only trophic cue; the packed expressed RGB stays byte
    // identical to the 2D marker and consumers remain unmodified.
    let producer_tint = vec3<f32>(0.20, 0.32, 0.22);
    albedo = mix(albedo, producer_tint, params.shadow.w * step(0.001, in.color.a));
    let sun = SUN_STRENGTH * max(dot(n, -params.sun_dir), 0.0)
        * shadow_visibility(in.light_clip, n);
    let ao = mix(1.0, in.ground_ao, params.shadow.y);
    let ambient = mix(params.ground_ambient, params.sky_ambient, n.z * 0.5 + 0.5) * ao;
    let lit = albedo * (vec3<f32>(sun) + ambient);
    let dist = length(in.pos);
    let fog = smoothstep(params.fog_start, params.fog_end, dist);
    return vec4<f32>(mix(lit, params.fog_color, fog), 1.0);
}
