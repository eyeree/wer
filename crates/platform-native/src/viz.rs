//! CPU composition of the top-down false-color debug map
//! (phase-1-plan.md section 10; phase-2-plan.md §11).
//!
//! Composing on the CPU keeps the GPU surface area minimal (the renderer just
//! presents one texture) and makes every overlay trivial to draw. A continuous
//! field renders as smooth gradients; a chunk-replacement bug renders as a
//! visible seam or a flickering tile. Rivers are the Phase 2
//! popping-detector-in-chief: a drainage discontinuity is instantly visible as
//! a broken river line across a macro boundary.

use std::collections::BTreeMap;

use world_core::{
    splitmix64, Anchor, Biome, RegionCoord, POSSIBILITY_DIMS, REGION_SIZE, SEA_LEVEL,
};
use world_runtime::{
    RegionMap, CHANNEL_DIVERSITY, CHANNEL_ELEVATION, CHANNEL_FERTILITY, CHANNEL_HARDNESS,
    CHANNEL_HERBIVORE, CHANNEL_MOISTURE, CHANNEL_PREDATOR, CHANNEL_RIVER, CHANNEL_SOIL_DEPTH,
    CHANNEL_TEMPERATURE, CHANNEL_VEGETATION, CHANNEL_WETNESS,
};

/// Which scalar the map paints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// Composite: water depth, biome palette, river/wetness darkening.
    Composite,
    /// Terrain elevation (stable — must never move under drift).
    Elevation,
    /// Rock: lithology tint shaded by hardness (stable).
    Geology,
    /// Air temperature.
    Temperature,
    /// Surface moisture.
    Moisture,
    /// River expression over the (stable) drainage topology.
    River,
    /// Surface wetness.
    Wetness,
    /// Soil: fertility hue, depth brightness.
    Soil,
    /// Biome classification (categorical palette).
    Biome,
    /// Aggregate vegetation density.
    Vegetation,
    /// Herbivore pressure (aggregate ecology, L8).
    Herbivore,
    /// Predator pressure (aggregate ecology, L8).
    Predator,
    /// Species diversity (aggregate ecology, L8).
    Diversity,
    /// Dominant species (categorical palette by species-id hash).
    DominantSpecies,
    /// Summed anchor influence, tinted by the dominant steered domain
    /// (phase-4-plan.md §11 — an anchor's reach and which trait it pushes).
    Influence,
    /// The streaming stability ramp (white = pinned, black = free).
    Stability,
    /// Realized-state revision as a grayscale ramp (black = never churned,
    /// white = `REVISION_WHITE`+ realized-state changes): total convergence
    /// churn a region has accumulated.
    Revision,
    /// Mean per-domain gap between a region's realized (`current`) and target
    /// possibility state as a grayscale ramp (black = settled, white =
    /// `RESIDUAL_WHITE`+ mean gap): how far a region still has to converge.
    Residual,
}

impl Channel {
    /// Cycle order for the channel toggle key.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Channel::Composite => Channel::Elevation,
            Channel::Elevation => Channel::Geology,
            Channel::Geology => Channel::Temperature,
            Channel::Temperature => Channel::Moisture,
            Channel::Moisture => Channel::River,
            Channel::River => Channel::Wetness,
            Channel::Wetness => Channel::Soil,
            Channel::Soil => Channel::Biome,
            Channel::Biome => Channel::Vegetation,
            Channel::Vegetation => Channel::Herbivore,
            Channel::Herbivore => Channel::Predator,
            Channel::Predator => Channel::Diversity,
            Channel::Diversity => Channel::DominantSpecies,
            Channel::DominantSpecies => Channel::Influence,
            Channel::Influence => Channel::Stability,
            Channel::Stability => Channel::Revision,
            Channel::Revision => Channel::Residual,
            Channel::Residual => Channel::Composite,
        }
    }

    /// Parse a channel name (as printed by [`Channel::name`]).
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "composite" => Some(Channel::Composite),
            "elevation" => Some(Channel::Elevation),
            "geology" => Some(Channel::Geology),
            "temperature" => Some(Channel::Temperature),
            "moisture" => Some(Channel::Moisture),
            "river" => Some(Channel::River),
            "wetness" => Some(Channel::Wetness),
            "soil" => Some(Channel::Soil),
            "biome" => Some(Channel::Biome),
            "vegetation" => Some(Channel::Vegetation),
            "herbivore" => Some(Channel::Herbivore),
            "predator" => Some(Channel::Predator),
            "diversity" => Some(Channel::Diversity),
            "dominant" => Some(Channel::DominantSpecies),
            "influence" => Some(Channel::Influence),
            "stability" => Some(Channel::Stability),
            "revision" => Some(Channel::Revision),
            "residual" => Some(Channel::Residual),
            _ => None,
        }
    }

    /// Display name for logs.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Channel::Composite => "composite",
            Channel::Elevation => "elevation",
            Channel::Geology => "geology",
            Channel::Temperature => "temperature",
            Channel::Moisture => "moisture",
            Channel::River => "river",
            Channel::Wetness => "wetness",
            Channel::Soil => "soil",
            Channel::Biome => "biome",
            Channel::Vegetation => "vegetation",
            Channel::Herbivore => "herbivore",
            Channel::Predator => "predator",
            Channel::Diversity => "diversity",
            Channel::DominantSpecies => "dominant",
            Channel::Influence => "influence",
            Channel::Stability => "stability",
            Channel::Revision => "revision",
            Channel::Residual => "residual",
        }
    }
}

/// Overlay toggles (phase-1-plan.md section 10).
#[derive(Debug, Clone, Copy)]
pub struct Overlays {
    /// Region grid lines.
    pub grid: bool,
    /// Near/far stability-radius rings around the player.
    pub rings: bool,
    /// Flash regions whose revision advanced while pinned (a continuity bug).
    pub pinned_flash: bool,
    /// Near-field organism markers, coloured by expressed appearance
    /// (phase-3-plan.md §11 — the popping/coherence detector for Tier B).
    pub organisms: bool,
    /// Dim regions the explorer has never visited (the first appearance of
    /// the atlas map, phase-5-plan.md §11). Only active when a vault is open.
    pub discovered: bool,
}

impl Default for Overlays {
    fn default() -> Self {
        Self {
            grid: true,
            rings: true,
            pinned_flash: true,
            organisms: true,
            discovered: true,
        }
    }
}

/// Vault-derived map decorations (phase-5-plan.md §11): the discovered set,
/// preserve outlines, and route polylines. Built by the shell each frame from
/// the open vault; [`MapDecor::default`] (nothing drawn) when no vault is open.
#[derive(Debug, Default, Clone)]
pub struct MapDecor {
    /// Discovered regions within the view. `None` means no vault is open, so
    /// no dimming is applied at all.
    pub seen: Option<std::collections::BTreeSet<RegionCoord>>,
    /// Preserved regions (outlined so a pinned window reads at a glance).
    pub preserves: std::collections::BTreeSet<RegionCoord>,
    /// Route polylines: recorded node positions in travel order, plus the
    /// route's usage count (brightness — well-worn paths glow).
    pub routes: Vec<(Vec<(f64, f64)>, u32)>,
}

/// Linear blend of two RGB colors.
fn lerp_rgb(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
    [mix(a[0], b[0]), mix(a[1], b[1]), mix(a[2], b[2])]
}

/// Missing-tile placeholder (dark checker so "not generated yet" is obvious).
fn missing_color(cx: u16, cy: u16) -> [u8; 3] {
    if (cx / 4 + cy / 4) % 2 == 0 {
        [24, 24, 28]
    } else {
        [32, 32, 38]
    }
}

fn elevation_color(e: f32) -> [u8; 3] {
    if e < SEA_LEVEL {
        // Deep to shallow water.
        lerp_rgb(
            [8, 16, 64],
            [70, 130, 190],
            (1.0 + e / 600.0).clamp(0.0, 1.0),
        )
    } else {
        let t = (e / 900.0).clamp(0.0, 1.0);
        if t < 0.5 {
            lerp_rgb([70, 120, 60], [140, 120, 80], t * 2.0)
        } else {
            lerp_rgb([140, 120, 80], [245, 245, 245], (t - 0.5) * 2.0)
        }
    }
}

fn temperature_color(t: f32) -> [u8; 3] {
    lerp_rgb([40, 60, 200], [220, 60, 40], (t + 15.0) / 50.0)
}

fn moisture_color(m: f32) -> [u8; 3] {
    lerp_rgb([150, 110, 70], [40, 90, 200], m)
}

fn river_color(r: f32) -> [u8; 3] {
    lerp_rgb([20, 20, 26], [80, 170, 255], r)
}

fn wetness_color(w: f32) -> [u8; 3] {
    lerp_rgb([120, 100, 70], [30, 120, 160], w)
}

fn soil_color(depth: f32, fertility: f32) -> [u8; 3] {
    let hue = lerp_rgb([190, 170, 130], [80, 60, 30], fertility);
    let brightness = 0.35 + 0.65 * depth;
    [
        (f32::from(hue[0]) * brightness) as u8,
        (f32::from(hue[1]) * brightness) as u8,
        (f32::from(hue[2]) * brightness) as u8,
    ]
}

fn vegetation_color(v: f32) -> [u8; 3] {
    lerp_rgb([190, 175, 130], [20, 110, 40], v)
}

fn herbivore_color(h: f32) -> [u8; 3] {
    // Pressures are ecologically small (~10% steps down the pyramid); amplify
    // for legibility so a debug map still reads.
    lerp_rgb([20, 24, 20], [210, 200, 60], (h * 8.0).clamp(0.0, 1.0))
}

fn predator_color(p: f32) -> [u8; 3] {
    lerp_rgb([22, 18, 20], [220, 70, 60], (p * 40.0).clamp(0.0, 1.0))
}

fn diversity_color(d: f32) -> [u8; 3] {
    lerp_rgb([30, 20, 45], [90, 220, 200], d)
}

/// A categorical colour for a species id: hash to a vivid, well-separated hue.
fn species_color(species_id: u64) -> [u8; 3] {
    let h = splitmix64(species_id);
    // Bias toward saturated, mid-bright colours so distinct species read apart.
    [
        96 + (h & 0x7F) as u8,
        96 + ((h >> 20) & 0x7F) as u8,
        96 + ((h >> 40) & 0x7F) as u8,
    ]
}

/// Convert an HSV triple (all `[0, 1]`) to RGB — for organism markers coloured
/// by expressed appearance (hue, brightness from luminance).
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 3] {
    let h6 = (h.rem_euclid(1.0)) * 6.0;
    let c = v * s;
    let x = c * (1.0 - (h6 % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h6 as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    ]
}

/// The marker colour for an organism's expressed appearance (hue from the
/// Aesthetics-biased genome, brightness from luminance).
fn expressed_color(expressed: &world_core::Expressed) -> [u8; 3] {
    hsv_to_rgb(expressed.hue, 0.75, 0.45 + 0.55 * expressed.luminance)
}

/// Distinct tints per possibility domain, indexed like
/// [`world_core::PossibilityDomain::ALL`] — used to colour the anchor-influence
/// channel by which trait an anchor steers (phase-4-plan.md §11).
const DOMAIN_TINTS: [[u8; 3]; POSSIBILITY_DIMS] = [
    [90, 150, 230],  // Planetary
    [230, 120, 90],  // Climate
    [170, 140, 110], // Geology
    [80, 170, 220],  // Hydrology
    [110, 200, 90],  // Ecology
    [210, 150, 220], // Morphology
    [230, 200, 90],  // Behavior
    [240, 120, 180], // Aesthetics
];

/// Summed anchor influence at a cell, coloured by the dominant steered domain
/// and brightened by total influence over a dark base (phase-4-plan.md §11).
fn influence_color(anchors: &[Anchor], world: (f64, f64)) -> [u8; 3] {
    let mut per_domain = [0.0f32; POSSIBILITY_DIMS];
    let mut total = 0.0f32;
    for anchor in anchors {
        let inf = anchor.influence(world);
        if inf <= 0.0 {
            continue;
        }
        total += inf;
        for (i, slot) in per_domain.iter_mut().enumerate() {
            if anchor.mask & (1 << i as u8) != 0 {
                *slot += inf;
            }
        }
    }
    if total <= 0.0 {
        return [18, 18, 22];
    }
    let dominant = per_domain
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map_or(0, |(i, _)| i);
    lerp_rgb([18, 18, 22], DOMAIN_TINTS[dominant], total.clamp(0.0, 1.0))
}

/// Distinct tints per lithology class (geology channel), shaded by hardness.
const LITHOLOGY_TINTS: [[u8; 3]; 8] = [
    [188, 143, 122],
    [140, 150, 170],
    [172, 165, 120],
    [120, 160, 140],
    [180, 130, 160],
    [150, 140, 100],
    [110, 140, 175],
    [165, 120, 100],
];

fn geology_color(world_x: f64, world_y: f64, hardness: f32) -> [u8; 3] {
    let tint = LITHOLOGY_TINTS[world_core::lithology_id(world_x, world_y) as usize];
    let shade = 0.45 + 0.55 * hardness;
    [
        (f32::from(tint[0]) * shade) as u8,
        (f32::from(tint[1]) * shade) as u8,
        (f32::from(tint[2]) * shade) as u8,
    ]
}

/// The categorical biome palette (phase-2-plan.md §11).
#[must_use]
pub const fn biome_color(biome: Biome) -> [u8; 3] {
    match biome {
        Biome::Ocean => [24, 44, 110],
        Biome::River => [58, 120, 216],
        Biome::Wetland => [70, 120, 110],
        Biome::Desert => [225, 200, 140],
        Biome::Grassland => [150, 180, 90],
        Biome::Shrubland => [170, 160, 100],
        Biome::TemperateForest => [45, 120, 55],
        Biome::Rainforest => [15, 95, 45],
        Biome::Taiga => [60, 100, 80],
        Biome::Tundra => [160, 160, 140],
        Biome::Bare => [130, 125, 120],
        Biome::Ice => [235, 240, 248],
    }
}

/// Composite: real biomes over water depth, with river/wetness expression
/// blended in so drift visibly breathes without moving the network.
fn composite_color(e: f32, biome: Biome, river: f32, wetness: f32) -> [u8; 3] {
    if e < SEA_LEVEL {
        return elevation_color(e);
    }
    let mut rgb = biome_color(biome);
    // Rivers draw as blue veins; wetness darkens the ground toward marsh.
    rgb = lerp_rgb(rgb, [58, 120, 216], river * 0.8);
    rgb = lerp_rgb(rgb, [35, 60, 70], wetness * 0.25);
    // High rock fades in above the vegetation line.
    lerp_rgb(rgb, [130, 125, 120], ((e - 500.0) / 400.0).clamp(0.0, 1.0))
}

/// Composes the active window into an RGBA8 image, one pixel per field cell,
/// and tracks pinned-region revisions for the changed-while-pinned detector.
#[derive(Debug)]
pub struct MapComposer {
    /// Half-extent of the view in regions (view is `2 * half + 1` square).
    half_regions: i32,
    /// Field cells per region edge (matches the stream config resolution).
    resolution: u16,
    pixels: Vec<u8>,
    /// Integer view magnification about the image center (scroll wheel).
    /// Presentation only: the base image is composed as usual and then the
    /// center block is blown up nearest-neighbor, so zooming reveals no data
    /// beyond the field resolution — it makes single-pixel markers (organisms)
    /// readable and pickable. [`Self::pixel_to_world`] inverts it.
    zoom: u32,
    /// Scratch buffer the magnify step writes into (swapped with `pixels`).
    zoom_scratch: Vec<u8>,
    /// Last revision seen per pinned region.
    pinned_revisions: BTreeMap<RegionCoord, u32>,
    /// Frames of highlight left per offending region.
    flash: BTreeMap<RegionCoord, u8>,
    /// Total changed-while-pinned events observed (a continuity-bug counter).
    pub pinned_violations: u64,
}

const FLASH_FRAMES: u8 = 45;

/// Realized-state revisions mapped to white on the [`Channel::Revision`] ramp.
/// A region that has changed its realized state this many times (or more) reads
/// as fully churned; steady/pinned regions stay near black.
const REVISION_WHITE: u32 = 256;

/// Mean per-domain `|current - target|` mapped to white on the
/// [`Channel::Residual`] ramp. Both vectors live in `[0, 1]`, so the raw mean
/// gap is bounded by 1, but real convergence residuals are small — a modest cap
/// keeps in-flight churn visible instead of washed out near black.
const RESIDUAL_WHITE: f32 = 0.25;

/// Preserve outline tint (phase-5-plan.md §11).
const PRESERVE_OUTLINE: [u8; 3] = [120, 255, 170];
/// Route polyline base tint; brightness saturates with usage.
const ROUTE_TINT: [u8; 3] = [255, 220, 120];

impl MapComposer {
    /// A composer viewing `half_regions` in every direction at `resolution`
    /// cells per region.
    #[must_use]
    pub fn new(half_regions: i32, resolution: u16) -> Self {
        let side = Self::side_for(half_regions, resolution);
        Self {
            half_regions,
            resolution,
            pixels: vec![0; side * side * 4],
            zoom: 1,
            zoom_scratch: Vec::new(),
            pinned_revisions: BTreeMap::new(),
            flash: BTreeMap::new(),
            pinned_violations: 0,
        }
    }

    /// Set the view magnification (clamped to at least 1). Applies to the CPU
    /// [`Self::compose`] path only; the shell falls back from the GPU map
    /// while zoomed so the base field and the overlays stay aligned.
    pub fn set_zoom(&mut self, zoom: u32) {
        self.zoom = zoom.max(1);
    }

    fn side_for(half_regions: i32, resolution: u16) -> usize {
        (2 * half_regions + 1) as usize * resolution as usize
    }

    /// Image edge length in pixels.
    #[must_use]
    pub fn side(&self) -> u32 {
        Self::side_for(self.half_regions, self.resolution) as u32
    }

    /// Regions viewed in every direction from the center (the view is a
    /// `2·half+1` square) — the bound the shell uses to gather per-region
    /// decor (discovered set, preserves) for exactly the visible window.
    #[must_use]
    pub const fn half_regions(&self) -> i32 {
        self.half_regions
    }

    /// Compose the map for this frame and return the RGBA buffer
    /// (row 0 = north edge, as the renderer expects).
    pub fn compose(
        &mut self,
        map: &RegionMap,
        player: (f64, f64),
        channel: Channel,
        overlays: Overlays,
        anchors: &[Anchor],
        decor: &MapDecor,
    ) -> &[u8] {
        self.detect_pinned_changes(map);

        let center = RegionCoord::from_world(player.0, player.1);

        for row_region in 0..=(2 * self.half_regions) {
            // Row 0 is the northernmost (max y) region.
            let ry = center.y + self.half_regions - row_region;
            for col_region in 0..=(2 * self.half_regions) {
                let rx = center.x - self.half_regions + col_region;
                let coord = RegionCoord::new(rx, ry);
                self.paint_region(
                    map, coord, channel, row_region, col_region, overlays, anchors,
                );
            }
        }

        if overlays.discovered {
            if let Some(seen) = &decor.seen {
                self.dim_undiscovered(center, seen);
            }
        }
        for (path, usage) in &decor.routes {
            self.draw_route(player, path, *usage);
        }
        for &coord in &decor.preserves {
            self.outline_region(center, coord, PRESERVE_OUTLINE);
        }
        if overlays.organisms {
            self.draw_organisms(map, player);
        }
        if overlays.rings {
            self.draw_rings(map, player);
        }
        self.draw_player_marker(player);
        self.magnify();
        &self.pixels
    }

    /// Compose only the sparse overlay content into a transparent RGBA
    /// buffer for the GPU-composed map (phase-6-plan.md §6.5): pinned-flash
    /// fills, undiscovered dimming, routes, preserve outlines, organisms,
    /// rings, and the player marker. The base field painting and the grid
    /// move to the GPU; the pinned-violation detector keeps running here —
    /// it reads world state, not pixels (ADR 0017).
    pub fn compose_overlays(
        &mut self,
        map: &RegionMap,
        player: (f64, f64),
        overlays: Overlays,
        decor: &MapDecor,
    ) -> &[u8] {
        self.detect_pinned_changes(map);
        self.pixels.fill(0);
        let center = RegionCoord::from_world(player.0, player.1);
        if overlays.pinned_flash {
            let flashing: Vec<RegionCoord> = self.flash.keys().copied().collect();
            for coord in flashing {
                // 60% red, matching the CPU path's lerp toward [255, 30, 30].
                self.fill_region(center, coord, [255, 30, 30], 153);
            }
        }
        if overlays.discovered {
            if let Some(seen) = &decor.seen {
                for row in 0..=(2 * self.half_regions) {
                    let ry = center.y + self.half_regions - row;
                    for col in 0..=(2 * self.half_regions) {
                        let rx = center.x - self.half_regions + col;
                        let coord = RegionCoord::new(rx, ry);
                        if !seen.contains(&coord) {
                            // alpha 113/255 ≈ keep 5/9, the CPU dim factor.
                            self.fill_region(center, coord, [0, 0, 0], 113);
                        }
                    }
                }
            }
        }
        for (path, usage) in &decor.routes {
            self.draw_route(player, path, *usage);
        }
        for &coord in &decor.preserves {
            self.outline_region(center, coord, PRESERVE_OUTLINE);
        }
        if overlays.organisms {
            self.draw_organisms(map, player);
        }
        if overlays.rings {
            self.draw_rings(map, player);
        }
        self.draw_player_marker(player);
        &self.pixels
    }

    /// Blend `rgb` at `alpha` over one region's pixel block (overlay mode).
    fn fill_region(&mut self, center: RegionCoord, coord: RegionCoord, rgb: [u8; 3], alpha: u8) {
        let row_region = center.y + self.half_regions - coord.y;
        let col_region = coord.x - center.x + self.half_regions;
        let span = 2 * self.half_regions;
        if !(0..=span).contains(&row_region) || !(0..=span).contains(&col_region) {
            return;
        }
        let res = self.resolution as usize;
        let side = self.side() as usize;
        for py in row_region as usize * res..(row_region as usize + 1) * res {
            let row = py * side;
            for px in col_region as usize * res..(col_region as usize + 1) * res {
                let offset = (row + px) * 4;
                self.pixels[offset] = rgb[0];
                self.pixels[offset + 1] = rgb[1];
                self.pixels[offset + 2] = rgb[2];
                self.pixels[offset + 3] = alpha;
            }
        }
    }

    /// Dim every view region the explorer has never visited (phase-5-plan.md
    /// §11): the discovered world reads bright, the unknown reads dark — the
    /// first appearance of the atlas map.
    fn dim_undiscovered(
        &mut self,
        center: RegionCoord,
        seen: &std::collections::BTreeSet<RegionCoord>,
    ) {
        let res = self.resolution as usize;
        let side = self.side() as usize;
        for row_region in 0..=(2 * self.half_regions) as usize {
            let ry = center.y + self.half_regions - row_region as i32;
            for col_region in 0..=(2 * self.half_regions) as usize {
                let rx = center.x - self.half_regions + col_region as i32;
                if seen.contains(&RegionCoord::new(rx, ry)) {
                    continue;
                }
                for py in row_region * res..(row_region + 1) * res {
                    let row = py * side;
                    for px in col_region * res..(col_region + 1) * res {
                        let offset = (row + px) * 4;
                        for c in &mut self.pixels[offset..offset + 3] {
                            *c = (u16::from(*c) * 5 / 9) as u8;
                        }
                    }
                }
            }
        }
    }

    /// Outline one region's pixel block (preserves, phase-5-plan.md §11).
    fn outline_region(&mut self, center: RegionCoord, coord: RegionCoord, rgb: [u8; 3]) {
        let row_region = center.y + self.half_regions - coord.y;
        let col_region = coord.x - center.x + self.half_regions;
        let span = 2 * self.half_regions;
        if !(0..=span).contains(&row_region) || !(0..=span).contains(&col_region) {
            return;
        }
        let res = self.resolution as i64;
        let x0 = i64::from(col_region) * res;
        let y0 = i64::from(row_region) * res;
        for k in 0..res {
            self.plot(x0 + k, y0, rgb);
            self.plot(x0 + k, y0 + res - 1, rgb);
            self.plot(x0, y0 + k, rgb);
            self.plot(x0 + res - 1, y0 + k, rgb);
        }
    }

    /// A recorded route as a polyline through its node positions, brightness
    /// saturating with usage ("frequently used routes become easier to
    /// follow", Overview; phase-5-plan.md §11).
    fn draw_route(&mut self, player: (f64, f64), path: &[(f64, f64)], usage: u32) {
        let cell = REGION_SIZE / f64::from(self.resolution);
        let (west, north) = self.view_origin(player);
        let brightness = 0.45 + 0.55 * (usage as f32 / (usage as f32 + 4.0));
        let rgb = [
            (ROUTE_TINT[0] as f32 * brightness) as u8,
            (ROUTE_TINT[1] as f32 * brightness) as u8,
            (ROUTE_TINT[2] as f32 * brightness) as u8,
        ];
        let to_px = |(wx, wy): (f64, f64)| ((wx - west) / cell, (north - wy) / cell);
        for pair in path.windows(2) {
            let (x0, y0) = to_px(pair[0]);
            let (x1, y1) = to_px(pair[1]);
            let steps = (x1 - x0).abs().max((y1 - y0).abs()).ceil().max(1.0) as usize;
            for i in 0..=steps {
                let t = i as f64 / steps as f64;
                self.plot(
                    (x0 + (x1 - x0) * t) as i64,
                    (y0 + (y1 - y0) * t) as i64,
                    rgb,
                );
            }
        }
    }

    /// Near-field organism markers, coloured by expressed appearance. A marker
    /// that contradicts its cell's aggregate tint is instantly visible — the
    /// Tier-B popping/coherence detector (phase-3-plan.md §11).
    fn draw_organisms(&mut self, map: &RegionMap, player: (f64, f64)) {
        let cell = REGION_SIZE / f64::from(self.resolution);
        let (west, north) = self.view_origin(player);
        for organism in map.organisms() {
            let (wx, wy) = organism.world_pos;
            let px = ((wx - west) / cell) as i64;
            let py = ((north - wy) / cell) as i64;
            let rgb = expressed_color(&organism.expressed);
            self.plot(px, py, rgb);
        }
    }

    /// The most recently composed RGBA buffer (valid after [`Self::compose`]).
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Update the pinned-revision ledger; regions whose revision advanced
    /// while pinned are continuity bugs by definition (phase-1-plan.md §10).
    fn detect_pinned_changes(&mut self, map: &RegionMap) {
        let mut next = BTreeMap::new();
        for region in map.iter_active() {
            if region.stability >= 1.0 {
                if let Some(&prev) = self.pinned_revisions.get(&region.coord) {
                    if region.revision != prev {
                        self.flash.insert(region.coord, FLASH_FRAMES);
                        self.pinned_violations += 1;
                        log::warn!(
                            "continuity: region ({}, {}) revision {} -> {} while pinned",
                            region.coord.x,
                            region.coord.y,
                            prev,
                            region.revision
                        );
                    }
                }
                next.insert(region.coord, region.revision);
            }
        }
        self.pinned_revisions = next;
        self.flash.retain(|_, frames| {
            *frames = frames.saturating_sub(1);
            *frames > 0
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_region(
        &mut self,
        map: &RegionMap,
        coord: RegionCoord,
        channel: Channel,
        row_region: i32,
        col_region: i32,
        overlays: Overlays,
        anchors: &[Anchor],
    ) {
        let res = self.resolution;
        let side = self.side() as usize;
        let state = map.get(coord);
        let tiles = map.cache().get(coord);
        let flashing = overlays.pinned_flash && self.flash.contains_key(&coord);

        let tile = |channel_index: usize| tiles.and_then(|t| t.channels[channel_index].as_deref());
        let elevation = tile(CHANNEL_ELEVATION);
        let hardness = tile(CHANNEL_HARDNESS);
        let temperature = tile(CHANNEL_TEMPERATURE);
        let moisture = tile(CHANNEL_MOISTURE);
        let river = tile(CHANNEL_RIVER);
        let wetness = tile(CHANNEL_WETNESS);
        let soil_depth = tile(CHANNEL_SOIL_DEPTH);
        let fertility = tile(CHANNEL_FERTILITY);
        let vegetation = tile(CHANNEL_VEGETATION);
        let herbivore = tile(CHANNEL_HERBIVORE);
        let predator = tile(CHANNEL_PREDATOR);
        let diversity = tile(CHANNEL_DIVERSITY);
        let biome = tiles.and_then(|t| t.biome.as_deref());

        let (origin_x, origin_y) = coord.origin();
        let cell = REGION_SIZE / f64::from(res);

        for cy in 0..res {
            for cx in 0..res {
                let scalar = |t: Option<&world_core::FieldTile<f32>>,
                              paint: &dyn Fn(f32) -> [u8; 3]| {
                    t.map(|t| paint(t.get(cx, cy)))
                        .unwrap_or_else(|| missing_color(cx, cy))
                };
                let mut rgb = match channel {
                    Channel::Elevation => scalar(elevation, &elevation_color),
                    Channel::Temperature => scalar(temperature, &temperature_color),
                    Channel::Moisture => scalar(moisture, &moisture_color),
                    Channel::River => scalar(river, &river_color),
                    Channel::Wetness => scalar(wetness, &wetness_color),
                    Channel::Vegetation => scalar(vegetation, &vegetation_color),
                    Channel::Herbivore => scalar(herbivore, &herbivore_color),
                    Channel::Predator => scalar(predator, &predator_color),
                    Channel::Diversity => scalar(diversity, &diversity_color),
                    Channel::DominantSpecies => match map.dominant_species_id(coord, cx, cy) {
                        Some(id) => species_color(id),
                        None => missing_color(cx, cy),
                    },
                    Channel::Influence => {
                        let wx = origin_x + (f64::from(cx) + 0.5) * cell;
                        let wy = origin_y + (f64::from(cy) + 0.5) * cell;
                        influence_color(anchors, (wx, wy))
                    }
                    Channel::Geology => match hardness {
                        Some(h) => {
                            let wx = origin_x + (f64::from(cx) + 0.5) * cell;
                            let wy = origin_y + (f64::from(cy) + 0.5) * cell;
                            geology_color(wx, wy, h.get(cx, cy))
                        }
                        None => missing_color(cx, cy),
                    },
                    Channel::Soil => match (soil_depth, fertility) {
                        (Some(d), Some(f)) => soil_color(d.get(cx, cy), f.get(cx, cy)),
                        _ => missing_color(cx, cy),
                    },
                    Channel::Biome => match biome {
                        Some(b) => biome_color(Biome::from_id(b.get(cx, cy))),
                        None => missing_color(cx, cy),
                    },
                    Channel::Stability => match state {
                        Some(r) => {
                            let s = (r.stability * 255.0) as u8;
                            [s, s, s]
                        }
                        None => missing_color(cx, cy),
                    },
                    Channel::Revision => match state {
                        Some(r) => {
                            let g = (r.revision.min(REVISION_WHITE) as f32 / REVISION_WHITE as f32
                                * 255.0) as u8;
                            [g, g, g]
                        }
                        None => missing_color(cx, cy),
                    },
                    Channel::Residual => match state {
                        Some(r) => {
                            let mut sum = 0.0f32;
                            for i in 0..POSSIBILITY_DIMS {
                                sum += (r.current.dims[i] - r.target.dims[i]).abs();
                            }
                            let mean = sum / POSSIBILITY_DIMS as f32;
                            let g = ((mean / RESIDUAL_WHITE).clamp(0.0, 1.0) * 255.0) as u8;
                            [g, g, g]
                        }
                        None => missing_color(cx, cy),
                    },
                    Channel::Composite => match (elevation, biome, river, wetness) {
                        (Some(e), Some(b), Some(r), Some(w)) => {
                            let elev = e.get(cx, cy);
                            let mut rgb = composite_color(
                                elev,
                                Biome::from_id(b.get(cx, cy)),
                                r.get(cx, cy),
                                w.get(cx, cy),
                            );
                            // Tint land by dominant-species colour so ecosystem
                            // zonation reads at a glance (phase-3-plan.md §11).
                            if elev >= SEA_LEVEL {
                                if let Some(id) = map.dominant_species_id(coord, cx, cy) {
                                    rgb = lerp_rgb(rgb, species_color(id), 0.18);
                                }
                            }
                            rgb
                        }
                        _ => missing_color(cx, cy),
                    },
                };

                if flashing {
                    rgb = lerp_rgb(rgb, [255, 30, 30], 0.6);
                }
                if overlays.grid && (cx == 0 || cy == 0) {
                    rgb = lerp_rgb(rgb, [0, 0, 0], 0.35);
                }

                // Cell (cx, cy) has cy growing north; image rows grow south.
                let px = col_region as usize * res as usize + cx as usize;
                let py_region = row_region as usize * res as usize;
                let py = py_region + (res - 1 - cy) as usize;
                let offset = (py * side + px) * 4;
                self.pixels[offset] = rgb[0];
                self.pixels[offset + 1] = rgb[1];
                self.pixels[offset + 2] = rgb[2];
                self.pixels[offset + 3] = 255;
            }
        }
    }

    /// Blow the center block of the composed image up by the zoom factor,
    /// nearest-neighbor, in place (a swap with the scratch buffer). Everything
    /// already drawn — field cells and overlay markers alike — magnifies
    /// together, so the zoomed view stays a faithful crop of the base map.
    fn magnify(&mut self) {
        if self.zoom <= 1 {
            return;
        }
        let side = self.side() as usize;
        let zoom = f64::from(self.zoom);
        let center = side as f64 / 2.0;
        // Source index for each output row/column: the continuous zoom-about-
        // center mapping, floored to a base pixel (mirrors `pixel_to_world`).
        let src: Vec<usize> = (0..side)
            .map(|i| {
                let s = (i as f64 + 0.5 - center) / zoom + center;
                (s.max(0.0) as usize).min(side - 1)
            })
            .collect();
        self.zoom_scratch.resize(self.pixels.len(), 0);
        for (oy, &sy) in src.iter().enumerate() {
            let src_row = sy * side;
            let dst_row = oy * side;
            for (ox, &sx) in src.iter().enumerate() {
                let s = (src_row + sx) * 4;
                let d = (dst_row + ox) * 4;
                self.zoom_scratch[d..d + 4].copy_from_slice(&self.pixels[s..s + 4]);
            }
        }
        std::mem::swap(&mut self.pixels, &mut self.zoom_scratch);
    }

    /// World position at the center of image pixel `(px, py)` — the inverse of
    /// the compose mapping (including the zoom magnification), for mouse
    /// picking. Returns `None` outside the map.
    #[must_use]
    pub fn pixel_to_world(&self, player: (f64, f64), px: f64, py: f64) -> Option<(f64, f64)> {
        let side = f64::from(self.side());
        if px < 0.0 || py < 0.0 || px >= side || py >= side {
            return None;
        }
        // Undo the magnify-about-center step first (identity at zoom 1).
        let zoom = f64::from(self.zoom);
        let center = side / 2.0;
        let px = (px - center) / zoom + center;
        let py = (py - center) / zoom + center;
        let cell = REGION_SIZE / f64::from(self.resolution);
        let (west, north) = self.view_origin(player);
        Some((west + (px + 0.5) * cell, north - (py + 0.5) * cell))
    }

    /// World position of the view's north-west pixel corner.
    fn view_origin(&self, player: (f64, f64)) -> (f64, f64) {
        let center = RegionCoord::from_world(player.0, player.1);
        let west = (center.x - self.half_regions) as f64 * REGION_SIZE;
        let north = (center.y + self.half_regions + 1) as f64 * REGION_SIZE;
        (west, north)
    }

    fn plot(&mut self, px: i64, py: i64, rgb: [u8; 3]) {
        let side = self.side() as i64;
        if px < 0 || py < 0 || px >= side || py >= side {
            return;
        }
        let offset = ((py * side + px) * 4) as usize;
        self.pixels[offset] = rgb[0];
        self.pixels[offset + 1] = rgb[1];
        self.pixels[offset + 2] = rgb[2];
        self.pixels[offset + 3] = 255;
    }

    /// Near (white) and far (orange) stability rings around the player.
    fn draw_rings(&mut self, map: &RegionMap, player: (f64, f64)) {
        let cell = REGION_SIZE / f64::from(self.resolution);
        let (west, north) = self.view_origin(player);
        let rings = [
            (map.config().near_radius, [255u8, 255, 255]),
            (map.config().far_radius, [255, 160, 40]),
        ];
        for (radius, rgb) in rings {
            // Enough angular steps that adjacent plotted pixels touch.
            let steps = ((radius * core::f64::consts::TAU / cell) as usize).max(64);
            for i in 0..steps {
                let a = i as f64 / steps as f64 * core::f64::consts::TAU;
                let wx = player.0 + radius * a.cos();
                let wy = player.1 + radius * a.sin();
                let px = ((wx - west) / cell) as i64;
                let py = ((north - wy) / cell) as i64;
                self.plot(px, py, rgb);
            }
        }
    }

    /// Small cross marking the player's exact world position.
    fn draw_player_marker(&mut self, player: (f64, f64)) {
        let cell = REGION_SIZE / f64::from(self.resolution);
        let (west, north) = self.view_origin(player);
        let px = ((player.0 - west) / cell) as i64;
        let py = ((north - player.1) / cell) as i64;
        for d in -3i64..=3 {
            self.plot(px + d, py, [255, 255, 255]);
            self.plot(px, py + d, [255, 255, 255]);
        }
        self.plot(px, py, [255, 40, 40]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_preserves_the_view_center() {
        // The magnification is about the image center, so the world position
        // under the center pixel must not move as the zoom changes.
        let player = (300.0, -10.0);
        let mut composer = MapComposer::new(3, 16);
        let center = f64::from(composer.side()) / 2.0;
        let base = composer
            .pixel_to_world(player, center, center)
            .expect("center is inside the map");
        for zoom in [2, 4, 8, 16] {
            composer.set_zoom(zoom);
            let zoomed = composer
                .pixel_to_world(player, center, center)
                .expect("center is inside the map");
            assert!(
                (zoomed.0 - base.0).abs() < 1e-9 && (zoomed.1 - base.1).abs() < 1e-9,
                "center moved at zoom x{zoom}: {base:?} -> {zoomed:?}"
            );
        }
    }

    #[test]
    fn zoomed_picking_shrinks_the_world_span() {
        // Pixel-to-world across the full image must cover exactly 1/zoom of
        // the base world extent (the inverse of the magnify step).
        let player = (0.0, 0.0);
        let mut composer = MapComposer::new(3, 16);
        let side = f64::from(composer.side());
        let span = |c: &MapComposer| {
            let w0 = c.pixel_to_world(player, 0.0, 0.0).unwrap();
            let w1 = c.pixel_to_world(player, side - 1.0, 0.0).unwrap();
            w1.0 - w0.0
        };
        let base = span(&composer);
        composer.set_zoom(4);
        let zoomed = span(&composer);
        assert!(
            (zoomed - base / 4.0).abs() < 1e-6,
            "zoom x4 span {zoomed} != base {base} / 4"
        );
    }

    #[test]
    fn magnify_blows_up_the_center_block() {
        // Paint a distinct color per base pixel, magnify, and check output
        // pixels sample the base pixel the pixel_to_world inverse names.
        let mut composer = MapComposer::new(1, 4);
        let side = composer.side() as usize;
        for i in 0..side * side {
            let v = (i % 251) as u8;
            composer.pixels[i * 4..i * 4 + 4].copy_from_slice(&[v, v.wrapping_add(1), 0, 255]);
        }
        let before = composer.pixels.clone();
        composer.set_zoom(2);
        composer.magnify();
        let zoom = 2.0;
        let center = side as f64 / 2.0;
        for oy in 0..side {
            for ox in 0..side {
                let src = |i: usize| {
                    let s = (i as f64 + 0.5 - center) / zoom + center;
                    (s.max(0.0) as usize).min(side - 1)
                };
                let (sx, sy) = (src(ox), src(oy));
                assert_eq!(
                    composer.pixels[(oy * side + ox) * 4..(oy * side + ox) * 4 + 4],
                    before[(sy * side + sx) * 4..(sy * side + sx) * 4 + 4],
                    "output ({ox},{oy}) should show base ({sx},{sy})"
                );
            }
        }
    }
}
