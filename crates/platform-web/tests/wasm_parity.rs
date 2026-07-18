#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn loom_stage_zero_a_frozen_vector_matches_native() {
    assert!(loom_transport::frozen_parity_vector_matches());
}

#[wasm_bindgen_test]
fn loom_stage_zero_a_probe_meets_wasm_interaction_gate() {
    let (source, intent, _) = loom_transport::parity_fixture().unwrap();
    for _ in 0..10 {
        assert!(matches!(
            loom_transport::probe(&source, &intent, u64::MAX),
            loom_transport::ProbeOutcome::Complete(_)
        ));
    }
    let started = js_sys::Date::now();
    for _ in 0..100 {
        assert!(matches!(
            loom_transport::probe(&source, &intent, u64::MAX),
            loom_transport::ProbeOutcome::Complete(_)
        ));
    }
    let average_ms = (js_sys::Date::now() - started) / 100.0;
    assert!(
        average_ms < 10.0,
        "average wasm probe took {average_ms:.3} ms"
    );
}

#[wasm_bindgen_test]
fn every_public_parity_probe_matches_the_shared_native_golden() {
    assert_eq!(
        platform_web::origin_feature_hash(),
        platform_web::EXPECTED_ORIGIN_FEATURE_HASH
    );
    assert_eq!(
        platform_web::terrain_gradient_seed_sample(),
        platform_web::EXPECTED_TERRAIN_GRADIENT_SEED
    );
    assert_eq!(
        platform_web::control_point_seed_sample(),
        platform_web::EXPECTED_CONTROL_POINT_SEED
    );
    assert_eq!(
        platform_web::lithology_seed_sample(),
        platform_web::EXPECTED_LITHOLOGY_SEED
    );
    assert_eq!(
        platform_web::drainage_routing_sample(),
        platform_web::EXPECTED_DRAINAGE_ROUTING
    );
    assert_eq!(
        platform_web::drainage_topology_sample(),
        platform_web::EXPECTED_DRAINAGE_TOPOLOGY
    );
    assert_eq!(platform_web::genome_sample(), platform_web::EXPECTED_GENOME);
    assert_eq!(
        platform_web::food_web_sample(),
        platform_web::EXPECTED_FOOD_WEB
    );
    assert_eq!(platform_web::steer_sample(), platform_web::EXPECTED_STEER);
    assert_eq!(
        platform_web::canonical_anchor_signature_sample(),
        platform_web::EXPECTED_CANONICAL_ANCHOR_SIGNATURE
    );
    assert_eq!(
        platform_web::record_codec_sample(),
        platform_web::EXPECTED_RECORD_CODEC
    );
    assert_eq!(
        platform_web::shared_steer_sample(),
        platform_web::EXPECTED_SHARED_STEER
    );
    assert_eq!(
        platform_web::route_attraction_sample(),
        platform_web::EXPECTED_ROUTE_ATTRACTION
    );
}
