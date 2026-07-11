// Debug top-down false-color map (phase-1-plan.md section 10, milestone M5).
//
// The CPU composes the active world window into an RGBA texture (false color
// plus overlays); this pipeline just presents it. One fullscreen triangle,
// generated from the vertex index — no vertex buffers. Kept WGSL-only so the
// renderer stays WebGPU-portable (AGENTS.md, Conventions).

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    // Oversized triangle covering the viewport: (-1,-3), (-1,1), (3,1).
    var out: VertexOutput;
    let x = f32(i32(index) / 2) * 4.0 - 1.0;
    let y = f32(i32(index) % 2) * 4.0 - 1.0;
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    // v flipped: texture row 0 is the north (max-y) edge of the window.
    out.uv = vec2<f32>((x + 1.0) * 0.5, 1.0 - (y + 1.0) * 0.5);
    return out;
}

@group(0) @binding(0) var map_texture: texture_2d<f32>;
@group(0) @binding(1) var map_sampler: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(map_texture, map_sampler, in.uv);
}
