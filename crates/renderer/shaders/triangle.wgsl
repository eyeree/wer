// Placeholder WGSL shader for the Phase 0 shell.
//
// Not yet wired into a pipeline — the bootstrap renderer only clears the
// surface. It exists so the shader-loading path and WGSL toolchain are exercised
// from the start (all shaders use WGSL for browser portability, section 19).

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) color: vec3<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
    // A full-screen-ish triangle in clip space.
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.6),
        vec2<f32>(-0.6, -0.6),
        vec2<f32>(0.6, -0.6),
    );
    var colors = array<vec3<f32>, 3>(
        vec3<f32>(1.0, 0.3, 0.4),
        vec3<f32>(0.3, 0.9, 0.5),
        vec3<f32>(0.3, 0.5, 1.0),
    );
    var out: VsOut;
    out.clip_pos = vec4<f32>(positions[vertex_index], 0.0, 1.0);
    out.color = colors[vertex_index];
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
