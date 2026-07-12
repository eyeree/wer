//! WGSL structural validation without a GPU (phase-6-plan.md §11.5): CI has
//! no adapter, so the shaders are parsed and validated with naga directly —
//! the same front end wgpu runs at pipeline creation.

fn validate(label: &str, source: &str) {
    let module = naga::front::wgsl::parse_str(source)
        .unwrap_or_else(|e| panic!("{label}: WGSL parse failed:\n{e}"));
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    );
    validator
        .validate(&module)
        .unwrap_or_else(|e| panic!("{label}: WGSL validation failed:\n{e:?}"));
}

#[test]
fn debug_map_shader_validates() {
    validate("debug_map.wgsl", renderer::SHADER_DEBUG_MAP);
}

#[test]
fn compose_map_shader_validates() {
    validate("compose_map.wgsl", renderer::gpumap::SHADER_COMPOSE_MAP);
}
