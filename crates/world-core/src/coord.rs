//! Stable coordinate systems for the hierarchical world.
//!
//! The world is addressed at multiple levels (macro regions, world regions,
//! local cells — section 6.3 of the plan). Region identity is integer-based and
//! quantized from continuous world positions so that it never depends on
//! unstable floating-point behavior.

/// Edge length, in world units, of a level-0 region. World position is quantized
/// into regions by integer floor division of this value.
pub const REGION_SIZE: f64 = 256.0;

/// Integer identity of a region in the sparse region grid / quadtree.
///
/// `level` selects the hierarchy tier (0 = finest world region; higher levels
/// cover exponentially larger areas), matching the macro/world/local layering in
/// the plan. Coordinates are signed so the world extends infinitely in every
/// direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RegionCoord {
    /// Region column.
    pub x: i32,
    /// Region row.
    pub y: i32,
    /// Hierarchy level (0 = finest).
    pub level: u16,
}

impl RegionCoord {
    /// A region at the finest level.
    #[inline]
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y, level: 0 }
    }

    /// A region at an explicit hierarchy level.
    #[inline]
    #[must_use]
    pub const fn at_level(x: i32, y: i32, level: u16) -> Self {
        Self { x, y, level }
    }

    /// Quantize a continuous world position (x, y) into a level-0 region.
    ///
    /// Uses `f64::floor` division; the result is deterministic because the
    /// quantization boundary is an exact power-of-two-scaled constant and the
    /// output is an integer (no fractional state is retained).
    #[inline]
    #[must_use]
    pub fn from_world(world_x: f64, world_y: f64) -> Self {
        Self::new(
            (world_x / REGION_SIZE).floor() as i32,
            (world_y / REGION_SIZE).floor() as i32,
        )
    }

    /// The parent region one level coarser (each parent covers a 2×2 block of
    /// children), using arithmetic shift so negatives round toward negative
    /// infinity.
    #[inline]
    #[must_use]
    pub const fn parent(&self) -> Self {
        Self {
            x: self.x >> 1,
            y: self.y >> 1,
            level: self.level + 1,
        }
    }

    /// World-space position of this region's minimum (south-west) corner.
    #[inline]
    #[must_use]
    pub fn origin(&self) -> (f64, f64) {
        let scale = REGION_SIZE * (1u64 << self.level) as f64;
        (self.x as f64 * scale, self.y as f64 * scale)
    }
}

/// A position within a region, quantized to a fixed sub-grid.
///
/// Local cell coordinates are integers so that per-cell feature indices are
/// stable inputs to [`crate::feature_hash`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalPos {
    /// Cell column within the region.
    pub cx: u16,
    /// Cell row within the region.
    pub cy: u16,
}

impl LocalPos {
    /// Construct a local cell position.
    #[inline]
    #[must_use]
    pub const fn new(cx: u16, cy: u16) -> Self {
        Self { cx, cy }
    }

    /// Flatten to a single index given the region's cell resolution.
    #[inline]
    #[must_use]
    pub const fn to_index(&self, resolution: u16) -> u32 {
        self.cy as u32 * resolution as u32 + self.cx as u32
    }
}
