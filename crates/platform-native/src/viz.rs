//! Compatibility exports for the canonical shared map presenter.
//!
//! Map composition moved to viewer_host::map in native/web alignment
//! Milestone 4. Keeping this module thin preserves native call sites while the
//! platform shell is migrated independently.

pub use viewer_host::map::{Channel, MapComposer, MapDecor, Overlays};
