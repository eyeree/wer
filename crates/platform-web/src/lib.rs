//! `platform-web` — the browser/wasm application shell (section 3.2 & Phase 7).
//!
//! For the bootstrap this is a **minimal WebGPU/wasm smoke target**: it exists so
//! `world-core` is exercised through a real `wasm32` entry point from the start,
//! before native-only assumptions accumulate (section 19). The full runtime
//! (Web Workers, browser storage, WebGPU tiers, suspend/resume) arrives in
//! Phase 7. Phase 2 grew the shell only by two parity exports: the lithology
//! seed and a drainage routing sample (phase-2-plan.md §12.5).

use world_core::{
    anchor::{
        anchor_set_signature, bound_target, domain_mask, project_plausible, steer, Anchor,
        AnchorKind, AnchorSource,
    },
    dephash::drainage_dep_hash_default,
    drainage::{drainage, MACRO_APRON, MACRO_LEVEL},
    feature_hash,
    foodweb::food_web,
    genome::Genome,
    geology::lithology_seed,
    habitat::HabitatSignature,
    hash::mix,
    possibility::PossibilityDomain,
    record::{
        encode_record, DiscoveryRecord, PossibilitySignature, RecordKind, RouteNode, RouteRecord,
    },
    route::attraction_anchors,
    species::{species_roster, species_seed},
    terrain, Biome, FeatureKey, PossibilityField, PossibilityVector, RegionCoord, POSSIBILITY_DIMS,
    REGION_SIZE, WORLD_ALGORITHM_VERSION,
};
use world_runtime::budget::Budget;
use world_runtime::stream::RegionMap;
use world_runtime::task::{InlineExecutor, TaskExecutor};
use world_runtime::tier::ResourceTier;
use world_runtime::{
    mapcolor, CHANNEL_DIVERSITY, CHANNEL_ELEVATION, CHANNEL_FERTILITY, CHANNEL_HARDNESS,
    CHANNEL_HERBIVORE, CHANNEL_MOISTURE, CHANNEL_PREDATOR, CHANNEL_RIVER, CHANNEL_SOIL_DEPTH,
    CHANNEL_TEMPERATURE, CHANNEL_VEGETATION, CHANNEL_WETNESS,
};

/// Shared native/wasm parity expectations. The wasm integration suite imports
/// these exact constants, so the two execution gates cannot drift apart.
pub const EXPECTED_ORIGIN_FEATURE_HASH: u64 = 0x4C6C_A5DE_38F9_0B17;
pub const EXPECTED_TERRAIN_GRADIENT_SEED: u64 = 0x557D_9B95_E708_EDFF;
pub const EXPECTED_CONTROL_POINT_SEED: u64 = 0xAAF0_551F_3E6F_1A1C;
pub const EXPECTED_LITHOLOGY_SEED: u64 = 0x61DD_60E4_EEF6_FD16;
pub const EXPECTED_DRAINAGE_ROUTING: u64 = 0x0000_0001_0000_000D;
pub const EXPECTED_DRAINAGE_TOPOLOGY: u64 = 0xB9FA_AD5C_9521_6B3F;
pub const EXPECTED_GENOME: u64 = 0x6023_7E3E_43E5_2590;
pub const EXPECTED_FOOD_WEB: u64 = 0x6272_09D2_6720_001B;
pub const EXPECTED_STEER: u64 = 0x9A4E_77F9_D151_9EC2;
pub const EXPECTED_CANONICAL_ANCHOR_SIGNATURE: u64 = 0xBDAA_C72D_CA08_3AF7;
pub const EXPECTED_RECORD_CODEC: u64 = 0xF42F_DCB5_3552_F850;
pub const EXPECTED_SHARED_STEER: u64 = 0xF0FB_820F_2030_1752;
pub const EXPECTED_ROUTE_ATTRACTION: u64 = 0x3D54_75F6_34AF_1C41;

/// The fixed habitat used by the Phase 3 parity exports. Only the (portable)
/// integer signature → seed → genome / roster / web chain is asserted
/// cross-platform; how a cell arrives at this signature is presentation-grade
/// (§9.3, ADR 0010).
const PARITY_SIGNATURE: HabitatSignature = HabitatSignature {
    biome: 6, // Biome::TemperateForest
    temperature_band: 3,
    moisture_band: 3,
    fertility_band: 2,
};

/// A portable smoke computation: the deterministic hash of the origin feature.
///
/// Must return the identical value on native and wasm — that equality is the
/// determinism guarantee the browser port depends on (section 23.5). It is also
/// covered by the native determinism golden test.
#[must_use]
pub fn origin_feature_hash() -> u64 {
    feature_hash(&FeatureKey {
        world_version: WORLD_ALGORITHM_VERSION,
        region: RegionCoord::new(0, 0),
        layer: 0,
        feature_index: 0,
        possibility_revision: 0,
    })
}

/// Parity sample for the terrain identity layer: the integer seed that
/// selects the gradient at lattice corner `(3, -2)` of octave 1
/// (phase-1-plan.md section 11.2). Must equal the native value — float
/// elevation is presentation state and is *not* asserted bit-equal, but the
/// integer seeds that decide where mountains are must be.
#[must_use]
pub fn terrain_gradient_seed_sample() -> u64 {
    terrain::gradient_seed(3, -2, 1)
}

/// Parity sample for the possibility-field identity layer: the control-point
/// seed at lattice coordinate `(-5, 9)` with the default spacing.
#[must_use]
pub fn control_point_seed_sample() -> u64 {
    PossibilityField::default().control_point_seed(-5, 9)
}

/// Parity sample for the geology identity layer: the lithology seed of cell
/// `(3, -2)` (phase-2-plan.md §12.5).
#[must_use]
pub fn lithology_seed_sample() -> u64 {
    lithology_seed(3, -2)
}

/// Parity sample for the drainage identity layer: flow direction and
/// accumulation of a fixed cell of macro tile `(0, 0)`, packed as
/// `(dir << 32) | accum`. Routing is all-integer topology, so **full**
/// cross-platform equality is required here — not just seed equality
/// (phase-2-plan.md §12.5, ADR 0009).
#[must_use]
pub fn drainage_routing_sample() -> u64 {
    let field = PossibilityField::default();
    let mc = RegionCoord::at_level(0, 0, MACRO_LEVEL);
    let tile = drainage(mc, &field, drainage_dep_hash_default(mc));
    let apron = MACRO_APRON as usize;
    let (gx, gy) = (apron + 7, apron + 4);
    (u64::from(tile.flow_dir_at(gx, gy)) << 32) | u64::from(tile.accum_at(gx, gy))
}

/// Broad fixed-topology parity fold spanning signs, field lattice boundaries,
/// a non-power-of-two field recipe, and three complete macro tiles (ADR 0027).
#[must_use]
pub fn drainage_topology_sample() -> u64 {
    world_core::drainage_topology_sample()
}

/// Parity sample for the procedural-genetics identity layer (phase-3-plan.md
/// §12.5): the fingerprint of the genome of a fixed species seed. `genome(seed)`
/// is the *portable* Phase 3 surface — pure integer→integer hashing — so **full**
/// cross-platform equality is required. Signature derivation is deliberately not
/// exported: it reads `f32` tiles and is presentation-grade by decision
/// (§9.3, ADR 0010).
#[must_use]
pub fn genome_sample() -> u64 {
    Genome::from_seed(species_seed(PARITY_SIGNATURE, 0)).fingerprint()
}

/// Parity sample for the food-web layer (phase-3-plan.md §12.5): the tier
/// biomass of the fixed roster's web at a fixed productivity, folded to a
/// fingerprint. Tier biomass is portable `f32`, so full cross-platform equality
/// is required (§9.3). Signature derivation is not exported.
#[must_use]
pub fn food_web_sample() -> u64 {
    let roster = species_roster(PARITY_SIGNATURE);
    food_web(&roster, 0.7).tier_biomass_fingerprint()
}

/// Parity sample for the Phase 4 steering math (phase-4-plan.md §12.5): the
/// steered-and-projected possibility vector for a fixed base and a fixed
/// scripted anchor set (an Emphasize and a Suppress, overlapping on one domain),
/// folded to a fingerprint. `steer`/`project_plausible` are pure float-
/// deterministic functions of `(base, anchor set, position)`, so full
/// cross-platform equality is required. Live capture and resonance are *not*
/// exported — they read `f32` tiles/organisms and are presentation-grade by
/// decision (§9.2, ADR 0010/0011).
#[must_use]
pub fn steer_sample() -> u64 {
    let mask_a = domain_mask(&[PossibilityDomain::Ecology, PossibilityDomain::Aesthetics]);
    let mask_b = domain_mask(&[PossibilityDomain::Aesthetics, PossibilityDomain::Morphology]);
    let anchors = [
        Anchor {
            world_pos: (64.0, -32.0),
            target: bound_target(mask_a, 0.9),
            mask: mask_a,
            kind: AnchorKind::Emphasize,
            strength: 0.75,
            falloff_radius: 1500.0,
            source: AnchorSource::Manual,
        },
        Anchor {
            world_pos: (-100.0, 40.0),
            target: bound_target(mask_b, 0.2),
            mask: mask_b,
            kind: AnchorKind::Suppress,
            strength: 0.5,
            falloff_radius: 1200.0,
            source: AnchorSource::Manual,
        },
    ];
    let base = PossibilityVector::neutral();
    let v = project_plausible(steer(base, &anchors, (0.0, 0.0)));
    let mut h: u64 = 0x57EE_5000_0DE0_0004;
    for d in v.dims {
        h = mix(h, u64::from(d.to_bits()));
    }
    h
}

/// Parity sample for ADR 0025's canonical anchor-multiset signature. It
/// includes both polarities, different masks, an exact duplicate, and inert
/// unmasked target storage. Identical IEEE inputs must hash equally on native
/// and wasm regardless of slice order.
fn canonical_anchor_sample_anchors() -> [Anchor; 3] {
    let ecology = domain_mask(&[PossibilityDomain::Ecology]);
    let living = domain_mask(&[
        PossibilityDomain::Ecology,
        PossibilityDomain::Morphology,
        PossibilityDomain::Aesthetics,
    ]);
    let mut first_target = bound_target(ecology, 0.875);
    first_target.set(PossibilityDomain::Climate, 0.9375);
    let first = Anchor {
        world_pos: (-320.5, 144.25),
        target: first_target,
        mask: ecology,
        kind: AnchorKind::Emphasize,
        strength: 0.625,
        falloff_radius: 1536.0,
        source: AnchorSource::Landform,
    };
    let mut second_target = bound_target(living, 0.1875);
    second_target.set(PossibilityDomain::Climate, 0.03125);
    let second = Anchor {
        world_pos: (96.0, -48.0),
        target: second_target,
        mask: living,
        kind: AnchorKind::Suppress,
        strength: 0.3125,
        falloff_radius: 768.5,
        source: AnchorSource::Atmosphere,
    };
    [first, second, first]
}

/// Return the fixed canonical anchor-multiset signature parity probe.
#[must_use]
pub fn canonical_anchor_signature_sample() -> u64 {
    anchor_set_signature(&canonical_anchor_sample_anchors())
}

/// The fixed shareable record used by the Phase 5 parity exports: a discovery
/// of the parity habitat's first species, built entirely from integers, so it
/// is bit-identical on every platform (ADR 0013).
fn parity_discovery() -> DiscoveryRecord {
    let mask = domain_mask(&[PossibilityDomain::Morphology, PossibilityDomain::Aesthetics]);
    let mut target = PossibilitySignature {
        buckets: [2048; world_core::POSSIBILITY_DIMS],
    };
    target.buckets[PossibilityDomain::Morphology.index()] = 3600;
    target.buckets[PossibilityDomain::Aesthetics.index()] = 3300;
    DiscoveryRecord {
        id: 0,
        source: AnchorSource::Organism {
            species: species_seed(PARITY_SIGNATURE, 0),
        },
        signature_seed: PARITY_SIGNATURE.seed(),
        target,
        mask,
        kind: AnchorKind::Emphasize,
        strength_q: 3277,
        falloff_q: 1500,
        pos_q: (300, -10),
        sequence: 7,
        name: String::from("glowfin"),
        journal: String::new(),
    }
}

/// Parity sample for the Phase 5 record codec (phase-5-plan.md §12.5): the
/// byte fold of the canonical encoding of a fixed record. The wire format is
/// the interoperability surface native-written bundles and the future browser
/// runtime share, so **byte-level** cross-platform equality is required.
#[must_use]
pub fn record_codec_sample() -> u64 {
    let mut record = parity_discovery();
    record.id = record.content_id();
    let bytes = encode_record(RecordKind::Discovery, &record);
    let mut h: u64 = 0x5EC0_7D00_C0DE_0001;
    h = mix(h, bytes.len() as u64);
    for b in bytes {
        h = mix(h, u64::from(b));
    }
    h
}

/// Parity sample for shared-anchor steering (phase-5-plan.md §12.5): the
/// steered-and-projected vector produced by an anchor reconstructed from a
/// fixed [`DiscoveryRecord`]. The record carries only quantized integers and
/// `steer`/`project_plausible` are float-deterministic, so shared steering is
/// portable **end-to-end** — the shared-anchor guarantee (ADR 0013). Live
/// vault I/O is deliberately not exported (no browser storage until Phase 7).
#[must_use]
pub fn shared_steer_sample() -> u64 {
    let mut record = parity_discovery();
    record.id = record.content_id();
    let anchor = record.to_anchor();
    let base = PossibilityVector::neutral();
    let v = project_plausible(steer(base, &[anchor], (256.0, 0.0)));
    let mut h: u64 = 0x5A4E_D57E_E200_0005;
    h = mix(h, record.id);
    for d in v.dims {
        h = mix(h, u64::from(d.to_bits()));
    }
    h
}

/// Additive ADR 0026 parity probe for aggregate route normalization. The two
/// quantized routes contribute more candidates than are selected and more raw
/// peak pull than the global route-channel cap. Count, nearest-order strength
/// bits, and the projected steering result are folded into one portable value.
#[must_use]
pub fn route_attraction_sample() -> u64 {
    let mut signature = PossibilitySignature {
        buckets: [2048; world_core::POSSIBILITY_DIMS],
    };
    signature.buckets[PossibilityDomain::Ecology.index()] = 3900;
    signature.buckets[PossibilityDomain::Aesthetics.index()] = 3500;
    let node = |x, cost| RouteNode {
        pos_q: (x, 0),
        signature,
        current_signature: None,
        cost_q: cost,
        stability_q: 0,
        anchor_sig: 0,
        distance_q: 0,
    };
    let mut first = RouteRecord::new(
        vec![node(0, 10), node(32, 11), node(64, 12), node(96, 13)],
        vec![],
        1,
        String::from("parity-a"),
    );
    first.usage = 3;
    let mut second = RouteRecord::new(
        vec![node(0, 20), node(16, 21), node(48, 22), node(80, 23)],
        vec![],
        2,
        String::from("parity-b"),
    );
    second.usage = 19;
    let anchors = attraction_anchors([&second, &first], (0.0, 0.0), 5);
    let value = project_plausible(steer(PossibilityVector::neutral(), &anchors, (24.0, 0.0)));
    let mut h = mix(0xA77A_C710_0026_0001, anchors.len() as u64);
    for anchor in anchors {
        h = mix(h, u64::from(anchor.strength.to_bits()));
    }
    for dimension in value.dims {
        h = mix(h, u64::from(dimension.to_bits()));
    }
    h
}

/// The browser map channels (phase-7-plan.md §3.3 "map channel selection"):
/// the presentation-only subset of the native shell's channel list the web
/// toolbar exposes, painted from the shared `world_runtime::mapcolor` table
/// so both viewers show the identical false-color world.
const MAP_CHANNELS: [&str; 14] = [
    "composite",
    "elevation",
    "geology",
    "temperature",
    "moisture",
    "river",
    "wetness",
    "soil",
    "biome",
    "vegetation",
    "herbivore",
    "predator",
    "diversity",
    "species",
];

/// Settle passes for a fresh window: fresh regions snap to target at load,
/// and a handful of extra passes drains realization/resonance — the same
/// fixture count the native headless screenshot path uses.
const SETTLE_PASSES: u32 = 8;

#[derive(Debug)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
struct WebAppState {
    frame_index: u64,
    world_pos: (f64, f64),
    possibility: PossibilityVector,
    target: PossibilityVector,
    active_channel: u8,
    /// The real streamed world window (phase-7-plan.md §4.1 milestone 2):
    /// the same `RegionMap` the native shell drives, settled inline —
    /// wasm has no threads here, and ADR 0018 makes the settled state
    /// identical regardless.
    map: RegionMap,
    field: PossibilityField,
    /// View half-extent in regions, derived from the tier's load radius
    /// exactly as the native shell derives its composer size.
    half_regions: i32,
    /// Lazily settled on first compose so constructing the facade (and the
    /// many native unit tests that never render) stays cheap.
    settled: bool,
    tier: &'static str,
    cache_ceiling_mb: u32,
    runtime_tier: ResourceTier,
    benchmark_ms: f32,
    worker_mode: &'static str,
    worker_backlog: u32,
    workers: u32,
    cancellations: u32,
    stale_results: u32,
    storage: &'static str,
    pending_writes: u32,
    storage_failures: u32,
    record_count: u32,
    session_snapshot: Option<String>,
    renderer: &'static str,
    view_mode: &'static str,
    pov_supported: bool,
    pointer_lock: bool,
    /// POV motion mode (`pov:walk` toggles walk ↔ fly), mirroring the native
    /// shell's `F` key.
    pov_walk: bool,
    /// The native POV diagnostic toggles (`B`/`N`/`V`), mirrored so the
    /// browser viewer drives the same presentation switches the shared 3D
    /// renderer exposes: baked sun-visibility/AO, per-fragment detail
    /// normals, and the water passes. Presentation-only — never part of the
    /// settle hash.
    pov_baked_light: bool,
    pov_detail_normals: bool,
    pov_water: bool,
    /// POV render scale (the native `WER_POV_SCALE`): the fraction of the
    /// canvas resolution the 3D pass rasterizes at before the upscale blit —
    /// the practical fps knob wherever fragment cost is CPU-bound.
    pov_render_scale: f32,
    compose_enabled: bool,
    refinement_enabled: bool,
    device_losses: u32,
    warnings: Vec<String>,
    executor_parallelism: usize,
    last_command: String,
}

impl Default for WebAppState {
    fn default() -> Self {
        let tier = ResourceTier::Low;
        let cfg = tier.stream_config();
        Self {
            frame_index: 0,
            world_pos: (0.0, 0.0),
            possibility: PossibilityVector::neutral(),
            target: PossibilityVector::neutral(),
            active_channel: 0,
            half_regions: (cfg.load_radius / REGION_SIZE).ceil() as i32,
            map: RegionMap::new(cfg),
            field: PossibilityField::default(),
            settled: false,
            tier: "WebLow",
            cache_ceiling_mb: 48,
            runtime_tier: ResourceTier::Low,
            benchmark_ms: 0.0,
            worker_mode: "inline",
            worker_backlog: 0,
            workers: 1,
            cancellations: 0,
            stale_results: 0,
            storage: "memory",
            pending_writes: 0,
            storage_failures: 0,
            record_count: 0,
            session_snapshot: None,
            renderer: "cpu-fallback",
            view_mode: "map",
            pov_supported: false,
            pointer_lock: false,
            pov_walk: false,
            pov_baked_light: true,
            pov_detail_normals: true,
            pov_water: true,
            pov_render_scale: 1.0,
            compose_enabled: true,
            refinement_enabled: false,
            device_losses: 0,
            warnings: Vec::new(),
            executor_parallelism: InlineExecutor.parallelism(),
            last_command: String::new(),
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
impl WebAppState {
    fn new(config: &str) -> Self {
        let mut state = Self::default();
        if config.contains("\"tier\":\"mid\"") {
            state.set_tier(ResourceTier::Mid);
        } else if config.contains("\"tier\":\"high\"") {
            state.set_tier(ResourceTier::High);
        }
        if config.contains("\"storage\":true") {
            state.storage = "indexeddb-pending";
            state
                .warnings
                .push(String::from("IndexedDB storage lands in Phase 7-7"));
        }
        if config.contains("\"webgpu\":true") {
            state.set_renderer_webgpu();
        }
        if config.contains("\"worker_mode\":\"workers\"") {
            state.worker_mode = "workers";
            state.workers = 2;
        }
        state
    }

    fn update(&mut self, dt_ms: f64, input: &str) {
        self.frame_index = self.frame_index.wrapping_add(1);
        let step = (dt_ms / 16.666_667).clamp(0.0, 4.0);
        let speed = 3.0 * step;
        if input.contains("\"move_x\":1") {
            self.world_pos.0 += speed;
        }
        if input.contains("\"move_x\":-1") {
            self.world_pos.0 -= speed;
        }
        if input.contains("\"move_y\":1") {
            self.world_pos.1 += speed;
        }
        if input.contains("\"move_y\":-1") {
            self.world_pos.1 -= speed;
        }
    }

    fn apply_command(&mut self, command: &str) {
        self.last_command.clear();
        self.last_command.push_str(command);
        if command.contains("channel:composite") {
            self.active_channel = 0;
        } else if command.contains("\"id\":\"channel\"") {
            // The channel select (phase-7-plan.md §3.3): value names index
            // the shared MAP_CHANNELS table.
            if let Some(index) = MAP_CHANNELS
                .iter()
                .position(|name| command.contains(&format!("\"value\":\"{name}\"")))
            {
                self.active_channel = index as u8;
            }
        } else if command.contains("toggle:refinement") {
            self.refinement_enabled = !self.refinement_enabled;
            self.target.set(PossibilityDomain::Aesthetics, 0.625);
        } else if command.contains("toggle:compose") {
            self.compose_enabled = !self.compose_enabled;
            self.target.set(PossibilityDomain::Planetary, 0.5625);
        } else if command.contains("renderer:webgpu") {
            self.set_renderer_webgpu();
        } else if command.contains("renderer:device-lost") {
            self.set_renderer_cpu();
            self.device_losses = self.device_losses.saturating_add(1);
            self.warnings
                .push(String::from("WebGPU device lost; CPU map fallback active"));
        } else if command.contains("renderer:cpu") {
            self.set_renderer_cpu();
        } else if command.contains("worker:inline") {
            self.worker_mode = "inline";
            self.worker_backlog = 0;
            self.workers = 1;
        } else if command.contains("worker:workers") {
            self.worker_mode = "workers";
            self.workers = 2;
        } else if command.contains("worker:shared") {
            self.worker_mode = "shared-memory";
            self.workers = 2;
        } else if command.contains("worker:cancel-storm") {
            self.worker_backlog = 0;
            self.cancellations = self.cancellations.saturating_add(8);
            self.stale_results = self.stale_results.saturating_add(3);
        } else if command.contains("storage:enable") {
            self.storage = "indexeddb";
        } else if command.contains("storage:disable") {
            self.storage = "memory";
        } else if command.contains("storage:save") {
            self.pending_writes = self.pending_writes.saturating_add(1);
            self.record_count = self.record_count.saturating_add(1);
            self.session_snapshot = Some(self.snapshot_json());
            self.pending_writes = self.pending_writes.saturating_sub(1);
        } else if command.contains("storage:reload") {
            if self.session_snapshot.is_none() {
                self.storage_failures = self.storage_failures.saturating_add(1);
            }
        } else if command.contains("storage:reset") {
            self.record_count = 0;
            self.session_snapshot = None;
        } else if command.contains("storage:import") {
            self.record_count = self.record_count.saturating_add(1);
        } else if command.contains("\"tier\":\"mid\"") || command.contains("\"value\":\"mid\"") {
            self.set_tier(ResourceTier::Mid);
        } else if command.contains("\"tier\":\"high\"") || command.contains("\"value\":\"high\"") {
            self.set_tier(ResourceTier::High);
        } else if command.contains("\"tier\":\"low\"") || command.contains("\"value\":\"low\"") {
            self.set_tier(ResourceTier::Low);
        } else if command.contains("tier:benchmark") {
            self.benchmark_ms = 1.0 + self.workers as f32;
        } else if command.contains("mode:pov") {
            if self.pov_supported {
                self.view_mode = "pov";
            } else {
                self.view_mode = "map";
                self.warnings.push(String::from(
                    "POV renderer unavailable; staying in map mode",
                ));
            }
        } else if command.contains("mode:map") {
            self.view_mode = "map";
            self.pointer_lock = false;
        } else if command.contains("pov:pointer-lock") {
            self.pointer_lock = self.pov_supported;
        } else if command.contains("pov:walk") {
            self.pov_walk = !self.pov_walk;
        } else if command.contains("pov:toggle-baked") {
            self.pov_baked_light = !self.pov_baked_light;
        } else if command.contains("pov:toggle-detail") {
            self.pov_detail_normals = !self.pov_detail_normals;
        } else if command.contains("pov:toggle-water") {
            self.pov_water = !self.pov_water;
        } else if command.contains("pov:scale") {
            // The native WER_POV_SCALE ladder, as a select (the `tier`
            // pattern): full/half/quarter canvas resolution for the 3D pass.
            self.pov_render_scale = if command.contains("\"value\":\"quarter\"") {
                0.25
            } else if command.contains("\"value\":\"half\"") {
                0.5
            } else {
                1.0
            };
        }
    }

    /// WebGPU presentation is up: the atlas path composes the map, and the
    /// POV mode gate opens — the shared 3D renderer is GPU-only on every
    /// platform (no CPU POV twin exists; phase-7-plan.md §9.9).
    fn set_renderer_webgpu(&mut self) {
        self.renderer = "webgpu-atlas";
        self.pov_supported = true;
    }

    /// CPU fallback (chosen or device loss): the map keeps working, and POV
    /// — GPU-only — closes cleanly, returning to map mode rather than
    /// stranding the viewer in an unrenderable state (phase-7-plan.md §9.9:
    /// "device-loss and unsupported-feature paths return to map mode
    /// cleanly").
    fn set_renderer_cpu(&mut self) {
        self.renderer = "cpu-fallback";
        self.pov_supported = false;
        self.pointer_lock = false;
        if self.view_mode == "pov" {
            self.view_mode = "map";
            self.warnings
                .push(String::from("POV requires WebGPU; returned to map mode"));
        }
    }

    fn set_tier(&mut self, tier: ResourceTier) {
        self.runtime_tier = tier;
        self.tier = match tier {
            ResourceTier::Low => "WebLow",
            ResourceTier::Mid => "WebMid",
            ResourceTier::High => "WebHigh",
        };
        self.cache_ceiling_mb = match tier {
            ResourceTier::Low => 48,
            ResourceTier::Mid => 96,
            ResourceTier::High => 160,
        };
        self.refinement_enabled = tier.refinement();
        // Tiers change the streamed window (radii, caches, organism density)
        // but never world identity (ADR 0018), so rebuilding the map here
        // re-settles to the same authoritative state at the new extent.
        let cfg = tier.stream_config();
        self.half_regions = (cfg.load_radius / REGION_SIZE).ceil() as i32;
        self.map = RegionMap::new(cfg);
        self.settled = false;
    }

    /// Settle the streamed window around the current position, inline. Fresh
    /// regions snap to their target on load, so a fixed pass count fully
    /// settles the view — the native headless screenshot fixture pattern.
    fn ensure_settled(&mut self) {
        if self.settled {
            return;
        }
        let bias = [0.0f32; POSSIBILITY_DIMS];
        for _ in 0..SETTLE_PASSES {
            self.map.update(
                self.world_pos,
                0.0,
                &self.field,
                &[],
                &bias,
                &Budget::unlimited(),
                &InlineExecutor,
                false,
            );
        }
        self.settled = true;
    }

    fn region(&self) -> RegionCoord {
        RegionCoord::from_world(self.world_pos.0, self.world_pos.1)
    }

    fn settle_hash(&self) -> u64 {
        let region = self.region();
        let mut h = mix(0xB207_0000_0000_0003, origin_feature_hash());
        h = mix(h, region.x as u32 as u64);
        h = mix(h, region.y as u32 as u64);
        for dim in self.possibility.dims {
            h = mix(h, u64::from(dim.to_bits()));
        }
        h
    }

    fn snapshot_json(&self) -> String {
        let region = self.region();
        format!(
            concat!(
                "{{",
                "\"frame_index\":{},",
                "\"world_pos\":[{:.3},{:.3}],",
                "\"region\":[{},{}],",
                "\"possibility\":{},",
                "\"target\":{},",
                "\"active_channel\":{},",
                "\"channel\":\"{}\",",
                "\"cache\":{{\"regions\":1,\"bytes\":0}},",
                "\"executor\":{{\"mode\":\"{}\",\"parallelism\":{},\"workers\":{},\"backlog\":{},\"cancellations\":{},\"stale_results\":{}}},",
                "\"storage\":{{\"mode\":\"{}\",\"pending_writes\":{},\"failures\":{},\"records\":{}}},",
                "\"renderer\":{{\"mode\":\"{}\",\"compose\":{},\"refinement\":{},\"device_losses\":{}}},",
                "\"view\":{{\"mode\":\"{}\",\"pov_supported\":{},\"pointer_lock\":{},",
                "\"pov\":{{\"motion\":\"{}\",\"baked_light\":{},\"detail_normals\":{},\"water\":{},\"render_scale\":{:.2}}}}},",
                "\"tier\":{{\"name\":\"{}\",\"runtime\":\"{}\",\"cache_ceiling_mb\":{},\"benchmark_ms\":{:.3}}},",
                "\"settle_hash\":\"{:#018x}\",",
                "\"last_command\":\"{}\",",
                "\"warnings\":[{}]",
                "}}"
            ),
            self.frame_index,
            self.world_pos.0,
            self.world_pos.1,
            region.x,
            region.y,
            vector_json(self.possibility),
            vector_json(self.target),
            self.active_channel,
            MAP_CHANNELS[usize::from(self.active_channel)],
            self.worker_mode,
            self.executor_parallelism,
            self.workers,
            self.worker_backlog,
            self.cancellations,
            self.stale_results,
            self.storage,
            self.pending_writes,
            self.storage_failures,
            self.record_count,
            self.renderer,
            self.compose_enabled,
            self.refinement_enabled,
            self.device_losses,
            self.view_mode,
            self.pov_supported,
            self.pointer_lock,
            if self.pov_walk { "walk" } else { "fly" },
            self.pov_baked_light,
            self.pov_detail_normals,
            self.pov_water,
            self.pov_render_scale,
            self.tier,
            self.runtime_tier.name(),
            self.cache_ceiling_mb,
            self.benchmark_ms,
            self.settle_hash(),
            json_escape(&self.last_command),
            self.warnings
                .iter()
                .map(|warning| format!("\"{}\"", json_escape(warning)))
                .collect::<Vec<_>>()
                .join(",")
        )
    }

    /// Image edge length in pixels: one pixel per field cell across the
    /// `2·half+1` region window, exactly like the native composer.
    fn map_side(&self) -> usize {
        (2 * self.half_regions + 1) as usize * usize::from(self.map.config().field_resolution)
    }

    /// The CPU map header. The pixel payload travels separately through
    /// [`WebAppState::compose_map`] as raw bytes — a Phase 7-4 window is
    /// hundreds of kilobytes, far too large for a number-per-byte JSON array.
    fn cpu_map_json(&self) -> String {
        let side = self.map_side();
        format!(
            "{{\"kind\":\"rgba8\",\"renderer\":\"{}\",\"width\":{side},\"height\":{side},\"resolution\":{},\"channel\":\"{}\"}}",
            self.renderer,
            self.map.config().field_resolution,
            MAP_CHANNELS[usize::from(self.active_channel)]
        )
    }

    /// Compose the settled window into an RGBA8 image (row 0 = north), the
    /// browser twin of the native `MapComposer` base pass: the same
    /// `mapcolor` per-cell table over the same settled `RegionMap` channels,
    /// plus the grid darkening and the player cross. Native-only overlays
    /// (routes, preserves, organisms, pinned-flash) arrive with the vault
    /// and realization steps of Phase 7.
    fn compose_map(&mut self) -> Vec<u8> {
        self.ensure_settled();
        let side = self.map_side();
        let mut pixels = vec![0u8; side * side * 4];
        let center = RegionCoord::from_world(self.world_pos.0, self.world_pos.1);
        let channel = MAP_CHANNELS[usize::from(self.active_channel)];

        for row_region in 0..=(2 * self.half_regions) {
            // Row 0 is the northernmost (max y) region.
            let ry = center.y + self.half_regions - row_region;
            for col_region in 0..=(2 * self.half_regions) {
                let rx = center.x - self.half_regions + col_region;
                let coord = RegionCoord::new(rx, ry);
                self.paint_region(
                    &mut pixels,
                    coord,
                    channel,
                    row_region as usize,
                    col_region as usize,
                );
            }
        }

        // The player cross (the native marker), drawn last so it stays
        // visible over any channel. The window is centered on the player's
        // *region*, so the marker sits at the player's own pixel, exactly
        // like the native `draw_player_marker`.
        let res = f64::from(self.map.config().field_resolution);
        let cell = REGION_SIZE / res;
        let west = f64::from(center.x - self.half_regions) * REGION_SIZE;
        let north = f64::from(center.y + self.half_regions + 1) * REGION_SIZE;
        let player_px = ((self.world_pos.0 - west) / cell) as i64;
        let player_py = ((north - self.world_pos.1) / cell) as i64;
        for d in -3i64..=3 {
            for (px, py) in [(player_px + d, player_py), (player_px, player_py + d)] {
                if px >= 0 && py >= 0 && (px as usize) < side && (py as usize) < side {
                    let offset = (py as usize * side + px as usize) * 4;
                    pixels[offset..offset + 3].copy_from_slice(&[245, 245, 245]);
                }
            }
        }
        pixels
    }

    /// Paint one region's cells into the window image — the browser twin of
    /// the native `paint_region`, restricted to the channels the web toolbar
    /// exposes.
    fn paint_region(
        &self,
        pixels: &mut [u8],
        coord: RegionCoord,
        channel: &str,
        row_region: usize,
        col_region: usize,
    ) {
        let res = self.map.config().field_resolution;
        let side = self.map_side();
        let tiles = self.map.cache().get(coord);
        let tile = |channel_index: usize| tiles.and_then(|t| t.channels[channel_index].as_deref());
        let biome = tiles.and_then(|t| t.biome.as_deref());
        let (origin_x, origin_y) = coord.origin();
        let cell = REGION_SIZE / f64::from(res);

        for cy in 0..res {
            for cx in 0..res {
                let scalar = |t: Option<&world_core::FieldTile<f32>>,
                              paint: &dyn Fn(f32) -> [u8; 3]| {
                    t.map(|t| paint(t.get(cx, cy)))
                        .unwrap_or_else(|| mapcolor::missing_color(cx, cy))
                };
                let missing = || mapcolor::missing_color(cx, cy);
                let world = || {
                    (
                        origin_x + (f64::from(cx) + 0.5) * cell,
                        origin_y + (f64::from(cy) + 0.5) * cell,
                    )
                };
                let mut rgb = match channel {
                    "elevation" => scalar(tile(CHANNEL_ELEVATION), &mapcolor::elevation_color),
                    "geology" => match tile(CHANNEL_HARDNESS) {
                        Some(h) => {
                            let (wx, wy) = world();
                            mapcolor::geology_color(wx, wy, h.get(cx, cy))
                        }
                        None => missing(),
                    },
                    "temperature" => {
                        scalar(tile(CHANNEL_TEMPERATURE), &mapcolor::temperature_color)
                    }
                    "moisture" => scalar(tile(CHANNEL_MOISTURE), &mapcolor::moisture_color),
                    "river" => scalar(tile(CHANNEL_RIVER), &mapcolor::river_color),
                    "wetness" => scalar(tile(CHANNEL_WETNESS), &mapcolor::wetness_color),
                    "soil" => match (tile(CHANNEL_SOIL_DEPTH), tile(CHANNEL_FERTILITY)) {
                        (Some(d), Some(f)) => mapcolor::soil_color(d.get(cx, cy), f.get(cx, cy)),
                        _ => missing(),
                    },
                    "biome" => match biome {
                        Some(b) => mapcolor::biome_color(Biome::from_id(b.get(cx, cy))),
                        None => missing(),
                    },
                    "vegetation" => scalar(tile(CHANNEL_VEGETATION), &mapcolor::vegetation_color),
                    "herbivore" => scalar(tile(CHANNEL_HERBIVORE), &mapcolor::herbivore_color),
                    "predator" => scalar(tile(CHANNEL_PREDATOR), &mapcolor::predator_color),
                    "diversity" => scalar(tile(CHANNEL_DIVERSITY), &mapcolor::diversity_color),
                    "species" => match self.map.dominant_species_id(coord, cx, cy) {
                        Some(id) => mapcolor::species_color(id),
                        None => missing(),
                    },
                    // Composite (and any unknown name, defensively).
                    _ => match (
                        tile(CHANNEL_ELEVATION),
                        biome,
                        tile(CHANNEL_RIVER),
                        tile(CHANNEL_WETNESS),
                    ) {
                        (Some(e), Some(b), Some(r), Some(w)) => mapcolor::composite_cell_color(
                            e.get(cx, cy),
                            Biome::from_id(b.get(cx, cy)),
                            r.get(cx, cy),
                            w.get(cx, cy),
                            self.map.dominant_species_id(coord, cx, cy),
                        ),
                        _ => missing(),
                    },
                };
                // Region grid, on by default like the native shell.
                if cx == 0 || cy == 0 {
                    rgb = mapcolor::lerp_rgb(rgb, [0, 0, 0], 0.35);
                }

                // Cell (cx, cy) has cy growing north; image rows grow south.
                let px = col_region * usize::from(res) + usize::from(cx);
                let py = row_region * usize::from(res) + usize::from(res - 1 - cy);
                let offset = (py * side + px) * 4;
                pixels[offset] = rgb[0];
                pixels[offset + 1] = rgb[1];
                pixels[offset + 2] = rgb[2];
                pixels[offset + 3] = 255;
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn vector_json(vector: PossibilityVector) -> String {
    format!(
        "[{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6}]",
        vector.dims[0],
        vector.dims[1],
        vector.dims[2],
        vector.dims[3],
        vector.dims[4],
        vector.dims[5],
        vector.dims[6],
        vector.dims[7]
    )
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use wasm_bindgen::prelude::*;

    /// wasm entry point, invoked automatically when the module is instantiated.
    #[wasm_bindgen(start)]
    pub fn start() {
        console_error_panic_hook::set_once();
        let hash = super::origin_feature_hash();
        web_sys::console::log_1(
            &format!("[wer] wasm smoke ok — origin feature hash: {hash:#018x}").into(),
        );
    }

    /// Exposed to JS so the host page can confirm the core computed the expected
    /// deterministic value.
    #[wasm_bindgen]
    #[must_use]
    pub fn origin_feature_hash() -> u64 {
        super::origin_feature_hash()
    }

    /// Terrain-gradient identity sample (phase-1-plan.md section 11.2).
    #[wasm_bindgen]
    #[must_use]
    pub fn terrain_gradient_seed_sample() -> u64 {
        super::terrain_gradient_seed_sample()
    }

    /// Possibility-field control-point identity sample.
    #[wasm_bindgen]
    #[must_use]
    pub fn control_point_seed_sample() -> u64 {
        super::control_point_seed_sample()
    }

    /// Lithology identity sample (phase-2-plan.md §12.5).
    #[wasm_bindgen]
    #[must_use]
    pub fn lithology_seed_sample() -> u64 {
        super::lithology_seed_sample()
    }

    /// Drainage routing sample — all-integer topology, full equality required.
    #[wasm_bindgen]
    #[must_use]
    pub fn drainage_routing_sample() -> u64 {
        super::drainage_routing_sample()
    }

    /// Broad fixed-topology fold — complete macro content and signed samples.
    #[wasm_bindgen]
    #[must_use]
    pub fn drainage_topology_sample() -> u64 {
        super::drainage_topology_sample()
    }

    /// Procedural-genome identity sample (phase-3-plan.md §12.5).
    #[wasm_bindgen]
    #[must_use]
    pub fn genome_sample() -> u64 {
        super::genome_sample()
    }

    /// Food-web tier-biomass identity sample (phase-3-plan.md §12.5).
    #[wasm_bindgen]
    #[must_use]
    pub fn food_web_sample() -> u64 {
        super::food_web_sample()
    }

    /// Phase 4 steering-math identity sample (phase-4-plan.md §12.5).
    #[wasm_bindgen]
    #[must_use]
    pub fn steer_sample() -> u64 {
        super::steer_sample()
    }

    /// ADR 0025 canonical anchor-multiset signature sample.
    #[wasm_bindgen]
    #[must_use]
    pub fn canonical_anchor_signature_sample() -> u64 {
        super::canonical_anchor_signature_sample()
    }

    /// Phase 5 record-codec byte-identity sample (phase-5-plan.md §12.5).
    #[wasm_bindgen]
    #[must_use]
    pub fn record_codec_sample() -> u64 {
        super::record_codec_sample()
    }

    /// Phase 5 shared-anchor steering identity sample (phase-5-plan.md §12.5).
    #[wasm_bindgen]
    #[must_use]
    pub fn shared_steer_sample() -> u64 {
        super::shared_steer_sample()
    }

    /// ADR 0026 aggregate route-attraction parity sample.
    #[wasm_bindgen]
    #[must_use]
    pub fn route_attraction_sample() -> u64 {
        super::route_attraction_sample()
    }

    /// Phase 7 browser application facade. JS sends batched commands/input and
    /// reads compact JSON snapshots, keeping DOM/browser APIs out of neutral
    /// crates and avoiding per-field wasm calls in the frame loop.
    #[wasm_bindgen]
    #[derive(Debug)]
    pub struct WebApp {
        state: super::WebAppState,
        shutdown: bool,
    }

    #[wasm_bindgen]
    impl WebApp {
        /// Create a browser app with inline execution. Worker/storage/GPU modes
        /// are surfaced as explicit runtime status until later Phase 7 steps
        /// wire their browser adapters.
        #[wasm_bindgen(constructor)]
        pub fn new(config: JsValue) -> Result<WebApp, JsValue> {
            let config = config.as_string().unwrap_or_default();
            Ok(WebApp {
                state: super::WebAppState::new(&config),
                shutdown: false,
            })
        }

        /// Advance the browser facade by one batched input update.
        pub fn update(&mut self, dt_ms: f64, input: JsValue) -> Result<JsValue, JsValue> {
            if self.shutdown {
                return Err(JsValue::from_str("WebApp is shut down"));
            }
            self.state
                .update(dt_ms, &input.as_string().unwrap_or_default());
            Ok(JsValue::from_str(&self.state.snapshot_json()))
        }

        /// Return the CPU map header (size, channel, renderer) as JSON. The
        /// pixel payload comes from [`WebApp::map_pixels`] as raw bytes —
        /// the deterministic CPU-composed presentation of the settled window
        /// (phase-7-plan.md §4.1 milestone 2); WebGPU atlas composition
        /// remains a later presentation path.
        pub fn render_cpu_map(&mut self) -> Result<JsValue, JsValue> {
            if self.shutdown {
                return Err(JsValue::from_str("WebApp is shut down"));
            }
            Ok(JsValue::from_str(&self.state.cpu_map_json()))
        }

        /// Compose the settled window and return the RGBA8 bytes (row 0 =
        /// north), sized per the [`WebApp::render_cpu_map`] header. Settles
        /// the window inline on first use.
        pub fn map_pixels(&mut self) -> Result<Vec<u8>, JsValue> {
            if self.shutdown {
                return Err(JsValue::from_str("WebApp is shut down"));
            }
            Ok(self.state.compose_map())
        }

        /// Apply one normalized command from the shared browser command
        /// registry.
        pub fn apply_command(&mut self, command: JsValue) -> Result<JsValue, JsValue> {
            if self.shutdown {
                return Err(JsValue::from_str("WebApp is shut down"));
            }
            self.state
                .apply_command(&command.as_string().unwrap_or_default());
            Ok(JsValue::from_str(&self.state.snapshot_json()))
        }

        /// Return the most recent structured snapshot as JSON.
        pub fn info_snapshot(&self) -> Result<JsValue, JsValue> {
            Ok(JsValue::from_str(&self.state.snapshot_json()))
        }

        /// Stop accepting frame updates.
        pub fn shutdown(&mut self) {
            self.shutdown = true;
        }
    }
}

#[cfg(test)]
mod tests {
    //! Native side of the parity guarantee: the exact functions the wasm module
    //! exports are pinned here to the same golden constants asserted in
    //! `world-core`'s determinism suite. The wasm build compiles the identical
    //! pure code (CI's `wasm32` check), and the integer-only identity layer
    //! (ADR 0003) makes cross-platform agreement structural, not luck.

    #[test]
    fn parity_samples_match_goldens() {
        assert_eq!(
            super::origin_feature_hash(),
            super::EXPECTED_ORIGIN_FEATURE_HASH
        );
        assert_eq!(
            super::terrain_gradient_seed_sample(),
            super::EXPECTED_TERRAIN_GRADIENT_SEED
        );
        assert_eq!(
            super::control_point_seed_sample(),
            super::EXPECTED_CONTROL_POINT_SEED
        );
        assert_eq!(
            super::lithology_seed_sample(),
            super::EXPECTED_LITHOLOGY_SEED
        );
        assert_eq!(
            super::drainage_routing_sample(),
            super::EXPECTED_DRAINAGE_ROUTING
        );
        assert_eq!(
            super::drainage_topology_sample(),
            super::EXPECTED_DRAINAGE_TOPOLOGY
        );
        assert_eq!(super::genome_sample(), super::EXPECTED_GENOME);
        assert_eq!(super::food_web_sample(), super::EXPECTED_FOOD_WEB);
        assert_eq!(super::steer_sample(), super::EXPECTED_STEER);
        assert_eq!(
            super::canonical_anchor_signature_sample(),
            super::EXPECTED_CANONICAL_ANCHOR_SIGNATURE
        );
        assert_eq!(super::record_codec_sample(), super::EXPECTED_RECORD_CODEC);
        assert_eq!(super::shared_steer_sample(), super::EXPECTED_SHARED_STEER);
        assert_eq!(
            super::route_attraction_sample(),
            super::EXPECTED_ROUTE_ATTRACTION
        );
    }

    #[test]
    fn canonical_anchor_signature_is_permutation_invariant() {
        let mut anchors = super::canonical_anchor_sample_anchors();
        let forward = world_core::anchor_set_signature(&anchors);
        anchors.reverse();
        assert_eq!(forward, world_core::anchor_set_signature(&anchors));
        assert_eq!(forward, super::canonical_anchor_signature_sample());
    }

    #[test]
    fn web_app_snapshot_tracks_inline_state() {
        let mut app = super::WebAppState::new("{\"tier\":\"mid\"}");
        app.update(16.666_667, "{\"move_x\":1}");
        app.apply_command("{\"id\":\"toggle:refinement\"}");
        let snapshot = app.snapshot_json();
        assert!(snapshot.contains("\"tier\":{\"name\":\"WebMid\""));
        assert!(snapshot.contains("\"region\":[0,0]"));
        assert!(snapshot.contains("\"executor\":{\"mode\":\"inline\""));
        assert!(snapshot.contains("\"renderer\":{\"mode\":\"cpu-fallback\""));
        assert!(snapshot.contains("\"settle_hash\":\"0x"));
        assert!(snapshot.contains("\"last_command\":\"{\\\"id\\\":\\\"toggle:refinement\\\"}\""));
    }

    /// A small settle-able window so the map tests stay fast (the viz.rs
    /// fixture pattern).
    fn small_window_app() -> super::WebAppState {
        use world_core::REGION_SIZE;
        super::WebAppState {
            map: world_runtime::RegionMap::new(world_runtime::StreamConfig {
                near_radius: 1.5 * REGION_SIZE,
                far_radius: 3.0 * REGION_SIZE,
                load_radius: 3.0 * REGION_SIZE,
                unload_radius: 4.0 * REGION_SIZE,
                field_resolution: 8,
                ..world_runtime::StreamConfig::default()
            }),
            half_regions: 3,
            ..super::WebAppState::default()
        }
    }

    #[test]
    fn cpu_map_header_describes_the_window() {
        let app = super::WebAppState::default();
        let header = app.cpu_map_json();
        // Low tier: load radius 12 regions -> a 25-region window at the
        // default 32 cells/region.
        assert!(header.contains("\"kind\":\"rgba8\""));
        assert!(header.contains("\"width\":800"));
        assert!(header.contains("\"height\":800"));
        assert!(header.contains("\"channel\":\"composite\""));
        assert!(
            !header.contains("\"pixels\""),
            "pixels travel as raw bytes, not JSON"
        );
    }

    #[test]
    fn map_pixels_compose_the_settled_window() {
        let mut app = small_window_app();
        let side = app.map_side();
        assert_eq!(side, 7 * 8);
        let pixels = app.compose_map();
        assert_eq!(pixels.len(), side * side * 4);
        let first = &pixels[0..4];
        assert!(
            pixels.chunks_exact(4).any(|px| px != first),
            "a composed window must not be a solid color"
        );
        assert!(
            pixels.chunks_exact(4).all(|px| px[3] == 255),
            "the map is opaque"
        );

        // Orientation + palette pin: an interior cell of region (0, 0) must
        // match the shared mapcolor Composite table exactly, at the pixel
        // the north-up layout places it (the viz.rs paint contract).
        let coord = world_core::RegionCoord::new(0, 0);
        let tiles = app.map.cache().get(coord).expect("settled window");
        let (cx, cy) = (5u16, 5u16);
        let expected = world_runtime::mapcolor::composite_cell_color(
            tiles.channels[world_runtime::CHANNEL_ELEVATION]
                .as_ref()
                .expect("tile")
                .get(cx, cy),
            world_core::Biome::from_id(tiles.biome.as_ref().expect("tile").get(cx, cy)),
            tiles.channels[world_runtime::CHANNEL_RIVER]
                .as_ref()
                .expect("tile")
                .get(cx, cy),
            tiles.channels[world_runtime::CHANNEL_WETNESS]
                .as_ref()
                .expect("tile")
                .get(cx, cy),
            app.map.dominant_species_id(coord, cx, cy),
        );
        let px = 3 * 8 + usize::from(cx);
        let py = 3 * 8 + usize::from(8 - 1 - cy);
        let offset = (py * side + px) * 4;
        assert_eq!(&pixels[offset..offset + 3], &expected);
    }

    #[test]
    fn channel_commands_change_the_painted_channel() {
        let mut app = small_window_app();
        let composite = app.compose_map();
        app.apply_command("{\"id\":\"channel\",\"value\":\"elevation\"}");
        assert!(app.cpu_map_json().contains("\"channel\":\"elevation\""));
        let elevation = app.compose_map();
        assert_ne!(composite, elevation, "channels paint differently");
        app.apply_command("{\"id\":\"channel:composite\"}");
        assert!(app.cpu_map_json().contains("\"channel\":\"composite\""));
        assert_eq!(app.compose_map(), composite);
    }

    #[test]
    fn renderer_device_loss_falls_back_to_cpu() {
        let mut app = super::WebAppState::new("{\"webgpu\":true}");
        assert!(app.snapshot_json().contains("\"mode\":\"webgpu-atlas\""));
        app.apply_command("renderer:device-lost");
        let snapshot = app.snapshot_json();
        assert!(snapshot.contains("\"mode\":\"cpu-fallback\""));
        assert!(snapshot.contains("\"device_losses\":1"));
        assert!(snapshot.contains("WebGPU device lost"));
    }

    #[test]
    fn worker_modes_preserve_settle_hash() {
        let mut app = super::WebAppState::default();
        let inline = app.settle_hash();
        app.apply_command("worker:workers");
        assert_eq!(inline, app.settle_hash());
        app.apply_command("worker:shared");
        assert_eq!(inline, app.settle_hash());
        app.apply_command("worker:cancel-storm");
        let snapshot = app.snapshot_json();
        assert!(snapshot.contains("\"mode\":\"shared-memory\""));
        assert!(snapshot.contains("\"cancellations\":8"));
        assert!(snapshot.contains("\"stale_results\":3"));
    }

    #[test]
    fn tier_changes_preserve_settle_hash() {
        let mut app = super::WebAppState::default();
        let low = app.settle_hash();
        app.apply_command("{\"value\":\"mid\"}");
        assert_eq!(low, app.settle_hash());
        app.apply_command("{\"value\":\"high\"}");
        assert_eq!(low, app.settle_hash());
        let snapshot = app.snapshot_json();
        assert!(snapshot.contains("\"runtime\":\"high\""));
        assert!(snapshot.contains("\"cache_ceiling_mb\":160"));
    }

    #[test]
    fn storage_save_reload_preserves_settle_hash() {
        let mut app = super::WebAppState::default();
        app.update(16.0, "{\"move_x\":1}");
        let before = app.settle_hash();
        app.apply_command("storage:enable");
        app.apply_command("storage:save");
        app.apply_command("storage:reload");
        let snapshot = app.snapshot_json();
        assert_eq!(before, app.settle_hash());
        assert!(snapshot.contains("\"mode\":\"indexeddb\""));
        assert!(snapshot.contains("\"records\":1"));
        assert!(snapshot.contains("\"failures\":0"));
    }

    #[test]
    fn unavailable_pov_keeps_map_mode() {
        let mut app = super::WebAppState::default();
        let before = app.settle_hash();
        app.apply_command("mode:pov");
        let snapshot = app.snapshot_json();
        assert_eq!(before, app.settle_hash());
        assert!(snapshot.contains("\"view\":{\"mode\":\"map\""));
        assert!(snapshot.contains("POV renderer unavailable"));
    }

    #[test]
    fn webgpu_opens_the_pov_gate_and_device_loss_returns_to_map() {
        // phase-7-plan.md §9.9: the POV gate follows the GPU renderer (no
        // CPU POV twin exists on any platform), and device-loss paths
        // return to map mode cleanly instead of stranding the viewer.
        let mut app = super::WebAppState::new("{\"webgpu\":true}");
        let before = app.settle_hash();
        app.apply_command("mode:pov");
        app.apply_command("pov:pointer-lock");
        let snapshot = app.snapshot_json();
        assert!(snapshot.contains("\"view\":{\"mode\":\"pov\",\"pov_supported\":true"));
        assert!(snapshot.contains("\"pointer_lock\":true"));
        app.apply_command("renderer:device-lost");
        let snapshot = app.snapshot_json();
        assert!(snapshot.contains("\"view\":{\"mode\":\"map\",\"pov_supported\":false"));
        assert!(snapshot.contains("\"pointer_lock\":false"));
        assert!(snapshot.contains("POV requires WebGPU; returned to map mode"));
        // Recovery re-opens the gate without touching world identity.
        app.apply_command("renderer:webgpu");
        app.apply_command("mode:pov");
        assert!(app.snapshot_json().contains("\"view\":{\"mode\":\"pov\""));
        assert_eq!(before, app.settle_hash());
    }

    #[test]
    fn pov_config_commands_are_presentation_only() {
        // The native POV surface mirrored in the browser (walk, the B/N/V
        // diagnostic toggles, the render scale) is derived presentation:
        // every command leaves the settle hash untouched.
        let mut app = super::WebAppState::new("{\"webgpu\":true}");
        app.apply_command("mode:pov");
        let before = app.settle_hash();
        app.apply_command("pov:walk");
        app.apply_command("pov:toggle-baked");
        app.apply_command("pov:toggle-detail");
        app.apply_command("pov:toggle-water");
        app.apply_command("{\"id\":\"pov:scale\",\"value\":\"half\"}");
        let snapshot = app.snapshot_json();
        assert_eq!(before, app.settle_hash());
        assert!(snapshot.contains(
            "\"pov\":{\"motion\":\"walk\",\"baked_light\":false,\"detail_normals\":false,\
             \"water\":false,\"render_scale\":0.50}"
        ));
        app.apply_command("pov:walk");
        app.apply_command("{\"id\":\"pov:scale\",\"value\":\"full\"}");
        let snapshot = app.snapshot_json();
        assert!(snapshot.contains("\"motion\":\"fly\""));
        assert!(snapshot.contains("\"render_scale\":1.00"));
        assert_eq!(before, app.settle_hash());
    }
}
