//! CPU composition of the top-down false-color debug map
//! (phase-1-plan.md section 10, milestone M5).
//!
//! Composing on the CPU keeps the GPU surface area minimal (the renderer just
//! presents one texture) and makes every overlay trivial to draw. A continuous
//! field renders as smooth gradients; a chunk-replacement bug renders as a
//! visible seam or a flickering tile — precisely what this map exists to catch.

use std::collections::BTreeMap;

use world_core::{splitmix64, RegionCoord, REGION_SIZE, SEA_LEVEL};
use world_runtime::{
    RegionMap, CHANNEL_ELEVATION, CHANNEL_MOISTURE, CHANNEL_TEMPERATURE, CHANNEL_VEGETATION,
};

/// Which scalar the map paints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// Composite: water depth, vegetation/moisture-shaded land, snowline.
    Biome,
    /// Terrain elevation (the stable layer — must never move under drift).
    Elevation,
    /// Air temperature.
    Temperature,
    /// Surface moisture.
    Moisture,
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
            Channel::Biome => Channel::Elevation,
            Channel::Elevation => Channel::Temperature,
            Channel::Temperature => Channel::Moisture,
            Channel::Moisture => Channel::Vegetation,
            Channel::Vegetation => Channel::Stability,
            Channel::Stability => Channel::Revision,
            Channel::Revision => Channel::Biome,
        }
    }

    /// Display name for logs.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Channel::Biome => "biome",
            Channel::Elevation => "elevation",
            Channel::Temperature => "temperature",
            Channel::Moisture => "moisture",
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

fn vegetation_color(v: f32) -> [u8; 3] {
    lerp_rgb([190, 175, 130], [20, 110, 40], v)
}

fn biome_color(e: f32, t: f32, m: f32, v: f32) -> [u8; 3] {
    if e < SEA_LEVEL {
        return elevation_color(e);
    }
    if t < -2.0 {
        return [235, 240, 248]; // snow
    }
    let ground = lerp_rgb([200, 185, 140], [90, 80, 60], m);
    let land = lerp_rgb(ground, [25, 105, 45], v);
    // High rock fades in above the vegetation line.
    lerp_rgb(land, [130, 125, 120], ((e - 500.0) / 400.0).clamp(0.0, 1.0))
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

        let tile = |channel_index: usize| tiles.and_then(|t| t.channels[channel_index].as_ref());
        let elevation = tile(CHANNEL_ELEVATION);
        let temperature = tile(CHANNEL_TEMPERATURE);
        let moisture = tile(CHANNEL_MOISTURE);
        let vegetation = tile(CHANNEL_VEGETATION);

        for cy in 0..res {
            for cx in 0..res {
                let mut rgb = match channel {
                    Channel::Elevation => elevation
                        .map(|t| elevation_color(t.get(cx, cy)))
                        .unwrap_or_else(|| missing_color(cx, cy)),
                    Channel::Temperature => temperature
                        .map(|t| temperature_color(t.get(cx, cy)))
                        .unwrap_or_else(|| missing_color(cx, cy)),
                    Channel::Moisture => moisture
                        .map(|t| moisture_color(t.get(cx, cy)))
                        .unwrap_or_else(|| missing_color(cx, cy)),
                    Channel::Vegetation => vegetation
                        .map(|t| vegetation_color(t.get(cx, cy)))
                        .unwrap_or_else(|| missing_color(cx, cy)),
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
                    Channel::Biome => match (elevation, temperature, moisture, vegetation) {
                        (Some(e), Some(t), Some(m), Some(v)) => {
                            biome_color(e.get(cx, cy), t.get(cx, cy), m.get(cx, cy), v.get(cx, cy))
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
