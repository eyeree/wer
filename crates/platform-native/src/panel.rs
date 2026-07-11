//! The HUD: an information panel composed to the right of the debug map.
//!
//! Rendered on the CPU with an embedded public-domain 8×8 bitmap font
//! (`font8x8`), into the same RGBA texture the map uses — no new GPU surface
//! area, and the whole HUD shows up in headless `--screenshot` output, so it
//! is testable without a window (phase-1-plan.md section 10).

use font8x8::legacy::BASIC_LEGACY;
use world_core::{Anchor, AnchorKind, AnchorSource, LAYER_COUNT, POSSIBILITY_DIMS};
use world_runtime::FrameStats;

use crate::viz::Channel;

/// Panel width in pixels: 30 columns of scale-2 (16 px) glyphs.
pub const PANEL_WIDTH: usize = 480;

/// Glyph scale (8 px font cells × 2 = 16 px).
const SCALE: usize = 2;
/// Vertical advance per text line.
const LINE_HEIGHT: usize = 18;
/// Left padding inside the panel.
const MARGIN: usize = 12;

// Palette.
const BG: [u8; 3] = [16, 18, 24];
const RULE: [u8; 3] = [50, 54, 68];
const TITLE: [u8; 3] = [255, 255, 255];
const HEADER: [u8; 3] = [120, 170, 255];
const LABEL: [u8; 3] = [130, 135, 150];
const VALUE: [u8; 3] = [225, 228, 235];
const ACTIVE: [u8; 3] = [255, 200, 90];
const ALERT: [u8; 3] = [255, 90, 90];
const KEY: [u8; 3] = [170, 220, 170];

/// Everything the panel shows about the cell under the mouse.
#[derive(Debug, Clone)]
pub struct CursorInfo {
    /// World position under the cursor.
    pub world: (f64, f64),
    /// Region coordinate under the cursor.
    pub region: (i32, i32),
    /// Region streaming state, if resident.
    pub stability: f32,
    /// Realized-state revision of the region.
    pub revision: u32,
    /// Generation status label.
    pub status: &'static str,
    /// Sampled field values (present when the tiles are generated).
    pub elevation: Option<f32>,
    /// Temperature (°C) under the cursor.
    pub temperature: Option<f32>,
    /// Moisture under the cursor.
    pub moisture: Option<f32>,
    /// Rock hardness under the cursor.
    pub hardness: Option<f32>,
    /// River expression under the cursor.
    pub river: Option<f32>,
    /// Surface wetness under the cursor.
    pub wetness: Option<f32>,
    /// Soil depth under the cursor.
    pub soil_depth: Option<f32>,
    /// Soil fertility under the cursor.
    pub fertility: Option<f32>,
    /// Vegetation density under the cursor.
    pub vegetation: Option<f32>,
    /// Canopy height under the cursor.
    pub canopy: Option<f32>,
    /// Biome classification of the cell (from the biome id tile).
    pub biome: Option<&'static str>,
    /// Aggregate ecology readout at the cell (from L8), when settled.
    pub ecology: Option<EcologyInfo>,
}

/// The aggregate-ecology facts the panel shows for the cell under the cursor
/// (phase-3-plan.md §11).
#[derive(Debug, Clone)]
pub struct EcologyInfo {
    /// Roster size for the cell's habitat signature.
    pub roster_size: usize,
    /// Dominant species id.
    pub dominant_id: u64,
    /// Species count per trophic role: producer, herbivore, omnivore,
    /// carnivore, decomposer.
    pub trophic_counts: [usize; 5],
    /// Herbivore pressure.
    pub herbivore: f32,
    /// Predator pressure.
    pub predator: f32,
    /// Species diversity.
    pub diversity: f32,
}

/// One frame's worth of panel content.
#[derive(Debug)]
pub struct PanelInfo<'a> {
    /// Frames presented in the last second.
    pub fps: u32,
    /// Average `RegionMap::update` time over the last second, ms.
    pub update_ms: f64,
    /// The most recent frame's streaming stats.
    pub stats: FrameStats,
    /// Cumulative regenerated-tile counts per layer since startup
    /// (phase-2-plan.md §11 — per-layer regen visibility).
    pub regen_totals: &'a [u64; LAYER_COUNT as usize],
    /// Resident macro drainage tiles.
    pub macro_tiles: usize,
    /// Resident roster-cache entries (distinct habitat signatures).
    pub rosters: usize,
    /// Realized near-field organisms currently resident.
    pub organisms: usize,
    /// Generation jobs dispatched but not yet integrated.
    pub jobs_in_flight: usize,
    /// Changed-while-pinned events observed so far (0 = continuity holds).
    pub pinned_violations: u64,
    /// The channel the map is painting.
    pub channel: Channel,
    /// Player world position.
    pub player: (f64, f64),
    /// Current possibility-bias vector (the 1–8 nudges).
    pub bias: &'a [f32; POSSIBILITY_DIMS],
    /// Active anchors, in placement order.
    pub anchors: &'a [Anchor],
    /// The trait category a capture (`K`) will anchor.
    pub capture_category: &'static str,
    /// Whether a capture emphasizes or suppresses.
    pub capture_polarity: AnchorKind,
    /// Whether the shell is in deliberate transition-movement mode.
    pub transition_mode: bool,
    /// Data under the mouse, when it is over the map.
    pub cursor: Option<CursorInfo>,
}

/// Short display names for the eight possibility domains, indexed like
/// [`world_core::PossibilityDomain::ALL`].
const DOMAIN_SHORT: [&str; POSSIBILITY_DIMS] = [
    "Plan", "Clim", "Geol", "Hydr", "Ecol", "Morp", "Behv", "Aest",
];

/// Composes `map (side × side)` + `panel (PANEL_WIDTH × side)` into one RGBA
/// image for the renderer.
#[derive(Debug)]
pub struct Hud {
    map_side: usize,
    pixels: Vec<u8>,
}

impl Hud {
    /// A HUD for a `map_side`-pixel square map.
    #[must_use]
    pub fn new(map_side: usize) -> Self {
        Self {
            map_side,
            pixels: vec![0; (map_side + PANEL_WIDTH) * map_side * 4],
        }
    }

    /// Composed image dimensions (width, height).
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        ((self.map_side + PANEL_WIDTH) as u32, self.map_side as u32)
    }

    /// Blit the map and draw the panel; returns the full RGBA image.
    pub fn compose(&mut self, map_rgba: &[u8], info: &PanelInfo<'_>) -> &[u8] {
        let width = self.map_side + PANEL_WIDTH;
        // Left: the map, row by row.
        for row in 0..self.map_side {
            let src = row * self.map_side * 4;
            let dst = row * width * 4;
            self.pixels[dst..dst + self.map_side * 4]
                .copy_from_slice(&map_rgba[src..src + self.map_side * 4]);
        }
        // Right: panel background.
        for row in 0..self.map_side {
            let start = (row * width + self.map_side) * 4;
            for px in self.pixels[start..start + PANEL_WIDTH * 4].chunks_exact_mut(4) {
                px[0] = BG[0];
                px[1] = BG[1];
                px[2] = BG[2];
                px[3] = 255;
            }
        }
        self.draw_panel(info);
        &self.pixels
    }

    fn draw_panel(&mut self, info: &PanelInfo<'_>) {
        let x = self.map_side + MARGIN;
        let mut cur = PanelCursor { x, y: MARGIN };

        cur.line(self, "INFINITE WORLD  PHASE 4", TITLE);
        cur.rule(self);

        cur.pair(
            self,
            "fps",
            &format!("{}", info.fps),
            "update",
            &format!("{:.2}ms", info.update_ms),
        );
        cur.pair(
            self,
            "regions",
            &format!("{}", info.stats.active_regions),
            "cache",
            &format!(
                "{:.1}MB",
                (info.stats.cache_bytes + info.stats.macro_cache_bytes) as f64 / (1024.0 * 1024.0)
            ),
        );
        cur.pair(
            self,
            "jobs",
            &format!("{}", info.jobs_in_flight),
            "deferred",
            &format!("{}", info.stats.deferred_regens),
        );
        cur.pair(
            self,
            "converged",
            &format!("{}", info.stats.converged),
            "cost",
            &format!("{}", info.stats.regen_cost_spent),
        );
        cur.pair(
            self,
            "organisms",
            &format!("{}", info.organisms),
            "realized",
            &format!("{}", info.stats.organisms_realized),
        );
        cur.pair(
            self,
            "resonance",
            &format!("{:.2}", info.stats.resonance_strength),
            "mode",
            if info.transition_mode {
                "trans"
            } else {
                "free"
            },
        );
        cur.pair(
            self,
            "macro tiles",
            &format!("{}", info.macro_tiles),
            "rosters",
            &format!("{}", info.rosters),
        );
        let viol_color = if info.pinned_violations == 0 {
            VALUE
        } else {
            ALERT
        };
        cur.label_value(
            self,
            "pinned violations",
            &format!("{}", info.pinned_violations),
            viol_color,
        );
        cur.rule(self);

        // Cumulative per-layer regen counters, two layers per line
        // (phase-2-plan.md §11): watching these is how invalidation precision
        // is eyeballed live — an Ecology nudge must tick vegetation only.
        cur.line(self, "REGEN BY LAYER", HEADER);
        for pair in (0..LAYER_COUNT as usize).step_by(2) {
            let entry = |layer: usize| {
                format!(
                    "{:<9}{:>6}",
                    world_core::layer::LAYERS[layer].name,
                    info.regen_totals[layer]
                )
            };
            let row_y = cur.y;
            self.text(cur.x, row_y, &entry(pair), VALUE);
            if pair + 1 < LAYER_COUNT as usize {
                self.text(cur.x + 16 * 8 * SCALE, row_y, &entry(pair + 1), VALUE);
            }
            cur.y += LINE_HEIGHT;
        }
        cur.rule(self);

        cur.pair(
            self,
            "chan",
            info.channel.name(),
            "player",
            &format!("{:.0},{:.0}", info.player.0, info.player.1),
        );
        cur.rule(self);

        cur.line(self, "BIAS  1-8 up, shift down, Z", HEADER);
        for pair in (0..POSSIBILITY_DIMS).step_by(2) {
            let row_y = cur.y;
            for (slot, i) in (pair..(pair + 2).min(POSSIBILITY_DIMS)).enumerate() {
                let value = info.bias[i];
                let color = if value.abs() > f32::EPSILON {
                    ACTIVE
                } else {
                    LABEL
                };
                self.text(
                    cur.x + slot * 15 * 8 * SCALE,
                    row_y,
                    &format!("{} {} {:+.2}", i + 1, DOMAIN_SHORT[i], value),
                    color,
                );
            }
            cur.y += LINE_HEIGHT;
        }
        cur.rule(self);

        cur.line(self, "ANCHORS  E/Q drop, K capture", HEADER);
        // The capture selection (T cycles category, Y toggles polarity, K fires).
        let polarity = match info.capture_polarity {
            AnchorKind::Emphasize => "emph",
            AnchorKind::Suppress => "supp",
        };
        cur.label_value(
            self,
            "capture",
            &format!("{} {}", polarity, info.capture_category),
            ACTIVE,
        );
        if info.anchors.is_empty() {
            cur.line(self, "none active", LABEL);
        } else {
            const SHOWN: usize = 2;
            for anchor in info.anchors.iter().take(SHOWN) {
                let kind = match anchor.kind {
                    AnchorKind::Emphasize => "EMPH",
                    AnchorKind::Suppress => "SUPP",
                };
                let source = match anchor.source {
                    AnchorSource::Organism { .. } => "org",
                    AnchorSource::Landform => "land",
                    AnchorSource::River => "river",
                    AnchorSource::Atmosphere => "atmo",
                    AnchorSource::Manual => "man",
                };
                cur.line(
                    self,
                    &format!(
                        "{} {} {:.0},{:.0} s{:.1}",
                        kind, source, anchor.world_pos.0, anchor.world_pos.1, anchor.strength
                    ),
                    VALUE,
                );
            }
            if info.anchors.len() > SHOWN {
                cur.line(
                    self,
                    &format!("+{} more", info.anchors.len() - SHOWN),
                    LABEL,
                );
            }
        }
        cur.rule(self);

        cur.line(self, "CURSOR", HEADER);
        match &info.cursor {
            None => cur.line(self, "(move mouse over map)", LABEL),
            Some(c) => {
                cur.label_value(
                    self,
                    "world",
                    &format!("{:.0}, {:.0}", c.world.0, c.world.1),
                    VALUE,
                );
                cur.label_value(
                    self,
                    "region",
                    &format!("{}, {}  {}", c.region.0, c.region.1, c.status),
                    VALUE,
                );
                cur.pair(
                    self,
                    "stability",
                    &format!("{:.2}", c.stability),
                    "rev",
                    &format!("{}", c.revision),
                );
                match (c.elevation, c.temperature, c.moisture) {
                    (Some(e), Some(t), Some(m)) => {
                        cur.pair(
                            self,
                            "elev",
                            &format!("{e:.0}"),
                            "temp",
                            &format!("{t:.1}C"),
                        );
                        cur.pair(
                            self,
                            "moisture",
                            &format!("{m:.2}"),
                            "rock",
                            &c.hardness.map_or("-".into(), |h| format!("{h:.2}")),
                        );
                        cur.pair(
                            self,
                            "river",
                            &c.river.map_or("-".into(), |r| format!("{r:.2}")),
                            "wet",
                            &c.wetness.map_or("-".into(), |w| format!("{w:.2}")),
                        );
                        cur.pair(
                            self,
                            "soil",
                            &c.soil_depth.map_or("-".into(), |d| format!("{d:.2}")),
                            "fert",
                            &c.fertility.map_or("-".into(), |f| format!("{f:.2}")),
                        );
                        cur.pair(
                            self,
                            "veg",
                            &c.vegetation.map_or("-".into(), |v| format!("{v:.2}")),
                            "canopy",
                            &c.canopy.map_or("-".into(), |h| format!("{h:.1}m")),
                        );
                        cur.label_value(self, "biome", c.biome.unwrap_or("?"), ACTIVE);
                    }
                    _ => cur.line(self, "(tiles not generated yet)", LABEL),
                }
                if let Some(e) = &c.ecology {
                    cur.pair(
                        self,
                        "species",
                        &format!("{}", e.roster_size),
                        "domspp",
                        &format!("{:04x}", e.dominant_id & 0xFFFF),
                    );
                    cur.pair(
                        self,
                        "herb",
                        &format!("{:.3}", e.herbivore),
                        "pred",
                        &format!("{:.3}", e.predator),
                    );
                    cur.pair(
                        self,
                        "diversity",
                        &format!("{:.2}", e.diversity),
                        "P/H/C",
                        &format!(
                            "{}/{}/{}",
                            e.trophic_counts[0], e.trophic_counts[1], e.trophic_counts[3]
                        ),
                    );
                }
            }
        }
        cur.rule(self);

        cur.line(self, "KEYS", HEADER);
        for (keys, action) in [
            ("WASD 1-8 Z", "move, bias, reset"),
            ("E / Q / C", "anchors, clear"),
            ("T Y K", "categ,polar,capture"),
            ("R", "transition mode"),
            ("V G N X M", "channel,overlays"),
        ] {
            let row_y = cur.y;
            self.text(cur.x, row_y, keys, KEY);
            self.text(cur.x + 13 * 8 * SCALE, row_y, action, LABEL);
            cur.y += LINE_HEIGHT;
        }
    }

    /// Draw `text` at pixel `(x, y)` in `color`, clipped to the image.
    fn text(&mut self, x: usize, y: usize, text: &str, color: [u8; 3]) {
        let width = self.map_side + PANEL_WIDTH;
        let height = self.map_side;
        let mut pen_x = x;
        for ch in text.chars() {
            let glyph = BASIC_LEGACY[if ch.is_ascii() {
                ch as usize
            } else {
                b'?' as usize
            }];
            for (gy, row) in glyph.iter().enumerate() {
                for gx in 0..8usize {
                    if row >> gx & 1 == 0 {
                        continue;
                    }
                    for sy in 0..SCALE {
                        for sx in 0..SCALE {
                            let px = pen_x + gx * SCALE + sx;
                            let py = y + gy * SCALE + sy;
                            if px >= width || py >= height {
                                continue;
                            }
                            let offset = (py * width + px) * 4;
                            self.pixels[offset] = color[0];
                            self.pixels[offset + 1] = color[1];
                            self.pixels[offset + 2] = color[2];
                        }
                    }
                }
            }
            pen_x += 8 * SCALE;
        }
    }

    /// Horizontal separator across the panel.
    fn hline(&mut self, y: usize) {
        let width = self.map_side + PANEL_WIDTH;
        if y >= self.map_side {
            return;
        }
        for px in self.map_side + MARGIN..width - MARGIN {
            let offset = (y * width + px) * 4;
            self.pixels[offset] = RULE[0];
            self.pixels[offset + 1] = RULE[1];
            self.pixels[offset + 2] = RULE[2];
        }
    }
}

/// Tracks the panel's text-flow position (a plain struct sidesteps the
/// closure-borrowck tangle of mutating `Hud` from a captured `y`).
struct PanelCursor {
    x: usize,
    y: usize,
}

impl PanelCursor {
    fn line(&mut self, hud: &mut Hud, text: &str, color: [u8; 3]) {
        hud.text(self.x, self.y, text, color);
        self.y += LINE_HEIGHT;
    }

    /// `label value` with the label dimmed.
    fn label_value(&mut self, hud: &mut Hud, label: &str, value: &str, color: [u8; 3]) {
        hud.text(self.x, self.y, label, LABEL);
        hud.text(self.x + (label.len() + 1) * 8 * SCALE, self.y, value, color);
        self.y += LINE_HEIGHT;
    }

    /// Two label/value pairs on one line (fixed second column).
    fn pair(&mut self, hud: &mut Hud, l1: &str, v1: &str, l2: &str, v2: &str) {
        hud.text(self.x, self.y, l1, LABEL);
        hud.text(self.x + (l1.len() + 1) * 8 * SCALE, self.y, v1, VALUE);
        let col2 = self.x + 15 * 8 * SCALE;
        hud.text(col2, self.y, l2, LABEL);
        hud.text(col2 + (l2.len() + 1) * 8 * SCALE, self.y, v2, VALUE);
        self.y += LINE_HEIGHT;
    }

    /// Blank half-line, separator, blank half-line.
    fn rule(&mut self, hud: &mut Hud) {
        self.y += LINE_HEIGHT / 3;
        hud.hline(self.y);
        self.y += LINE_HEIGHT * 2 / 3;
    }
}
