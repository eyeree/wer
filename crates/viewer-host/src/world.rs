//! Common exploration-state values. `ViewerController` becomes the sole
//! update authority in Milestone 3.

/// One authoritative traveler shared by Map, POV, and Split presentations.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct TravelerState {
    /// Current world XY used for streaming and inspection.
    pub position: (f64, f64),
    /// Position at the previous logical tick.
    pub previous_position: (f64, f64),
}

impl TravelerState {
    /// Distance contributed to convergence by this logical tick.
    #[must_use]
    pub fn travel(self) -> f64 {
        f64::hypot(
            self.position.0 - self.previous_position.0,
            self.position.1 - self.previous_position.1,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traveler_distance_is_computed_once_from_shared_xy() {
        let traveler = TravelerState {
            previous_position: (1.0, 2.0),
            position: (4.0, 6.0),
        };
        assert_eq!(traveler.travel(), 5.0);
    }
}
