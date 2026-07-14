//! `platform-web` — the browser/wasm application shell (section 3.2 & Phase 7).
//!
//! For the bootstrap this is a **minimal WebGPU/wasm smoke target**: it exists so
//! `world-core` is exercised through a real `wasm32` entry point from the start,
//! before native-only assumptions accumulate (section 19). The full runtime
//! (Web Workers, browser storage, WebGPU tiers, suspend/resume) arrives in
//! Phase 7. Phase 2 grew the shell only by two parity exports: the lithology
//! seed and a drainage routing sample (phase-2-plan.md §12.5).

#[cfg(target_arch = "wasm32")]
use viewer_host::input::{ButtonPhase, Modifiers, PhysicalKey, PointerButton, WheelDelta};
use viewer_host::PreparedMapSource;
use viewer_host::{
    action::{ServiceRequestId, ViewerEffect, WorkerBackend, ACTION_DESCRIPTORS},
    atlas::{AtlasManager, RefinementRequest},
    controller::{GroundSample, PovGroundSampler},
    input::{InputContext, InputFrame, InputMapper, NormalizedInputEvent},
    map::{Channel, MapBackend, MapComposer, MapDecor, MapRenderRequest},
    panel::{Severity, ViewerWarning},
    resolve_view_layout, ExplorationWorld, MapViewportProjection, NoopWorldTickHook, PixelRect,
    PlatformTelemetry, PresentationMode, ResolvedViewLayout, ServiceNotification, ServiceResponse,
    ServiceResponseResult, ServiceResponseSequence, TickInput, TickOutput, ViewKind, ViewerAction,
    ViewerController,
};
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
use world_runtime::stream::RegionMap;
use world_runtime::task::{InlineExecutor, TaskExecutor};
use world_runtime::tier::ResourceTier;
use world_runtime::{
    CHANNEL_DIVERSITY, CHANNEL_ELEVATION, CHANNEL_FERTILITY, CHANNEL_HARDNESS, CHANNEL_HERBIVORE,
    CHANNEL_MOISTURE, CHANNEL_PREDATOR, CHANNEL_RIVER, CHANNEL_SOIL_DEPTH, CHANNEL_TEMPERATURE,
    CHANNEL_VEGETATION, CHANNEL_WETNESS,
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

/// Browser-owned presentation/service state around the one shared controller.
/// World/viewer semantics deliberately do not live here.
#[derive(Debug)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
struct BrowserHostState {
    controller: ViewerController,
    composer: MapComposer,
    atlas: AtlasManager,
    overlay_hashes: [Option<u64>; 2],
    prepared_cpu_key: Option<u64>,
    last_stats: world_runtime::FrameStats,
    regen_totals: [u64; world_core::layer::LAYER_COUNT as usize],
    pov_chunks: pov_host::PovChunkManager,
    pov_organisms: pov_host::PovOrganismManager,
    pov_radius: i32,
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
    renderer: &'static str,
    renderer_ready: bool,
    force_cpu_map_redraw: bool,
    gpu_map_retry_scheduled: bool,
    device_losses: u32,
    warnings: Vec<String>,
    pending_effects: Vec<ViewerEffect>,
    service_sequence: u64,
    last_action: &'static str,
    last_output: Option<TickOutput>,
    surface: PixelRect,
}

/// Browser-owned raw-event adapter around the platform-neutral mapper.
///
/// DOM controls and physical input both enter the mapper's one ordered action
/// queue. The driver samples one typed [`InputFrame`] only after reducing all
/// queued actions, matching the ordering contract in the alignment plan.
#[derive(Debug, Default)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
struct BrowserViewerDriver {
    mapper: InputMapper,
    surface_focused: bool,
    dirty: bool,
}

/// Presentation work prepared after the controller's sole logical update.
#[derive(Debug)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
struct BrowserFrame {
    output: TickOutput,
    uploads: Vec<renderer::TerrainChunkUpload>,
    removes: Vec<u64>,
    organisms_changed: bool,
    service_response_queued: bool,
    presenter_needs_frame: bool,
    presenter_dirty: bool,
}

struct BrowserGround<'a> {
    chunks: &'a pov_host::PovChunkManager,
}

impl PovGroundSampler for BrowserGround<'_> {
    fn sample_ground(&self, map: &RegionMap, position: (f64, f64)) -> GroundSample {
        let (height, mesh_resident) = pov_host::walk_ground(self.chunks, map, position);
        GroundSample {
            height,
            mesh_resident,
        }
    }
}

/// Correlated services that this scaffold cannot truthfully complete yet.
/// The browser returns a typed failure on the following tick instead of
/// leaving controller request state pending forever.
fn unavailable_service_request(effect: &ViewerEffect) -> Option<(ServiceRequestId, &'static str)> {
    match effect {
        ViewerEffect::WriteDebugCapture(request) => {
            Some((request.request_id, "browser debug capture"))
        }
        ViewerEffect::PersistSession(request) => Some((request.request_id, "session persistence")),
        ViewerEffect::LoadSession(request) => Some((*request, "session loading")),
        ViewerEffect::WriteDiscovery(request) => {
            Some((request.request_id, "discovery persistence"))
        }
        ViewerEffect::LoadDiscoveries(request) => Some((*request, "discovery loading")),
        ViewerEffect::MutatePreserve(request) => Some((request.request_id, "preserve persistence")),
        ViewerEffect::WriteRoute(request) => Some((request.request_id, "route persistence")),
        ViewerEffect::ClearRoutes(request) => Some((*request, "route clearing")),
        ViewerEffect::ConfigurePathTracking { request_id, .. } => {
            Some((*request_id, "path-tracking persistence"))
        }
        ViewerEffect::OpenAtlasImport(request) => Some((*request, "atlas import")),
        ViewerEffect::DownloadAtlasBundle(request) => Some((*request, "atlas export")),
        ViewerEffect::Exit
        | ViewerEffect::ConfigureWorkerBackend(_)
        | ViewerEffect::CancelSupersededJobs
        | ViewerEffect::ConfigureStorage { .. }
        | ViewerEffect::ResetLocalVault
        | ViewerEffect::SelectMapBackend(_)
        | ViewerEffect::RunTierBenchmark
        | ViewerEffect::ConfigureResourceTier(_)
        | ViewerEffect::ReportWarning(_) => None,
    }
}

impl Default for BrowserHostState {
    fn default() -> Self {
        let tier = ResourceTier::Low;
        let cfg = tier.stream_config();
        let half_regions = (cfg.load_radius / REGION_SIZE).ceil() as i32;
        Self {
            // Conservative shared defaults keep POV unavailable until an
            // actual renderer/device success notification reaches a tick.
            controller: ViewerController::new(ExplorationWorld::new(tier)),
            composer: MapComposer::new(half_regions, cfg.field_resolution),
            atlas: AtlasManager::default(),
            overlay_hashes: [None; 2],
            prepared_cpu_key: None,
            last_stats: world_runtime::FrameStats::default(),
            regen_totals: [0; world_core::layer::LAYER_COUNT as usize],
            pov_chunks: pov_host::PovChunkManager::new(),
            pov_organisms: pov_host::PovOrganismManager::new(),
            pov_radius: 2,
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
            renderer: "cpu-fallback",
            renderer_ready: false,
            force_cpu_map_redraw: false,
            gpu_map_retry_scheduled: false,
            device_losses: 0,
            warnings: Vec::new(),
            last_action: "",
            pending_effects: Vec::new(),
            service_sequence: 0,
            last_output: None,
            surface: PixelRect::new(0, 0, 1, 1),
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
impl BrowserHostState {
    fn new(config: &str) -> Self {
        let tier = if config.contains("\"tier\":\"mid\"") {
            ResourceTier::Mid
        } else if config.contains("\"tier\":\"high\"") {
            ResourceTier::High
        } else {
            ResourceTier::Low
        };
        let cfg = tier.stream_config();
        let half_regions = (cfg.load_radius / REGION_SIZE).ceil() as i32;
        let mut state = Self {
            controller: ViewerController::new(ExplorationWorld::new(tier)),
            composer: MapComposer::new(half_regions, cfg.field_resolution),
            ..Self::default()
        };
        if config.contains("\"storage\":true") {
            state.storage = "indexeddb-pending";
            state
                .warnings
                .push(String::from("IndexedDB storage lands in Phase 7-7"));
        }
        if config.contains("\"worker_mode\":\"workers\"") {
            state.worker_mode = "workers";
            state.workers = 2;
        }
        state
    }

    fn resize_surface(&mut self, width: u32, height: u32) {
        let next = PixelRect::new(0, 0, width.max(1), height.max(1));
        if self.surface == next {
            return;
        }
        self.surface = next;
        // Grid coverage is destination-pixel dependent, so a resize changes
        // shader parameters even when the world and sparse planes did not.
        self.prepared_cpu_key = None;
        self.overlay_hashes = [None; 2];
        self.force_cpu_map_redraw = true;
    }

    fn resolved_layout(&self) -> ResolvedViewLayout {
        resolve_view_layout(self.surface, self.controller.layout())
    }

    fn map_projection(&self) -> Option<MapViewportProjection> {
        let destination = self.resolved_layout().map_content?;
        let world = self.controller.world();
        MapViewportProjection::new(
            destination,
            world.traveler().position,
            self.composer.half_regions(),
            world.map().config().field_resolution,
            self.controller.map_preferences().zoom,
        )
    }

    fn layout_json(&self) -> String {
        fn rect(value: Option<PixelRect>) -> String {
            value.map_or_else(
                || String::from("null"),
                |rect| format!("[{}, {}, {}, {}]", rect.x, rect.y, rect.width, rect.height),
            )
        }

        let layout = self.resolved_layout();
        format!(
            concat!(
                "{{\"content\":{},\"map_pane\":{},\"map_content\":{},",
                "\"pov_pane\":{},\"pov_aspect\":{}}}"
            ),
            rect(Some(layout.content)),
            rect(layout.map_pane),
            rect(layout.map_content),
            rect(layout.pov_pane),
            layout
                .pov_aspect
                .map_or_else(|| String::from("null"), |aspect| format!("{aspect:.9}")),
        )
    }

    fn frame(&mut self, dt_ms: f64, input: InputFrame) -> BrowserFrame {
        let telemetry = self.platform_telemetry();
        let ground = BrowserGround {
            chunks: &self.pov_chunks,
        };
        let mut hook = NoopWorldTickHook;
        let output = self.controller.tick(
            TickInput {
                dt_seconds: dt_ms / 1000.0,
                input,
                platform: telemetry,
            },
            &InlineExecutor,
            &mut hook,
            &ground,
        );
        let presenter_update = self
            .composer
            .update_for_tick(output.update_serial, self.controller.world().map());
        self.absorb_stats(output.stats);
        let mut unavailable = Vec::new();
        for effect in &output.effects {
            if let ViewerEffect::ReportWarning(warning) = effect {
                self.warn_once(&warning.message);
            }
            if let Some(request) = unavailable_service_request(effect) {
                unavailable.push(request);
            }
        }
        self.pending_effects.extend(output.effects.iter().cloned());
        for (request_id, service) in &unavailable {
            self.enqueue_unavailable_response(*request_id, service);
        }

        let mut uploads = Vec::new();
        let mut removes = Vec::new();
        let mut organisms_changed = false;
        if output.mode != PresentationMode::Map {
            let camera = self.controller.pov_camera();
            let position = (camera.pos.x, camera.pos.y);
            (uploads, removes) = self.pov_chunks.sync(
                self.controller.world().map(),
                position,
                self.pov_radius,
                &InlineExecutor,
            );
            organisms_changed = self.pov_organisms.sync(
                self.controller.world().map(),
                &self.pov_chunks,
                position,
                pov_host::pov_fog_end(self.pov_radius),
            );
        }
        self.last_output = Some(output.clone());
        BrowserFrame {
            output,
            uploads,
            removes,
            organisms_changed,
            service_response_queued: !unavailable.is_empty(),
            presenter_needs_frame: presenter_update.flashing_regions > 0,
            presenter_dirty: presenter_update.presentation_changed,
        }
    }

    fn platform_telemetry(&self) -> PlatformTelemetry {
        PlatformTelemetry {
            executor_backend: match self.worker_mode {
                "workers" => WorkerBackend::Workers,
                "shared-memory" => WorkerBackend::SharedWorkers,
                _ => WorkerBackend::Inline,
            },
            workers: self.workers as usize,
            storage_available: self.storage == "indexeddb",
            ..PlatformTelemetry::default()
        }
    }

    /// Fold one stream update's statistics into the panel's rolling state.
    fn absorb_stats(&mut self, stats: world_runtime::FrameStats) {
        for (total, count) in self
            .regen_totals
            .iter_mut()
            .zip(stats.regenerated_by_layer.iter())
        {
            *total += *count as u64;
        }
        self.last_stats = stats;
    }

    fn enqueue_action(&mut self, action: ViewerAction) {
        self.last_action = action.id().as_str();
        self.controller.enqueue_action(action);
    }

    fn warn_once(&mut self, warning: &str) {
        if !self.warnings.iter().any(|entry| entry == warning) {
            self.warnings.push(String::from(warning));
        }
    }

    fn set_renderer_webgpu(&mut self) {
        self.renderer_ready = true;
        self.atlas = AtlasManager::default();
        self.overlay_hashes = [None; 2];
        self.prepared_cpu_key = None;
        self.gpu_map_retry_scheduled = false;
        // Device readiness alone is not a claim that a GPU map was drawn.
        self.renderer = "webgpu-ready";
        self.enqueue_pov_availability(true, None);
    }

    fn record_map_backend(&mut self, backend: MapBackend) -> bool {
        let renderer = match backend {
            MapBackend::Cpu => "cpu-fallback",
            MapBackend::GpuAtlas => "webgpu-atlas",
        };
        let changed = self.renderer != renderer;
        self.renderer = renderer;
        changed
    }

    fn enqueue_pov_availability(&mut self, supported: bool, reason: Option<ViewerWarning>) {
        let sequence = self.next_service_sequence();
        self.controller
            .enqueue_service_notification(ServiceNotification::PovAvailability {
                sequence,
                supported,
                reason,
            });
    }

    fn enqueue_unavailable_response(&mut self, request_id: ServiceRequestId, service: &str) {
        let sequence = self.next_service_sequence();
        self.controller.enqueue_service_response(ServiceResponse {
            sequence,
            request_id,
            result: ServiceResponseResult::Failed(ViewerWarning {
                id: "browser-service-unavailable",
                message: format!("{service} is unavailable in this browser runtime."),
                severity: Severity::Warning,
            }),
        });
    }

    fn next_service_sequence(&mut self) -> ServiceResponseSequence {
        self.service_sequence = self
            .service_sequence
            .checked_add(1)
            .expect("browser service sequence exhausted");
        ServiceResponseSequence(self.service_sequence)
    }

    /// Device loss changes platform renderer state immediately, then queues
    /// the shared capability/fallback transition for the next logical tick.
    fn renderer_lost(&mut self) {
        self.renderer_ready = false;
        self.renderer = "cpu-fallback";
        self.atlas = AtlasManager::default();
        self.overlay_hashes = [None; 2];
        self.prepared_cpu_key = None;
        self.force_cpu_map_redraw = true;
        self.gpu_map_retry_scheduled = false;
        self.device_losses = self.device_losses.saturating_add(1);
        self.enqueue_pov_availability(
            false,
            Some(ViewerWarning {
                id: "webgpu-device-lost",
                message: String::from("WebGPU device lost; CPU map fallback active"),
                severity: Severity::Warning,
            }),
        );
    }

    fn region(&self) -> RegionCoord {
        let traveler = self.controller.world().traveler().position;
        RegionCoord::from_world(traveler.0, traveler.1)
    }

    fn settle_hash(&self) -> u64 {
        let region = self.region();
        let mut h = mix(0xB207_0000_0000_0003, origin_feature_hash());
        h = mix(h, region.x as u32 as u64);
        h = mix(h, region.y as u32 as u64);
        for dim in self.controller.world().field().sample(region).dims {
            h = mix(h, u64::from(dim.to_bits()));
        }
        h
    }

    fn snapshot_json(&self) -> String {
        let region = self.region();
        let map_preferences = self.controller.map_preferences();
        let layout = self.controller.layout();
        let pov = self.controller.pov_state();
        let world = self.controller.world();
        let traveler = world.traveler().position;
        let possibility = world.field().sample(region);
        let active_channel = Channel::ALL
            .iter()
            .position(|channel| *channel == map_preferences.channel)
            .unwrap_or(0);
        let focused = match layout.focused {
            ViewKind::Map => "map",
            ViewKind::Pov => "pov",
        };
        let tier = world.tier();
        let (tier_name, cache_ceiling_mb) = match tier {
            ResourceTier::Low => ("WebLow", 48),
            ResourceTier::Mid => ("WebMid", 96),
            ResourceTier::High => ("WebHigh", 160),
        };
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
                "\"zoom\":{},",
                "\"map\":{{\"backend\":\"{}\",\"decor_status\":\"browser-vault-unavailable\",",
                "\"overlays\":{{\"grid\":{},\"rings\":{},\"pinned_flash\":{},\"organisms\":{},\"discovered\":{}}}}},",
                "\"cache\":{{\"regions\":1,\"bytes\":0}},",
                "\"executor\":{{\"mode\":\"{}\",\"parallelism\":{},\"workers\":{},\"backlog\":{},\"cancellations\":{},\"stale_results\":{}}},",
                "\"storage\":{{\"mode\":\"{}\",\"pending_writes\":{},\"failures\":{},\"records\":{}}},",
                "\"renderer\":{{\"mode\":\"{}\",\"compose\":{},\"refinement\":{},\"device_losses\":{}}},",
                "\"view\":{{\"mode\":\"{}\",\"focused\":\"{}\",\"split_ratio\":{:.3},\"pov_supported\":{},",
                "\"pov\":{{\"motion\":\"{}\",\"shadow_ao\":{},\"detail_normals\":{},\"water\":{},\"render_scale\":{:.2}}}}},",
                "\"tier\":{{\"name\":\"{}\",\"runtime\":\"{}\",\"cache_ceiling_mb\":{},\"benchmark_ms\":{:.3}}},",
                "\"bias\":{},",
                "\"stats\":{},",
                "\"settle_hash\":\"{:#018x}\",",
                "\"last_action\":\"{}\",",
                "\"warnings\":[{}]",
                "}}"
            ),
            world.update_serial(),
            traveler.0,
            traveler.1,
            region.x,
            region.y,
            vector_json(possibility),
            vector_json(possibility),
            active_channel,
            map_preferences.channel.id(),
            map_preferences.zoom,
            match map_preferences.backend {
                MapBackend::Cpu => "cpu",
                MapBackend::GpuAtlas => "gpu-atlas",
            },
            map_preferences.overlays.grid,
            map_preferences.overlays.rings,
            map_preferences.overlays.pinned_flash,
            map_preferences.overlays.organisms,
            map_preferences.overlays.discovered,
            self.worker_mode,
            InlineExecutor.parallelism(),
            self.workers,
            self.worker_backlog,
            self.cancellations,
            self.stale_results,
            self.storage,
            self.pending_writes,
            self.storage_failures,
            self.record_count,
            self.renderer,
            self.renderer == "webgpu-atlas",
            map_preferences.refinement,
            self.device_losses,
            layout.mode.as_str(),
            focused,
            layout.split_ratio,
            pov.supported,
            if pov.walk { "walk" } else { "fly" },
            pov.shadow_ao,
            pov.detail_normals,
            pov.water,
            pov.render_scale,
            tier_name,
            tier.name(),
            cache_ceiling_mb,
            self.benchmark_ms,
            bias_json(world.bias()),
            self.stats_json(),
            self.settle_hash(),
            self.last_action,
            self.warnings
                .iter()
                .map(|warning| format!("\"{}\"", json_escape(warning)))
                .collect::<Vec<_>>()
                .join(",")
        )
    }

    /// The streaming/ecology statistics block for the browser info panel —
    /// the same values the native painted panel reads from `FrameStats`
    /// (panel.rs), plus the cumulative per-layer regen totals.
    fn stats_json(&self) -> String {
        let stats = &self.last_stats;
        let regen = world_core::layer::LAYERS
            .iter()
            .zip(self.regen_totals.iter())
            .map(|(layer, total)| format!("{{\"name\":\"{}\",\"total\":{total}}}", layer.name))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            concat!(
                "{{",
                "\"loaded\":{},\"evicted\":{},\"converged\":{},",
                "\"dispatched\":{},\"regenerated\":{},\"macro_jobs\":{},",
                "\"regen_cost\":{},\"deferred_regens\":{},\"active_regions\":{},",
                "\"cache_bytes\":{},\"macro_cache_bytes\":{},",
                "\"rosters_built\":{},\"roster_cache_bytes\":{},",
                "\"authoritative_realized\":{},\"organisms_realized\":{},\"organisms\":{},",
                "\"resonance\":{:.3},\"resonance_nodes\":{},\"anchors\":{},",
                "\"cancelled\":{},\"dropped\":{},\"failed\":{},",
                "\"pool_hits\":{},\"pool_misses\":{},\"pool_bytes\":{},",
                "\"regen_by_layer\":[{}]",
                "}}"
            ),
            stats.loaded,
            stats.evicted,
            stats.converged,
            stats.layers_dispatched,
            stats.layers_regenerated,
            stats.macro_jobs,
            stats.regen_cost_spent,
            stats.deferred_regens,
            stats.active_regions,
            stats.cache_bytes,
            stats.macro_cache_bytes,
            stats.rosters_built,
            stats.roster_cache_bytes,
            stats.authoritative_organisms_realized,
            stats.organisms_realized,
            stats.organisms,
            stats.resonance_strength,
            stats.resonance_nodes,
            stats.anchors_active,
            stats.jobs_cancelled,
            stats.results_dropped,
            stats.jobs_failed,
            stats.pool_hits,
            stats.pool_misses,
            stats.pool_bytes,
            regen,
        )
    }

    /// The native panel's CURSOR block, as JSON: the settled channel values
    /// of the cell under a world position. Reads the cache only — never
    /// generates — so hovering costs nothing.
    fn inspect_json(&self, wx: f64, wy: f64) -> String {
        let map = self.controller.world().map();
        let coord = RegionCoord::from_world(wx, wy);
        let res = map.config().field_resolution;
        let cell = REGION_SIZE / f64::from(res);
        let (ox, oy) = coord.origin();
        let cx = (((wx - ox) / cell) as i64).clamp(0, i64::from(res) - 1) as u16;
        let cy = (((wy - oy) / cell) as i64).clamp(0, i64::from(res) - 1) as u16;
        let state = map.get(coord);
        let tiles = map.cache().get(coord);
        let channel =
            |index: usize| tiles.and_then(|t| t.channels[index].as_deref().map(|t| t.get(cx, cy)));
        let number =
            |value: Option<f32>| value.map_or_else(|| String::from("null"), |v| format!("{v:.3}"));
        let biome = tiles.and_then(|t| t.biome.as_deref()).map_or_else(
            || String::from("null"),
            |b| format!("\"{:?}\"", Biome::from_id(b.get(cx, cy))),
        );
        let status = state.map_or("unloaded", |s| match s.status {
            world_runtime::GenerationStatus::Unloaded => "unloaded",
            world_runtime::GenerationStatus::Generating => "generating",
            world_runtime::GenerationStatus::Ready => "ready",
        });
        format!(
            concat!(
                "{{",
                "\"world\":[{:.0},{:.0}],\"region\":[{},{}],\"cell\":[{},{}],",
                "\"status\":\"{}\",\"stability\":{},\"revision\":{},",
                "\"elevation\":{},\"temperature\":{},\"moisture\":{},",
                "\"hardness\":{},\"river\":{},\"wetness\":{},",
                "\"soil_depth\":{},\"fertility\":{},\"vegetation\":{},",
                "\"herbivore\":{},\"predator\":{},\"diversity\":{},",
                "\"biome\":{}",
                "}}"
            ),
            wx,
            wy,
            coord.x,
            coord.y,
            cx,
            cy,
            status,
            state.map_or_else(|| String::from("null"), |s| format!("{:.2}", s.stability)),
            state.map_or_else(|| String::from("null"), |s| s.revision.to_string()),
            number(channel(CHANNEL_ELEVATION)),
            number(channel(CHANNEL_TEMPERATURE)),
            number(channel(CHANNEL_MOISTURE)),
            number(channel(CHANNEL_HARDNESS)),
            number(channel(CHANNEL_RIVER)),
            number(channel(CHANNEL_WETNESS)),
            number(channel(CHANNEL_SOIL_DEPTH)),
            number(channel(CHANNEL_FERTILITY)),
            number(channel(CHANNEL_VEGETATION)),
            number(channel(CHANNEL_HERBIVORE)),
            number(channel(CHANNEL_PREDATOR)),
            number(channel(CHANNEL_DIVERSITY)),
            biome,
        )
    }

    /// Image edge length in pixels: one pixel per field cell across the
    /// `2·half+1` region window, exactly like the native composer.
    fn map_side(&self) -> usize {
        self.composer.side() as usize
    }

    /// The CPU map header. The pixel payload travels separately through
    /// [`BrowserHostState::cpu_map_pixels`] as raw bytes — a Phase 7-4 window is
    /// hundreds of kilobytes, far too large for a number-per-byte JSON array.
    fn cpu_map_json(&self) -> String {
        let side = self.map_side();
        let map = self.controller.world().map();
        format!(
            "{{\"kind\":\"rgba8\",\"renderer\":\"{}\",\"width\":{side},\"height\":{side},\"resolution\":{},\"channel\":\"{}\"}}",
            self.renderer,
            map.config().field_resolution,
            self.controller.map_preferences().channel.id()
        )
    }

    /// Compose the canonical shared CPU map. Browser vault-derived decor is
    /// currently an explicit empty source; realized organisms, grid, rings,
    /// pinned flashes, and the player marker still use native's exact code.
    fn cpu_map_pixels(&mut self) -> &[u8] {
        let destination = self
            .map_projection()
            .expect("CPU Map presentation has a resolved destination")
            .destination;
        let preferences = self.controller.map_preferences();
        let world = self.controller.world();
        let dirty_key = world.update_serial();
        if self.prepared_cpu_key == Some(dirty_key) {
            return self.composer.pixels();
        }
        let traveler = world.traveler().position;
        self.composer.set_zoom(preferences.zoom);
        let decor = MapDecor::default();
        let packet = self.composer.prepare_render(
            &mut self.atlas,
            MapRenderRequest {
                map: world.map(),
                player: traveler,
                destination,
                channel: preferences.channel,
                overlays: preferences.overlays,
                anchors: world.anchors(),
                decor: &decor,
                requested_backend: MapBackend::Cpu,
                gpu_available: false,
                refinement: RefinementRequest::default(),
                dirty_key,
            },
        );
        self.prepared_cpu_key = Some(packet.dirty_key);
        match packet.source {
            PreparedMapSource::Cpu(cpu) => cpu.rgba,
            PreparedMapSource::GpuAtlas(_) => {
                unreachable!("an explicit CPU request must prepare canonical RGBA")
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
impl BrowserViewerDriver {
    fn context(&self, state: &BrowserHostState) -> InputContext {
        state.controller.input_context(self.surface_focused)
    }

    fn handle(&mut self, state: &mut BrowserHostState, event: NormalizedInputEvent) -> bool {
        let handled = self.mapper.handle_event(event, self.context(state));
        self.drain_actions(state);
        self.dirty |= handled;
        handled
    }

    fn enqueue_action(&mut self, state: &mut BrowserHostState, action: ViewerAction) {
        self.mapper.enqueue_action(action);
        self.drain_actions(state);
        self.dirty = true;
    }

    fn drain_actions(&mut self, state: &mut BrowserHostState) {
        for action in self.mapper.drain_actions() {
            state.enqueue_action(action);
        }
    }

    fn set_surface_focus(&mut self, state: &BrowserHostState, focused: bool) {
        self.surface_focused = focused;
        if !focused {
            self.mapper.clear_held_state();
        }
        self.mapper.set_context(self.context(state));
        self.dirty = true;
    }

    fn take_frame(&mut self, state: &mut BrowserHostState) -> InputFrame {
        self.drain_actions(state);
        // Re-sample the controller's preview after the final action drain so
        // already-held navigation follows a same-batch Tab/focus action.
        let context = self.context(state);
        self.mapper.set_context(context);
        let frame = self.mapper.take_frame();
        self.dirty = false;
        frame
    }

    fn synchronize_context(&mut self, state: &BrowserHostState) {
        self.mapper.set_context(self.context(state));
    }

    fn needs_frame(&self) -> bool {
        self.dirty || self.mapper.has_continuous_input()
    }

    fn has_continuous_input(&self) -> bool {
        self.mapper.has_continuous_input()
    }
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn action_descriptors_json() -> String {
    format!(
        "[{}]",
        ACTION_DESCRIPTORS
            .iter()
            .map(|descriptor| format!(
                "{{\"id\":\"{}\",\"label\":\"{}\",\"help\":\"{}\"}}",
                descriptor.id.as_str(),
                json_escape(descriptor.label),
                json_escape(descriptor.help),
            ))
            .collect::<Vec<_>>()
            .join(",")
    )
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn map_descriptors_json() -> String {
    let channels = viewer_host::CHANNEL_DESCRIPTORS
        .iter()
        .map(|descriptor| {
            format!(
                "{{\"id\":\"{}\",\"label\":\"{}\",\"group\":\"{}\",\"group_label\":\"{}\",\"order\":{}}}",
                descriptor.id,
                json_escape(descriptor.label),
                descriptor.group.id(),
                json_escape(descriptor.group.label()),
                descriptor.order,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let overlays = viewer_host::MAP_OVERLAY_DESCRIPTORS
        .iter()
        .map(|descriptor| {
            format!(
                "{{\"id\":\"{}\",\"label\":\"{}\",\"group\":\"{}\",\"group_label\":\"{}\",\"order\":{}}}",
                descriptor.id,
                json_escape(descriptor.label),
                descriptor.group.id(),
                json_escape(descriptor.group.label()),
                descriptor.order,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("{{\"channels\":[{channels}],\"overlays\":[{overlays}]}}")
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn effects_json(effects: Vec<ViewerEffect>) -> String {
    let effects = effects
        .into_iter()
        .map(|effect| match effect {
            ViewerEffect::Exit => String::from("{\"kind\":\"exit\"}"),
            ViewerEffect::WriteDebugCapture(request) => format!(
                "{{\"kind\":\"debug-capture\",\"request\":{}}}",
                request.request_id.0
            ),
            ViewerEffect::PersistSession(request) => {
                format!(
                    "{{\"kind\":\"persist-session\",\"request\":{}}}",
                    request.request_id.0
                )
            }
            ViewerEffect::LoadSession(request) => {
                format!("{{\"kind\":\"load-session\",\"request\":{}}}", request.0)
            }
            ViewerEffect::WriteDiscovery(request) => format!(
                "{{\"kind\":\"write-discovery\",\"request\":{}}}",
                request.request_id.0
            ),
            ViewerEffect::LoadDiscoveries(request) => format!(
                "{{\"kind\":\"load-discoveries\",\"request\":{}}}",
                request.0
            ),
            ViewerEffect::MutatePreserve(request) => format!(
                "{{\"kind\":\"mutate-preserve\",\"request\":{}}}",
                request.request_id.0
            ),
            ViewerEffect::WriteRoute(request) => format!(
                "{{\"kind\":\"write-route\",\"request\":{}}}",
                request.request_id.0
            ),
            ViewerEffect::ClearRoutes(request) => {
                format!("{{\"kind\":\"clear-routes\",\"request\":{}}}", request.0)
            }
            ViewerEffect::ConfigurePathTracking {
                request_id,
                enabled,
            } => format!(
                "{{\"kind\":\"configure-path-tracking\",\"request\":{},\"enabled\":{enabled}}}",
                request_id.0
            ),
            ViewerEffect::OpenAtlasImport(request) => {
                format!("{{\"kind\":\"open-atlas\",\"request\":{}}}", request.0)
            }
            ViewerEffect::DownloadAtlasBundle(request) => {
                format!("{{\"kind\":\"download-atlas\",\"request\":{}}}", request.0)
            }
            ViewerEffect::ConfigureWorkerBackend(_) => {
                String::from("{\"kind\":\"configure-workers\"}")
            }
            ViewerEffect::CancelSupersededJobs => String::from("{\"kind\":\"cancel-jobs\"}"),
            ViewerEffect::ConfigureStorage { enabled } => {
                format!("{{\"kind\":\"configure-storage\",\"enabled\":{enabled}}}")
            }
            ViewerEffect::ResetLocalVault => String::from("{\"kind\":\"reset-vault\"}"),
            ViewerEffect::SelectMapBackend(_) => String::from("{\"kind\":\"select-map-backend\"}"),
            ViewerEffect::RunTierBenchmark => String::from("{\"kind\":\"benchmark\"}"),
            ViewerEffect::ConfigureResourceTier(tier) => format!(
                "{{\"kind\":\"configure-tier\",\"tier\":\"{}\"}}",
                tier.name()
            ),
            ViewerEffect::ReportWarning(warning) => format!(
                "{{\"kind\":\"warning\",\"message\":\"{}\"}}",
                json_escape(&warning.message)
            ),
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{effects}]")
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn bias_json(bias: &[f32; POSSIBILITY_DIMS]) -> String {
    format!(
        "[{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3}]",
        bias[0], bias[1], bias[2], bias[3], bias[4], bias[5], bias[6], bias[7]
    )
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

/// Claim the single automatic follow-up frame allowed for one run of failed
/// GPU map submissions. A successful/CPU/device-reset path clears the flag.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn claim_gpu_map_retry(already_scheduled: &mut bool) -> bool {
    let should_schedule = !*already_scheduled;
    *already_scheduled = true;
    should_schedule
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::cell::RefCell;

    use wasm_bindgen::prelude::*;

    /// The native shell's clear/fog color, mirrored (main.rs `CLEAR_COLOR`).
    const CLEAR_COLOR: [f64; 4] = [0.02, 0.02, 0.04, 1.0];

    thread_local! {
        /// The shared wgpu renderer over the GPU stage canvas. A thread-local slot
        /// (wasm is single-threaded) rather than a `WebApp` field because
        /// device acquisition is async on WebGPU — JS awaits
        /// [`viewer_renderer_init`]
        /// once, then per-frame calls stay synchronous.
        static VIEWER_RENDERER: RefCell<Option<renderer::Renderer>> = const { RefCell::new(None) };
    }

    fn parse_view(value: &str) -> Result<super::ViewKind, JsValue> {
        match value {
            "map" => Ok(super::ViewKind::Map),
            "pov" => Ok(super::ViewKind::Pov),
            _ => Err(JsValue::from_str("view must be exactly `map` or `pov`")),
        }
    }

    /// Bring up the shared 3D renderer over the given canvas
    /// (phase-7-plan.md §9.9). Idempotent; safe to call again after a
    /// device loss (the old renderer is dropped and rebuilt). Rejects when
    /// no adapter/device is available — the caller falls back to map mode.
    #[wasm_bindgen]
    pub async fn viewer_renderer_init(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
    ) -> Result<(), JsValue> {
        if VIEWER_RENDERER.with(|slot| slot.borrow().is_some()) {
            return Ok(());
        }
        let built = renderer::Renderer::new(renderer::canvas_surface_source(canvas), width, height)
            .await
            .map_err(|err| JsValue::from_str(&format!("pov renderer init failed: {err}")))?;
        VIEWER_RENDERER.with(|slot| *slot.borrow_mut() = Some(built));
        Ok(())
    }

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

    /// Phase 7 browser application facade. JS forwards primitive raw events
    /// and exact action ids; the shared mapper produces typed frame intent and
    /// ordered actions before this facade updates presentation state. Compact
    /// snapshots keep DOM/browser APIs out of neutral crates.
    #[wasm_bindgen]
    #[derive(Debug)]
    pub struct WebApp {
        state: super::BrowserHostState,
        driver: super::BrowserViewerDriver,
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
                state: super::BrowserHostState::new(&config),
                driver: super::BrowserViewerDriver::default(),
                shutdown: false,
            })
        }

        /// Advance exactly one logical viewer frame in every presentation.
        /// Input is sampled once and the shared controller performs the sole
        /// traveler/world update before any optional POV presentation work.
        pub fn frame(&mut self, dt_ms: f64, time_seconds: f64) -> Result<JsValue, JsValue> {
            if self.shutdown {
                return Err(JsValue::from_str("WebApp is shut down"));
            }
            let input = self.driver.take_frame(&mut self.state);
            let frame = self.state.frame(dt_ms, input);
            if frame.service_response_queued {
                self.driver.dirty = true;
            }
            self.driver.synchronize_context(&self.state);

            let pov_active = frame.output.mode == super::PresentationMode::Pov;
            let map_active = frame.output.mode == super::PresentationMode::Map;
            let map_projection = map_active.then(|| self.state.map_projection()).flatten();
            let (prepared_map_backend, gpu_map_rendered, map_upload_bytes, schedule_map_retry) =
                if map_active {
                    VIEWER_RENDERER.with(|slot| {
                        let mut slot = slot.borrow_mut();
                        let gpu_available = self.state.renderer_ready && slot.is_some();
                        let super::BrowserHostState {
                            controller,
                            composer,
                            atlas,
                            overlay_hashes,
                            prepared_cpu_key,
                            gpu_map_retry_scheduled,
                            ..
                        } = &mut self.state;
                        let world = controller.world();
                        let traveler = world.traveler().position;
                        let preferences = controller.map_preferences();
                        composer.set_zoom(preferences.zoom);
                        let decor = super::MapDecor::default();
                        let packet = composer.prepare_render(
                            atlas,
                            super::MapRenderRequest {
                                map: world.map(),
                                player: traveler,
                                destination: map_projection
                                    .expect("active Map mode has a resolved destination")
                                    .destination,
                                channel: preferences.channel,
                                overlays: preferences.overlays,
                                anchors: world.anchors(),
                                decor: &decor,
                                requested_backend: preferences.backend,
                                gpu_available,
                                refinement: super::RefinementRequest {
                                    enabled: preferences.refinement,
                                    octave_count: 3,
                                },
                                dirty_key: frame.output.update_serial,
                            },
                        );
                        let backend = packet.backend;
                        let dirty_key = packet.dirty_key;
                        let destination = packet.viewport.destination;
                        match packet.source {
                            super::PreparedMapSource::Cpu(cpu) => {
                                debug_assert_eq!(
                                    cpu.rgba.len(),
                                    packet.projection.side as usize
                                        * packet.projection.side as usize
                                        * 4
                                );
                                *prepared_cpu_key = Some(dirty_key);
                                *gpu_map_retry_scheduled = false;
                                (backend, false, 0, false)
                            }
                            super::PreparedMapSource::GpuAtlas(gpu) => {
                                *prepared_cpu_key = None;
                                let pre_grid_changed = overlay_hashes[0] != Some(gpu.pre_grid_hash);
                                let post_grid_changed =
                                    overlay_hashes[1] != Some(gpu.post_grid_hash);
                                let renderer = slot.as_mut().expect(
                                    "a prepared GPU packet requires an initialized renderer",
                                );
                                let result = renderer.render_map_gpu_in(
                                    &gpu.params,
                                    &gpu.slots,
                                    &gpu.uploads,
                                    pre_grid_changed.then_some(gpu.pre_grid_rgba),
                                    post_grid_changed.then_some(gpu.post_grid_rgba),
                                    None,
                                    renderer::SurfaceViewport::new(
                                        destination.x,
                                        destination.y,
                                        destination.width,
                                        destination.height,
                                    ),
                                    None,
                                    CLEAR_COLOR,
                                );
                                if let Some(bytes) = result {
                                    *overlay_hashes =
                                        [Some(gpu.pre_grid_hash), Some(gpu.post_grid_hash)];
                                    *gpu_map_retry_scheduled = false;
                                    (backend, true, bytes, false)
                                } else {
                                    // The manager may have consumed delta keys
                                    // before a failed surface submission. Reset it
                                    // so a recovered renderer receives full tiles.
                                    *atlas = super::AtlasManager::default();
                                    *overlay_hashes = [None; 2];
                                    let schedule_retry =
                                        super::claim_gpu_map_retry(gpu_map_retry_scheduled);
                                    (backend, false, 0, schedule_retry)
                                }
                            }
                        }
                    })
                } else {
                    (super::MapBackend::Cpu, false, 0, false)
                };
            if schedule_map_retry {
                // Surface Outdated/Lost is repaired by the failed acquire;
                // request one bounded follow-up frame to present it without
                // turning persistent Timeout/Occluded into a busy loop.
                self.driver.dirty = true;
            }
            let (map_path, map_backend_changed) = if gpu_map_rendered {
                (
                    "gpu-atlas",
                    self.state.record_map_backend(super::MapBackend::GpuAtlas),
                )
            } else {
                (
                    "cpu",
                    map_active && self.state.record_map_backend(super::MapBackend::Cpu),
                )
            };
            let force_cpu_map_redraw = std::mem::take(&mut self.state.force_cpu_map_redraw);
            let map_dirty = frame.output.dirty.map
                || frame.presenter_dirty
                || map_backend_changed
                || force_cpu_map_redraw
                || (map_active
                    && prepared_map_backend == super::MapBackend::GpuAtlas
                    && !gpu_map_rendered);
            let organism_upload = frame
                .organisms_changed
                .then(|| self.state.pov_organisms.upload());
            let time = time_seconds.rem_euclid(f64::from(renderer::pov::WOBBLE_PERIOD)) as f32;
            let pov_rendered = pov_active
                && VIEWER_RENDERER.with(|slot| {
                    let mut slot = slot.borrow_mut();
                    let Some(renderer) = slot.as_mut() else {
                        return false;
                    };
                    let (width, height) = renderer.size();
                    let camera = self.state.controller.pov_camera();
                    let shadow = pov_host::shadow_frame(
                        camera,
                        &self.state.pov_chunks,
                        self.state.pov_organisms.shadow_bounds(),
                        pov_host::shadow_resolution(self.state.controller.world().tier()),
                    );
                    let params = pov_host::frame_params(
                        camera,
                        width as f32 / height.max(1) as f32,
                        self.state.pov_radius,
                        CLEAR_COLOR,
                        time,
                        self.state.controller.pov_toggles(),
                        shadow,
                    );
                    renderer.render_pov(
                        &params,
                        &frame.uploads,
                        &frame.removes,
                        organism_upload,
                        CLEAR_COLOR,
                        None,
                        frame.output.pov.render_scale,
                    )
                });
            let counters = self.state.pov_chunks.counters();
            let organism_counts = self.state.pov_organisms.counters();
            let snapshot = self.state.snapshot_json();
            let layout = self.state.layout_json();
            let needs_frame = frame.output.needs_frame
                || frame.presenter_needs_frame
                || self.driver.needs_frame();
            Ok(JsValue::from_str(&format!(
                concat!(
                    "{{\"snapshot\":{},\"layout\":{},\"map_dirty\":{},\"needs_frame\":{},",
                    "\"update_serial\":{},\"travel\":{:.6},",
                    "\"map\":{{\"active\":{},\"path\":\"{}\",\"gpu_submitted\":{},\"upload_bytes\":{}}},",
                    "\"pov\":{{\"active\":{},\"rendered\":{},",
                    "\"camera\":[{:.1},{:.1},{:.1}],",
                    "\"chunks\":{},\"meshed\":{},\"uploads\":{},",
                    "\"organisms\":{{\"published\":{},\"drawn\":{},",
                    "\"waiting_for_ground\":{}}}}}}}"
                ),
                snapshot,
                layout,
                map_dirty,
                needs_frame,
                frame.output.update_serial,
                frame.output.travel,
                map_active,
                map_path,
                gpu_map_rendered,
                map_upload_bytes,
                pov_active,
                pov_rendered,
                frame.output.pov.position[0],
                frame.output.pov.position[1],
                frame.output.pov.position[2],
                self.state.pov_chunks.len(),
                counters.meshed,
                frame.uploads.len(),
                organism_counts.published,
                organism_counts.drawn(),
                organism_counts.waiting_for_ground,
            )))
        }

        /// Resize both shared layout and the live wgpu surface to an exact
        /// physical canvas backing size. CSS/DPR conversion remains a thin JS
        /// adapter concern; all fitted rectangles are resolved here.
        pub fn resize_surface(&mut self, width: u32, height: u32) -> Result<JsValue, JsValue> {
            if self.shutdown {
                return Err(JsValue::from_str("WebApp is shut down"));
            }
            self.state.resize_surface(width, height);
            VIEWER_RENDERER.with(|slot| {
                if let Some(renderer) = slot.borrow_mut().as_mut() {
                    renderer.resize(width.max(1), height.max(1));
                }
            });
            self.driver.dirty = true;
            Ok(JsValue::from_str(&self.state.layout_json()))
        }

        /// Queue one exact descriptor id and optional payload. DOM controls
        /// use this same ordered mapper queue as keyboard and pointer input.
        pub fn action(&mut self, id: String, value: Option<String>) -> Result<(), JsValue> {
            if self.shutdown {
                return Err(JsValue::from_str("WebApp is shut down"));
            }
            let action = super::ViewerAction::decode_exact(&id, value.as_deref())
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            self.driver.enqueue_action(&mut self.state, action);
            Ok(())
        }

        /// Descriptor metadata used to validate toolbar markup at startup.
        pub fn action_descriptors(&self) -> String {
            super::action_descriptors_json()
        }

        /// Shared channel/overlay metadata used to build grouped map controls.
        pub fn map_descriptors(&self) -> String {
            super::map_descriptors_json()
        }

        /// Translate an exact `KeyboardEvent.code` transition.
        #[allow(clippy::too_many_arguments)]
        pub fn key_event(
            &mut self,
            code: String,
            pressed: bool,
            repeat: bool,
            shift: bool,
            control: bool,
            alt: bool,
            super_key: bool,
        ) -> bool {
            let Some(key) = super::PhysicalKey::from_dom_code(&code) else {
                return false;
            };
            let event = super::NormalizedInputEvent::Key {
                key,
                phase: if pressed {
                    super::ButtonPhase::Pressed
                } else {
                    super::ButtonPhase::Released
                },
                repeat,
                modifiers: super::Modifiers {
                    shift,
                    control,
                    alt,
                    super_key,
                },
            };
            self.driver.handle(&mut self.state, event)
        }

        /// Translate one pointer position in physical canvas pixels.
        pub fn pointer_move(
            &mut self,
            pointer: u32,
            x: f64,
            y: f64,
            view: String,
        ) -> Result<bool, JsValue> {
            let view = parse_view(&view)?;
            self.driver.handle(
                &mut self.state,
                super::NormalizedInputEvent::PointerMoved {
                    pointer: u64::from(pointer),
                    position: [x, y],
                    view,
                },
            );
            Ok(self.driver.has_continuous_input())
        }

        /// Translate one pointer-button transition.
        pub fn pointer_button(
            &mut self,
            pointer: u32,
            button: u16,
            pressed: bool,
            x: f64,
            y: f64,
            view: String,
        ) -> Result<bool, JsValue> {
            let view = parse_view(&view)?;
            let button = match button {
                0 => super::PointerButton::Primary,
                1 => super::PointerButton::Auxiliary,
                2 => super::PointerButton::Secondary,
                3 => super::PointerButton::Other(3),
                4 => super::PointerButton::Other(4),
                other => super::PointerButton::Other(other),
            };
            Ok(self.driver.handle(
                &mut self.state,
                super::NormalizedInputEvent::PointerButton {
                    pointer: u64::from(pointer),
                    button,
                    phase: if pressed {
                        super::ButtonPhase::Pressed
                    } else {
                        super::ButtonPhase::Released
                    },
                    position: [x, y],
                    view,
                },
            ))
        }

        /// End a pointer gesture after DOM `pointercancel` or lost capture.
        pub fn pointer_cancel(&mut self, pointer: u32) -> bool {
            self.driver.handle(
                &mut self.state,
                super::NormalizedInputEvent::PointerCancelled {
                    pointer: u64::from(pointer),
                },
            )
        }

        /// Translate a unit-preserving vertical wheel delta.
        pub fn wheel(&mut self, delta: f64, lines: bool, view: String) -> Result<bool, JsValue> {
            let view = parse_view(&view)?;
            Ok(self.driver.handle(
                &mut self.state,
                super::NormalizedInputEvent::Wheel {
                    delta: if lines {
                        super::WheelDelta::Lines(delta)
                    } else {
                        super::WheelDelta::Pixels(delta)
                    },
                    view,
                },
            ))
        }

        /// Track whether a canvas, rather than a form control, owns focus.
        pub fn surface_focus(&mut self, focused: bool) {
            self.driver.set_surface_focus(&self.state, focused);
        }

        /// Clear every held gesture on browser/window focus loss.
        pub fn host_focus(&mut self, focused: bool) {
            self.driver.handle(
                &mut self.state,
                super::NormalizedInputEvent::FocusChanged { focused },
            );
        }

        /// Whether held input or queued work needs another animation frame.
        pub fn needs_frame(&self) -> bool {
            self.driver.needs_frame()
                || self
                    .state
                    .last_output
                    .as_ref()
                    .is_some_and(|output| output.needs_frame)
        }

        /// Drain typed platform effects after the action frame is reduced.
        pub fn take_effects(&mut self) -> String {
            super::effects_json(std::mem::take(&mut self.state.pending_effects))
        }

        /// Accept the completed synchronous browser benchmark measurement.
        pub fn benchmark_result(&mut self, milliseconds: f64) {
            self.state.benchmark_ms = milliseconds.max(0.0) as f32;
        }

        /// Report successful completion of the actual asynchronous renderer
        /// initialization. A navigator capability probe must not call this.
        pub fn renderer_available(&mut self) {
            self.state.set_renderer_webgpu();
            self.driver.enqueue_action(
                &mut self.state,
                super::ViewerAction::SetMapBackend(super::MapBackend::GpuAtlas),
            );
        }

        /// Report device loss/initialization failure without forging an action.
        pub fn renderer_lost(&mut self) {
            VIEWER_RENDERER.with(|slot| {
                slot.borrow_mut().take();
            });
            self.state.renderer_lost();
            self.driver.mapper.clear_held_state();
            self.driver
                .mapper
                .set_context(self.driver.context(&self.state));
            self.driver.dirty = true;
        }

        /// Return the CPU map header (size, channel, renderer) as JSON. The
        /// pixel payload comes from [`WebApp::map_pixels`] as raw bytes —
        /// the deterministic CPU-composed presentation of the settled window
        /// (phase-7-plan.md §4.1 milestone 2). The browser selects this
        /// canonical fallback whenever the shared WebGPU atlas path cannot
        /// submit a frame.
        pub fn render_cpu_map(&self) -> Result<JsValue, JsValue> {
            if self.shutdown {
                return Err(JsValue::from_str("WebApp is shut down"));
            }
            Ok(JsValue::from_str(&self.state.cpu_map_json()))
        }

        /// Compose current cache state into RGBA8 bytes (row 0 = north).
        /// This is presentation-only and performs no world update.
        pub fn map_pixels(&mut self) -> Result<Vec<u8>, JsValue> {
            if self.shutdown {
                return Err(JsValue::from_str("WebApp is shut down"));
            }
            Ok(self.state.cpu_map_pixels().to_vec())
        }

        /// Invert a physical surface point through the exact shared fitted Map
        /// rectangle. Letterbox/outside points return `null`.
        pub fn map_world_at(&self, physical_x: f64, physical_y: f64) -> Result<JsValue, JsValue> {
            let value = self
                .state
                .map_projection()
                .and_then(|projection| projection.physical_to_world((physical_x, physical_y)))
                .map_or_else(|| String::from("null"), |(x, y)| format!("[{x:.9},{y:.9}]"));
            Ok(JsValue::from_str(&value))
        }

        /// Pick the shared realized-organism marker under a physical surface
        /// point. Identity is returned as hex so JavaScript never rounds the
        /// stable `u64`.
        pub fn map_organism_at(
            &self,
            physical_x: f64,
            physical_y: f64,
        ) -> Result<JsValue, JsValue> {
            let world = self.state.controller.world();
            let zoom = self.state.controller.map_preferences().zoom;
            let value = self
                .state
                .map_projection()
                .and_then(|projection| projection.physical_to_world((physical_x, physical_y)))
                .and_then(|position| viewer_host::pick_organism(world.map(), position, zoom))
                .map_or_else(
                    || String::from("null"),
                    |organism| {
                        format!(
                            "{{\"id\":\"{:#018x}\",\"world\":[{:.9},{:.9}]}}",
                            organism.id, organism.world_pos.0, organism.world_pos.1
                        )
                    },
                );
            Ok(JsValue::from_str(&value))
        }

        /// Return the most recent structured snapshot as JSON.
        pub fn info_snapshot(&self) -> Result<JsValue, JsValue> {
            Ok(JsValue::from_str(&self.state.snapshot_json()))
        }

        /// The cursor block (the native panel's CURSOR readout): settled
        /// channel values of the cell under a world position, as JSON.
        /// Cache-read only.
        pub fn inspect(&self, world_x: f64, world_y: f64) -> Result<JsValue, JsValue> {
            Ok(JsValue::from_str(
                &self.state.inspect_json(world_x, world_y),
            ))
        }

        /// Stop accepting frame updates.
        pub fn shutdown(&mut self) {
            self.shutdown = true;
        }
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_input_tests {
    use wasm_bindgen::JsValue;
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::wasm::WebApp;

    fn snapshot(app: &WebApp) -> String {
        app.info_snapshot()
            .expect("snapshot")
            .as_string()
            .expect("string snapshot")
    }

    #[wasm_bindgen_test]
    fn exact_actions_and_dom_keys_share_the_wasm_boundary() {
        let mut button = WebApp::new(JsValue::from_str("{}")).expect("web app");
        button.surface_focus(true);
        button
            .action(String::from("toggle-refinement"), None)
            .expect("exact pulse action");
        let first_frame = button
            .frame(0.0, 0.0)
            .expect("button frame")
            .as_string()
            .expect("frame JSON");
        assert!(first_frame.contains("\"update_serial\":1"));
        assert!(snapshot(&button).contains("\"refinement\":true"));
        assert!(button
            .action(String::from("Toggle-Refinement"), None)
            .is_err());
        assert!(button
            .action(
                String::from("toggle-refinement"),
                Some(String::from("ignored"))
            )
            .is_err());
        let second_frame = button
            .frame(0.0, 0.0)
            .expect("second button frame")
            .as_string()
            .expect("frame JSON");
        assert!(second_frame.contains("\"update_serial\":2"));

        let mut keyboard = WebApp::new(JsValue::from_str("{}")).expect("web app");
        keyboard.surface_focus(true);
        assert!(keyboard.key_event(
            String::from("Period"),
            true,
            false,
            false,
            false,
            false,
            false,
        ));
        assert!(keyboard.key_event(
            String::from("Period"),
            true,
            true,
            false,
            false,
            false,
            false,
        ));
        keyboard.frame(0.0, 0.0).expect("key frame");
        assert!(snapshot(&keyboard).contains("\"refinement\":true"));
        assert!(!keyboard.key_event(
            String::from("period"),
            true,
            false,
            false,
            false,
            false,
            false,
        ));

        keyboard.surface_focus(false);
        assert!(!keyboard.key_event(String::from("Tab"), true, false, false, false, false, false,));
        keyboard.frame(0.0, 0.0).expect("toolbar-focus frame");
        assert!(snapshot(&keyboard).contains("\"view\":{\"mode\":\"map\""));
    }

    #[wasm_bindgen_test]
    fn wasm_adapter_controller_trace_matches_the_native_golden() {
        let mut app = WebApp::new(JsValue::from_str("{}")).expect("web app");
        app.renderer_available();
        app.surface_focus(true);
        for code in ["KeyW", "Tab", "KeyB"] {
            assert!(app.key_event(String::from(code), true, false, false, false, false, false,));
        }
        let frame = app
            .frame(100.0, 0.0)
            .expect("preview frame")
            .as_string()
            .expect("frame JSON");
        assert!(frame.contains("\"update_serial\":1"));
        assert!(frame.contains("\"travel\":4.000000"));
        let snapshot = snapshot(&app);
        assert!(snapshot.contains("\"view\":{\"mode\":\"pov\""));
        assert!(snapshot.contains("\"shadow_ao\":false"));
        assert!(snapshot.contains("\"world_pos\":[-0.000,4.000]"));
    }

    #[wasm_bindgen_test]
    fn wasm_map_pixels_use_the_shared_canonical_composer() {
        let mut app = WebApp::new(JsValue::from_str("{}")).expect("web app");
        let mut composite = Vec::new();
        for frame in 0..64 {
            app.frame(16.0, f64::from(frame) * 0.016)
                .expect("settling frame");
            composite = app.map_pixels().expect("canonical CPU map");
            if composite
                .chunks_exact(4)
                .any(|pixel| pixel != &composite[0..4])
            {
                break;
            }
        }
        assert!(!composite.is_empty());
        assert!(composite.chunks_exact(4).all(|pixel| pixel[3] == 255));
        assert!(
            composite
                .chunks_exact(4)
                .any(|pixel| pixel != &composite[0..4]),
            "the actual wasm facade must compose the settled shared RegionMap"
        );

        let before_read = snapshot(&app);
        let reread = app.map_pixels().expect("repeat canonical CPU map");
        assert_eq!(
            snapshot(&app),
            before_read,
            "presentation cannot tick the world"
        );
        assert_eq!(
            reread, composite,
            "an unchanged map reuses identical composition"
        );

        app.action(
            String::from("set-map-channel"),
            Some(String::from("elevation")),
        )
        .expect("typed shared channel action");
        app.frame(0.0, 2.0).expect("channel frame");
        assert_ne!(
            app.map_pixels().expect("elevation CPU map"),
            composite,
            "the wasm facade must expose the shared channel selection"
        );
    }

    #[wasm_bindgen_test]
    fn missing_gpu_surface_reports_and_redraws_the_cpu_fallback() {
        let mut app = WebApp::new(JsValue::from_str("{}")).expect("web app");
        app.renderer_available();
        let frame = app
            .frame(0.0, 0.0)
            .expect("fallback frame")
            .as_string()
            .expect("frame JSON");
        assert!(frame.contains("\"path\":\"cpu\""));
        assert!(frame.contains("\"gpu_submitted\":false"));
        assert!(frame.contains("\"map_dirty\":true"));
        assert!(snapshot(&app).contains("\"mode\":\"cpu-fallback\""));
        assert!(!app.map_pixels().expect("fallback pixels").is_empty());

        app.renderer_lost();
        let loss = app
            .frame(0.0, 0.1)
            .expect("device-loss fallback frame")
            .as_string()
            .expect("frame JSON");
        assert!(loss.contains("\"path\":\"cpu\""));
        assert!(loss.contains("\"gpu_submitted\":false"));
        assert!(loss.contains("\"map_dirty\":true"));
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
    fn failed_gpu_map_submission_gets_one_bounded_retry() {
        let mut scheduled = false;
        assert!(super::claim_gpu_map_retry(&mut scheduled));
        assert!(!super::claim_gpu_map_retry(&mut scheduled));
        scheduled = false;
        assert!(super::claim_gpu_map_retry(&mut scheduled));
    }

    fn attribute_values<'a>(source: &'a str, attribute: &str) -> Vec<&'a str> {
        let needle = format!("{attribute}=\"");
        source
            .split(&needle)
            .skip(1)
            .filter_map(|rest| rest.split_once('"').map(|(value, _)| value))
            .collect()
    }

    #[test]
    fn browser_controls_and_help_use_registered_action_ids() {
        let index = include_str!("../web/index.html");
        let help = include_str!("../web/help/index.html");
        let controls = attribute_values(index, "data-action");
        let documented = attribute_values(help, "data-help-action");
        assert!(!controls.is_empty());
        for id in &controls {
            assert!(
                viewer_host::action::action_descriptor(id).is_some(),
                "unknown browser control action {id}"
            );
            assert!(documented.contains(id), "browser help omits control {id}");
        }
        for descriptor in viewer_host::action::ACTION_DESCRIPTORS {
            assert!(
                documented.contains(&descriptor.id.as_str()),
                "browser help omits descriptor {}",
                descriptor.id.as_str()
            );
        }
        for id in documented {
            assert!(
                viewer_host::action::action_descriptor(id).is_some(),
                "help documents unknown action {id}"
            );
        }
        let app = include_str!("../web/assets/app.js");
        assert!(!app.contains("MOVE_KEYS"));
        assert!(!app.contains("POV_MOVE"));
        assert!(!app.contains("requestPointerLock"));
        assert!(app.contains("app.frame(dt, now / 1000)"));
        assert!(app.contains("app.map_descriptors()"));
        assert!(app.contains("installMapControls"));
        assert!(!app.contains("MAP_CHANNELS"));
        assert!(!app.contains("paint_region"));
        assert!(!app.contains("compose_map"));
        assert!(!app.contains("app.update("));
        assert!(!app.contains("app.pov_frame("));
    }

    #[test]
    fn browser_map_controls_serialize_every_shared_descriptor_once() {
        let descriptors = super::map_descriptors_json();
        let expected =
            viewer_host::CHANNEL_DESCRIPTORS.len() + viewer_host::MAP_OVERLAY_DESCRIPTORS.len();
        assert_eq!(descriptors.matches("\"id\":").count(), expected);
        for id in viewer_host::CHANNEL_DESCRIPTORS
            .iter()
            .map(|descriptor| descriptor.id)
            .chain(
                viewer_host::MAP_OVERLAY_DESCRIPTORS
                    .iter()
                    .map(|descriptor| descriptor.id),
            )
        {
            assert_eq!(
                descriptors.matches(&format!("\"id\":\"{id}\"")).count(),
                1,
                "descriptor {id} must appear exactly once"
            );
        }
        assert!(super::BrowserHostState::default()
            .snapshot_json()
            .contains("\"decor_status\":\"browser-vault-unavailable\""));
    }

    /// A small settle-able window so controller-integration tests stay fast.
    fn small_controller_app() -> super::BrowserHostState {
        use world_core::REGION_SIZE;

        let config = world_runtime::StreamConfig {
            near_radius: 0.5 * REGION_SIZE,
            far_radius: 1.5 * REGION_SIZE,
            load_radius: 1.5 * REGION_SIZE,
            unload_radius: 2.5 * REGION_SIZE,
            field_resolution: 8,
            ..world_runtime::StreamConfig::default()
        };
        let world = viewer_host::ExplorationWorld::with_runtime(
            config,
            world_runtime::Budget::unlimited(),
            world_runtime::ResourceTier::Low,
        );
        super::BrowserHostState {
            controller: viewer_host::ViewerController::new(world),
            composer: viewer_host::map::MapComposer::new(1, 8),
            ..super::BrowserHostState::default()
        }
    }

    fn settle(state: &mut super::BrowserHostState) {
        for _ in 0..12 {
            let _ = state.frame(0.0, viewer_host::input::InputFrame::default());
        }
    }

    #[test]
    fn browser_resize_resolves_one_exact_physical_draw_and_pick_rect() {
        let mut state = small_controller_app();
        state.resize_surface(901, 701);
        let layout = state.resolved_layout();
        assert_eq!(layout.content, viewer_host::PixelRect::new(0, 0, 901, 701));
        assert_eq!(
            layout.map_content,
            Some(viewer_host::PixelRect::new(100, 0, 701, 701))
        );
        let projection = state.map_projection().expect("Map has a projection");
        assert_eq!(projection.destination, layout.map_content.unwrap());
        assert_eq!(
            projection.physical_to_world((450.5, 350.5)),
            Some((world_core::REGION_SIZE * 0.5, world_core::REGION_SIZE * 0.5))
        );
        assert!(projection.physical_to_world((99.999, 350.5)).is_none());
        assert!(projection.grid_coverage().one_pixel_feasible);
        assert!(state
            .layout_json()
            .contains("\"map_content\":[100, 0, 701, 701]"));
    }

    #[test]
    fn browser_resize_invalidates_pixels_once_and_is_idempotent() {
        let mut state = small_controller_app();
        state.prepared_cpu_key = Some(17);
        state.overlay_hashes = [Some(23), Some(24)];
        state.force_cpu_map_redraw = false;
        state.resize_surface(640, 480);
        assert_eq!(state.prepared_cpu_key, None);
        assert_eq!(state.overlay_hashes, [None; 2]);
        assert!(state.force_cpu_map_redraw);

        state.force_cpu_map_redraw = false;
        state.prepared_cpu_key = Some(29);
        state.resize_surface(640, 480);
        assert_eq!(state.prepared_cpu_key, Some(29));
        assert!(!state.force_cpu_map_redraw);
    }

    #[test]
    fn browser_button_and_key_paths_share_one_controller_queue() {
        use viewer_host::input::{ButtonPhase, Modifiers, NormalizedInputEvent, PhysicalKey};

        let mut button_state = small_controller_app();
        let mut button_driver = super::BrowserViewerDriver::default();
        button_driver.set_surface_focus(&button_state, true);
        button_driver.enqueue_action(
            &mut button_state,
            viewer_host::ViewerAction::ToggleRefinement,
        );
        let input = button_driver.take_frame(&mut button_state);
        let _ = button_state.frame(0.0, input);

        let mut key_state = small_controller_app();
        let mut key_driver = super::BrowserViewerDriver::default();
        key_driver.set_surface_focus(&key_state, true);
        assert!(key_driver.handle(
            &mut key_state,
            NormalizedInputEvent::Key {
                key: PhysicalKey::Period,
                phase: ButtonPhase::Pressed,
                repeat: false,
                modifiers: Modifiers::default(),
            },
        ));
        let input = key_driver.take_frame(&mut key_state);
        let _ = key_state.frame(0.0, input);

        assert!(button_state.controller.map_preferences().refinement);
        assert_eq!(
            button_state.controller.map_preferences(),
            key_state.controller.map_preferences()
        );
        assert_eq!(button_state.last_action, key_state.last_action);
    }

    #[test]
    fn pre_frame_tab_previews_pov_context_for_held_and_following_keys() {
        use viewer_host::input::{ButtonPhase, Modifiers, NormalizedInputEvent, PhysicalKey};

        let mut state = small_controller_app();
        state.set_renderer_webgpu();
        let mut driver = super::BrowserViewerDriver::default();
        driver.set_surface_focus(&state, true);

        // W starts held in Map, then Tab changes the preview before both the
        // following B event and the one per-frame intent sample.
        for key in [PhysicalKey::KeyW, PhysicalKey::Tab, PhysicalKey::KeyB] {
            assert!(driver.handle(
                &mut state,
                NormalizedInputEvent::Key {
                    key,
                    phase: ButtonPhase::Pressed,
                    repeat: false,
                    modifiers: Modifiers::default(),
                },
            ));
        }

        let input = driver.take_frame(&mut state);
        assert_eq!(input.map_axis, [0, 0]);
        assert_eq!(input.pov_axis, [0, 1, 0]);
        let frame = state.frame(0.0, input);
        assert_eq!(frame.output.mode, viewer_host::PresentationMode::Pov);
        assert!(
            !frame.output.pov.shadow_ao,
            "B used the previewed POV binding"
        );
        assert!(!frame.output.effects.iter().any(|effect| matches!(
            effect,
            viewer_host::ViewerEffect::ReportWarning(warning)
                if warning.id == "discovery-no-anchor"
        )));
    }

    #[test]
    fn browser_surface_focus_and_primary_drag_remain_mapper_owned() {
        use viewer_host::input::{
            ButtonPhase, Modifiers, NormalizedInputEvent, PhysicalKey, PointerButton,
        };

        let mut state = small_controller_app();
        state.set_renderer_webgpu();
        state.enqueue_action(viewer_host::ViewerAction::SetPresentation(
            viewer_host::PresentationMode::Pov,
        ));
        let _ = state.frame(0.0, viewer_host::input::InputFrame::default());

        let mut driver = super::BrowserViewerDriver::default();
        driver.set_surface_focus(&state, false);
        assert!(!driver.handle(
            &mut state,
            NormalizedInputEvent::Key {
                key: PhysicalKey::Tab,
                phase: ButtonPhase::Pressed,
                repeat: false,
                modifiers: Modifiers::default(),
            },
        ));
        assert_eq!(
            state.controller.layout().mode,
            viewer_host::PresentationMode::Pov
        );

        driver.set_surface_focus(&state, true);
        driver.handle(
            &mut state,
            NormalizedInputEvent::PointerMoved {
                pointer: 7,
                position: [10.0, 20.0],
                view: viewer_host::ViewKind::Pov,
            },
        );
        assert_eq!(driver.take_frame(&mut state).look_delta, [0.0, 0.0]);
        assert!(driver.handle(
            &mut state,
            NormalizedInputEvent::PointerButton {
                pointer: 7,
                button: PointerButton::Primary,
                phase: ButtonPhase::Pressed,
                position: [10.0, 20.0],
                view: viewer_host::ViewKind::Pov,
            },
        ));
        assert!(driver.handle(
            &mut state,
            NormalizedInputEvent::PointerMoved {
                pointer: 7,
                position: [14.0, 17.0],
                view: viewer_host::ViewKind::Pov,
            },
        ));
        assert_eq!(driver.take_frame(&mut state).look_delta, [4.0, -3.0]);
        driver.handle(
            &mut state,
            NormalizedInputEvent::PointerCancelled { pointer: 7 },
        );
        assert!(!driver.take_frame(&mut state).primary_drag);
    }

    #[test]
    fn one_browser_frame_performs_exactly_one_shared_world_update() {
        let mut state = small_controller_app();
        assert_eq!(state.controller.world().update_serial(), 0);

        let first = state.frame(0.0, viewer_host::input::InputFrame::default());
        assert_eq!(first.output.frame, 1);
        assert_eq!(first.output.update_serial, 1);
        assert_eq!(state.controller.world().update_serial(), 1);

        let second = state.frame(0.0, viewer_host::input::InputFrame::default());
        assert_eq!(second.output.frame, 2);
        assert_eq!(second.output.update_serial, 2);
        assert_eq!(state.controller.world().update_serial(), 2);

        let serial = state.controller.world().update_serial();
        let _ = state.cpu_map_pixels();
        let _ = state.cpu_map_json();
        let _ = state.inspect_json(0.0, 0.0);
        let _ = state.snapshot_json();
        assert_eq!(state.controller.world().update_serial(), serial);
    }

    #[test]
    fn map_input_uses_the_shared_movement_contract() {
        let mut state = small_controller_app();
        let moved = state.frame(
            16.666_667,
            viewer_host::input::InputFrame {
                map_axis: [1, 0],
                ..viewer_host::input::InputFrame::default()
            },
        );
        assert!((moved.output.traveler.0 - 500.0 / 60.0).abs() < 0.01);
        assert_eq!(moved.output.traveler.1, 0.0);

        let mut diagonal = small_controller_app();
        let moved = diagonal.frame(
            1000.0,
            viewer_host::input::InputFrame {
                map_axis: [1, 1],
                ..viewer_host::input::InputFrame::default()
            },
        );
        let expected = 500.0 * 0.1 / std::f64::consts::SQRT_2;
        assert!((moved.output.traveler.0 - expected).abs() < 1e-9);
        assert!((moved.output.traveler.1 - expected).abs() < 1e-9);
    }

    #[test]
    fn pov_tick_keeps_camera_traveler_and_stream_center_aligned() {
        let mut state = small_controller_app();
        state.set_renderer_webgpu();
        state.enqueue_action(viewer_host::ViewerAction::SetPresentation(
            viewer_host::PresentationMode::Pov,
        ));
        let entered = state.frame(0.0, viewer_host::input::InputFrame::default());
        assert_eq!(entered.output.mode, viewer_host::PresentationMode::Pov);
        assert!(entered.output.pov.initialized);
        assert!(!state.pov_chunks.is_empty());

        let before = entered.output.pov.position;
        let moved = state.frame(
            100.0,
            viewer_host::input::InputFrame {
                pov_axis: [0, 1, 0],
                ..viewer_host::input::InputFrame::default()
            },
        );
        assert_eq!(moved.output.update_serial, entered.output.update_serial + 1);
        assert_ne!(moved.output.pov.position, before);
        assert_eq!(
            moved.output.traveler,
            (moved.output.pov.position[0], moved.output.pov.position[1])
        );
        assert_eq!(
            state.controller.world().traveler().position,
            moved.output.traveler
        );
        let camera_center = world_core::RegionCoord::from_world(
            moved.output.pov.position[0],
            moved.output.pov.position[1],
        );
        let traveler_center =
            world_core::RegionCoord::from_world(moved.output.traveler.0, moved.output.traveler.1);
        assert_eq!(camera_center, traveler_center);
        assert!(
            state
                .controller
                .world()
                .map()
                .cache()
                .get(traveler_center)
                .is_some(),
            "the single world update streamed around the shared traveler"
        );
    }

    #[test]
    fn map_composition_reads_the_current_cache_without_updating_it() {
        let mut state = small_controller_app();
        settle(&mut state);
        let serial = state.controller.world().update_serial();
        let side = state.map_side();
        let pixels = state.cpu_map_pixels();

        assert_eq!(side, 3 * 8);
        assert_eq!(pixels.len(), side * side * 4);
        assert!(pixels.chunks_exact(4).all(|pixel| pixel[3] == 255));
        assert!(
            pixels.chunks_exact(4).any(|pixel| pixel != &pixels[0..4]),
            "the settled cache must not compose as one flat color"
        );
        assert_eq!(state.controller.world().update_serial(), serial);

        let snapshot = state.snapshot_json();
        assert!(snapshot.contains("\"stats\":{\"loaded\":"));
        assert!(state
            .inspect_json(10.0, 10.0)
            .contains("\"status\":\"ready\""));
    }

    #[test]
    fn controller_actions_drive_map_and_platform_effects() {
        let mut state = small_controller_app();
        settle(&mut state);
        let composite = state.cpu_map_pixels().to_vec();
        state.enqueue_action(viewer_host::ViewerAction::SetMapChannel(
            viewer_host::Channel::Elevation,
        ));
        state.enqueue_action(viewer_host::ViewerAction::SetStorageEnabled(true));
        state.enqueue_action(viewer_host::ViewerAction::SaveSession);
        state.enqueue_action(viewer_host::ViewerAction::LoadSession);
        let frame = state.frame(0.0, viewer_host::input::InputFrame::default());

        assert_eq!(
            state.controller.map_preferences().channel,
            viewer_host::Channel::Elevation
        );
        assert_ne!(state.cpu_map_pixels(), composite);
        assert_eq!(state.storage, "memory");
        assert!(frame.output.effects.iter().any(|effect| matches!(
            effect,
            viewer_host::ViewerEffect::ConfigureStorage { enabled: true }
        )));
        assert!(frame
            .output
            .effects
            .iter()
            .any(|effect| matches!(effect, viewer_host::ViewerEffect::PersistSession(_))));
        assert!(frame
            .output
            .effects
            .iter()
            .any(|effect| matches!(effect, viewer_host::ViewerEffect::LoadSession(_))));
        let save_id = frame
            .output
            .effects
            .iter()
            .find_map(|effect| match effect {
                viewer_host::ViewerEffect::PersistSession(request) => Some(request.request_id),
                _ => None,
            })
            .expect("save request");
        let load_id = frame
            .output
            .effects
            .iter()
            .find_map(|effect| match effect {
                viewer_host::ViewerEffect::LoadSession(request) => Some(*request),
                _ => None,
            })
            .expect("load request");
        assert!(frame.service_response_queued);
        assert!(state.controller.request_pending(save_id));
        assert!(state.controller.request_pending(load_id));
        assert_eq!(state.service_sequence, 2);

        let failed = state.frame(0.0, viewer_host::input::InputFrame::default());
        assert!(!state.controller.request_pending(save_id));
        assert!(!state.controller.request_pending(load_id));
        assert_eq!(
            failed
                .output
                .effects
                .iter()
                .filter(|effect| matches!(
                    effect,
                    viewer_host::ViewerEffect::ReportWarning(warning)
                        if warning.id == "browser-service-unavailable"
                ))
                .count(),
            2
        );
        assert!(!failed.service_response_queued);
    }

    #[test]
    fn webgpu_capability_is_false_until_renderer_init_and_loss_falls_back() {
        let mut state = small_controller_app();
        assert!(!state.controller.pov_state().supported);
        state.enqueue_action(viewer_host::ViewerAction::SetPresentation(
            viewer_host::PresentationMode::Pov,
        ));
        let unavailable = state.frame(0.0, viewer_host::input::InputFrame::default());
        assert_eq!(unavailable.output.mode, viewer_host::PresentationMode::Map);
        assert!(state.snapshot_json().contains("POV is unavailable"));

        state.set_renderer_webgpu();
        state.enqueue_action(viewer_host::ViewerAction::SetPresentation(
            viewer_host::PresentationMode::Pov,
        ));
        let available = state.frame(0.0, viewer_host::input::InputFrame::default());
        assert_eq!(available.output.mode, viewer_host::PresentationMode::Pov);
        let serial = state.controller.world().update_serial();

        state.renderer_lost();
        assert!(
            state.force_cpu_map_redraw,
            "GPU loss must invalidate an otherwise clean CPU fallback frame"
        );
        assert_eq!(
            state.controller.layout().mode,
            viewer_host::PresentationMode::Pov,
            "a platform callback cannot mutate the controller between ticks"
        );
        assert_eq!(state.controller.world().update_serial(), serial);
        let fallback = state.frame(0.0, viewer_host::input::InputFrame::default());
        assert_eq!(fallback.output.update_serial, serial + 1);
        assert_eq!(
            state.controller.layout().mode,
            viewer_host::PresentationMode::Map
        );
        assert!(!state.controller.pov_state().supported);
        assert!(fallback.output.effects.iter().any(|effect| matches!(
            effect,
            viewer_host::ViewerEffect::ReportWarning(warning)
                if warning.id == "webgpu-device-lost"
        )));
        assert_eq!(state.renderer, "cpu-fallback");
        assert_eq!(state.device_losses, 1);
    }
}
