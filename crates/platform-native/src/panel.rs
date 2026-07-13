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

/// Panel width in pixels: three stat columns of scale-2 (16 px) glyphs plus
/// the margins. Three-up rows keep the panel short enough that the cursor
/// and steering blocks stay visible on the Low-tier map strip.
pub const PANEL_WIDTH: usize = 3 * COL_CHARS * 8 * SCALE + 2 * MARGIN;

/// Column stride of the three-column stat grid, in glyphs.
const COL_CHARS: usize = 17;

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

/// Rasterize a small opaque HUD chip (panel palette, scale-2 glyphs) for the
/// POV mode's corner FPS counter — POV has no panel strip, so this is the
/// one piece of text it shows. Returns the RGBA8 image and its size; the
/// renderer blits it pixel-exact, so the chip is built at final size.
#[must_use]
pub fn hud_chip(text: &str) -> (Vec<u8>, u32, u32) {
    let pad = 6usize;
    let width = text.chars().count() * 8 * SCALE + 2 * pad;
    let height = 8 * SCALE + 2 * pad;
    let mut rgba = vec![0u8; width * height * 4];
    for px in rgba.chunks_exact_mut(4) {
        px.copy_from_slice(&[BG[0], BG[1], BG[2], 255]);
    }
    let mut pen_x = pad;
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
                        let py = pad + gy * SCALE + sy;
                        let offset = (py * width + px) * 4;
                        rgba[offset..offset + 3].copy_from_slice(&VALUE);
                    }
                }
            }
        }
        pen_x += 8 * SCALE;
    }
    (rgba, width as u32, height as u32)
}

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

/// The realized organism under the cursor when the view is zoomed in past
/// the pick threshold. Everything here is transient presentation state
/// (phase-3-plan.md §7.6) — the readout inspects a realized instance, it
/// never becomes identity.
#[derive(Debug, Clone, Copy)]
pub struct OrganismInfo {
    /// Stable instance identity (the `feature_hash` of its cell slot).
    pub id: u64,
    /// The species it instantiates ([`world_core::Species::id`]).
    pub species: u64,
    /// Trophic role label.
    pub trophic: &'static str,
    /// Jittered world position.
    pub world: (f64, f64),
    /// Expressed hue in `[0, 1)`.
    pub hue: f32,
    /// Expressed bioluminance in `[0, 1]`.
    pub luminance: f32,
    /// Expressed body size in world units.
    pub size: f32,
    /// Expressed activity in `[0, 1]`.
    pub activity: f32,
    /// Expressed aggression in `[0, 1]`.
    pub aggression: f32,
}

/// One frame's worth of panel content.
#[derive(Debug)]
pub struct PanelInfo<'a> {
    /// Frames presented in the last second.
    pub fps: u32,
    /// Average `RegionMap::update` time over the last second, ms.
    pub update_ms: f64,
    /// Average CPU map+HUD composition time over the last second, ms
    /// (phase-6-plan.md §12).
    pub compose_ms: f64,
    /// Average present time over the last second, ms — includes the vsync
    /// wait under FIFO, i.e. pacing idle, not work.
    pub render_ms: f64,
    /// Mean atlas/overlay/panel upload KB per frame (GPU path;
    /// phase-6-plan.md §12).
    pub upload_kb: f64,
    /// Whether the map composed on the GPU this frame (`,` toggles).
    pub gpu_compose: bool,
    /// The active resource tier preset (phase-6-plan.md §6.7).
    pub tier: &'static str,
    /// The field-cache byte ceiling of the active preset (§4.3).
    pub cache_ceiling_bytes: usize,
    /// Mean per-pass update milliseconds over the last second
    /// (phase-6-plan.md §5.2; zeros without the `pass-timing` feature).
    pub pass_ms: [f32; world_runtime::PASS_COUNT],
    /// Executor worker parallelism.
    pub workers: usize,
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
    /// Vault store telemetry, when a vault is open (phase-5-plan.md §11).
    pub vault: Option<VaultInfo>,
    /// The view magnification (mouse wheel; 1 = the full window).
    pub zoom: u32,
    /// Data under the mouse, when it is over the map.
    pub cursor: Option<CursorInfo>,
    /// The organism under the mouse when zoomed in past the pick threshold;
    /// shown in place of the cursor's region block while present.
    pub organism: Option<OrganismInfo>,
}

/// The panel's view of the open vault (phase-5-plan.md §8.2): live proof that
/// the store holds records, not geometry.
#[derive(Debug, Clone, Copy)]
pub struct VaultInfo {
    /// Loaded records (discoveries + routes + preserves).
    pub records: usize,
    /// Records waiting to be flushed (backpressure, not an error).
    pub dirty: usize,
    /// Total discovered regions in the seen-set.
    pub seen: u64,
    /// Non-fatal problems found opening the store.
    pub issues: usize,
    /// Additional issue identities omitted/displaced at the registry cap.
    pub suppressed_issues: u64,
    /// Occurrence count of the active retryable persistence failure.
    pub persistence_retries: u64,
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
    /// Standalone panel strip for the GPU-map path (phase-6-plan.md §6.5).
    panel_scratch: Vec<u8>,
}

impl Hud {
    /// A HUD for a `map_side`-pixel square map.
    #[must_use]
    pub fn new(map_side: usize) -> Self {
        Self {
            map_side,
            pixels: vec![0; (map_side + PANEL_WIDTH) * map_side * 4],
            panel_scratch: Vec::new(),
        }
    }

    /// Composed image dimensions (width, height).
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        ((self.map_side + PANEL_WIDTH) as u32, self.map_side as u32)
    }

    /// Draw the panel alone into its own strip (the GPU-map path blits it
    /// beside the GPU-composed map; phase-6-plan.md §6.5). Returns the RGBA
    /// strip and its size.
    pub fn panel_image(&mut self, info: &PanelInfo<'_>) -> (&[u8], u32, u32) {
        // Panel background over the strip region of the combined image.
        for row in 0..self.map_side {
            let width = self.map_side + PANEL_WIDTH;
            let start = (row * width + self.map_side) * 4;
            for px in self.pixels[start..start + PANEL_WIDTH * 4].chunks_exact_mut(4) {
                px[0] = BG[0];
                px[1] = BG[1];
                px[2] = BG[2];
                px[3] = 255;
            }
        }
        self.draw_panel(info);
        // Extract the strip (the panel drawing code addresses the combined
        // image; a row-wise copy keeps that code untouched).
        let width = self.map_side + PANEL_WIDTH;
        self.panel_scratch
            .resize(PANEL_WIDTH * self.map_side * 4, 0);
        for row in 0..self.map_side {
            let src = (row * width + self.map_side) * 4;
            let dst = row * PANEL_WIDTH * 4;
            self.panel_scratch[dst..dst + PANEL_WIDTH * 4]
                .copy_from_slice(&self.pixels[src..src + PANEL_WIDTH * 4]);
        }
        (
            &self.panel_scratch,
            PANEL_WIDTH as u32,
            self.map_side as u32,
        )
    }

    /// The last strip produced by [`Self::panel_image`].
    #[must_use]
    pub fn panel_pixels(&self) -> &[u8] {
        &self.panel_scratch
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

        cur.triple(
            self,
            [
                ("fps", &format!("{}", info.fps)),
                ("update", &format!("{:.2}ms", info.update_ms)),
                ("workers", &format!("{}", info.workers)),
            ],
        );
        // TIMINGS (phase-6-plan.md §10): per-pass update ms, three per line,
        // plus the shell-side compose/present split.
        cur.line(self, "TIMINGS", HEADER);
        for group in (0..world_runtime::PASS_COUNT).step_by(3) {
            let entry = |i: usize| {
                format!(
                    "{:<9}{:>6.2}",
                    world_runtime::Pass::ALL[i].name(),
                    info.pass_ms[i]
                )
            };
            let row_y = cur.y;
            for (slot, i) in (group..(group + 3).min(world_runtime::PASS_COUNT)).enumerate() {
                self.text(
                    cur.x + slot * COL_CHARS * 8 * SCALE,
                    row_y,
                    &entry(i),
                    VALUE,
                );
            }
            cur.y += LINE_HEIGHT;
        }
        cur.triple(
            self,
            [
                ("compose", &format!("{:.2}ms", info.compose_ms)),
                ("present", &format!("{:.2}ms", info.render_ms)),
                ("upload", &format!("{:.0}KB/f", info.upload_kb)),
            ],
        );
        // TIER (phase-6-plan.md §10): the active preset and how much of its
        // field-cache ceiling is in use.
        cur.triple(
            self,
            [
                ("map", if info.gpu_compose { "gpu" } else { "cpu" }),
                ("tier", info.tier),
                (
                    "ceiling",
                    &format!(
                        "{:.0}/{:.0}MB",
                        info.stats.cache_bytes as f64 / (1024.0 * 1024.0),
                        info.cache_ceiling_bytes as f64 / (1024.0 * 1024.0)
                    ),
                ),
            ],
        );
        // EXEC / POOL telemetry (phase-6-plan.md §10). The pool counters are
        // zeros until M3 lands the tile pool; cancelled counts until M2's
        // lane executor are zero too — placeholders by design (M1).
        cur.triple(
            self,
            [
                ("cancelled", &format!("{}", info.stats.jobs_cancelled)),
                (
                    "pool h/m",
                    &format!("{}/{}", info.stats.pool_hits, info.stats.pool_misses),
                ),
                (
                    "pool",
                    &format!("{:.1}MB", info.stats.pool_bytes as f64 / (1024.0 * 1024.0)),
                ),
            ],
        );
        cur.triple(
            self,
            [
                ("regions", &format!("{}", info.stats.active_regions)),
                (
                    "cache",
                    &format!(
                        "{:.1}MB",
                        (info.stats.cache_bytes + info.stats.macro_cache_bytes) as f64
                            / (1024.0 * 1024.0)
                    ),
                ),
                ("jobs", &format!("{}", info.jobs_in_flight)),
            ],
        );
        cur.triple(
            self,
            [
                ("deferred", &format!("{}", info.stats.deferred_regens)),
                ("converged", &format!("{}", info.stats.converged)),
                ("cost", &format!("{}", info.stats.regen_cost_spent)),
            ],
        );
        cur.triple(
            self,
            [
                ("organisms", &format!("{}", info.organisms)),
                (
                    "realized a/v",
                    &format!(
                        "{}/{}",
                        info.stats.authoritative_organisms_realized, info.stats.organisms_realized
                    ),
                ),
                (
                    "resonance",
                    &format!("{:.2}", info.stats.resonance_strength),
                ),
            ],
        );
        cur.triple(
            self,
            [
                (
                    "mode",
                    if info.transition_mode {
                        "trans"
                    } else {
                        "free"
                    },
                ),
                ("macro tiles", &format!("{}", info.macro_tiles)),
                ("rosters", &format!("{}", info.rosters)),
            ],
        );
        match info.vault {
            Some(v) => {
                cur.pair(
                    self,
                    "vault",
                    &format!("{}r {}d", v.records, v.dirty),
                    "seen",
                    &format!("{}", v.seen),
                );
                if v.issues > 0 || v.suppressed_issues > 0 {
                    cur.label_value(
                        self,
                        "vault issues",
                        &format!("{} +{} suppressed", v.issues, v.suppressed_issues),
                        ALERT,
                    );
                }
                if v.persistence_retries > 0 {
                    cur.label_value(
                        self,
                        "persist retry",
                        &format!("{}", v.persistence_retries),
                        ALERT,
                    );
                }
            }
            None => cur.label_value(self, "vault", "none (O save, L load)", LABEL),
        }
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

        // Cumulative per-layer regen counters, three layers per line
        // (phase-2-plan.md §11): watching these is how invalidation precision
        // is eyeballed live — an Ecology nudge must tick vegetation only.
        cur.line(self, "REGEN BY LAYER", HEADER);
        for group in (0..LAYER_COUNT as usize).step_by(3) {
            let entry = |layer: usize| {
                format!(
                    "{:<9}{:>6}",
                    world_core::layer::LAYERS[layer].name,
                    info.regen_totals[layer]
                )
            };
            let row_y = cur.y;
            for (slot, layer) in (group..(group + 3).min(LAYER_COUNT as usize)).enumerate() {
                self.text(
                    cur.x + slot * COL_CHARS * 8 * SCALE,
                    row_y,
                    &entry(layer),
                    VALUE,
                );
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

        // The cursor readout sits directly under the chan/player line so it
        // stays visible on every tier — the panel is taller than the Low-tier
        // map strip, and everything below this point may clip. The zoom level
        // rides the section header so the block height (and therefore the
        // panel layout) is unchanged by zooming.
        let zoom_suffix = if info.zoom > 1 {
            format!("  zoom x{}", info.zoom)
        } else {
            String::new()
        };
        if let Some(o) = &info.organism {
            // Zoomed-in organism picking: the organism under the mouse
            // replaces the region block (region info returns as soon as the
            // cursor leaves the marker).
            cur.line(self, &format!("ORGANISM{zoom_suffix}"), HEADER);
            cur.label_value(self, "id", &format!("{:016x}", o.id), VALUE);
            cur.label_value(self, "species", &format!("{:016x}", o.species), VALUE);
            // "decomposer" overflows a stat column, so trophic gets the
            // whole line.
            cur.label_value(self, "trophic", o.trophic, ACTIVE);
            cur.triple(
                self,
                [
                    ("size", &format!("{:.2}", o.size)),
                    ("hue", &format!("{:.2}", o.hue)),
                    ("lumin", &format!("{:.2}", o.luminance)),
                ],
            );
            cur.pair(
                self,
                "activity",
                &format!("{:.2}", o.activity),
                "aggression",
                &format!("{:.2}", o.aggression),
            );
            cur.label_value(
                self,
                "world",
                &format!("{:.0}, {:.0}", o.world.0, o.world.1),
                VALUE,
            );
            cur.rule(self);
        } else {
            self.draw_cursor_block(&mut cur, info, &zoom_suffix);
        }

        cur.line(self, "BIAS  1-8 up, shift down, Z", HEADER);
        for group in (0..POSSIBILITY_DIMS).step_by(3) {
            let row_y = cur.y;
            for (slot, i) in (group..(group + 3).min(POSSIBILITY_DIMS)).enumerate() {
                let value = info.bias[i];
                let color = if value.abs() > f32::EPSILON {
                    ACTIVE
                } else {
                    LABEL
                };
                self.text(
                    cur.x + slot * COL_CHARS * 8 * SCALE,
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

        cur.line(self, "KEYS", HEADER);
        for (keys, action) in [
            ("WASD 1-8 Z", "move, bias, reset"),
            ("E / Q / C", "anchors, clear"),
            ("T Y K", "categ,polar,capture"),
            ("R", "transition mode"),
            ("H J U Del", "paths: on,rec,attr,clr"),
            ("V G N X M", "channel,overlays"),
            ("scroll", "zoom (organism info)"),
        ] {
            let row_y = cur.y;
            self.text(cur.x, row_y, keys, KEY);
            self.text(cur.x + 13 * 8 * SCALE, row_y, action, LABEL);
            cur.y += LINE_HEIGHT;
        }
    }

    /// The region block under the cursor (the pre-zoom CURSOR section).
    fn draw_cursor_block(&mut self, cur: &mut PanelCursor, info: &PanelInfo<'_>, suffix: &str) {
        cur.line(self, &format!("CURSOR{suffix}"), HEADER);
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
                        cur.triple(
                            self,
                            [
                                ("elev", &format!("{e:.0}")),
                                ("temp", &format!("{t:.1}C")),
                                ("moisture", &format!("{m:.2}")),
                            ],
                        );
                        cur.triple(
                            self,
                            [
                                (
                                    "rock",
                                    &c.hardness.map_or("-".into(), |h| format!("{h:.2}")),
                                ),
                                ("river", &c.river.map_or("-".into(), |r| format!("{r:.2}"))),
                                ("wet", &c.wetness.map_or("-".into(), |w| format!("{w:.2}"))),
                            ],
                        );
                        cur.triple(
                            self,
                            [
                                (
                                    "soil",
                                    &c.soil_depth.map_or("-".into(), |d| format!("{d:.2}")),
                                ),
                                (
                                    "fert",
                                    &c.fertility.map_or("-".into(), |f| format!("{f:.2}")),
                                ),
                                (
                                    "veg",
                                    &c.vegetation.map_or("-".into(), |v| format!("{v:.2}")),
                                ),
                            ],
                        );
                        cur.pair(
                            self,
                            "canopy",
                            &c.canopy.map_or("-".into(), |h| format!("{h:.1}m")),
                            "biome",
                            c.biome.unwrap_or("?"),
                        );
                    }
                    _ => cur.line(self, "(tiles not generated yet)", LABEL),
                }
                if let Some(e) = &c.ecology {
                    cur.triple(
                        self,
                        [
                            ("species", &format!("{}", e.roster_size)),
                            ("domspp", &format!("{:04x}", e.dominant_id & 0xFFFF)),
                            ("herb", &format!("{:.3}", e.herbivore)),
                        ],
                    );
                    cur.triple(
                        self,
                        [
                            ("pred", &format!("{:.3}", e.predator)),
                            ("diversity", &format!("{:.2}", e.diversity)),
                            (
                                "P/H/C",
                                &format!(
                                    "{}/{}/{}",
                                    e.trophic_counts[0], e.trophic_counts[1], e.trophic_counts[3]
                                ),
                            ),
                        ],
                    );
                }
            }
        }
        cur.rule(self);
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

    /// Two label/value pairs on one line (fixed second column). The second
    /// value may run past its column, so pairs suit lines whose tail entry
    /// is open-ended (biome names, vault hints).
    fn pair(&mut self, hud: &mut Hud, l1: &str, v1: &str, l2: &str, v2: &str) {
        hud.text(self.x, self.y, l1, LABEL);
        hud.text(self.x + (l1.len() + 1) * 8 * SCALE, self.y, v1, VALUE);
        let col2 = self.x + COL_CHARS * 8 * SCALE;
        hud.text(col2, self.y, l2, LABEL);
        hud.text(col2 + (l2.len() + 1) * 8 * SCALE, self.y, v2, VALUE);
        self.y += LINE_HEIGHT;
    }

    /// Three label/value pairs on one line — the stat-grid row that keeps the
    /// panel's vertical extent inside the Low-tier map strip.
    fn triple(&mut self, hud: &mut Hud, entries: [(&str, &str); 3]) {
        for (slot, (label, value)) in entries.into_iter().enumerate() {
            let x = self.x + slot * COL_CHARS * 8 * SCALE;
            hud.text(x, self.y, label, LABEL);
            hud.text(x + (label.len() + 1) * 8 * SCALE, self.y, value, VALUE);
        }
        self.y += LINE_HEIGHT;
    }

    /// Blank half-line, separator, blank half-line.
    fn rule(&mut self, hud: &mut Hud) {
        self.y += LINE_HEIGHT / 3;
        hud.hline(self.y);
        self.y += LINE_HEIGHT * 2 / 3;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hud_chip_rasterizes_opaque_text_at_final_size() {
        let text = " 123 fps";
        let (rgba, w, h) = hud_chip(text);
        assert_eq!(w as usize, text.chars().count() * 8 * SCALE + 12);
        assert_eq!(h as usize, 8 * SCALE + 12);
        assert_eq!(rgba.len(), (w * h * 4) as usize);
        // Fully opaque, background-filled, with some glyph pixels lit.
        assert!(rgba.chunks_exact(4).all(|px| px[3] == 255));
        assert!(rgba.chunks_exact(4).any(|px| px[..3] == VALUE));
        assert!(rgba.chunks_exact(4).any(|px| px[..3] == BG));
    }

    fn info<'a>(
        regen: &'a [u64; LAYER_COUNT as usize],
        bias: &'a [f32; POSSIBILITY_DIMS],
        organism: Option<OrganismInfo>,
        zoom: u32,
    ) -> PanelInfo<'a> {
        PanelInfo {
            fps: 60,
            update_ms: 1.0,
            compose_ms: 1.0,
            render_ms: 1.0,
            upload_kb: 0.0,
            gpu_compose: false,
            tier: "low",
            cache_ceiling_bytes: 0,
            pass_ms: [0.0; world_runtime::PASS_COUNT],
            workers: 1,
            stats: FrameStats::default(),
            regen_totals: regen,
            macro_tiles: 0,
            rosters: 0,
            organisms: 0,
            jobs_in_flight: 0,
            pinned_violations: 0,
            channel: Channel::Composite,
            player: (0.0, 0.0),
            bias,
            anchors: &[],
            capture_category: "Morphology",
            capture_polarity: AnchorKind::Emphasize,
            transition_mode: false,
            vault: None,
            zoom,
            cursor: None,
            organism,
        }
    }

    /// The organism block draws (and differs from the region block) without
    /// panicking — the zoomed-in picking readout is renderable end to end.
    #[test]
    fn organism_block_renders_and_changes_the_panel() {
        let regen = [0u64; LAYER_COUNT as usize];
        let bias = [0.0f32; POSSIBILITY_DIMS];
        let side = 900usize;
        let map = vec![0u8; side * side * 4];

        let mut hud = Hud::new(side);
        let without = hud.compose(&map, &info(&regen, &bias, None, 4)).to_vec();

        let organism = OrganismInfo {
            id: 0x0123_4567_89AB_CDEF,
            species: 0xFEDC_BA98_7654_3210,
            trophic: "herbivore",
            world: (123.0, -456.0),
            hue: 0.25,
            luminance: 0.5,
            size: 1.5,
            activity: 0.7,
            aggression: 0.2,
        };
        let mut hud = Hud::new(side);
        let with = hud
            .compose(&map, &info(&regen, &bias, Some(organism), 4))
            .to_vec();
        assert_ne!(without, with, "organism block must change the panel");
    }
}
