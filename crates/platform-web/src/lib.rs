//! `platform-web` — the browser/wasm shell and service adapter.
//!
//! The landed static runtime drives the shared [`viewer_host`] controller,
//! composer, inspection, panel model, POV host, and multi-view renderer through
//! one logical frame facade. This crate owns only browser-facing wasm glue,
//! renderer/surface lifecycle, and typed platform capability injection; viewer
//! semantics stay platform-neutral under ADR 0028. Portable parity exports
//! remain here so CI executes deterministic world probes as actual wasm. The
//! current IndexedDB and worker modules are capability probes: correlated vault
//! effects remain unavailable and logical world ticks use [`InlineExecutor`].

#[cfg(target_arch = "wasm32")]
use viewer_host::input::{ButtonPhase, Modifiers, PhysicalKey, PointerButton, WheelDelta};
use viewer_host::PreparedMapSource;
use viewer_host::{
    action::{ActionScope, ServiceRequestId, ViewerEffect, WorkerBackend, ACTION_DESCRIPTORS},
    atlas::{AtlasManager, RefinementRequest},
    controller::{GroundSample, PovGroundSampler},
    input::{InputContext, InputFrame, InputMapper, NormalizedInputEvent, BINDING_DESCRIPTORS},
    map::{Channel, MapBackend, MapBackendFallback, MapComposer, MapDecor, MapRenderRequest},
    panel::{
        PanelBuildInput, PanelDocumentCache, PanelDocumentKey, PerformanceInfo, PersistenceInfo,
        RendererInfo, Severity, StreamingSupplement, VaultInfo, ViewerWarning, WarningRegistry,
    },
    resolve_view_layout, ExplorationWorld, MapViewportProjection, NoopWorldTickHook, PixelRect,
    PlatformTelemetry, PovHoverCache, PresentationMode, ResolvedViewLayout, ServiceNotification,
    ServiceResponse, ServiceResponseResult, ServiceResponseSequence, TickInput, TickOutput,
    ViewKind, ViewerAction, ViewerController,
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
    terrain, FeatureKey, PossibilityField, PossibilityVector, RegionCoord, REGION_SIZE,
    WORLD_ALGORITHM_VERSION,
};
use world_runtime::stream::RegionMap;
use world_runtime::task::InlineExecutor;
use world_runtime::tier::ResourceTier;

#[derive(Debug, Clone, Copy, Default, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum BrowserStartupTier {
    #[default]
    Auto,
    Low,
    Mid,
    High,
}

impl BrowserStartupTier {
    const fn resolve(self) -> ResourceTier {
        match self {
            Self::Auto | Self::Low => ResourceTier::Low,
            Self::Mid => ResourceTier::Mid,
            Self::High => ResourceTier::High,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum BrowserStartupWorkerMode {
    #[default]
    Inline,
    Workers,
    SharedWorkers,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct BrowserStartupConfig {
    tier: BrowserStartupTier,
    storage: bool,
    worker_mode: BrowserStartupWorkerMode,
}

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
/// vault I/O is deliberately not exported: the parity codec is live, while
/// correlated browser vault/session effects still return typed unavailable
/// responses.
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
    workers: u32,
    storage: &'static str,
    pending_writes: u32,
    storage_failures: u32,
    record_count: u32,
    renderer: &'static str,
    effective_map_backend: MapBackend,
    map_fallback: Option<MapBackendFallback>,
    renderer_ready: bool,
    force_cpu_map_redraw: bool,
    gpu_map_retry_scheduled: bool,
    device_losses: u32,
    surface_format: Option<String>,
    surface_losses: u32,
    warnings: WarningRegistry,
    panel_cache: PanelDocumentCache,
    panel_key: Option<PanelDocumentKey>,
    panel_revision: u64,
    hover_world: Option<(f64, f64)>,
    pov_hover: PovHoverCache,
    hover_revision: u64,
    performance: PerformanceInfo,
    telemetry_revision: u64,
    platform_revision: u64,
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
    hover_changed: bool,
}

/// Small immediate presentation state used by canvas/control adapters. The
/// information dock consumes [`viewer_host::PanelDocument`] instead; keeping
/// this DTO separate avoids rebuilding or serializing that document on every
/// animation frame.
#[derive(Debug, serde::Serialize)]
struct BrowserPresentation {
    view: BrowserViewPresentation,
    map: BrowserMapPresentation,
    tier: BrowserTierPresentation,
    executor: BrowserExecutorPresentation,
    storage: BrowserStoragePresentation,
    renderer: BrowserRendererPresentation,
    decor_status: &'static str,
}

#[derive(Debug, serde::Serialize)]
struct BrowserViewPresentation {
    mode: PresentationMode,
    focused: ViewKind,
    split_ratio: f32,
    pov_supported: bool,
    pov: BrowserPovPresentation,
}

#[derive(Debug, serde::Serialize)]
struct BrowserPovPresentation {
    motion: &'static str,
    shadow_ao: bool,
    detail_normals: bool,
    water: bool,
    render_scale: f32,
}

#[derive(Debug, serde::Serialize)]
struct BrowserMapPresentation {
    backend: MapBackend,
    channel: Channel,
    zoom: u32,
    refinement: bool,
    overlays: viewer_host::Overlays,
}

#[derive(Debug, serde::Serialize)]
struct BrowserTierPresentation {
    runtime: &'static str,
    benchmark_ms: f32,
}

#[derive(Debug, serde::Serialize)]
struct BrowserExecutorPresentation {
    mode: &'static str,
}

#[derive(Debug, serde::Serialize)]
struct BrowserStoragePresentation {
    mode: &'static str,
}

#[derive(Debug, serde::Serialize)]
struct BrowserRendererPresentation {
    mode: &'static str,
    device_losses: u32,
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
            workers: 1,
            storage: "memory",
            pending_writes: 0,
            storage_failures: 0,
            record_count: 0,
            renderer: "cpu-fallback",
            effective_map_backend: MapBackend::Cpu,
            map_fallback: None,
            renderer_ready: false,
            force_cpu_map_redraw: false,
            gpu_map_retry_scheduled: false,
            device_losses: 0,
            surface_format: None,
            surface_losses: 0,
            warnings: WarningRegistry::default(),
            panel_cache: PanelDocumentCache::default(),
            panel_key: None,
            panel_revision: 0,
            hover_world: None,
            pov_hover: PovHoverCache::new(),
            hover_revision: 0,
            performance: PerformanceInfo::default(),
            telemetry_revision: 0,
            platform_revision: 0,
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
    fn new(config: &str) -> Result<Self, serde_json::Error> {
        let config: BrowserStartupConfig = serde_json::from_str(config)?;
        let tier = config.tier.resolve();
        let cfg = tier.stream_config();
        let half_regions = (cfg.load_radius / REGION_SIZE).ceil() as i32;
        let mut state = Self {
            controller: ViewerController::new(ExplorationWorld::new(tier)),
            composer: MapComposer::new(half_regions, cfg.field_resolution),
            ..Self::default()
        };
        if config.storage {
            state.storage = "indexeddb-pending";
            state.warn_once(ViewerWarning {
                id: "indexeddb-pending",
                message: String::from(
                    "IndexedDB is not ready; in-memory persistence remains active.",
                ),
                severity: Severity::Warning,
            });
        }
        match config.worker_mode {
            BrowserStartupWorkerMode::Inline => {}
            BrowserStartupWorkerMode::Workers => {
                state.worker_mode = "workers";
                state.workers = 2;
            }
            BrowserStartupWorkerMode::SharedWorkers => {
                state.worker_mode = "shared-memory";
                state.workers = 2;
            }
        }
        Ok(state)
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

    /// Resolve one physical surface point to the visible pane that owns it.
    ///
    /// The shared half-open pane rectangles are the sole hit-test authority;
    /// browser pointer capture may preserve the returned view for the rest of
    /// a gesture, but JavaScript never reconstructs the split boundary.
    fn view_at(&self, x: f64, y: f64) -> Option<ViewKind> {
        self.resolved_layout().hit_view([x, y])
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

    fn layout_value(&self) -> serde_json::Value {
        let layout = self.resolved_layout();
        let focused = match layout.focused {
            ViewKind::Map => "map",
            ViewKind::Pov => "pov",
        };
        let rect = |rect: PixelRect| [rect.x, rect.y, rect.width, rect.height];
        serde_json::json!({
            "content": rect(layout.content),
            "map_pane": layout.map_pane.map(rect),
            "map_content": layout.map_content.map(rect),
            "pov_pane": layout.pov_pane.map(rect),
            "pov_aspect": layout.pov_aspect,
            "focused": focused,
            "split_ratio": layout.split_ratio,
            "focus_border": layout.focus_border(layout.focused).map(rect),
            "divider": layout.divider.map(rect),
        })
    }

    fn layout_json(&self) -> String {
        serde_json::to_string(&self.layout_value())
            .expect("resolved browser layout contains only finite serializable values")
    }

    #[cfg(test)]
    fn frame(&mut self, dt_ms: f64, input: InputFrame) -> BrowserFrame {
        self.frame_at(dt_ms, input, 0.0)
    }

    fn frame_at(&mut self, dt_ms: f64, input: InputFrame, time_seconds: f32) -> BrowserFrame {
        let pov_pointer = input.pov_pointer;
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
                self.warn_once(warning.clone());
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
        // Round once exactly as PovFrameParams/WGSL do, then reuse that
        // visible cutoff for selection and picking as well as drawing.
        let fog_end = f64::from(pov_host::pov_fog_end(self.pov_radius) as f32);
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
                fog_end,
            );
        }
        // Ordering is intentional: the controller has already applied held
        // primary-drag look, then resident terrain/organisms were synchronized;
        // hover therefore describes the post-drag camera drawn this frame.
        let hover_changed = if output.mode != PresentationMode::Map {
            let pov_pane = self.resolved_layout().pov_pane;
            self.pov_hover.update(
                self.controller.world().map(),
                self.controller.pov_camera(),
                &self.pov_chunks,
                &self.pov_organisms,
                pov_pointer,
                pov_pane,
                fog_end,
                time_seconds,
            )
        } else {
            false
        };
        if hover_changed {
            self.hover_revision = self.hover_revision.saturating_add(1);
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
            hover_changed,
        }
    }

    fn platform_telemetry(&self) -> PlatformTelemetry {
        PlatformTelemetry {
            present_ms: self.performance.present_ms,
            dom_updates: self.performance.dom_updates,
            executor_backend: match self.worker_mode {
                "workers" => WorkerBackend::Workers,
                "shared-memory" => WorkerBackend::SharedWorkers,
                _ => WorkerBackend::Inline,
            },
            workers: self.workers as usize,
            storage_available: self.storage == "indexeddb",
            surface_format: self.surface_format.clone(),
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

    fn warn_once(&mut self, warning: ViewerWarning) {
        if self.warnings.upsert(warning) {
            self.platform_revision = self.platform_revision.saturating_add(1);
        }
    }

    fn set_renderer_webgpu(&mut self) {
        let warning_removed = self.warnings.remove("renderer-device-loss")
            | self.warnings.remove("renderer-map-fallback");
        let panel_changed = !self.renderer_ready
            || self.renderer != "webgpu-ready"
            || self.map_fallback.is_some()
            || warning_removed;
        self.renderer_ready = true;
        self.atlas = AtlasManager::default();
        self.overlay_hashes = [None; 2];
        self.prepared_cpu_key = None;
        self.gpu_map_retry_scheduled = false;
        // Device readiness alone is not a claim that a GPU map was drawn.
        self.renderer = "webgpu-ready";
        self.map_fallback = None;
        if panel_changed {
            self.platform_revision = self.platform_revision.saturating_add(1);
        }
        self.enqueue_pov_availability(true, None);
    }

    fn set_surface_format(&mut self, surface_format: Option<String>) {
        if self.surface_format == surface_format {
            return;
        }
        self.surface_format = surface_format;
        self.platform_revision = self.platform_revision.saturating_add(1);
    }

    fn set_surface_losses(&mut self, surface_losses: u32) {
        if self.surface_losses == surface_losses {
            return;
        }
        self.surface_losses = surface_losses;
        self.platform_revision = self.platform_revision.saturating_add(1);
    }

    fn record_map_result(
        &mut self,
        backend: MapBackend,
        fallback: Option<MapBackendFallback>,
    ) -> bool {
        let renderer = match backend {
            MapBackend::Cpu => "cpu-fallback",
            MapBackend::GpuAtlas => "webgpu-atlas",
        };
        let changed = self.renderer != renderer
            || self.effective_map_backend != backend
            || self.map_fallback != fallback;
        self.renderer = renderer;
        self.effective_map_backend = backend;
        self.map_fallback = fallback;
        if changed {
            self.platform_revision = self.platform_revision.saturating_add(1);
        }
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

    /// Renderer failure changes platform state immediately, then queues the
    /// shared capability/fallback transition for the next logical tick.
    fn renderer_failed(&mut self, device_lost: bool) {
        self.renderer_ready = false;
        self.renderer = "cpu-fallback";
        self.atlas = AtlasManager::default();
        self.overlay_hashes = [None; 2];
        self.prepared_cpu_key = None;
        self.force_cpu_map_redraw = true;
        self.gpu_map_retry_scheduled = false;
        // A replacement Renderer starts with empty GPU terrain/organism
        // buffers. Reset the CPU-side publication managers as well so their
        // next POV/Split sync emits complete uploads instead of assuming the
        // lost device still owns the resident resources.
        self.pov_chunks = pov_host::PovChunkManager::new();
        self.pov_organisms = pov_host::PovOrganismManager::new();
        self.pov_hover = PovHoverCache::new();
        if device_lost {
            self.device_losses = self.device_losses.saturating_add(1);
        }
        self.effective_map_backend = MapBackend::Cpu;
        self.map_fallback = Some(MapBackendFallback::GpuUnavailable);
        self.platform_revision = self.platform_revision.saturating_add(1);
        self.enqueue_pov_availability(
            false,
            Some(ViewerWarning {
                id: if device_lost {
                    "renderer-device-loss"
                } else {
                    "renderer-map-fallback"
                },
                message: if device_lost {
                    String::from("WebGPU device lost; CPU map fallback active")
                } else {
                    String::from("WebGPU initialization failed; CPU map fallback active")
                },
                severity: Severity::Warning,
            }),
        );
    }

    fn renderer_lost(&mut self) {
        self.renderer_failed(true);
    }

    fn renderer_unavailable(&mut self) {
        self.renderer_failed(false);
    }

    fn set_hover_world(&mut self, world: Option<(f64, f64)>) -> bool {
        let world = world.filter(|(x, y)| x.is_finite() && y.is_finite());
        if self.hover_world == world {
            return false;
        }
        self.hover_world = world;
        self.hover_revision = self.hover_revision.saturating_add(1);
        true
    }

    fn set_storage_status(&mut self, mode: &str, failures: u32) -> Result<(), String> {
        let mode = match mode {
            "memory" => "memory",
            "indexeddb" => "indexeddb",
            "indexeddb-pending" => "indexeddb-pending",
            other => return Err(format!("unknown browser storage mode {other:?}")),
        };
        let changed = self.storage != mode || self.storage_failures != failures;
        self.storage = mode;
        self.storage_failures = failures;
        let warning_removed = mode == "indexeddb" && self.warnings.remove("indexeddb-pending");
        if changed || warning_removed {
            self.platform_revision = self.platform_revision.saturating_add(1);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn update_panel_telemetry(
        &mut self,
        fps: u32,
        update_ms: f64,
        compose_ms: f64,
        present_ms: f64,
        upload_kib_per_frame: f64,
        dom_updates: u64,
    ) -> bool {
        let milliseconds = |value: f64| {
            if value.is_finite() {
                value.max(0.0)
            } else {
                0.0
            }
        };
        let next = PerformanceInfo {
            fps,
            update_ms: milliseconds(update_ms),
            compose_ms: milliseconds(compose_ms),
            present_ms: milliseconds(present_ms),
            upload_kib_per_frame: milliseconds(upload_kib_per_frame),
            pass_ms: self.last_stats.pass_ms,
            dom_updates,
        };
        if self.performance == next {
            return false;
        }
        self.performance = next;
        self.telemetry_revision = self.telemetry_revision.saturating_add(1);
        true
    }

    fn streaming_supplement(&self) -> StreamingSupplement {
        let map = self.controller.world().map();
        StreamingSupplement {
            regen_totals: self.regen_totals,
            macro_tiles: map.macro_cache().len(),
            rosters: map.roster_cache().len(),
            organisms: map.organism_count(),
            jobs_in_flight: map.jobs_in_flight(),
            pinned_violations: self.composer.pinned_violations,
        }
    }

    fn persistence_info(&self) -> PersistenceInfo {
        PersistenceInfo {
            mode: String::from(self.storage),
            available: self.storage == "indexeddb",
            vault: (self.storage == "indexeddb").then_some(VaultInfo {
                records: self.record_count as usize,
                ..VaultInfo::default()
            }),
            pending_writes: u64::from(self.pending_writes),
            failures: u64::from(self.storage_failures),
            ..PersistenceInfo::default()
        }
    }

    fn renderer_info(&self) -> RendererInfo {
        RendererInfo {
            requested_map_backend: self.controller.map_preferences().backend,
            effective_map_backend: self.effective_map_backend,
            map_fallback: self.map_fallback,
            surface_format: self.surface_format.clone(),
            device_losses: self.device_losses,
            surface_losses: self.surface_losses,
        }
    }

    fn presentation(&self) -> Option<BrowserPresentation> {
        let output = self.last_output.as_ref()?;
        let map = self.controller.map_preferences();
        let pov = output.pov;
        Some(BrowserPresentation {
            view: BrowserViewPresentation {
                mode: output.mode,
                focused: output.focused,
                split_ratio: self.controller.layout().split_ratio,
                pov_supported: pov.supported,
                pov: BrowserPovPresentation {
                    motion: if pov.walk { "walk" } else { "fly" },
                    shadow_ao: pov.shadow_ao,
                    detail_normals: pov.detail_normals,
                    water: pov.water,
                    render_scale: pov.render_scale,
                },
            },
            map: BrowserMapPresentation {
                backend: map.backend,
                channel: map.channel,
                zoom: map.zoom,
                refinement: map.refinement,
                overlays: map.overlays,
            },
            tier: BrowserTierPresentation {
                runtime: self.controller.world().tier().name(),
                benchmark_ms: self.benchmark_ms,
            },
            executor: BrowserExecutorPresentation {
                mode: self.worker_mode,
            },
            storage: BrowserStoragePresentation { mode: self.storage },
            renderer: BrowserRendererPresentation {
                mode: self.renderer,
                device_losses: self.device_losses,
            },
            decor_status: if self.storage == "indexeddb" {
                "browser-vault"
            } else {
                "browser-vault-unavailable"
            },
        })
    }

    fn panel_document_json(&mut self) -> Result<Option<String>, serde_json::Error> {
        let Some(output) = self.last_output.as_ref() else {
            return Ok(None);
        };
        let key = PanelDocumentKey {
            state: output.frame,
            hover: self.hover_revision,
            telemetry: self.telemetry_revision,
            platform: self.platform_revision,
        };
        if self.panel_key != Some(key) {
            self.panel_revision = self.panel_revision.saturating_add(1);
            self.panel_key = Some(key);
        }

        let hover = match (output.mode, output.focused) {
            (PresentationMode::Map, _) | (PresentationMode::Split, ViewKind::Map) => {
                viewer_host::map_hover(
                    self.controller.world().map(),
                    self.hover_world,
                    output.map.zoom,
                )
            }
            (PresentationMode::Pov, _) | (PresentationMode::Split, ViewKind::Pov) => {
                self.pov_hover.hover().clone()
            }
        };
        let performance = self.performance;
        let streaming = self.streaming_supplement();
        let persistence = self.persistence_info();
        let renderer = self.renderer_info();
        let capture = self.controller.capture_preferences();
        let split_ratio = self.controller.layout().split_ratio;
        let revision = self.panel_revision;
        let world = self.controller.world();
        let warnings = self.warnings.warnings();
        let (document, _) = self.panel_cache.get_or_build(key, || {
            viewer_host::build_panel_document(PanelBuildInput {
                tick: output,
                world,
                hover,
                performance,
                streaming,
                persistence,
                renderer,
                capture,
                warnings,
                split_ratio,
                revision,
            })
        });
        serde_json::to_string(document).map(Some)
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
        serde_json::to_string(&serde_json::json!({
            "kind": "rgba8",
            "renderer": self.renderer,
            "width": side,
            "height": side,
            "resolution": map.config().field_resolution,
            "channel": self.controller.map_preferences().channel.id(),
        }))
        .expect("CPU map metadata contains only serializable values")
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
    let descriptors = ACTION_DESCRIPTORS
        .iter()
        .map(|descriptor| {
            let scope = match descriptor.scope {
                ActionScope::Global => "global",
                ActionScope::FocusedView => "focused-view",
                ActionScope::Map => "map",
                ActionScope::Pov => "pov",
            };
            let bindings = descriptor
                .default_binding_ids
                .iter()
                .map(|id| {
                    let binding = BINDING_DESCRIPTORS
                        .iter()
                        .find(|binding| binding.id == *id)
                        .expect("action descriptor names a registered binding");
                    serde_json::json!({
                        "id": binding.id,
                        "help": binding.help,
                    })
                })
                .collect::<Vec<_>>();
            serde_json::json!({
                "id": descriptor.id.as_str(),
                "label": descriptor.label,
                "help": descriptor.help,
                "scope": scope,
                "bindings": bindings,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&descriptors).expect("action descriptors are serializable")
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn map_descriptors_json() -> String {
    let channels = viewer_host::CHANNEL_DESCRIPTORS
        .iter()
        .map(|descriptor| {
            serde_json::json!({
                "id": descriptor.id,
                "label": descriptor.label,
                "group": descriptor.group.id(),
                "group_label": descriptor.group.label(),
                "order": descriptor.order,
            })
        })
        .collect::<Vec<_>>();
    let overlays = viewer_host::MAP_OVERLAY_DESCRIPTORS
        .iter()
        .map(|descriptor| {
            serde_json::json!({
                "id": descriptor.id,
                "label": descriptor.label,
                "group": descriptor.group.id(),
                "group_label": descriptor.group.label(),
                "order": descriptor.order,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&serde_json::json!({
        "channels": channels,
        "overlays": overlays,
    }))
    .expect("map descriptors are serializable")
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn effects_json(effects: Vec<ViewerEffect>) -> String {
    let effects = effects
        .into_iter()
        .map(|effect| match effect {
            ViewerEffect::Exit => serde_json::json!({"kind": "exit"}),
            ViewerEffect::WriteDebugCapture(request) => serde_json::json!({
                "kind": "debug-capture",
                "request": request.request_id.0,
            }),
            ViewerEffect::PersistSession(request) => serde_json::json!({
                "kind": "persist-session",
                "request": request.request_id.0,
            }),
            ViewerEffect::LoadSession(request) => serde_json::json!({
                "kind": "load-session",
                "request": request.0,
            }),
            ViewerEffect::WriteDiscovery(request) => serde_json::json!({
                "kind": "write-discovery",
                "request": request.request_id.0,
            }),
            ViewerEffect::LoadDiscoveries(request) => serde_json::json!({
                "kind": "load-discoveries",
                "request": request.0,
            }),
            ViewerEffect::MutatePreserve(request) => serde_json::json!({
                "kind": "mutate-preserve",
                "request": request.request_id.0,
            }),
            ViewerEffect::WriteRoute(request) => serde_json::json!({
                "kind": "write-route",
                "request": request.request_id.0,
            }),
            ViewerEffect::ClearRoutes(request) => serde_json::json!({
                "kind": "clear-routes",
                "request": request.0,
            }),
            ViewerEffect::ConfigurePathTracking {
                request_id,
                enabled,
            } => serde_json::json!({
                "kind": "configure-path-tracking",
                "request": request_id.0,
                "enabled": enabled,
            }),
            ViewerEffect::OpenAtlasImport(request) => serde_json::json!({
                "kind": "open-atlas",
                "request": request.0,
            }),
            ViewerEffect::DownloadAtlasBundle(request) => serde_json::json!({
                "kind": "download-atlas",
                "request": request.0,
            }),
            ViewerEffect::ConfigureWorkerBackend(_) => {
                serde_json::json!({"kind": "configure-workers"})
            }
            ViewerEffect::CancelSupersededJobs => serde_json::json!({"kind": "cancel-jobs"}),
            ViewerEffect::ConfigureStorage { enabled } => serde_json::json!({
                "kind": "configure-storage",
                "enabled": enabled,
            }),
            ViewerEffect::ResetLocalVault => serde_json::json!({"kind": "reset-vault"}),
            ViewerEffect::SelectMapBackend(_) => {
                serde_json::json!({"kind": "select-map-backend"})
            }
            ViewerEffect::RunTierBenchmark => serde_json::json!({"kind": "benchmark"}),
            ViewerEffect::ConfigureResourceTier(tier) => serde_json::json!({
                "kind": "configure-tier",
                "tier": tier.name(),
            }),
            ViewerEffect::ReportWarning(warning) => serde_json::json!({
                "kind": "warning",
                "message": warning.message,
            }),
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&effects).expect("browser effects are serializable")
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

    /// The native shell's presentation-surface clear color, mirrored.
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

    /// Generated browser controls/help metadata from the canonical shared
    /// action and binding registries. This free export lets the static help
    /// route stay a thin document without constructing a world/controller.
    #[wasm_bindgen]
    pub fn viewer_action_descriptors() -> String {
        super::action_descriptors_json()
    }

    /// Browser application facade. JS forwards primitive raw events
    /// and exact action ids; the shared mapper produces typed frame intent and
    /// ordered actions before this facade updates presentation state. A cached
    /// shared panel document keeps DOM/browser APIs out of neutral crates.
    #[wasm_bindgen]
    #[derive(Debug)]
    pub struct WebApp {
        state: super::BrowserHostState,
        driver: super::BrowserViewerDriver,
        shutdown: bool,
    }

    #[wasm_bindgen]
    impl WebApp {
        /// Create a browser app with inline execution. Worker and IndexedDB
        /// modes are exact capability/status inputs; they do not overclaim a
        /// wired worker executor or correlated persistence backend.
        #[wasm_bindgen(constructor)]
        pub fn new(config: JsValue) -> Result<WebApp, JsValue> {
            let config = config
                .as_string()
                .ok_or_else(|| JsValue::from_str("WebApp config must be a JSON string"))?;
            Ok(WebApp {
                state: super::BrowserHostState::new(&config).map_err(|error| {
                    JsValue::from_str(&format!("invalid WebApp config: {error}"))
                })?,
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
            // Device-loss callbacks are asynchronous. Fold the capability
            // transition before sampling input or advancing the one logical
            // world frame, then drop the slot so later polls cannot count the
            // same callback twice.
            let device_lost = VIEWER_RENDERER.with(|slot| {
                let mut slot = slot.borrow_mut();
                if slot
                    .as_ref()
                    .is_some_and(|renderer| renderer.device_losses() > 0)
                {
                    *slot = None;
                    true
                } else {
                    false
                }
            });
            if device_lost {
                self.renderer_lost();
            }
            let input = self.driver.take_frame(&mut self.state);
            let time = time_seconds.rem_euclid(f64::from(renderer::pov::WOBBLE_PERIOD)) as f32;
            let frame = self.state.frame_at(dt_ms, input, time);
            if frame.service_response_queued {
                self.driver.dirty = true;
            }
            self.driver.synchronize_context(&self.state);

            let split_active = frame.output.mode == super::PresentationMode::Split;
            let pov_active = matches!(
                frame.output.mode,
                super::PresentationMode::Pov | super::PresentationMode::Split
            );
            let map_active = matches!(
                frame.output.mode,
                super::PresentationMode::Map | super::PresentationMode::Split
            );
            let layout = self.state.resolved_layout();
            let map_projection = map_active.then(|| self.state.map_projection()).flatten();
            let (
                prepared_map_backend,
                prepared_map_fallback,
                map_drawn,
                map_upload_bytes,
                schedule_map_retry,
                pov_rendered,
                renderer_attempted,
                surface_presented,
            ) = VIEWER_RENDERER.with(|slot| {
                let mut slot = slot.borrow_mut();
                let gpu_available = self.state.renderer_ready && slot.is_some();
                let super::BrowserHostState {
                    controller,
                    composer,
                    atlas,
                    overlay_hashes,
                    prepared_cpu_key,
                    gpu_map_retry_scheduled,
                    pov_chunks,
                    pov_organisms,
                    pov_radius,
                    ..
                } = &mut self.state;

                // Prepare Map data first. Map and POV borrow the same
                // post-update world/controller state and are submitted below
                // through one surface frame.
                let map_packet = if map_active {
                    let world = controller.world();
                    let traveler = world.traveler().position;
                    let preferences = controller.map_preferences();
                    composer.set_zoom(preferences.zoom);
                    let decor = super::MapDecor::default();
                    Some(
                        composer.prepare_render(
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
                        ),
                    )
                } else {
                    None
                };
                let prepared_map_backend = map_packet
                    .as_ref()
                    .map_or(super::MapBackend::Cpu, |packet| packet.backend);
                let prepared_map_fallback = map_packet.as_ref().and_then(|packet| packet.fallback);
                if let Some(packet) = map_packet.as_ref() {
                    match &packet.source {
                        super::PreparedMapSource::Cpu(cpu) => {
                            debug_assert_eq!(
                                cpu.rgba.len(),
                                packet.projection.side as usize
                                    * packet.projection.side as usize
                                    * 4
                            );
                            *prepared_cpu_key = Some(packet.dirty_key);
                            *gpu_map_retry_scheduled = false;
                        }
                        super::PreparedMapSource::GpuAtlas(_) => {
                            *prepared_cpu_key = None;
                        }
                    }
                }

                let mut gpu_overlay_hashes = None;
                let mut cpu_map_upload_bytes = 0u64;
                let map_pane = map_packet.as_ref().and_then(|packet| {
                    let destination = packet.viewport.destination;
                    let viewport = renderer::SurfaceViewport::new(
                        destination.x,
                        destination.y,
                        destination.width,
                        destination.height,
                    );
                    match &packet.source {
                        // A Map-only CPU frame remains on the dedicated 2D
                        // fallback canvas. Split must upload that exact same
                        // canonical bitmap into the unified GPU frame so both
                        // panes share one acquire/submit/present.
                        super::PreparedMapSource::Cpu(cpu) if split_active && gpu_available => {
                            cpu_map_upload_bytes = cpu.rgba.len() as u64;
                            Some(renderer::MapFramePane {
                                source: renderer::MapFrameSource::Cpu {
                                    rgba: cpu.rgba,
                                    width: packet.projection.side,
                                    height: packet.projection.side,
                                },
                                viewport,
                                information: None,
                            })
                        }
                        super::PreparedMapSource::Cpu(_) => None,
                        super::PreparedMapSource::GpuAtlas(gpu) => {
                            gpu_overlay_hashes = Some([gpu.pre_grid_hash, gpu.post_grid_hash]);
                            Some(renderer::MapFramePane {
                                source: renderer::MapFrameSource::Gpu {
                                    params: &gpu.params,
                                    slots: &gpu.slots,
                                    uploads: &gpu.uploads,
                                    pre_grid_overlay: (overlay_hashes[0]
                                        != Some(gpu.pre_grid_hash))
                                    .then_some(gpu.pre_grid_rgba),
                                    post_grid_overlay: (overlay_hashes[1]
                                        != Some(gpu.post_grid_hash))
                                    .then_some(gpu.post_grid_rgba),
                                },
                                viewport,
                                information: None,
                            })
                        }
                    }
                });

                // The exact shared POV rectangle drives both projection and
                // pane-local color/depth sizing. Full-surface dimensions are
                // deliberately irrelevant here.
                let pov_params = pov_active.then(|| {
                    let camera = controller.pov_camera();
                    let shadow = pov_host::shadow_frame(
                        camera,
                        pov_chunks,
                        pov_organisms.shadow_bounds(),
                        pov_host::shadow_resolution(controller.world().tier()),
                    );
                    pov_host::frame_params(
                        camera,
                        layout
                            .pov_aspect
                            .expect("active POV mode has a resolved aspect"),
                        *pov_radius,
                        time,
                        controller.pov_toggles(),
                        shadow,
                    )
                });
                let organism_upload = frame.organisms_changed.then(|| pov_organisms.upload());
                let pov_pane = pov_params.as_ref().map(|params| {
                    let destination = layout
                        .pov_pane
                        .expect("active POV mode has a resolved pane");
                    renderer::PovFramePane {
                        frame: params,
                        uploads: &frame.uploads,
                        removes: &frame.removes,
                        organisms: organism_upload,
                        viewport: renderer::SurfaceViewport::new(
                            destination.x,
                            destination.y,
                            destination.width,
                            destination.height,
                        ),
                        information: None,
                        render_scale: frame.output.pov.render_scale,
                    }
                });

                let focus = split_active
                    .then(|| layout.focus_border(frame.output.focused))
                    .flatten()
                    .map(|destination| renderer::FocusDecoration {
                        viewport: renderer::SurfaceViewport::new(
                            destination.x,
                            destination.y,
                            destination.width,
                            destination.height,
                        ),
                        thickness: 2,
                    });

                // One call site owns the one acquire/submit/present attempt.
                // A CPU-only Map frame skips it so the hidden GPU stage does
                // not compete with the truthful 2D fallback presentation.
                let renderer_attempted =
                    slot.is_some() && (map_pane.is_some() || pov_pane.is_some());
                let render_result = if map_pane.is_some() || pov_pane.is_some() {
                    slot.as_mut()
                        .map_or_else(renderer::MultiViewFrameResult::default, |renderer| {
                            renderer.render_frame(renderer::MultiViewFrame {
                                clear: CLEAR_COLOR,
                                map: map_pane,
                                pov: pov_pane,
                                focus,
                            })
                        })
                } else {
                    renderer::MultiViewFrameResult::default()
                };

                let map_drawn = render_result.map_drawn;
                let schedule_map_retry = if let Some(hashes) = gpu_overlay_hashes {
                    if map_drawn {
                        *overlay_hashes = hashes.map(Some);
                        *gpu_map_retry_scheduled = false;
                        false
                    } else {
                        // Atlas keys were consumed before the failed unified
                        // presentation. Force complete tiles after recovery.
                        *atlas = super::AtlasManager::default();
                        *overlay_hashes = [None; 2];
                        super::claim_gpu_map_retry(gpu_map_retry_scheduled)
                    }
                } else {
                    false
                };
                (
                    prepared_map_backend,
                    prepared_map_fallback,
                    map_drawn,
                    if map_drawn {
                        render_result
                            .map_upload_bytes
                            .saturating_add(cpu_map_upload_bytes)
                    } else {
                        0
                    },
                    schedule_map_retry,
                    render_result.pov_drawn,
                    renderer_attempted,
                    render_result.presented,
                )
            });
            if schedule_map_retry {
                // Surface Outdated/Lost is repaired by the failed acquire;
                // request one bounded follow-up frame to present it without
                // turning persistent Timeout/Occluded into a busy loop.
                self.driver.dirty = true;
            }
            let map_path = if split_active {
                match prepared_map_backend {
                    super::MapBackend::Cpu => "gpu-cpu",
                    super::MapBackend::GpuAtlas => "gpu-atlas",
                }
            } else if map_drawn && prepared_map_backend == super::MapBackend::GpuAtlas {
                "gpu-atlas"
            } else {
                "cpu"
            };
            let map_backend_changed = if !map_active {
                false
            } else {
                match prepared_map_backend {
                    super::MapBackend::Cpu => self
                        .state
                        .record_map_result(super::MapBackend::Cpu, prepared_map_fallback),
                    super::MapBackend::GpuAtlas if map_drawn => self
                        .state
                        .record_map_result(super::MapBackend::GpuAtlas, None),
                    super::MapBackend::GpuAtlas => self.state.record_map_result(
                        super::MapBackend::Cpu,
                        Some(super::MapBackendFallback::GpuUnavailable),
                    ),
                }
            };
            let force_cpu_map_redraw = std::mem::take(&mut self.state.force_cpu_map_redraw);
            let map_dirty = frame.output.dirty.map
                || frame.presenter_dirty
                || map_backend_changed
                || force_cpu_map_redraw
                || (map_active
                    && prepared_map_backend == super::MapBackend::GpuAtlas
                    && !map_drawn)
                || (split_active && !map_drawn);
            let surface_losses = VIEWER_RENDERER.with(|slot| {
                slot.borrow()
                    .as_ref()
                    .map_or(0, renderer::Renderer::surface_losses)
            });
            self.state.set_surface_losses(surface_losses);
            let counters = self.state.pov_chunks.counters();
            let organism_counts = self.state.pov_organisms.counters();
            let presentation = self
                .state
                .presentation()
                .expect("the current frame installed presentation state");
            let layout = self.state.layout_value();
            let needs_frame = frame.output.needs_frame
                || frame.presenter_needs_frame
                || self.driver.needs_frame();
            let frame_document = serde_json::json!({
                "presentation": presentation,
                "layout": layout,
                "map_dirty": map_dirty,
                "hover_changed": frame.hover_changed,
                "needs_frame": needs_frame,
                "update_serial": frame.output.update_serial,
                "travel": frame.output.travel,
                "renderer_frame": {
                    "attempted": renderer_attempted,
                    "presented": surface_presented,
                },
                "map": {
                    "active": map_active,
                    "path": map_path,
                    "gpu_submitted": map_drawn
                        && prepared_map_backend == super::MapBackend::GpuAtlas,
                    "drawn": map_drawn,
                    "upload_bytes": map_upload_bytes,
                },
                "pov": {
                    "active": pov_active,
                    "rendered": pov_rendered,
                    "camera": frame.output.pov.position,
                    "orientation": [frame.output.pov.yaw, frame.output.pov.pitch],
                    "fly_speed": frame.output.pov.fly_speed,
                    "walk_speed": frame.output.pov.walk_speed,
                    "chunks": self.state.pov_chunks.len(),
                    "meshed": counters.meshed,
                    "uploads": frame.uploads.len(),
                    "hover_queries": self.state.pov_hover.geometry_queries(),
                    "organisms": {
                        "published": organism_counts.published,
                        "drawn": organism_counts.drawn(),
                        "waiting_for_ground": organism_counts.waiting_for_ground,
                    },
                },
            });
            Ok(JsValue::from_str(
                &serde_json::to_string(&frame_document)
                    .expect("browser frame document contains serializable finite values"),
            ))
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

        /// Return the visible pane at one physical surface point.
        ///
        /// JavaScript uses this shared-layout answer for uncaptured pointer
        /// and wheel events. A primary pointer capture preserves the press
        /// view until release so crossing the Split seam cannot retarget an
        /// in-progress POV drag.
        pub fn view_at(&self, x: f64, y: f64) -> Option<String> {
            self.state.view_at(x, y).map(|view| match view {
                super::ViewKind::Map => String::from("map"),
                super::ViewKind::Pov => String::from("pov"),
            })
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
            let handled = self.driver.handle(
                &mut self.state,
                super::NormalizedInputEvent::PointerMoved {
                    pointer: u64::from(pointer),
                    position: [x, y],
                    view,
                },
            );
            // A hover-only move still needs one frame so the CPU pick and
            // panel update run; continuous scheduling remains reserved for
            // held movement/drag and active presentation animation.
            Ok(handled || self.driver.has_continuous_input())
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
            let surface_format = VIEWER_RENDERER.with(|slot| {
                slot.borrow()
                    .as_ref()
                    .map(renderer::Renderer::surface_format_name)
            });
            self.state.set_surface_format(surface_format);
            self.state.set_renderer_webgpu();
            self.driver.enqueue_action(
                &mut self.state,
                super::ViewerAction::SetMapBackend(super::MapBackend::GpuAtlas),
            );
        }

        /// Report an actual device loss without forging an action.
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

        /// Report renderer initialization failure separately from a device
        /// that was successfully created and later lost.
        pub fn renderer_unavailable(&mut self) {
            VIEWER_RENDERER.with(|slot| {
                slot.borrow_mut().take();
            });
            self.state.renderer_unavailable();
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
                .map(|(x, y)| [x, y]);
            Ok(JsValue::from_str(&serde_json::to_string(&value).expect(
                "map projection returns finite serializable coordinates",
            )))
        }

        /// Set the shared cache-only map hover source. Sampling and organism
        /// precedence remain in `viewer_host::map_hover`; JavaScript forwards
        /// only the already-inverted world coordinate.
        pub fn map_hover(&mut self, world_x: f64, world_y: f64) -> bool {
            self.state.set_hover_world(Some((world_x, world_y)))
        }

        /// Clear hover when the pointer leaves Map content.
        pub fn clear_hover(&mut self) -> bool {
            self.state.set_hover_world(None)
        }

        /// Serialize the one cached shared panel document. Browser performance
        /// facts enter at this capped call site and invalidate only the panel
        /// cache; map/POV buffers and frame scheduling remain untouched.
        #[allow(clippy::too_many_arguments)]
        pub fn panel_document(
            &mut self,
            fps: u32,
            update_ms: f64,
            compose_ms: f64,
            present_ms: f64,
            upload_kib_per_frame: f64,
            dom_updates: u32,
        ) -> Result<JsValue, JsValue> {
            if self.shutdown {
                return Err(JsValue::from_str("WebApp is shut down"));
            }
            self.state.update_panel_telemetry(
                fps,
                update_ms,
                compose_ms,
                present_ms,
                upload_kib_per_frame,
                u64::from(dom_updates),
            );
            let json = self
                .state
                .panel_document_json()
                .map_err(|error| JsValue::from_str(&error.to_string()))?
                .ok_or_else(|| JsValue::from_str("panel unavailable before the first frame"))?;
            Ok(JsValue::from_str(&json))
        }

        /// Read-only cadence probe used by browser acceptance tests.
        pub fn panel_build_count(&self) -> u32 {
            self.state.panel_cache.builds().min(u64::from(u32::MAX)) as u32
        }

        /// Inject the exact result of the browser IndexedDB capability probe.
        /// This changes persistence/panel state only; it never schedules or
        /// advances a world frame.
        pub fn storage_status(&mut self, mode: String, failures: u32) -> Result<(), JsValue> {
            self.state
                .set_storage_status(&mode, failures)
                .map_err(|error| JsValue::from_str(&error))
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

    fn panel(app: &mut WebApp) -> String {
        app.panel_document(0, 0.0, 0.0, 0.0, 0.0, 0)
            .expect("panel document")
            .as_string()
            .expect("string panel document")
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
        assert!(panel(&mut button).contains("\"map_refinement\":true"));
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
        assert!(panel(&mut keyboard).contains("\"map_refinement\":true"));
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
        assert!(panel(&mut keyboard).contains("\"view\":{\"mode\":\"map\""));
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
        let frame: serde_json::Value = serde_json::from_str(&frame).expect("typed frame JSON");
        assert_eq!(frame["update_serial"], 1);
        assert_eq!(frame["travel"], 4.0);
        let document: serde_json::Value =
            serde_json::from_str(&panel(&mut app)).expect("typed panel JSON");
        assert_eq!(document["model"]["view"]["mode"], "pov");
        assert_eq!(document["model"]["view"]["camera"]["shadow_ao"], false);
        let traveler = document["model"]["frame"]["traveler"]
            .as_array()
            .expect("traveler array");
        assert!(traveler[0].as_f64().expect("traveler x").abs() < 5.0e-4);
        assert!((traveler[1].as_f64().expect("traveler y") - 4.0).abs() < 5.0e-4);
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

        let before_read = panel(&mut app);
        let reread = app.map_pixels().expect("repeat canonical CPU map");
        assert_eq!(
            panel(&mut app),
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
        assert!(panel(&mut app).contains("\"map_fallback\":\"gpu-unavailable\""));
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

    #[wasm_bindgen_test]
    fn split_facade_exposes_both_panes_and_routes_shared_hits() {
        let mut app = WebApp::new(JsValue::from_str("{}")).expect("web app");
        app.resize_surface(800, 600).expect("surface resize");
        // Node has no live renderer slot. The capability notification is
        // enough to exercise reducer/layout/input behavior while status stays
        // truthful about the absent surface attempt.
        app.renderer_available();
        app.action(
            String::from("set-presentation"),
            Some(String::from("split")),
        )
        .expect("Split action");
        app.surface_focus(true);

        let first: serde_json::Value = serde_json::from_str(
            &app.frame(0.0, 0.0)
                .expect("Split frame")
                .as_string()
                .expect("frame JSON"),
        )
        .expect("typed frame JSON");
        assert_eq!(first["presentation"]["view"]["mode"], "split");
        assert_eq!(first["presentation"]["view"]["focused"], "map");
        assert_eq!(first["map"]["active"], true);
        assert_eq!(first["pov"]["active"], true);
        assert_eq!(first["renderer_frame"]["attempted"], false);
        assert_eq!(first["renderer_frame"]["presented"], false);
        assert_eq!(first["map"]["path"], "gpu-cpu");
        assert_eq!(app.view_at(399.999, 300.0).as_deref(), Some("map"));
        assert_eq!(app.view_at(400.0, 300.0).as_deref(), Some("pov"));
        assert_eq!(app.view_at(800.0, 300.0), None);

        let view = app.view_at(600.0, 300.0).expect("POV pane hit");
        assert!(app
            .pointer_button(7, 0, true, 600.0, 300.0, view.clone())
            .expect("POV press"));
        assert!(app.wheel(1.0, true, view).expect("focused POV wheel"));
        let focused: serde_json::Value = serde_json::from_str(
            &app.frame(0.0, 0.1)
                .expect("focused Split frame")
                .as_string()
                .expect("frame JSON"),
        )
        .expect("typed focused frame JSON");
        assert_eq!(focused["presentation"]["view"]["mode"], "split");
        assert_eq!(focused["presentation"]["view"]["focused"], "pov");
        assert_eq!(focused["layout"]["focused"], "pov");
        assert_eq!(
            focused["layout"]["focus_border"],
            focused["layout"]["pov_pane"]
        );
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

    #[test]
    fn browser_startup_config_is_exact_typed_json() {
        let state = super::BrowserHostState::new(
            r#"{"tier":"high","storage":true,"worker_mode":"shared-workers"}"#,
        )
        .expect("exact startup config");
        assert_eq!(
            state.controller.world().tier(),
            world_runtime::ResourceTier::High
        );
        assert_eq!(state.storage, "indexeddb-pending");
        assert_eq!(state.worker_mode, "shared-memory");

        for invalid in [
            r#"{"tier":"HIGH"}"#,
            r#"{"tier":"midpoint"}"#,
            r#"{"storage":"true"}"#,
            r#"{"worker_mode":"worker"}"#,
            r#"{"unknown":true}"#,
            r#"prefix {"tier":"high"}"#,
        ] {
            assert!(
                super::BrowserHostState::new(invalid).is_err(),
                "substring-shaped config must be rejected: {invalid}"
            );
        }
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
        let help_script = include_str!("../web/assets/help.js");
        let controls = attribute_values(index, "data-action");
        let descriptors: serde_json::Value =
            serde_json::from_str(&super::action_descriptors_json()).expect("descriptor JSON");
        let documented = descriptors
            .as_array()
            .expect("action descriptor array")
            .iter()
            .map(|descriptor| descriptor["id"].as_str().expect("action descriptor id"))
            .collect::<Vec<_>>();
        assert!(!controls.is_empty());
        for id in &controls {
            assert!(
                viewer_host::action::action_descriptor(id).is_some(),
                "unknown browser control action {id}"
            );
            assert!(documented.contains(id), "shared help omits control {id}");
        }
        for descriptor in viewer_host::action::ACTION_DESCRIPTORS {
            assert!(
                documented.contains(&descriptor.id.as_str()),
                "browser help omits descriptor {}",
                descriptor.id.as_str()
            );
        }
        for id in &documented {
            assert!(
                viewer_host::action::action_descriptor(id).is_some(),
                "help documents unknown action {id}"
            );
        }
        assert!(help.contains("data-generated-help"));
        assert!(!help.contains("data-help-action="));
        assert!(help_script.contains("viewer_action_descriptors()"));
        assert!(help_script.contains("row.dataset.helpAction = descriptor.id"));
        let app = include_str!("../web/assets/app.js");
        assert!(!app.contains("MOVE_KEYS"));
        assert!(!app.contains("POV_MOVE"));
        assert!(!app.contains("requestPointerLock"));
        assert!(app.contains("app.frame(dt, now / 1000)"));
        assert!(app.contains("app.map_descriptors()"));
        assert!(app.contains("installMapControls"));
        assert!(app.contains("app.panel_document("));
        assert!(app.contains("applyPanelDocument"));
        assert!(app.contains("app.map_hover(wx, wy)"));
        assert!(app.contains("app.clear_hover()"));
        assert!(!app.contains("MAP_CHANNELS"));
        assert!(!app.contains("paint_region"));
        assert!(!app.contains("compose_map"));
        assert!(!app.contains("app.info_snapshot("));
        assert!(!app.contains("app.inspect("));
        assert!(!app.contains("app.map_organism_at("));
        assert!(!app.contains("updatePanelStats"));
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
        assert_eq!(
            super::BrowserHostState::default().persistence_info().mode,
            "memory"
        );
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
        let json: serde_json::Value =
            serde_json::from_str(&state.layout_json()).expect("typed layout JSON");
        assert_eq!(json["map_content"], serde_json::json!([100, 0, 701, 701]));
    }

    #[test]
    fn browser_split_hit_routing_uses_the_shared_physical_layout() {
        let mut state = small_controller_app();
        state.resize_surface(901, 701);
        state.set_renderer_webgpu();
        state.enqueue_action(viewer_host::ViewerAction::SetPresentation(
            viewer_host::PresentationMode::Split,
        ));
        let frame = state.frame(0.0, viewer_host::input::InputFrame::default());
        assert_eq!(frame.output.mode, viewer_host::PresentationMode::Split);
        assert_eq!(frame.output.focused, viewer_host::ViewKind::Map);

        let layout = state.resolved_layout();
        let map = layout.map_pane.expect("Split Map pane");
        let pov = layout.pov_pane.expect("Split POV pane");
        assert_eq!(map.right(), pov.x);
        assert_eq!(
            state.view_at(f64::from(map.right()) - 0.001, 1.0),
            Some(viewer_host::ViewKind::Map)
        );
        assert_eq!(
            state.view_at(f64::from(pov.x), 1.0),
            Some(viewer_host::ViewKind::Pov)
        );
        assert_eq!(state.view_at(f64::from(layout.content.right()), 1.0), None);
        let json: serde_json::Value =
            serde_json::from_str(&state.layout_json()).expect("typed layout JSON");
        assert_eq!(json["focused"], "map");
        assert_eq!(json["split_ratio"], 0.5);
        assert_eq!(json["focus_border"], serde_json::json!([0, 0, 451, 701]));
    }

    #[test]
    fn split_pointer_focus_previews_scoped_wheel_before_the_frame() {
        use viewer_host::input::{ButtonPhase, NormalizedInputEvent, PointerButton, WheelDelta};

        let mut state = small_controller_app();
        state.resize_surface(800, 600);
        state.set_renderer_webgpu();
        state.enqueue_action(viewer_host::ViewerAction::SetPresentation(
            viewer_host::PresentationMode::Split,
        ));
        let _ = state.frame(0.0, viewer_host::input::InputFrame::default());
        let mut driver = super::BrowserViewerDriver::default();
        driver.set_surface_focus(&state, true);
        let position = [600.0, 300.0];
        let view = state.view_at(position[0], position[1]).expect("POV hit");

        assert!(driver.handle(
            &mut state,
            NormalizedInputEvent::PointerButton {
                pointer: 9,
                button: PointerButton::Primary,
                phase: ButtonPhase::Pressed,
                position,
                view,
            },
        ));
        assert!(driver.handle(
            &mut state,
            NormalizedInputEvent::Wheel {
                delta: WheelDelta::Lines(1.0),
                view,
            },
        ));
        let input = driver.take_frame(&mut state);
        assert_eq!(
            input.wheel_steps, 1,
            "same-batch wheel uses previewed POV focus"
        );
        let frame = state.frame(0.0, input);
        assert_eq!(frame.output.mode, viewer_host::PresentationMode::Split);
        assert_eq!(frame.output.focused, viewer_host::ViewKind::Pov);
        assert_eq!(state.controller.world().update_serial(), 2);
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
        state.set_hover_world(Some((0.0, 0.0)));
        let _ = state
            .panel_document_json()
            .expect("serialize panel")
            .expect("panel after a frame");
        let presentation = state.presentation().expect("presentation after a frame");
        let _ = serde_json::to_string(&presentation).expect("serialize presentation");
        assert_eq!(state.controller.world().update_serial(), serial);
    }

    #[test]
    fn panel_document_cache_rebuilds_only_for_typed_inputs() {
        let mut state = small_controller_app();
        let _ = state.frame(0.0, viewer_host::input::InputFrame::default());
        assert_eq!(state.panel_cache.builds(), 0);

        let first = state
            .panel_document_json()
            .expect("serialize first panel")
            .expect("panel after a frame");
        assert_eq!(state.panel_cache.builds(), 1);
        let repeated = state
            .panel_document_json()
            .expect("serialize repeated panel")
            .expect("cached panel");
        assert_eq!(repeated, first);
        assert_eq!(state.panel_cache.builds(), 1);

        assert!(state.update_panel_telemetry(60, 1.0, 2.0, 3.0, 4.0, 5));
        let telemetry = state
            .panel_document_json()
            .expect("serialize telemetry panel")
            .expect("telemetry panel");
        assert_eq!(state.panel_cache.builds(), 2);
        assert!(telemetry.contains("\"performance.dom-updates\""));
        assert!(telemetry.contains("\"value\":\"5\""));
        assert!(!state.update_panel_telemetry(60, 1.0, 2.0, 3.0, 4.0, 5));
        let _ = state
            .panel_document_json()
            .expect("serialize unchanged telemetry panel");
        assert_eq!(state.panel_cache.builds(), 2);

        let world_serial = state.controller.world().update_serial();
        state
            .set_storage_status("indexeddb", 0)
            .expect("known storage mode");
        let persistence = state
            .panel_document_json()
            .expect("serialize persistence panel")
            .expect("persistence panel");
        assert_eq!(state.panel_cache.builds(), 3);
        assert!(persistence.contains("\"available\":true"));
        assert_eq!(state.controller.world().update_serial(), world_serial);
        assert!(state.set_storage_status("unknown", 0).is_err());
    }

    #[test]
    fn browser_hover_uses_the_shared_fixed_panel_schema() {
        let mut state = small_controller_app();
        settle(&mut state);
        assert!(state.set_hover_world(Some((10.0, 10.0))));
        assert!(!state.set_hover_world(Some((10.0, 10.0))));
        let terrain: serde_json::Value = serde_json::from_str(
            &state
                .panel_document_json()
                .expect("serialize terrain hover")
                .expect("terrain hover panel"),
        )
        .expect("typed terrain document");
        assert_eq!(terrain["model"]["hover"]["kind"], "terrain");
        let terrain_fields = terrain["sections"]
            .as_array()
            .expect("sections")
            .iter()
            .find(|section| section["id"] == "hover")
            .expect("hover section")["fields"]
            .as_array()
            .expect("hover fields");
        assert!(terrain_fields
            .iter()
            .any(|field| field["id"] == "hover.terrain.status" && field["visible"] == true));
        assert!(terrain_fields
            .iter()
            .any(|field| field["id"] == "hover.organism.id" && field["visible"] == false));

        assert!(state.set_hover_world(None));
        let none: serde_json::Value = serde_json::from_str(
            &state
                .panel_document_json()
                .expect("serialize cleared hover")
                .expect("cleared hover panel"),
        )
        .expect("typed cleared document");
        assert_eq!(none["model"]["hover"]["kind"], "none");
        assert_eq!(state.panel_cache.builds(), 2);
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
    fn pov_pointer_updates_shared_hover_after_input_and_reuses_steady_geometry() {
        let mut state = small_controller_app();
        state.resize_surface(400, 300);
        settle(&mut state);
        state.set_renderer_webgpu();
        state.enqueue_action(viewer_host::ViewerAction::SetPresentation(
            viewer_host::PresentationMode::Pov,
        ));

        let pointer = [200.0, 260.0];
        let mut frame = state.frame_at(
            0.0,
            viewer_host::input::InputFrame {
                pov_pointer: Some(pointer),
                ..viewer_host::input::InputFrame::default()
            },
            1.0,
        );
        for _ in 0..12 {
            if state.pov_chunks.is_idle() {
                break;
            }
            frame = state.frame_at(
                0.0,
                viewer_host::input::InputFrame {
                    pov_pointer: Some(pointer),
                    ..viewer_host::input::InputFrame::default()
                },
                1.0,
            );
        }
        assert_eq!(frame.output.mode, viewer_host::PresentationMode::Pov);
        assert!(matches!(
            state.pov_hover.hover(),
            viewer_host::HoverInfo::Terrain(_) | viewer_host::HoverInfo::Organism(_)
        ));
        let panel: serde_json::Value = serde_json::from_str(
            &state
                .panel_document_json()
                .expect("serialize POV hover")
                .expect("POV panel after frame"),
        )
        .expect("typed POV panel");
        assert_ne!(panel["model"]["hover"]["kind"], "none");

        let camera = state.controller.pov_camera();
        let orientation = (camera.yaw.to_bits(), camera.pitch.to_bits());
        let queries = state.pov_hover.geometry_queries();
        let unchanged = state.frame_at(
            0.0,
            viewer_host::input::InputFrame {
                pov_pointer: Some(pointer),
                ..viewer_host::input::InputFrame::default()
            },
            7.0,
        );
        assert!(!unchanged.hover_changed);
        assert_eq!(state.pov_hover.geometry_queries(), queries);
        let camera = state.controller.pov_camera();
        assert_eq!(
            (camera.yaw.to_bits(), camera.pitch.to_bits()),
            orientation,
            "unheld hover movement never becomes camera look"
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

        state.set_hover_world(Some((10.0, 10.0)));
        let document: serde_json::Value = serde_json::from_str(
            &state
                .panel_document_json()
                .expect("serialize panel")
                .expect("panel after settle"),
        )
        .expect("typed panel JSON");
        assert!(document["model"]["streaming"]["stats"]["active_regions"]
            .as_u64()
            .is_some_and(|regions| regions > 0));
        assert_eq!(document["model"]["hover"]["value"]["status"], "ready");
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
            viewer_host::PresentationMode::Split,
        ));
        let unavailable = state.frame(0.0, viewer_host::input::InputFrame::default());
        assert_eq!(unavailable.output.mode, viewer_host::PresentationMode::Map);
        assert!(state
            .panel_document_json()
            .expect("serialize unavailable panel")
            .expect("panel after unavailable frame")
            .contains("POV is unavailable"));

        state.set_renderer_webgpu();
        state.enqueue_action(viewer_host::ViewerAction::SetPresentation(
            viewer_host::PresentationMode::Split,
        ));
        let available = state.frame(0.0, viewer_host::input::InputFrame::default());
        assert_eq!(available.output.mode, viewer_host::PresentationMode::Split);
        assert!(!state.pov_chunks.is_empty());
        assert!(state.pov_organisms.visual_generation() > 0);
        let serial = state.controller.world().update_serial();
        let traveler = state.controller.world().traveler().position;

        state.renderer_lost();
        assert!(state.pov_chunks.is_empty());
        assert_eq!(state.pov_organisms.visual_generation(), 0);
        assert!(
            state.force_cpu_map_redraw,
            "GPU loss must invalidate an otherwise clean CPU fallback frame"
        );
        assert_eq!(
            state.controller.layout().mode,
            viewer_host::PresentationMode::Split,
            "a platform callback cannot mutate the controller between ticks"
        );
        assert_eq!(state.controller.world().update_serial(), serial);
        let fallback = state.frame(0.0, viewer_host::input::InputFrame::default());
        assert_eq!(fallback.output.update_serial, serial + 1);
        assert_eq!(
            state.controller.layout().mode,
            viewer_host::PresentationMode::Map
        );
        assert_eq!(
            state.controller.layout().focused,
            viewer_host::ViewKind::Map
        );
        assert_eq!(state.controller.world().traveler().position, traveler);
        assert!(!state.controller.pov_state().supported);
        assert!(fallback.output.effects.iter().any(|effect| matches!(
            effect,
            viewer_host::ViewerEffect::ReportWarning(warning)
                if warning.id == "renderer-device-loss"
        )));
        assert_eq!(state.renderer, "cpu-fallback");
        assert_eq!(state.device_losses, 1);

        state.set_renderer_webgpu();
        let recovered = state.frame(0.0, viewer_host::input::InputFrame::default());
        assert_eq!(recovered.output.mode, viewer_host::PresentationMode::Map);
        state.enqueue_action(viewer_host::ViewerAction::SetPresentation(
            viewer_host::PresentationMode::Split,
        ));
        let reentered = state.frame(0.0, viewer_host::input::InputFrame::default());
        assert_eq!(reentered.output.mode, viewer_host::PresentationMode::Split);
        assert!(
            !state.pov_chunks.is_empty(),
            "replacement renderer must receive a complete terrain publication"
        );
        assert!(
            state.pov_organisms.visual_generation() > 0,
            "replacement renderer must receive a complete organism publication"
        );

        let mut unavailable = small_controller_app();
        unavailable.renderer_unavailable();
        assert_eq!(unavailable.device_losses, 0);
        let fallback = unavailable.frame(0.0, viewer_host::input::InputFrame::default());
        assert!(fallback.output.effects.iter().any(|effect| matches!(
            effect,
            viewer_host::ViewerEffect::ReportWarning(warning)
                if warning.id == "renderer-map-fallback"
        )));
    }
}
