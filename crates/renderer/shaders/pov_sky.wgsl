// POV sky: a fullscreen triangle rasterized at the far plane, shaded as an
// analytic sky — zenith→horizon gradient, a sun disc placed at the frame's
// light direction so sky and lighting always agree, and a few procedural
// clouds. Depth compare is LessEqual against the cleared 1.0 depth with
// writes off, so the sky covers exactly the pixels no geometry claimed and
// costs nothing where terrain already resolved.
//
// Everything here is derived presentation (ADR 0017): the palette and cover
// arrive per frame from the shell (pov_host::PovSky — a fixed "earth
// standard" day for now, eventually driven by the world model), and the
// sin-hash cloud noise is never a source of identity. The horizon color is
// also the frame's fog color, so fogged terrain dissolves into this sky by
// construction.

struct SkyParams {
    // Inverse of the camera-relative view-projection. It contains no
    // translation, so unprojecting a far-plane NDC point yields a world-space
    // view ray directly.
    inv_view_proj: mat4x4<f32>,
    // Normalized, pointing from the sun toward the ground (the frame's
    // sun_dir); the visible sun sits at -sun_dir.
    sun_dir: vec3<f32>,
    // Cloud coverage in [0, 1]; 0 skips the noise entirely.
    cloud_cover: f32,
    // Linear-light gradient anchors.
    zenith: vec3<f32>,
    _pad0: f32,
    horizon: vec3<f32>,
    _pad1: f32,
}

@group(0) @binding(0) var<uniform> params: SkyParams;

struct SkyOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_sky(@builtin(vertex_index) vi: u32) -> SkyOut {
    // The standard single clip-space triangle covering the screen, pinned to
    // the far plane (z = w = 1, exactly NDC depth 1.0).
    let x = f32((vi << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vi & 2u) * 2.0 - 1.0;
    var out: SkyOut;
    out.clip = vec4<f32>(x, y, 1.0, 1.0);
    out.ndc = vec2<f32>(x, y);
    return out;
}

// Presentation-only sin-hash value noise for the cloud field. Cheap enough
// for llvmpipe (the sky shades only geometry-free pixels), stable per ray.
fn hash21(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Three octaves, normalized to [0, 1]. Non-integer lacunarity keeps octave
// lattices from ever aligning.
fn fbm(p: vec2<f32>) -> f32 {
    var total = 0.5 * value_noise(p);
    total = total + 0.25 * value_noise(p * 2.03 + vec2<f32>(19.7, 7.3));
    total = total + 0.125 * value_noise(p * 4.07 + vec2<f32>(41.3, 89.1));
    return total / 0.875;
}

@fragment
fn fs_sky(in: SkyOut) -> @location(0) vec4<f32> {
    let world = params.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let dir = normalize(world.xyz / world.w);
    let up = clamp(dir.z, -1.0, 1.0);

    // Zenith→horizon gradient; the shallow exponent widens the bright
    // horizon band the way scattering does on a clear day.
    let t = pow(clamp(up, 0.0, 1.0), 0.45);
    var sky = mix(params.horizon, params.zenith, t);
    // Below the horizon (visible only through frontier holes and off cliff
    // edges): settle into a darker ground haze instead of extrapolating.
    sky = mix(sky, params.horizon * 0.55, clamp(-up * 6.0, 0.0, 1.0));

    // The sun: a slightly-larger-than-life disc plus a warm near glow and a
    // wide haze, all keyed to the shared light direction.
    let toward_sun = dot(dir, -params.sun_dir);
    let disc = smoothstep(0.99985, 0.99995, toward_sun);
    let glow = pow(clamp(toward_sun, 0.0, 1.0), 180.0);
    let haze = pow(clamp(toward_sun, 0.0, 1.0), 8.0);
    sky = sky + vec3<f32>(1.0, 0.98, 0.92) * disc * 1.5;
    sky = sky + vec3<f32>(1.0, 0.85, 0.55) * glow * 0.45;
    sky = sky + vec3<f32>(0.35, 0.32, 0.28) * haze * 0.22;

    // Clouds: fbm over the ray projected onto a virtual cloud plane. The
    // +0.12 bound keeps the projection finite at the horizon, where the
    // fade hides the compression anyway.
    if (up > 0.015 && params.cloud_cover > 0.001) {
        let plane = dir.xy * (1.6 / (up + 0.12));
        let field = fbm(plane);
        let threshold = 1.0 - params.cloud_cover;
        let horizon_fade = smoothstep(0.02, 0.18, up);
        let density = smoothstep(threshold - 0.08, threshold + 0.22, field) * horizon_fade;
        // A touch of silver lining on the sun side.
        let lit = vec3<f32>(0.90, 0.92, 0.95) + vec3<f32>(0.25) * haze;
        sky = mix(sky, lit, density * 0.85);
    }

    return vec4<f32>(sky, 1.0);
}
