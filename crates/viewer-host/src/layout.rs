//! Physical-pixel view layout values shared by rendering, input routing, and
//! picking (`native-web-alignment.md` section 5.4).

/// Which presentation panes are visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

    /// The largest centered square contained by this rectangle.
    #[must_use]
    pub const fn fitted_square(self) -> Self {
        let side = if self.width < self.height {
            self.width
        } else {
            self.height
        };
        Self::new(
            self.x + (self.width - side) / 2,
            self.y + (self.height - side) / 2,
            side,
            side,
        )
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
}
