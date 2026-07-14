//! WGSL structural validation without a GPU (phase-6-plan.md §11.5): CI has
//! no adapter, so the shaders are parsed and validated with naga directly —
//! the same front end wgpu runs at pipeline creation.

fn validate(label: &str, source: &str) -> naga::Module {
    let module = naga::front::wgsl::parse_str(source)
        .unwrap_or_else(|e| panic!("{label}: WGSL parse failed:\n{e}"));
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    );
    validator
        .validate(&module)
        .unwrap_or_else(|e| panic!("{label}: WGSL validation failed:\n{e:?}"));
    module
}

fn assert_entry_points(module: &naga::Module, expected: &[&str]) {
    let actual: Vec<&str> = module
        .entry_points
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    for name in expected {
        assert!(
            actual.contains(name),
            "missing entry point {name:?}; have {actual:?}"
        );
    }
}

fn function_body<'a>(source: &'a str, name: &str) -> &'a str {
    let signature = format!("fn {name}");
    let start = source.find(&signature).expect("function exists");
    let open = source[start..].find('{').expect("function body") + start;
    let mut depth = 0u32;
    for (relative, byte) in source.as_bytes()[open..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[open + 1..open + relative];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated function {name}")
}

#[test]
fn debug_map_shader_validates() {
    validate("debug_map.wgsl", renderer::SHADER_DEBUG_MAP);
}

#[test]
fn compose_map_shader_validates() {
    validate("compose_map.wgsl", renderer::gpumap::SHADER_COMPOSE_MAP);
}

#[test]
fn pov_terrain_shader_validates() {
    let module = validate("pov_terrain.wgsl", renderer::SHADER_POV_TERRAIN);
    assert_entry_points(&module, &["vs_main", "vs_shadow", "fs_main"]);
    assert!(renderer::SHADER_POV_TERRAIN.contains("textureSampleCompareLevel"));
    assert!(!function_body(renderer::SHADER_POV_TERRAIN, "vs_shadow").contains("shadow_map"));
}

#[test]
fn pov_water_shader_validates() {
    let module = validate("pov_water.wgsl", renderer::SHADER_POV_WATER);
    assert_entry_points(&module, &["vs_sea", "fs_sea", "vs_overlay", "fs_overlay"]);
    assert!(function_body(renderer::SHADER_POV_WATER, "fs_overlay").contains("shadow_visibility"));
    assert!(!function_body(renderer::SHADER_POV_WATER, "fs_sea").contains("shadow_visibility"));
}

#[test]
fn pov_organism_shader_validates() {
    let module = validate("pov_organism.wgsl", renderer::SHADER_POV_ORGANISM);
    assert_entry_points(&module, &["vs_main", "vs_shadow", "fs_main"]);
    assert!(renderer::SHADER_POV_ORGANISM.contains("textureSampleCompareLevel"));
    assert!(!function_body(renderer::SHADER_POV_ORGANISM, "vs_shadow").contains("shadow_map"));
}
