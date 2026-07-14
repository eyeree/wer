//! Semantic inspection values shared by map and CPU-side POV picking.
//!
//! Sampling is deliberately cache-only: pointer movement may describe the
//! resident presentation state, but it never generates terrain, advances the
//! world, or becomes an identity authority (ADR 0028 and
//! `native-web-alignment.md` section 5.7).

use pov_host::{PovCamera, PovChunkManager, PovOrganismManager, PovSceneHit};
use world_core::{Biome, HabitatSignature, LocalPos, RegionCoord, Trophic, REGION_SIZE};
use world_runtime::{
    GenerationStatus, Organism, RegionMap, CHANNEL_CANOPY, CHANNEL_ELEVATION, CHANNEL_FERTILITY,
    CHANNEL_HARDNESS, CHANNEL_MOISTURE, CHANNEL_RIVER, CHANNEL_SOIL_DEPTH, CHANNEL_TEMPERATURE,
    CHANNEL_VEGETATION, CHANNEL_WETNESS,
};

use crate::layout::PixelRect;

/// Streaming/generation state reported for an inspected cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CellStatus {
    /// The region is outside the resident set.
    NotResident,
    /// Authoritative state exists without an admitted field working set.
    Unloaded,
    /// One or more field layers are pending.
    Generating,
    /// Required field layers are current.
    Ready,
}

impl CellStatus {
    /// Stable display label inherited from the native information panel.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotResident => "not resident",
            Self::Unloaded => "unloaded",
            Self::Generating => "generating",
            Self::Ready => "ready",
        }
    }
}

impl From<Option<GenerationStatus>> for CellStatus {
    fn from(status: Option<GenerationStatus>) -> Self {
        match status {
            None => Self::NotResident,
            Some(GenerationStatus::Unloaded) => Self::Unloaded,
            Some(GenerationStatus::Generating) => Self::Generating,
            Some(GenerationStatus::Ready) => Self::Ready,
        }
    }
}

/// Aggregate ecology facts at an inspected cell.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EcologyInfo {
    /// Quantized habitat inputs that key the roster and food web.
    pub signature: HabitatSignature,
    /// Species in the habitat roster.
    pub roster_size: usize,
    /// Index of the dominant species within that roster.
    pub dominant_index: u16,
    /// Dominant species id.
    #[serde(serialize_with = "serialize_hex_u64")]
    pub dominant_id: u64,
    /// Producer/herbivore/omnivore/carnivore/decomposer counts.
    pub trophic_counts: [usize; 5],
    /// Aggregate herbivore pressure.
    pub herbivore: Option<f32>,
    /// Aggregate predator pressure.
    pub predator: Option<f32>,
    /// Aggregate species diversity.
    pub diversity: Option<f32>,
}

/// Terrain, climate, soil, biome, and ecology data for one cell.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CellInfo {
    /// Continuous world position sampled.
    pub world: (f64, f64),
    /// Region containing the sample.
    pub region: RegionCoord,
    /// Quantized cell within the region.
    pub cell: LocalPos,
    /// Region pipeline state.
    pub status: CellStatus,
    /// Streaming stability. Non-resident samples report zero.
    pub stability: f32,
    /// Realized-state revision. Non-resident samples report zero.
    pub revision: u32,
    /// Elevation.
    pub elevation: Option<f32>,
    /// Temperature.
    pub temperature: Option<f32>,
    /// Moisture.
    pub moisture: Option<f32>,
    /// Rock hardness.
    pub hardness: Option<f32>,
    /// River expression.
    pub river: Option<f32>,
    /// Surface wetness.
    pub wetness: Option<f32>,
    /// Soil depth.
    pub soil_depth: Option<f32>,
    /// Soil fertility.
    pub fertility: Option<f32>,
    /// Vegetation density.
    pub vegetation: Option<f32>,
    /// Canopy height.
    pub canopy: Option<f32>,
    /// Stable biome display name, when generated.
    pub biome: Option<&'static str>,
    /// Aggregate ecology, when generated.
    pub ecology: Option<EcologyInfo>,
}

/// A realized presentation organism under the pointer.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct OrganismInfo {
    /// Stable instance identity.
    #[serde(serialize_with = "serialize_hex_u64")]
    pub id: u64,
    /// Density slot that produced this presentation instance.
    pub slot: u16,
    /// Cell within the organism's region.
    pub cell: LocalPos,
    /// Species identity.
    #[serde(serialize_with = "serialize_hex_u64")]
    pub species: u64,
    /// Stable trophic role.
    pub trophic: Trophic,
    /// Jittered XY world position.
    pub world: (f64, f64),
    /// Morphology archetype in `0..=15`.
    pub form: u8,
    /// Expressed hue.
    pub hue: f32,
    /// Expressed bioluminance.
    pub luminance: f32,
    /// Expressed body size.
    pub size: f32,
    /// Expressed activity.
    pub activity: f32,
    /// Expressed aggression.
    pub aggression: f32,
}

/// Nearest visible semantic object under a pointer.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum HoverInfo {
    /// Pointer is outside a pane or over sky/missing geometry.
    None,
    /// Terrain/cell information.
    Terrain(CellInfo),
    /// Realized organism information.
    Organism(OrganismInfo),
}

/// Exact presentation inputs that can change a CPU-side POV geometry query.
///
/// The world update serial is deliberately absent. Resident terrain and
/// renderer-ready organisms expose narrower generations, so a continuous POV
/// frame does not repeat the ray query merely because unrelated simulation or
/// telemetry advanced (`native-web-alignment.md` section 5.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PovHoverGeometryKey {
    pointer: [u64; 2],
    pane: PixelRect,
    camera_position: [u64; 3],
    camera_orientation: [u32; 2],
    terrain_generation: u64,
    organism_generation: u64,
    fog_end: u64,
    animation_time: u32,
}

impl PovHoverGeometryKey {
    fn new(
        pointer: [f64; 2],
        pane: PixelRect,
        camera: &PovCamera,
        chunks: &PovChunkManager,
        organisms: &PovOrganismManager,
        fog_end: f64,
        time_seconds: f32,
    ) -> Self {
        Self {
            pointer: pointer.map(f64::to_bits),
            pane,
            camera_position: [camera.pos.x, camera.pos.y, camera.pos.z].map(f64::to_bits),
            camera_orientation: [camera.yaw, camera.pitch].map(f32::to_bits),
            terrain_generation: chunks.resident_generation(),
            organism_generation: organisms.visual_generation(),
            fog_end: fog_end.to_bits(),
            // Static visuals must not turn an animated presentation clock into
            // an unconditional per-frame ray query. Once a nonzero bob exists,
            // its wrapped frame time is part of the geometry actually drawn.
            animation_time: if organisms.has_animated_visuals() {
                time_seconds.to_bits()
            } else {
                0
            },
        }
    }
}

/// Cached CPU-side POV picking plus the current shared semantic hover value.
///
/// Geometry is recomputed only when its exact presentation inputs change.
/// [`HoverInfo`] is nevertheless rebuilt on every [`Self::update`] so a
/// resident cell's generation status, ecology, or other information can
/// advance without forcing an otherwise-identical ray/triangle query.
pub struct PovHoverCache {
    geometry_key: Option<PovHoverGeometryKey>,
    hit: Option<PovSceneHit>,
    hover: HoverInfo,
    geometry_queries: u64,
}

impl std::fmt::Debug for PovHoverCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PovHoverCache")
            .field("geometry_key", &self.geometry_key)
            .field("hover", &self.hover)
            .field("geometry_queries", &self.geometry_queries)
            .finish_non_exhaustive()
    }
}

impl Default for PovHoverCache {
    fn default() -> Self {
        Self {
            geometry_key: None,
            hit: None,
            hover: HoverInfo::None,
            geometry_queries: 0,
        }
    }
}

impl PovHoverCache {
    /// Construct an empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Update the cached POV hit and rebuild its semantic information.
    ///
    /// `surface_pointer` is expressed in the same physical surface pixels as
    /// `pov_pane`. A missing pane/pointer, a point outside the half-open pane,
    /// or an invalid camera ray resolves to [`HoverInfo::None`] without
    /// querying resident geometry. `true` means the semantic hover value
    /// changed, which shells use for immediate panel invalidation.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        map: &RegionMap,
        camera: &PovCamera,
        chunks: &PovChunkManager,
        organisms: &PovOrganismManager,
        surface_pointer: Option<[f64; 2]>,
        pov_pane: Option<PixelRect>,
        fog_end: f64,
        time_seconds: f32,
    ) -> bool {
        let routed = surface_pointer.zip(pov_pane).and_then(|(pointer, pane)| {
            pane.local_point(pointer)
                .map(|local| (pointer, pane, local))
        });

        let next_key = routed.map(|(pointer, pane, _)| {
            PovHoverGeometryKey::new(
                pointer,
                pane,
                camera,
                chunks,
                organisms,
                fog_end,
                time_seconds,
            )
        });
        if self.geometry_key != next_key {
            self.geometry_key = next_key;
            self.hit = routed
                .and_then(|(_, pane, local)| camera.screen_ray(local, [pane.width, pane.height]))
                .and_then(|ray| {
                    self.geometry_queries = self.geometry_queries.saturating_add(1);
                    pov_host::raycast_scene(chunks, organisms, ray, time_seconds, fog_end)
                });
        }

        let next_hover = pov_scene_hover(map, self.hit.as_ref());
        let changed = self.hover != next_hover;
        self.hover = next_hover;
        changed
    }

    /// Current semantic information under the POV pointer.
    #[must_use]
    pub const fn hover(&self) -> &HoverInfo {
        &self.hover
    }

    /// Number of resident geometry queries performed since construction.
    ///
    /// This excludes missing/outside pointers and semantic-only refreshes, so
    /// tests and shell telemetry can verify steady-state hover caching without
    /// conflating it with ordinary panel sampling.
    #[must_use]
    pub const fn geometry_queries(&self) -> u64 {
        self.geometry_queries
    }
}

fn pov_scene_hover(map: &RegionMap, hit: Option<&PovSceneHit>) -> HoverInfo {
    match hit {
        None => HoverInfo::None,
        Some(PovSceneHit::Terrain(hit)) => {
            HoverInfo::Terrain(sample_cell(map, (hit.position.x, hit.position.y)))
        }
        Some(PovSceneHit::Organism(hit)) => HoverInfo::Organism(organism_info(&hit.organism)),
    }
}

fn serialize_hex_u64<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&format!("{value:016x}"))
}

/// Read one world cell from the resident cache without advancing generation.
///
/// The quantization and absent-state defaults preserve the native panel's
/// pre-extraction behavior exactly. In particular, a resident parked region is
/// distinct from a position with no authoritative region at all.
#[must_use]
pub fn sample_cell(map: &RegionMap, world: (f64, f64)) -> CellInfo {
    let region = RegionCoord::from_world(world.0, world.1);
    let state = map.get(region);
    let status = CellStatus::from(state.map(|state| state.status));
    let (stability, revision) = state.map_or((0.0, 0), |state| (state.stability, state.revision));

    let resolution = map.config().field_resolution;
    let (origin_x, origin_y) = region.origin();
    let cell_size = REGION_SIZE / f64::from(resolution);
    // World-to-region quantization normally makes both local components
    // non-negative. Retaining the native saturating float-to-u16 cast plus the
    // upper clamp makes the operation total at representable edge positions.
    let cx = (((world.0 - origin_x) / cell_size) as u16).min(resolution - 1);
    let cy = (((world.1 - origin_y) / cell_size) as u16).min(resolution - 1);
    let cell = LocalPos::new(cx, cy);
    let sample = |channel: usize| {
        map.cache()
            .channel(region, channel)
            .map(|tile| tile.get(cx, cy))
    };

    CellInfo {
        world,
        region,
        cell,
        status,
        stability,
        revision,
        elevation: sample(CHANNEL_ELEVATION),
        temperature: sample(CHANNEL_TEMPERATURE),
        moisture: sample(CHANNEL_MOISTURE),
        hardness: sample(CHANNEL_HARDNESS),
        river: sample(CHANNEL_RIVER),
        wetness: sample(CHANNEL_WETNESS),
        soil_depth: sample(CHANNEL_SOIL_DEPTH),
        fertility: sample(CHANNEL_FERTILITY),
        vegetation: sample(CHANNEL_VEGETATION),
        canopy: sample(CHANNEL_CANOPY),
        biome: map
            .cache()
            .biome(region)
            .map(|tile| Biome::from_id(tile.get(cx, cy)).name()),
        ecology: map.cell_ecology(region, cx, cy).map(|ecology| EcologyInfo {
            signature: ecology.signature,
            roster_size: ecology.roster.roster.species.len(),
            dominant_index: ecology.dominant_index,
            dominant_id: ecology.dominant_id,
            trophic_counts: ecology.trophic_counts,
            herbivore: ecology.herbivore,
            predator: ecology.predator,
            diversity: ecology.diversity,
        }),
    }
}

/// Convert one runtime presentation organism into the shared information
/// model without changing or narrowing any stable identity.
#[must_use]
pub const fn organism_info(organism: &Organism) -> OrganismInfo {
    OrganismInfo {
        id: organism.id,
        slot: organism.slot,
        cell: organism.cell,
        species: organism.species,
        trophic: organism.trophic,
        world: organism.world_pos,
        form: organism.expressed.form,
        hue: organism.expressed.hue,
        luminance: organism.expressed.luminance,
        size: organism.expressed.size,
        activity: organism.expressed.activity,
        aggression: organism.expressed.aggression,
    }
}

/// Pick and convert the nearest inspectable top-down organism marker.
///
/// Marker eligibility and nearest-hit ordering remain owned by the canonical
/// map presenter; this function makes its result use the same semantic shape
/// as future POV inspection.
#[must_use]
pub fn pick_map_organism_info(
    map: &RegionMap,
    world: (f64, f64),
    zoom: u32,
) -> Option<OrganismInfo> {
    crate::map::pick_organism(map, world, zoom).map(organism_info)
}

/// Resolve the top-down hover model with organism markers taking precedence
/// over the terrain cell beneath them, matching the established native panel.
#[must_use]
pub fn map_hover(map: &RegionMap, world: Option<(f64, f64)>, zoom: u32) -> HoverInfo {
    let Some(world) = world else {
        return HoverInfo::None;
    };
    pick_map_organism_info(map, world, zoom).map_or_else(
        || HoverInfo::Terrain(sample_cell(map, world)),
        HoverInfo::Organism,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use world_core::{Expressed, PossibilityField, Trophic, POSSIBILITY_DIMS};
    use world_runtime::{Budget, InlineExecutor, StreamConfig};

    const PLAYER: (f64, f64) = (0.0, 0.0);
    const NO_BIAS: [f32; POSSIBILITY_DIMS] = [0.0; POSSIBILITY_DIMS];

    fn config() -> StreamConfig {
        StreamConfig {
            near_radius: 1.5 * REGION_SIZE,
            far_radius: 3.0 * REGION_SIZE,
            load_radius: 3.0 * REGION_SIZE,
            unload_radius: 4.0 * REGION_SIZE,
            field_resolution: 8,
            ..StreamConfig::default()
        }
    }

    fn update(map: &mut RegionMap, budget: &Budget) {
        map.update(
            PLAYER,
            0.0,
            &PossibilityField::default(),
            &[],
            &NO_BIAS,
            budget,
            &InlineExecutor,
            false,
        );
    }

    fn settled_map() -> RegionMap {
        let mut map = RegionMap::new(config());
        for _ in 0..6 {
            update(&mut map, &Budget::unlimited());
        }
        map
    }

    #[test]
    fn runtime_generation_states_are_typed_and_distinct() {
        assert_eq!(CellStatus::from(None), CellStatus::NotResident);
        assert_eq!(CellStatus::NotResident.as_str(), "not resident");
        assert_eq!(CellStatus::Unloaded.as_str(), "unloaded");
        assert_eq!(CellStatus::Generating.as_str(), "generating");
        assert_eq!(CellStatus::Ready.as_str(), "ready");

        let absent = sample_cell(&RegionMap::new(config()), PLAYER);
        assert_eq!(absent.status, CellStatus::NotResident);
        assert_eq!((absent.stability, absent.revision), (0.0, 0));
        assert!(absent.elevation.is_none());

        let mut parked_config = config();
        parked_config.near_radius = 0.0;
        parked_config.max_field_cache_bytes = 0;
        let mut parked = RegionMap::new(parked_config);
        update(&mut parked, &Budget::unlimited());
        let unloaded = sample_cell(&parked, PLAYER);
        assert_eq!(unloaded.status, CellStatus::Unloaded);
        assert!(unloaded.elevation.is_none());

        let mut generating = RegionMap::new(config());
        update(
            &mut generating,
            &Budget {
                max_regen_cost: 0,
                ..Budget::unlimited()
            },
        );
        assert_eq!(
            sample_cell(&generating, PLAYER).status,
            CellStatus::Generating
        );

        let ready = sample_cell(&settled_map(), PLAYER);
        assert_eq!(ready.status, CellStatus::Ready);
        assert!(ready.elevation.is_some());
    }

    #[test]
    fn cell_quantization_is_half_open_across_negative_and_positive_boundaries() {
        let map = RegionMap::new(config());
        let epsilon = f64::EPSILON * REGION_SIZE;
        let cases = [
            ((0.0, 0.0), RegionCoord::new(0, 0), LocalPos::new(0, 0)),
            (
                (REGION_SIZE - epsilon, REGION_SIZE - epsilon),
                RegionCoord::new(0, 0),
                LocalPos::new(7, 7),
            ),
            (
                (REGION_SIZE, REGION_SIZE),
                RegionCoord::new(1, 1),
                LocalPos::new(0, 0),
            ),
            (
                (-epsilon, -epsilon),
                RegionCoord::new(-1, -1),
                LocalPos::new(7, 7),
            ),
            (
                (-REGION_SIZE, -REGION_SIZE),
                RegionCoord::new(-1, -1),
                LocalPos::new(0, 0),
            ),
        ];
        for (world, expected_region, expected_cell) in cases {
            let sampled = sample_cell(&map, world);
            assert_eq!(sampled.region, expected_region, "world {world:?}");
            assert_eq!(sampled.cell, expected_cell, "world {world:?}");
        }
    }

    #[test]
    fn organism_conversion_preserves_large_ids_slot_cell_form_and_expression() {
        let source = Organism {
            id: 0xFEDC_BA98_7654_3210,
            species: 0xF123_4567_89AB_CDEF,
            trophic: Trophic::Decomposer,
            slot: 3,
            cell: LocalPos::new(7, 5),
            world_pos: (-12.5, 900.25),
            expressed: Expressed {
                hue: 0.125,
                luminance: 0.25,
                size: 1.75,
                activity: 0.625,
                aggression: 0.875,
                form: 15,
            },
        };
        let info = organism_info(&source);
        assert!(info.id > (1_u64 << 53));
        assert!(info.species > (1_u64 << 53));
        assert_eq!(info.id, source.id);
        assert_eq!(info.species, source.species);
        assert_eq!(info.slot, 3);
        assert_eq!(info.cell, LocalPos::new(7, 5));
        assert_eq!(info.trophic, Trophic::Decomposer);
        assert_eq!(info.world, source.world_pos);
        assert_eq!(info.form, 15);
        assert_eq!(info.hue.to_bits(), source.expressed.hue.to_bits());
        assert_eq!(
            info.luminance.to_bits(),
            source.expressed.luminance.to_bits()
        );
        assert_eq!(info.size.to_bits(), source.expressed.size.to_bits());
        assert_eq!(info.activity.to_bits(), source.expressed.activity.to_bits());
        assert_eq!(
            info.aggression.to_bits(),
            source.expressed.aggression.to_bits()
        );
    }

    /// The M0 fixture's exact source values are the extraction oracle. This
    /// intentionally compares bits rather than formatted panel pixels.
    #[test]
    fn shared_inspection_preserves_native_characterization_values() {
        let map = settled_map();
        let source = map
            .organisms()
            .min_by_key(|organism| (organism.id, organism.slot))
            .copied()
            .expect("settled characterization map has organisms");
        let cell = sample_cell(&map, source.world_pos);
        let organism = pick_map_organism_info(&map, source.world_pos, 4)
            .expect("sampling a rendered organism selects it");
        let ecology = cell.ecology.as_ref().expect("settled L8 ecology");

        assert_eq!(cell.world.0.to_bits(), 0x4053_d054_5000_0000);
        assert_eq!(cell.world.1.to_bits(), 0x4040_4744_a000_0000);
        assert_eq!(cell.region, RegionCoord::new(0, 0));
        assert_eq!(cell.cell, LocalPos::new(2, 1));
        assert_eq!(cell.status, CellStatus::Ready);
        assert_eq!(cell.stability.to_bits(), 0x3f80_0000);
        assert_eq!(cell.revision, 0);
        for (actual, expected) in [
            (cell.elevation, 0x4256_e516),
            (cell.temperature, 0x3fc4_5147),
            (cell.moisture, 0x3ebd_6aa9),
            (cell.hardness, 0x3f03_d3e2),
            (cell.river, 0x0000_0000),
            (cell.wetness, 0x3e5a_c554),
            (cell.soil_depth, 0x3f00_c5f2),
            (cell.fertility, 0x3e8c_ff6c),
            (cell.vegetation, 0x3e85_37d3),
            (cell.canopy, 0x40dd_b6d2),
        ] {
            assert_eq!(actual.map(f32::to_bits), Some(expected));
        }
        assert_eq!(cell.biome, Some("taiga"));
        let source_ecology = map
            .cell_ecology(cell.region, cell.cell.cx, cell.cell.cy)
            .expect("source ecology");
        assert_eq!(ecology.signature, source_ecology.signature);
        assert_eq!(ecology.roster_size, 8);
        assert_eq!(ecology.dominant_index, source_ecology.dominant_index);
        assert_eq!(ecology.dominant_id, 0xd3a8_f04e_f787_4415);
        assert_eq!(ecology.trophic_counts, [3, 3, 0, 1, 1]);
        assert_eq!(ecology.herbivore.map(f32::to_bits), Some(0x3bb6_424c));
        assert_eq!(ecology.predator.map(f32::to_bits), Some(0));
        assert_eq!(ecology.diversity.map(f32::to_bits), Some(0x3f52_ba36));

        assert_eq!(organism.id, 0x0308_7c3d_4bf5_90b7);
        assert_eq!(organism.slot, 0);
        assert_eq!(organism.cell, LocalPos::new(2, 1));
        assert_eq!(organism.species, 0x2eb2_cbe2_ec38_3555);
        assert_eq!(organism.trophic, Trophic::Herbivore);
        assert_eq!(organism.world, cell.world);
        assert_eq!(organism.form, source.expressed.form);
        for (actual, expected) in [
            (organism.hue, 0x3f19_8c12),
            (organism.luminance, 0x3f40_34b5),
            (organism.size, 0x3dad_b99a),
            (organism.activity, 0x3c49_afb0),
            (organism.aggression, 0x3ef7_10d1),
        ] {
            assert_eq!(actual.to_bits(), expected);
        }
    }

    #[test]
    fn map_hover_prefers_an_inspectable_organism_then_falls_back_to_terrain() {
        let map = settled_map();
        let source = map
            .organisms()
            .next()
            .copied()
            .expect("settled map has an organism");
        assert_eq!(map_hover(&map, None, 4), HoverInfo::None);
        assert!(matches!(
            map_hover(&map, Some(source.world_pos), 1),
            HoverInfo::Terrain(_)
        ));
        let HoverInfo::Organism(info) = map_hover(&map, Some(source.world_pos), 4) else {
            panic!("inspectable organism must take precedence over terrain");
        };
        assert_eq!(info.id, source.id);
        assert_eq!(info.slot, source.slot);
    }

    #[test]
    fn pov_hover_cache_keys_exact_geometry_inputs_and_ignores_static_time() {
        let map = RegionMap::new(config());
        let mut camera = PovCamera::new();
        camera.pos.x = 1_000_000.25;
        camera.pos.y = -2_000_000.5;
        camera.pos.z = 80.0;
        let chunks = PovChunkManager::new();
        let organisms = PovOrganismManager::new();
        assert!(!organisms.has_animated_visuals());

        let mut cache = PovHoverCache::new();
        let pane = PixelRect::new(13, 29, 200, 100);
        let pointer = [113.0, 79.0];
        let update = |cache: &mut PovHoverCache,
                      camera: &PovCamera,
                      pointer: Option<[f64; 2]>,
                      pane: Option<PixelRect>,
                      fog_end: f64,
                      time_seconds: f32| {
            cache.update(
                &map,
                camera,
                &chunks,
                &organisms,
                pointer,
                pane,
                fog_end,
                time_seconds,
            )
        };

        assert!(!update(
            &mut cache,
            &camera,
            Some(pointer),
            Some(pane),
            640.0,
            1.0,
        ));
        assert_eq!(cache.hover(), &HoverInfo::None);
        assert_eq!(cache.geometry_queries(), 1);

        // An identical frame and a presentation clock change cannot repeat a
        // static scene query.
        assert!(!update(
            &mut cache,
            &camera,
            Some(pointer),
            Some(pane),
            640.0,
            1.0,
        ));
        assert!(!update(
            &mut cache,
            &camera,
            Some(pointer),
            Some(pane),
            640.0,
            31.5,
        ));
        assert_eq!(cache.geometry_queries(), 1);

        camera.yaw += 0.125;
        assert!(!update(
            &mut cache,
            &camera,
            Some(pointer),
            Some(pane),
            640.0,
            31.5,
        ));
        assert_eq!(cache.geometry_queries(), 2);

        assert!(!update(
            &mut cache,
            &camera,
            Some([pointer[0] + 0.25, pointer[1]]),
            Some(pane),
            640.0,
            31.5,
        ));
        assert_eq!(cache.geometry_queries(), 3);

        assert!(!update(
            &mut cache,
            &camera,
            Some([pointer[0] + 0.25, pointer[1]]),
            Some(PixelRect::new(pane.x, pane.y, pane.width + 1, pane.height)),
            640.0,
            31.5,
        ));
        assert_eq!(cache.geometry_queries(), 4);

        assert!(!update(
            &mut cache,
            &camera,
            Some([pointer[0] + 0.25, pointer[1]]),
            Some(PixelRect::new(pane.x, pane.y, pane.width + 1, pane.height)),
            641.0,
            31.5,
        ));
        assert_eq!(cache.geometry_queries(), 5);

        // Outside/missing pane state clears the cached hit without querying
        // the resident geometry. Returning inside performs one fresh query.
        assert!(!update(
            &mut cache,
            &camera,
            Some([f64::from(pane.right()), pointer[1]]),
            Some(pane),
            641.0,
            31.5,
        ));
        assert_eq!(cache.geometry_queries(), 5);
        assert!(!update(
            &mut cache,
            &camera,
            Some(pointer),
            None,
            641.0,
            31.5,
        ));
        assert_eq!(cache.geometry_queries(), 5);
        assert!(!update(
            &mut cache,
            &camera,
            Some(pointer),
            Some(pane),
            641.0,
            31.5,
        ));
        assert_eq!(cache.geometry_queries(), 6);
    }

    #[test]
    fn pov_hover_cache_refreshes_semantics_without_requerying_geometry() {
        let mut map = RegionMap::new(config());
        let camera = PovCamera::new();
        let chunks = PovChunkManager::new();
        let organisms = PovOrganismManager::new();
        let pane = PixelRect::new(10, 20, 200, 100);
        let pointer = [110.0, 70.0];
        let fog_end = 640.0;
        let time_seconds = 0.0;
        let mut position = camera.pos;
        position.x = PLAYER.0;
        position.y = PLAYER.1;
        position.z = 12.0;
        let terrain_hit = PovSceneHit::Terrain(pov_host::PovTerrainHit {
            distance: 25.0,
            position,
            region: RegionCoord::new(0, 0),
            cell: [0, 0],
            triangle: 0,
        });
        let initial_hover = HoverInfo::Terrain(sample_cell(&map, PLAYER));
        let mut cache = PovHoverCache {
            geometry_key: Some(PovHoverGeometryKey::new(
                pointer,
                pane,
                &camera,
                &chunks,
                &organisms,
                fog_end,
                time_seconds,
            )),
            hit: Some(terrain_hit),
            hover: initial_hover,
            geometry_queries: 1,
        };

        assert!(!cache.update(
            &map,
            &camera,
            &chunks,
            &organisms,
            Some(pointer),
            Some(pane),
            fog_end,
            time_seconds,
        ));
        assert_eq!(cache.geometry_queries(), 1);

        update(
            &mut map,
            &Budget {
                max_regen_cost: 0,
                ..Budget::unlimited()
            },
        );
        assert!(cache.update(
            &map,
            &camera,
            &chunks,
            &organisms,
            Some(pointer),
            Some(pane),
            fog_end,
            time_seconds,
        ));
        let HoverInfo::Terrain(cell) = cache.hover() else {
            panic!("cached terrain hit must remain terrain semantic information");
        };
        assert_eq!(cell.status, CellStatus::Generating);
        assert_eq!(cache.geometry_queries(), 1);
    }
}
