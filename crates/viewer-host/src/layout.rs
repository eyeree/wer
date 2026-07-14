//! Physical-pixel view layout values shared by rendering, input routing, and
//! picking (`native-web-alignment.md` section 5.4).

use world_core::{RegionCoord, REGION_SIZE};

/// Minimum map share of a split view.
pub const MIN_SPLIT_RATIO: f32 = 0.1;
/// Maximum map share of a split view.
pub const MAX_SPLIT_RATIO: f32 = 0.9;
/// Physical-pixel hit width centered on the split boundary.
pub const SPLIT_DIVIDER_HIT_WIDTH: u32 = 9;
/// Maximum normalized source-cell error allowed by documented projection
/// round trips at ordinary world coordinates. Near the extreme `i32` region
/// limits, callers should compare source coordinates instead because a `f64`
/// world position has fewer fractional bits available.
pub const MAP_ROUND_TRIP_CELL_TOLERANCE: f64 = 1.0e-9;

/// Which presentation panes are visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PresentationMode {
    /// Top-down map only.
    Map,
    /// First-person view only.
    Pov,
    /// Side-by-side map and first-person panes.
    Split,
}

impl PresentationMode {
    /// Stable id used by platform adapters and low-rate snapshots.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Map => "map",
            Self::Pov => "pov",
            Self::Split => "split",
        }
    }

    /// Parse an exact stable presentation id.
    #[must_use]
    pub fn parse(id: &str) -> Option<Self> {
        match id {
            "map" => Some(Self::Map),
            "pov" => Some(Self::Pov),
            "split" => Some(Self::Split),
            _ => None,
        }
    }
}

/// A concrete presentation pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ViewKind {
    /// Top-down map.
    Map,
    /// First-person view.
    Pov,
}

/// A half-open rectangle in physical surface pixels.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PixelRect {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width in physical pixels.
    pub width: u32,
    /// Height in physical pixels.
    pub height: u32,
}

impl PixelRect {
    /// Construct a rectangle.
    #[must_use]
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Exclusive right edge, saturated for malformed external dimensions.
    #[must_use]
    pub const fn right(self) -> u32 {
        self.x.saturating_add(self.width)
    }

    /// Exclusive bottom edge, saturated for malformed external dimensions.
    #[must_use]
    pub const fn bottom(self) -> u32 {
        self.y.saturating_add(self.height)
    }

    /// Whether a physical-pixel point is inside this half-open rectangle.
    #[must_use]
    pub const fn contains(self, x: u32, y: u32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// Whether a continuous physical-pixel edge coordinate is inside this
    /// half-open rectangle.
    #[must_use]
    pub fn contains_f64(self, x: f64, y: f64) -> bool {
        x.is_finite()
            && y.is_finite()
            && x >= f64::from(self.x)
            && x < f64::from(self.right())
            && y >= f64::from(self.y)
            && y < f64::from(self.bottom())
    }

    /// Convert a continuous physical-surface point to coordinates relative
    /// to this rectangle's top-left corner.
    ///
    /// The same half-open rule as [`Self::contains_f64`] applies. Keeping this
    /// transform beside the rectangle prevents camera picking adapters from
    /// independently reconstructing pane origins or accepting a point on the
    /// exclusive right/bottom edges.
    #[must_use]
    pub fn local_point(self, point: [f64; 2]) -> Option<[f64; 2]> {
        self.contains_f64(point[0], point[1])
            .then(|| [point[0] - f64::from(self.x), point[1] - f64::from(self.y)])
    }

    /// Whether `other` is wholly contained, including empty edge rectangles.
    #[must_use]
    pub const fn contains_rect(self, other: Self) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.right() <= self.right()
            && other.bottom() <= self.bottom()
    }

    /// Whether two non-empty half-open rectangles overlap.
    #[must_use]
    pub const fn overlaps(self, other: Self) -> bool {
        self.width > 0
            && self.height > 0
            && other.width > 0
            && other.height > 0
            && self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// The largest centered square contained by this rectangle.
    #[must_use]
    pub const fn fitted_square(self) -> Self {
        let side = if self.width < self.height {
            self.width
        } else {
            self.height
        };
        Self::new(
            self.x.saturating_add((self.width - side) / 2),
            self.y.saturating_add((self.height - side) / 2),
            side,
            side,
        )
    }

    /// The largest centered integer rectangle with the requested source
    /// aspect fitted inside this rectangle.
    ///
    /// The constrained dimension is rounded to its nearest physical pixel,
    /// so an unavoidable sub-pixel aspect error can remain. Invalid zero
    /// source dimensions return `None`; an empty destination produces a
    /// contained empty rectangle.
    #[must_use]
    pub const fn fitted_aspect(self, source_width: u32, source_height: u32) -> Option<Self> {
        if source_width == 0 || source_height == 0 {
            return None;
        }

        let available = self.width as u64 * source_height as u64;
        let requested = self.height as u64 * source_width as u64;
        let (width, height) = if available > requested {
            let width = (self.height as u64 * source_width as u64 + source_height as u64 / 2)
                / source_height as u64;
            (width as u32, self.height)
        } else {
            let height = (self.width as u64 * source_height as u64 + source_width as u64 / 2)
                / source_width as u64;
            (self.width, height as u32)
        };
        Some(Self::new(
            self.x.saturating_add((self.width - width) / 2),
            self.y.saturating_add((self.height - height) / 2),
            width,
            height,
        ))
    }
}

/// Persistent visibility/focus state; resolved pane rectangles are added by
/// the layout milestone and are never reconstructed independently by shells.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewLayout {
    /// Visible presentation mode.
    pub mode: PresentationMode,
    /// Pane receiving view-scoped input.
    pub focused: ViewKind,
    /// Map share of a future adjustable split; initially exactly `0.5`.
    pub split_ratio: f32,
}

impl Default for ViewLayout {
    fn default() -> Self {
        Self {
            mode: PresentationMode::Map,
            focused: ViewKind::Map,
            split_ratio: 0.5,
        }
    }
}

/// Fully resolved physical rectangles for one presentation frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedViewLayout {
    /// Physical content rectangle supplied by the platform.
    pub content: PixelRect,
    /// Normalized mode used to resolve the panes.
    pub mode: PresentationMode,
    /// Visible pane receiving view-scoped input.
    pub focused: ViewKind,
    /// Sanitized/clamped map share used in Split mode.
    pub split_ratio: f32,
    /// Full map pane, including letterbox area.
    pub map_pane: Option<PixelRect>,
    /// Full POV pane.
    pub pov_pane: Option<PixelRect>,
    /// Centered square map draw/pick rectangle.
    pub map_content: Option<PixelRect>,
    /// POV width/height for camera projection. Degenerate panes use `1.0`.
    pub pov_aspect: Option<f32>,
    /// Border rectangle to draw when Map owns focus.
    pub map_focus_border: Option<PixelRect>,
    /// Border rectangle to draw when POV owns focus.
    pub pov_focus_border: Option<PixelRect>,
    /// Split-boundary pointer hit area; it intentionally overlaps both panes.
    pub divider: Option<PixelRect>,
}

impl ResolvedViewLayout {
    /// Visible pane for `kind`.
    #[must_use]
    pub const fn pane(self, kind: ViewKind) -> Option<PixelRect> {
        match kind {
            ViewKind::Map => self.map_pane,
            ViewKind::Pov => self.pov_pane,
        }
    }

    /// Focus-border geometry for `kind`, if that visible pane owns focus.
    #[must_use]
    pub const fn focus_border(self, kind: ViewKind) -> Option<PixelRect> {
        match kind {
            ViewKind::Map => self.map_focus_border,
            ViewKind::Pov => self.pov_focus_border,
        }
    }
}

/// Resolve visibility, focus, fitted Map content, POV aspect, and split hit
/// geometry from one platform-supplied physical content rectangle.
#[must_use]
pub fn resolve_view_layout(content: PixelRect, requested: ViewLayout) -> ResolvedViewLayout {
    let split_ratio = if requested.split_ratio.is_finite() {
        requested
            .split_ratio
            .clamp(MIN_SPLIT_RATIO, MAX_SPLIT_RATIO)
    } else {
        0.5
    };

    let (map_pane, pov_pane, divider, focused) = match requested.mode {
        PresentationMode::Map => (Some(content), None, None, ViewKind::Map),
        PresentationMode::Pov => (None, Some(content), None, ViewKind::Pov),
        PresentationMode::Split => {
            let map_width = if content.width >= 2 {
                ((f64::from(content.width) * f64::from(split_ratio)).round() as u32)
                    .clamp(1, content.width - 1)
            } else if split_ratio >= 0.5 {
                content.width
            } else {
                0
            };
            let map = PixelRect::new(content.x, content.y, map_width, content.height);
            let pov = PixelRect::new(
                content.x.saturating_add(map_width),
                content.y,
                content.width - map_width,
                content.height,
            );
            let hit_width = SPLIT_DIVIDER_HIT_WIDTH.min(content.width);
            let boundary = map.right();
            let max_left = content.right().saturating_sub(hit_width);
            let hit_left = boundary
                .saturating_sub(hit_width / 2)
                .clamp(content.x, max_left.max(content.x));
            let divider = (hit_width > 0).then_some(PixelRect::new(
                hit_left,
                content.y,
                hit_width,
                content.height,
            ));
            let focused = requested.focused;
            (Some(map), Some(pov), divider, focused)
        }
    };

    let map_content = map_pane.map(PixelRect::fitted_square);
    let pov_aspect = pov_pane.map(|pane| {
        if pane.width == 0 || pane.height == 0 {
            1.0
        } else {
            pane.width as f32 / pane.height as f32
        }
    });
    ResolvedViewLayout {
        content,
        mode: requested.mode,
        focused,
        split_ratio,
        map_pane,
        pov_pane,
        map_content,
        pov_aspect,
        map_focus_border: (focused == ViewKind::Map).then_some(map_pane).flatten(),
        pov_focus_border: (focused == ViewKind::Pov).then_some(pov_pane).flatten(),
        divider,
    }
}

/// Exact transform between a fitted physical Map rectangle, base source-cell
/// coordinates, and world coordinates. Source `(0, 0)` is the view's
/// north-west edge; source y grows south. Coordinates are continuous pixel
/// edges, which makes forward and inverse transforms exact at any scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapViewportProjection {
    /// Exact square destination used for both draw and picking.
    pub destination: PixelRect,
    /// Base source image edge in field cells.
    pub source_side: u32,
    /// Field cells per region edge. Grid widening cannot exceed this value
    /// without erasing the distinction between adjacent boundaries.
    pub region_resolution: u16,
    /// Integer center magnification.
    pub zoom: u32,
    /// World x at the base source's west edge.
    pub world_west: f64,
    /// World y at the base source's north edge.
    pub world_north: f64,
    /// World units represented by one base source cell.
    pub world_units_per_source: f64,
}

/// Screen-space coverage produced by the shared region-grid width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapGridCoverage {
    /// Unclamped source-cell width needed to span one physical destination
    /// pixel after zoom.
    pub required_source_cells: f32,
    /// Width passed to map presentation, clamped to one region's resolution.
    pub source_cells: f32,
    /// Continuous destination-pixel width produced by `source_cells`.
    pub destination_pixels: f32,
    /// Whether distinct adjacent boundaries are at least one physical pixel
    /// apart, making the one-covered-pixel guarantee physically possible.
    ///
    /// When false, `source_cells` remains a best-effort resolution-clamped
    /// width; no rasterization can show every distinct interior boundary in
    /// the available destination pixels.
    pub one_pixel_feasible: bool,
}

impl MapViewportProjection {
    /// Construct the exact projection used by a `MapComposer` configured with
    /// the same half-extent and field resolution. Invalid generation geometry
    /// (`half_regions < 0` or `resolution == 0`) returns `None`.
    #[must_use]
    pub fn new(
        destination: PixelRect,
        player: (f64, f64),
        half_regions: i32,
        resolution: u16,
        zoom: u32,
    ) -> Option<Self> {
        if half_regions < 0 || resolution == 0 {
            return None;
        }
        let span = i64::from(half_regions).checked_mul(2)?.checked_add(1)?;
        let source_side = span.checked_mul(i64::from(resolution))?;
        let source_side = u32::try_from(source_side).ok()?;
        let center = RegionCoord::from_world(player.0, player.1);
        Some(Self {
            destination,
            source_side,
            region_resolution: resolution,
            zoom: zoom.max(1),
            world_west: (i64::from(center.x) - i64::from(half_regions)) as f64 * REGION_SIZE,
            world_north: (i64::from(center.y) + i64::from(half_regions) + 1) as f64 * REGION_SIZE,
            world_units_per_source: REGION_SIZE / f64::from(resolution),
        })
    }

    /// Convert world coordinates to base source-cell edge coordinates.
    #[must_use]
    pub fn world_to_source(self, world: (f64, f64)) -> (f64, f64) {
        (
            (world.0 - self.world_west) / self.world_units_per_source,
            (self.world_north - world.1) / self.world_units_per_source,
        )
    }

    /// Convert base source-cell edge coordinates to world coordinates.
    #[must_use]
    pub fn source_to_world(self, source: (f64, f64)) -> (f64, f64) {
        (
            self.world_west + source.0 * self.world_units_per_source,
            self.world_north - source.1 * self.world_units_per_source,
        )
    }

    /// Project a base source coordinate into the exact fitted physical rect.
    /// Returns `None` when zoom crops it or the destination is empty.
    #[must_use]
    pub fn source_to_physical(self, source: (f64, f64)) -> Option<(f64, f64)> {
        if self.destination.width == 0 || self.destination.height == 0 {
            return None;
        }
        let side = f64::from(self.source_side);
        let center = side * 0.5;
        let zoom = f64::from(self.zoom);
        let displayed = (
            (source.0 - center) * zoom + center,
            (source.1 - center) * zoom + center,
        );
        if !displayed.0.is_finite()
            || !displayed.1.is_finite()
            || displayed.0 < 0.0
            || displayed.1 < 0.0
            || displayed.0 >= side
            || displayed.1 >= side
        {
            return None;
        }
        Some((
            f64::from(self.destination.x) + displayed.0 / side * f64::from(self.destination.width),
            f64::from(self.destination.y) + displayed.1 / side * f64::from(self.destination.height),
        ))
    }

    /// Invert a physical point through the exact fitted rect into base source
    /// coordinates. Letterbox and outside points return `None`.
    #[must_use]
    pub fn physical_to_source(self, physical: (f64, f64)) -> Option<(f64, f64)> {
        if !self.destination.contains_f64(physical.0, physical.1) {
            return None;
        }
        let side = f64::from(self.source_side);
        let displayed = (
            (physical.0 - f64::from(self.destination.x)) / f64::from(self.destination.width) * side,
            (physical.1 - f64::from(self.destination.y)) / f64::from(self.destination.height)
                * side,
        );
        let center = side * 0.5;
        let zoom = f64::from(self.zoom);
        Some((
            (displayed.0 - center) / zoom + center,
            (displayed.1 - center) / zoom + center,
        ))
    }

    /// Base source texel selected at a continuous physical coordinate.
    ///
    /// In particular, passing a destination physical-pixel center selects the
    /// same texel as the CPU nearest-neighbor raster and the GPU atlas shader.
    /// This direct continuous-edge convention deliberately does not add the
    /// legacy composer's extra half-cell picking offset.
    #[must_use]
    pub fn physical_to_source_texel(self, physical: (f64, f64)) -> Option<(u32, u32)> {
        self.physical_to_source(physical).map(|source| {
            let last = self.source_side - 1;
            (
                (source.0.floor() as u32).min(last),
                (source.1.floor() as u32).min(last),
            )
        })
    }

    /// Project a visible world point into physical pixels.
    #[must_use]
    pub fn world_to_physical(self, world: (f64, f64)) -> Option<(f64, f64)> {
        self.source_to_physical(self.world_to_source(world))
    }

    /// Invert a physical point inside the fitted Map rect into world space.
    #[must_use]
    pub fn physical_to_world(self, physical: (f64, f64)) -> Option<(f64, f64)> {
        self.physical_to_source(physical)
            .map(|source| self.source_to_world(source))
    }

    /// Grid width and its physical feasibility for this destination.
    ///
    /// The source width is clamped to the same one-region maximum as the CPU
    /// composer. Consequently, `one_pixel_feasible` is the precondition for
    /// claiming that every sampled visible interior boundary covers at least
    /// one physical pixel.
    #[must_use]
    pub fn grid_coverage(self) -> MapGridCoverage {
        let physical_side = self.destination.width.min(self.destination.height);
        let resolution = f64::from(self.region_resolution);
        if physical_side == 0 {
            return MapGridCoverage {
                required_source_cells: f32::INFINITY,
                source_cells: self.region_resolution.into(),
                destination_pixels: 0.0,
                one_pixel_feasible: false,
            };
        }

        let physical_per_source =
            f64::from(physical_side) * f64::from(self.zoom) / f64::from(self.source_side);
        let required = 1.0 / physical_per_source;
        let source_cells = required.clamp(1.0, resolution);
        MapGridCoverage {
            required_source_cells: required as f32,
            source_cells: source_cells as f32,
            destination_pixels: (source_cells * physical_per_source) as f32,
            one_pixel_feasible: required <= resolution,
        }
    }

    /// Best-effort base source-cell grid thickness. CPU paths ceil this value;
    /// GPU paths use the same float threshold. Consult [`Self::grid_coverage`]
    /// before claiming minimum-one-physical-pixel coverage.
    #[must_use]
    pub fn grid_thickness_source_pixels(self) -> f32 {
        self.grid_coverage().source_cells
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_rect_is_half_open_and_square_fit_is_contained() {
        let rect = PixelRect::new(10, 20, 101, 60);
        assert!(rect.contains(10, 20));
        assert!(rect.contains(110, 79));
        assert!(!rect.contains(111, 79));
        assert!(!rect.contains(110, 80));
        assert_eq!(rect.fitted_square(), PixelRect::new(30, 20, 60, 60));
    }

    #[test]
    fn pane_local_points_preserve_fractional_pixels_and_half_open_edges() {
        let pane = PixelRect::new(13, 29, 101, 67);
        assert_eq!(pane.local_point([13.0, 29.0]), Some([0.0, 0.0]));
        assert_eq!(pane.local_point([113.75, 95.5]), Some([100.75, 66.5]));
        assert_eq!(pane.local_point([114.0, 29.0]), None);
        assert_eq!(pane.local_point([13.0, 96.0]), None);
        assert_eq!(pane.local_point([12.999, 40.0]), None);
        assert_eq!(pane.local_point([f64::NAN, 40.0]), None);
        assert_eq!(pane.local_point([40.0, f64::INFINITY]), None);
    }

    #[test]
    fn arbitrary_aspect_fit_is_centered_contained_and_handles_odd_pixels() {
        for (parent, source, expected) in [
            (
                PixelRect::new(10, 20, 101, 61),
                (16, 9),
                PixelRect::new(10, 22, 101, 57),
            ),
            (
                PixelRect::new(10, 20, 61, 101),
                (9, 16),
                PixelRect::new(12, 20, 57, 101),
            ),
            (
                PixelRect::new(3, 5, 101, 60),
                (1, 1),
                PixelRect::new(23, 5, 60, 60),
            ),
            (
                PixelRect::new(3, 5, 100, 61),
                (4, 3),
                PixelRect::new(12, 5, 81, 61),
            ),
            (
                PixelRect::new(4, 5, 0, 9),
                (16, 9),
                PixelRect::new(4, 9, 0, 0),
            ),
        ] {
            let fitted = parent.fitted_aspect(source.0, source.1).unwrap();
            assert_eq!(fitted, expected);
            assert!(parent.contains_rect(fitted));
        }
        assert_eq!(PixelRect::new(0, 0, 10, 10).fitted_aspect(0, 9), None);
        assert_eq!(PixelRect::new(0, 0, 10, 10).fitted_aspect(16, 0), None);
    }

    #[test]
    fn default_layout_is_the_existing_map_view() {
        assert_eq!(
            ViewLayout::default(),
            ViewLayout {
                mode: PresentationMode::Map,
                focused: ViewKind::Map,
                split_ratio: 0.5,
            }
        );
    }

    #[test]
    fn presentation_ids_are_exact_and_round_trip() {
        for mode in [
            PresentationMode::Map,
            PresentationMode::Pov,
            PresentationMode::Split,
        ] {
            assert_eq!(PresentationMode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(PresentationMode::parse("POV"), None);
    }

    fn physical_rect(css: (u32, u32), dpr: f64) -> PixelRect {
        PixelRect::new(
            7,
            11,
            (f64::from(css.0) * dpr).round() as u32,
            (f64::from(css.1) * dpr).round() as u32,
        )
    }

    #[test]
    fn layout_table_is_contained_square_and_non_overlapping() {
        let css_sizes = [
            (1280, 720),
            (720, 1280),
            (901, 701),
            (333, 199),
            (3, 2),
            (1, 1),
            (0, 0),
        ];
        let dprs = [1.0, 1.25, 1.5, 2.0];
        let layouts = [
            ViewLayout {
                mode: PresentationMode::Map,
                focused: ViewKind::Pov,
                split_ratio: 0.5,
            },
            ViewLayout {
                mode: PresentationMode::Pov,
                focused: ViewKind::Map,
                split_ratio: 0.5,
            },
            ViewLayout {
                mode: PresentationMode::Split,
                focused: ViewKind::Map,
                split_ratio: 0.33,
            },
            ViewLayout {
                mode: PresentationMode::Split,
                focused: ViewKind::Pov,
                split_ratio: 0.77,
            },
        ];

        for css in css_sizes {
            for dpr in dprs {
                let content = physical_rect(css, dpr);
                for requested in layouts {
                    let resolved = resolve_view_layout(content, requested);
                    assert_eq!(resolved.content, content);
                    assert_eq!(resolved.mode, requested.mode);
                    for rect in [
                        resolved.map_pane,
                        resolved.pov_pane,
                        resolved.map_content,
                        resolved.map_focus_border,
                        resolved.pov_focus_border,
                        resolved.divider,
                    ]
                    .into_iter()
                    .flatten()
                    {
                        assert!(
                            content.contains_rect(rect),
                            "{rect:?} escaped {content:?} for {requested:?} at DPR {dpr}"
                        );
                    }
                    if let Some(map_content) = resolved.map_content {
                        assert_eq!(map_content.width, map_content.height);
                        assert!(resolved.map_pane.unwrap().contains_rect(map_content));
                    }
                    if let Some(pov) = resolved.pov_pane {
                        let expected = if pov.width == 0 || pov.height == 0 {
                            1.0
                        } else {
                            pov.width as f32 / pov.height as f32
                        };
                        assert_eq!(resolved.pov_aspect, Some(expected));
                    } else {
                        assert_eq!(resolved.pov_aspect, None);
                    }
                    if let (Some(map), Some(pov)) = (resolved.map_pane, resolved.pov_pane) {
                        assert!(!map.overlaps(pov));
                        assert_eq!(map.right(), pov.x);
                        assert_eq!(map.width + pov.width, content.width);
                    }
                    assert_eq!(
                        resolved.focus_border(resolved.focused),
                        resolved.pane(resolved.focused)
                    );
                    assert_eq!(
                        resolved.focus_border(match resolved.focused {
                            ViewKind::Map => ViewKind::Pov,
                            ViewKind::Pov => ViewKind::Map,
                        }),
                        None
                    );
                    assert_eq!(
                        resolved.divider.is_some(),
                        requested.mode == PresentationMode::Split && content.width > 0
                    );
                }
            }
        }
    }

    #[test]
    fn split_ratio_is_sanitized_and_clamped_before_pixel_partition() {
        let content = PixelRect::new(13, 17, 101, 55);
        for (requested, expected) in [
            (-1.0, MIN_SPLIT_RATIO),
            (0.05, MIN_SPLIT_RATIO),
            (0.1, 0.1),
            (0.5, 0.5),
            (0.9, 0.9),
            (0.95, MAX_SPLIT_RATIO),
            (2.0, MAX_SPLIT_RATIO),
            (f32::NAN, 0.5),
            (f32::INFINITY, 0.5),
        ] {
            let resolved = resolve_view_layout(
                content,
                ViewLayout {
                    mode: PresentationMode::Split,
                    focused: ViewKind::Map,
                    split_ratio: requested,
                },
            );
            assert_eq!(resolved.split_ratio, expected);
            let map = resolved.map_pane.unwrap();
            let pov = resolved.pov_pane.unwrap();
            assert!((1..content.width).contains(&map.width));
            assert_eq!(map.width + pov.width, content.width);
        }
    }

    #[test]
    fn map_projection_round_trips_through_the_exact_fitted_rect() {
        let css_sizes = [(1280, 720), (720, 1280), (901, 701), (17, 11), (1, 1)];
        let dprs = [1.0, 1.25, 1.5, 2.0];
        let zooms = [1, 2, 4, 8, 16];
        // Far enough to exercise a non-origin region, while deliberately far
        // from the i32 coordinate extremes scoped out by the tolerance docs.
        let player = (1_000_000.25, -2_000_000.5);

        for css in css_sizes {
            for dpr in dprs {
                let resolved = resolve_view_layout(
                    physical_rect(css, dpr),
                    ViewLayout {
                        mode: PresentationMode::Split,
                        focused: ViewKind::Map,
                        split_ratio: 0.37,
                    },
                );
                let map_rect = resolved.map_content.unwrap();
                if map_rect.width == 0 {
                    continue;
                }
                for zoom in zooms {
                    let projection =
                        MapViewportProjection::new(map_rect, player, 3, 16, zoom).unwrap();
                    for (fx, fy) in [(0.125, 0.2), (0.5, 0.5), (0.875, 0.8)] {
                        let physical = (
                            f64::from(map_rect.x) + f64::from(map_rect.width) * fx,
                            f64::from(map_rect.y) + f64::from(map_rect.height) * fy,
                        );
                        let source = projection.physical_to_source(physical).unwrap();
                        let world = projection.physical_to_world(physical).unwrap();
                        let physical_again = projection.world_to_physical(world).unwrap();
                        assert!((physical_again.0 - physical.0).abs() < 1.0e-6);
                        assert!((physical_again.1 - physical.1).abs() < 1.0e-6);

                        let source_again = projection.world_to_source(world);
                        let normalized_error = (source_again.0 - source.0)
                            .abs()
                            .max((source_again.1 - source.1).abs());
                        assert!(
                            normalized_error <= MAP_ROUND_TRIP_CELL_TOLERANCE,
                            "source error {normalized_error} at {css:?} DPR {dpr} zoom {zoom}"
                        );
                        let world_again = projection.source_to_world(source);
                        let world_error = (world_again.0 - world.0)
                            .abs()
                            .max((world_again.1 - world.1).abs());
                        assert!(
                            world_error
                                <= projection.world_units_per_source
                                    * MAP_ROUND_TRIP_CELL_TOLERANCE
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn physical_pixel_centers_select_the_cpu_and_gpu_source_texel() {
        fn cpu_axis(projection: MapViewportProjection, index: u32, extent: u32) -> u32 {
            let side = f64::from(projection.source_side);
            let output = ((f64::from(index) + 0.5) / f64::from(extent) * side).floor();
            let center = side * 0.5;
            (((output + 0.5 - center) / f64::from(projection.zoom) + center).floor() as u32)
                .min(projection.source_side - 1)
        }

        fn gpu_axis(projection: MapViewportProjection, index: u32, extent: u32) -> u32 {
            let side = f64::from(projection.source_side);
            let center = side * 0.5;
            ((((f64::from(index) + 0.5) / f64::from(extent) * side - center)
                / f64::from(projection.zoom)
                + center)
                .floor() as u32)
                .min(projection.source_side - 1)
        }

        let mut boundary_samples = 0;
        for css in [(1280, 720), (901, 701), (17, 11), (3, 2), (1, 1)] {
            for dpr in [1.0, 1.25, 1.5, 2.0] {
                let destination = physical_rect(css, dpr).fitted_square();
                if destination.width == 0 {
                    continue;
                }
                for zoom in [1, 2, 4, 8, 16] {
                    let projection =
                        MapViewportProjection::new(destination, (300.25, -10.5), 3, 16, zoom)
                            .unwrap();
                    let middle = destination.height / 2;
                    for column in 0..destination.width {
                        let physical = (
                            f64::from(destination.x + column) + 0.5,
                            f64::from(destination.y + middle) + 0.5,
                        );
                        let selected = projection.physical_to_source_texel(physical).unwrap();
                        assert_eq!(selected.0, cpu_axis(projection, column, destination.width));
                        assert_eq!(selected.0, gpu_axis(projection, column, destination.width));

                        let world = projection.physical_to_world(physical).unwrap();
                        let source_again = projection.world_to_source(world);
                        let source = projection.physical_to_source(physical).unwrap();
                        assert!((source_again.0 - source.0).abs() <= MAP_ROUND_TRIP_CELL_TOLERANCE);
                        assert!((source_again.1 - source.1).abs() <= MAP_ROUND_TRIP_CELL_TOLERANCE);
                        assert_eq!(source_again.0.floor() as u32, selected.0);
                        if selected.0 % u32::from(projection.region_resolution) <= 1
                            || selected.0 % u32::from(projection.region_resolution)
                                >= u32::from(projection.region_resolution) - 2
                        {
                            boundary_samples += 1;
                        }
                    }

                    let middle = destination.width / 2;
                    for row in 0..destination.height {
                        let physical = (
                            f64::from(destination.x + middle) + 0.5,
                            f64::from(destination.y + row) + 0.5,
                        );
                        let selected = projection.physical_to_source_texel(physical).unwrap();
                        assert_eq!(selected.1, cpu_axis(projection, row, destination.height));
                        assert_eq!(selected.1, gpu_axis(projection, row, destination.height));
                    }
                }
            }
        }
        assert!(
            boundary_samples > 0,
            "matrix must exercise region-boundary texels"
        );
    }

    #[test]
    fn feasible_grid_width_survives_actual_destination_sampling() {
        let css_sizes = [(1280, 720), (333, 199), (31, 17), (3, 2), (1, 1)];
        let layouts = [
            ViewLayout::default(),
            ViewLayout {
                mode: PresentationMode::Split,
                focused: ViewKind::Map,
                split_ratio: 0.1,
            },
            ViewLayout {
                mode: PresentationMode::Split,
                focused: ViewKind::Map,
                split_ratio: 0.33,
            },
            ViewLayout {
                mode: PresentationMode::Split,
                focused: ViewKind::Map,
                split_ratio: 0.5,
            },
            ViewLayout {
                mode: PresentationMode::Split,
                focused: ViewKind::Map,
                split_ratio: 0.9,
            },
        ];
        let mut feasible_cases = 0;
        let mut infeasible_cases = 0;
        let mut sampled_boundaries = 0;
        for css in css_sizes {
            for dpr in [1.0, 1.25, 1.5, 2.0] {
                let content = physical_rect(css, dpr);
                for layout in layouts {
                    let map_rect = resolve_view_layout(content, layout).map_content.unwrap();
                    if map_rect.width == 0 {
                        continue;
                    }
                    for zoom in [1, 2, 4, 8, 16] {
                        let projection =
                            MapViewportProjection::new(map_rect, (0.0, 0.0), 3, 16, zoom).unwrap();
                        let coverage = projection.grid_coverage();
                        assert_eq!(
                            projection.grid_thickness_source_pixels(),
                            coverage.source_cells
                        );
                        assert!(coverage.source_cells <= f32::from(projection.region_resolution));
                        if !coverage.one_pixel_feasible {
                            infeasible_cases += 1;
                            assert_eq!(
                                coverage.source_cells,
                                f32::from(projection.region_resolution)
                            );
                            assert!(coverage.destination_pixels < 1.0);
                            continue;
                        }
                        feasible_cases += 1;
                        assert!(coverage.destination_pixels >= 1.0 - 1.0e-6);

                        let cells = (coverage.source_cells.ceil() as u32)
                            .clamp(1, u32::from(projection.region_resolution));
                        let x_samples = (0..map_rect.width)
                            .map(|column| {
                                projection
                                    .physical_to_source_texel((
                                        f64::from(map_rect.x + column) + 0.5,
                                        f64::from(map_rect.y) + 0.5,
                                    ))
                                    .unwrap()
                                    .0
                            })
                            .collect::<Vec<_>>();
                        let y_samples = (0..map_rect.height)
                            .map(|row| {
                                projection
                                    .physical_to_source_texel((
                                        f64::from(map_rect.x) + 0.5,
                                        f64::from(map_rect.y + row) + 0.5,
                                    ))
                                    .unwrap()
                                    .1
                            })
                            .collect::<Vec<_>>();

                        for boundary in (u32::from(projection.region_resolution)
                            ..projection.source_side)
                            .step_by(usize::from(projection.region_resolution))
                        {
                            // A boundary is visibly interior only when actual
                            // destination samples occur on both sides of it.
                            if x_samples.first().unwrap() < &boundary
                                && x_samples.last().unwrap() >= &boundary
                            {
                                let covered = x_samples
                                    .iter()
                                    .filter(|&&cell| cell >= boundary && cell < boundary + cells)
                                    .count();
                                assert!(
                                    covered >= 1,
                                    "vertical boundary {boundary} vanished for {map_rect:?}, zoom {zoom}"
                                );
                                sampled_boundaries += 1;
                            }
                            if y_samples.first().unwrap() < &boundary
                                && y_samples.last().unwrap() >= &boundary
                            {
                                let covered = y_samples
                                    .iter()
                                    .filter(|&&cell| cell >= boundary - cells && cell < boundary)
                                    .count();
                                assert!(
                                    covered >= 1,
                                    "horizontal boundary {boundary} vanished for {map_rect:?}, zoom {zoom}"
                                );
                                sampled_boundaries += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(feasible_cases > 0);
        assert!(infeasible_cases > 0);
        assert!(sampled_boundaries > 0);

        let one_pixel =
            MapViewportProjection::new(PixelRect::new(0, 0, 1, 1), (0.0, 0.0), 3, 16, 1)
                .unwrap()
                .grid_coverage();
        assert_eq!(one_pixel.required_source_cells, 112.0);
        assert_eq!(one_pixel.source_cells, 16.0);
        assert!((one_pixel.destination_pixels - 1.0 / 7.0).abs() < 1.0e-6);
        assert!(!one_pixel.one_pixel_feasible);
    }

    #[test]
    fn projection_rejects_letterbox_outside_and_invalid_generation_geometry() {
        let pane = PixelRect::new(0, 0, 100, 50);
        let map_rect = pane.fitted_square();
        assert_eq!(map_rect, PixelRect::new(25, 0, 50, 50));
        let projection = MapViewportProjection::new(map_rect, (0.0, 0.0), 1, 8, 4).unwrap();
        assert!(projection.physical_to_world((24.999, 25.0)).is_none());
        assert!(projection.physical_to_world((75.0, 25.0)).is_none());
        assert!(projection
            .physical_to_source_texel((24.999, 25.0))
            .is_none());
        assert!(projection.physical_to_source_texel((75.0, 25.0)).is_none());
        assert!(projection.physical_to_source_texel((25.5, 0.5)).is_some());
        assert!(projection
            .source_to_physical((0.0, f64::from(projection.source_side) * 0.5))
            .is_none());
        assert!(MapViewportProjection::new(map_rect, (0.0, 0.0), -1, 8, 1).is_none());
        assert!(MapViewportProjection::new(map_rect, (0.0, 0.0), 1, 0, 1).is_none());
    }
}
