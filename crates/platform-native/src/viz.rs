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

use world_core::{splitmix64, Biome, RegionCoord, REGION_SIZE, SEA_LEVEL};
use world_runtime::{
    RegionMap, CHANNEL_ELEVATION, CHANNEL_FERTILITY, CHANNEL_HARDNESS, CHANNEL_MOISTURE,
    CHANNEL_RIVER, CHANNEL_SOIL_DEPTH, CHANNEL_TEMPERATURE, CHANNEL_VEGETATION, CHANNEL_WETNESS,
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
    /// The streaming stability ramp (white = pinned, black = free).
    Stability,
    /// Realized-state revision, hashed to a color: convergence churn flickers.
    Revision,
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
            Channel::Vegetation => Channel::Stability,
            Channel::Stability => Channel::Revision,
            Channel::Revision => Channel::Composite,
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
            "stability" => Some(Channel::Stability),
            "revision" => Some(Channel::Revision),
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
            Channel::Stability => "stability",
            Channel::Revision => "revision",
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
}

impl Default for Overlays {
    fn default() -> Self {
        Self {
            grid: true,
            rings: true,
            pinned_flash: true,
        }
    }
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
    /// Last revision seen per pinned region.
    pinned_revisions: BTreeMap<RegionCoord, u32>,
    /// Frames of highlight left per offending region.
    flash: BTreeMap<RegionCoord, u8>,
    /// Total changed-while-pinned events observed (a continuity-bug counter).
    pub pinned_violations: u64,
}

const FLASH_FRAMES: u8 = 45;

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
            pinned_revisions: BTreeMap::new(),
            flash: BTreeMap::new(),
            pinned_violations: 0,
        }
    }

    fn side_for(half_regions: i32, resolution: u16) -> usize {
        (2 * half_regions + 1) as usize * resolution as usize
    }

    /// Image edge length in pixels.
    #[must_use]
    pub fn side(&self) -> u32 {
        Self::side_for(self.half_regions, self.resolution) as u32
    }

    /// Compose the map for this frame and return the RGBA buffer
    /// (row 0 = north edge, as the renderer expects).
    pub fn compose(
        &mut self,
        map: &RegionMap,
        player: (f64, f64),
        channel: Channel,
        overlays: Overlays,
    ) -> &[u8] {
        self.detect_pinned_changes(map);

        let center = RegionCoord::from_world(player.0, player.1);

        for row_region in 0..=(2 * self.half_regions) {
            // Row 0 is the northernmost (max y) region.
            let ry = center.y + self.half_regions - row_region;
            for col_region in 0..=(2 * self.half_regions) {
                let rx = center.x - self.half_regions + col_region;
                let coord = RegionCoord::new(rx, ry);
                self.paint_region(map, coord, channel, row_region, col_region, overlays);
            }
        }

        if overlays.rings {
            self.draw_rings(map, player);
        }
        self.draw_player_marker(player);
        &self.pixels
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
                            let h = splitmix64(u64::from(r.revision));
                            [(h >> 16) as u8, (h >> 32) as u8, (h >> 48) as u8]
                        }
                        None => missing_color(cx, cy),
                    },
                    Channel::Composite => match (elevation, biome, river, wetness) {
                        (Some(e), Some(b), Some(r), Some(w)) => composite_color(
                            e.get(cx, cy),
                            Biome::from_id(b.get(cx, cy)),
                            r.get(cx, cy),
                            w.get(cx, cy),
                        ),
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

    /// World position at the center of image pixel `(px, py)` — the inverse of
    /// the compose mapping, for mouse picking. Returns `None` outside the map.
    #[must_use]
    pub fn pixel_to_world(&self, player: (f64, f64), px: f64, py: f64) -> Option<(f64, f64)> {
        let side = f64::from(self.side());
        if px < 0.0 || py < 0.0 || px >= side || py >= side {
            return None;
        }
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
