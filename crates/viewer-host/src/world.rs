//! Platform-neutral exploration state and the single world-update authority
//! (`native-web-alignment.md` sections 4.2, 4.3, and 5.3).
//!
//! [`ExplorationWorld`] deliberately owns no storage or executor
//! implementation. A thin platform service may contribute derived route
//! anchors immediately before the update and observe the completed update
//! immediately after it, but only this module calls [`RegionMap::update`].

use world_core::{
    Anchor, PossibilityField, RouteRecord, RouteRecorderSnapshot, RouteTrackerSnapshot,
    POSSIBILITY_DIMS,
};
use world_runtime::{
    Budget, FrameStats, RegionMap, ResourceTier, RouteRecorder, RouteTracker, StreamConfig,
    TaskExecutor,
};

/// Map navigation speed retained from both pre-alignment viewers.
pub const MAP_MOVEMENT_SPEED: f64 = 500.0;

/// Maximum simulation delta consumed by one logical viewer tick.
pub const MAX_TICK_SECONDS: f64 = 0.1;

/// One authoritative traveler shared by Map, POV, and Split presentations.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct TravelerState {
    /// Current world XY used for streaming, records, and inspection.
    pub position: (f64, f64),
    /// Position at the previous completed logical tick.
    pub previous_position: (f64, f64),
}

impl TravelerState {
    /// Construct a traveler whose first update contributes no travel.
    #[must_use]
    pub const fn at(position: (f64, f64)) -> Self {
        Self {
            position,
            previous_position: position,
        }
    }

    /// Distance contributed to convergence by this logical tick.
    #[must_use]
    pub fn travel(self) -> f64 {
        f64::hypot(
            self.position.0 - self.previous_position.0,
            self.position.1 - self.previous_position.1,
        )
    }

    /// Finish a logical tick after every consumer has observed its travel.
    fn finish_tick(&mut self) {
        self.previous_position = self.position;
    }
}

/// Values available to a neutral/platform service immediately before the
/// single runtime update.
#[derive(Debug, Clone, Copy)]
pub struct WorldPreUpdate<'a> {
    /// New traveler position after continuous input was applied.
    pub traveler: (f64, f64),
    /// Active temporal budget, including the route-attraction node cap.
    pub budget: &'a Budget,
    /// Whether the optional route/path subsystem is enabled.
    pub path_tracking: bool,
    /// Whether retained routes should currently attract the traveler.
    pub route_attraction: bool,
}

/// Platform-owned persistent data needed by the shared logical route state
/// during one update.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct WorldServiceInput {
    /// Weak route anchors derived at the new traveler position.
    pub derived_anchors: Vec<Anchor>,
    /// Active retained routes in canonical id order. The shared tracker owns
    /// leg state and consumes these records; the platform service owns their
    /// durable storage.
    pub active_routes: Vec<RouteRecord>,
}

/// Values available to a neutral/platform service immediately after the
/// single runtime update.
#[derive(Debug)]
pub struct WorldPostUpdate<'a> {
    /// The updated authoritative region map.
    pub map: &'a RegionMap,
    /// Traveler position used by the update.
    pub traveler: (f64, f64),
    /// Travel passed to the runtime exactly once.
    pub travel: f64,
    /// Manual/captured anchors followed by derived anchors in their canonical
    /// service-provided order.
    pub effective_anchors: &'a [Anchor],
    /// Retained route ids whose traversal leg completed this tick.
    pub traversed_route_ids: &'a [u64],
    /// Runtime counters. Services may add their Flush timing/counters here;
    /// they must not perform a second world update.
    pub stats: &'a mut FrameStats,
}

/// Neutral seam for vault/route behavior that surrounds a world update.
///
/// Native uses this to retain route attraction, route recording, discovered
/// region tracking, traversal bumps, and budgeted vault flushing. Browser
/// hosts without those services use the default no-op implementation. The
/// hook contains no filesystem, thread, DOM, or window API.
pub trait WorldTickHook {
    /// Supply retained route data at the *new* traveler position.
    fn before_world_update(&mut self, _input: WorldPreUpdate<'_>) -> WorldServiceInput {
        WorldServiceInput::default()
    }

    /// Observe the one completed update and optionally account for Flush work.
    fn after_world_update(&mut self, _output: WorldPostUpdate<'_>) {}
}

/// Browser/headless hook when no route or persistence service is attached.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopWorldTickHook;

impl WorldTickHook for NoopWorldTickHook {}

/// Result of exactly one [`ExplorationWorld::update`] call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldTickOutput {
    /// Monotonic count of logical world updates in this controller.
    pub update_serial: u64,
    /// Traveler position used as the streaming center.
    pub traveler: (f64, f64),
    /// Distance supplied to convergence.
    pub travel: f64,
    /// Runtime update counters, including optional post-hook Flush accounting.
    pub stats: FrameStats,
}

/// Shared exploration state above generation and below the platform shells.
///
/// The map, possibility recipe, steering state, and traveler live together so
/// a shell cannot accidentally update Map and POV around different centers.
#[derive(Debug)]
pub struct ExplorationWorld {
    map: RegionMap,
    field: PossibilityField,
    anchors: Vec<Anchor>,
    bias: [f32; POSSIBILITY_DIMS],
    traveler: TravelerState,
    transition_mode: bool,
    path_tracking: bool,
    route_attraction: bool,
    recorder: Option<RouteRecorder>,
    tracker: RouteTracker,
    budget: Budget,
    tier: ResourceTier,
    update_serial: u64,
}

impl ExplorationWorld {
    /// Construct the tier's ordinary streaming world at the origin.
    #[must_use]
    pub fn new(tier: ResourceTier) -> Self {
        Self::with_runtime(tier.stream_config(), tier.budget(), tier)
    }

    /// Construct an explicitly configured world. Tools and tests use this to
    /// keep pacing independent from platform service construction.
    #[must_use]
    pub fn with_runtime(config: StreamConfig, budget: Budget, tier: ResourceTier) -> Self {
        Self {
            map: RegionMap::new(config),
            field: PossibilityField::default(),
            anchors: Vec::new(),
            bias: [0.0; POSSIBILITY_DIMS],
            traveler: TravelerState::default(),
            transition_mode: false,
            path_tracking: false,
            route_attraction: true,
            recorder: None,
            tracker: RouteTracker::new(),
            budget,
            tier,
            update_serial: 0,
        }
    }

    /// Authoritative generated/streamed state for presentation and inspection.
    #[must_use]
    pub const fn map(&self) -> &RegionMap {
        &self.map
    }

    /// Mutable access is crate-private so only the shared controller can apply
    /// typed persistence responses. Platform shells cannot obtain a second
    /// `RegionMap::update` authority through this type.
    pub(crate) fn map_mut(&mut self) -> &mut RegionMap {
        &mut self.map
    }

    /// Install canonically ordered durable preserve contributions before a
    /// world is handed to [`crate::controller::ViewerController`].
    pub fn apply_preserve_contributions(
        &mut self,
        contributions: Vec<(
            u64,
            world_core::RegionCoord,
            world_core::PossibilitySignature,
        )>,
    ) {
        self.map.apply_preserve_contributions(contributions);
    }

    /// Infinite possibility-field recipe.
    #[must_use]
    pub const fn field(&self) -> PossibilityField {
        self.field
    }

    /// Replace the field recipe without changing generated identity rules.
    pub fn set_field(&mut self, field: PossibilityField) {
        self.field = field;
    }

    /// Active manual/captured/summoned anchors (derived route anchors are
    /// transient and intentionally absent).
    #[must_use]
    pub fn anchors(&self) -> &[Anchor] {
        &self.anchors
    }

    /// Mutable anchor storage used only by the ordered action reducer and
    /// typed service responses.
    pub(crate) fn anchors_mut(&mut self) -> &mut Vec<Anchor> {
        &mut self.anchors
    }

    /// Direct possibility bias.
    #[must_use]
    pub const fn bias(&self) -> &[f32; POSSIBILITY_DIMS] {
        &self.bias
    }

    /// Mutable bias used by the ordered action reducer.
    pub(crate) fn bias_mut(&mut self) -> &mut [f32; POSSIBILITY_DIMS] {
        &mut self.bias
    }

    /// Single traveler state.
    #[must_use]
    pub const fn traveler(&self) -> TravelerState {
        self.traveler
    }

    /// Move the traveler without updating the runtime. The controller calls
    /// this during the input phase, then calls [`Self::update`] once.
    pub(crate) fn set_traveler_position(&mut self, position: (f64, f64)) {
        self.traveler.position = position;
    }

    /// Restore both current and prior traveler positions from a typed session.
    pub fn restore_traveler(&mut self, position: (f64, f64), previous: (f64, f64)) {
        self.traveler = TravelerState {
            position,
            previous_position: previous,
        };
    }

    /// Deliberate slow-convergence movement mode.
    #[must_use]
    pub const fn transition_mode(&self) -> bool {
        self.transition_mode
    }

    pub(crate) fn set_transition_mode(&mut self, enabled: bool) {
        self.transition_mode = enabled;
    }

    /// Whether the optional path subsystem is active.
    #[must_use]
    pub const fn path_tracking(&self) -> bool {
        self.path_tracking
    }

    pub(crate) fn set_path_tracking(&mut self, enabled: bool) {
        self.path_tracking = enabled;
    }

    /// Whether retained routes contribute weak derived anchors.
    #[must_use]
    pub const fn route_attraction(&self) -> bool {
        self.route_attraction
    }

    pub(crate) fn set_route_attraction(&mut self, enabled: bool) {
        self.route_attraction = enabled;
    }

    /// Whether an expedition is currently being recorded.
    #[must_use]
    pub const fn route_recording(&self) -> bool {
        self.recorder.is_some()
    }

    /// Start a fresh shared route recording.
    pub(crate) fn start_route_recording(&mut self) {
        self.recorder = Some(RouteRecorder::new());
    }

    /// Finish the active route recording, if any.
    pub(crate) fn finish_route_recording(
        &mut self,
    ) -> Option<(Vec<world_core::RouteNode>, Vec<u64>)> {
        self.recorder.take().map(RouteRecorder::finish)
    }

    /// Discard only an unfinished expedition. Completed-route tracker legs
    /// remain intact across path disable and failed durable route removal.
    pub(crate) fn discard_route_recording(&mut self) {
        self.recorder = None;
    }

    /// Retain tracker legs only for routes the durable service reports still
    /// present after a complete or partial clear operation.
    pub(crate) fn retain_route_tracking(&mut self, retained_ids: &[u64]) {
        self.tracker.retain(|id| retained_ids.contains(&id));
    }

    /// Discard transient path state while leaving durable route records to the
    /// platform vault service.
    pub(crate) fn clear_route_state(&mut self) {
        self.discard_route_recording();
        self.tracker = RouteTracker::new();
    }

    /// Attach a newly persisted discovery to the active expedition.
    pub(crate) fn attach_recorded_discovery(&mut self, id: u64) {
        if let Some(recorder) = self.recorder.as_mut() {
            recorder.attach_discovery(id);
        }
    }

    /// Portable route-recorder state for a session write request.
    #[must_use]
    pub fn recorder_snapshot(&self) -> Option<RouteRecorderSnapshot> {
        self.recorder.as_ref().map(RouteRecorder::snapshot)
    }

    /// Portable route-tracker state for a session write request.
    #[must_use]
    pub fn tracker_snapshot(&self) -> RouteTrackerSnapshot {
        self.tracker.snapshot()
    }

    /// Restore portable route state supplied by a decoded session response.
    pub fn restore_route_state(
        &mut self,
        recorder: Option<RouteRecorderSnapshot>,
        tracker: RouteTrackerSnapshot,
    ) {
        self.recorder = recorder.map(RouteRecorder::from_snapshot);
        self.tracker = RouteTracker::from_snapshot(tracker);
    }

    /// Active update budget.
    #[must_use]
    pub const fn budget(&self) -> &Budget {
        &self.budget
    }

    /// Runtime resource tier (identity-neutral; changes pacing/capacity only).
    #[must_use]
    pub const fn tier(&self) -> ResourceTier {
        self.tier
    }

    /// Number of completed logical updates.
    #[must_use]
    pub const fn update_serial(&self) -> u64 {
        self.update_serial
    }

    /// Perform the sole runtime update for one logical viewer tick.
    pub(crate) fn update(
        &mut self,
        executor: &dyn TaskExecutor,
        hook: &mut dyn WorldTickHook,
    ) -> WorldTickOutput {
        let travel = self.traveler.travel();
        let mut service_input = hook.before_world_update(WorldPreUpdate {
            traveler: self.traveler.position,
            budget: &self.budget,
            path_tracking: self.path_tracking,
            route_attraction: self.route_attraction,
        });
        let mut effective_anchors = self.anchors.clone();
        effective_anchors.append(&mut service_input.derived_anchors);

        let mut stats = self.map.update(
            self.traveler.position,
            travel,
            &self.field,
            &effective_anchors,
            &self.bias,
            &self.budget,
            executor,
            self.transition_mode,
        );
        if let Some(recorder) = self.recorder.as_mut() {
            recorder.observe(
                &self.map,
                self.traveler.position,
                travel,
                &effective_anchors,
                stats.resonance_strength,
            );
        }
        let traversed_route_ids = if self.path_tracking {
            self.tracker
                .observe(service_input.active_routes.iter(), self.traveler.position)
        } else {
            Vec::new()
        };
        hook.after_world_update(WorldPostUpdate {
            map: &self.map,
            traveler: self.traveler.position,
            travel,
            effective_anchors: &effective_anchors,
            traversed_route_ids: &traversed_route_ids,
            stats: &mut stats,
        });

        self.traveler.finish_tick();
        self.update_serial = self.update_serial.saturating_add(1);
        WorldTickOutput {
            update_serial: self.update_serial,
            traveler: self.traveler.position,
            travel,
            stats,
        }
    }
}

impl Default for ExplorationWorld {
    fn default() -> Self {
        Self::new(ResourceTier::Low)
    }
}

#[cfg(test)]
mod tests {
    use world_core::{bound_target, AnchorKind, AnchorSource, PossibilityVector};
    use world_runtime::InlineExecutor;

    use super::*;

    #[derive(Debug, Default)]
    struct RecordingHook {
        before: usize,
        after: usize,
        pre_position: Option<(f64, f64)>,
        post_travel: Option<f64>,
        effective_count: usize,
    }

    impl WorldTickHook for RecordingHook {
        fn before_world_update(&mut self, input: WorldPreUpdate<'_>) -> WorldServiceInput {
            self.before += 1;
            self.pre_position = Some(input.traveler);
            WorldServiceInput {
                derived_anchors: vec![Anchor {
                    world_pos: input.traveler,
                    target: bound_target(1, 1.0),
                    mask: 1,
                    kind: AnchorKind::Emphasize,
                    strength: 0.1,
                    falloff_radius: 10.0,
                    source: AnchorSource::Manual,
                }],
                active_routes: Vec::new(),
            }
        }

        fn after_world_update(&mut self, output: WorldPostUpdate<'_>) {
            self.after += 1;
            self.post_travel = Some(output.travel);
            self.effective_count = output.effective_anchors.len();
            assert_eq!(output.stats.anchors_active, self.effective_count);
        }
    }

    fn empty_world() -> ExplorationWorld {
        let config = StreamConfig {
            near_radius: 0.0,
            far_radius: 0.0,
            load_radius: 0.0,
            unload_radius: 1.0,
            ..StreamConfig::default()
        };
        ExplorationWorld::with_runtime(config, Budget::unlimited(), ResourceTier::Low)
    }

    #[test]
    fn traveler_distance_is_computed_from_one_shared_xy() {
        let traveler = TravelerState {
            previous_position: (1.0, 2.0),
            position: (4.0, 6.0),
        };
        assert_eq!(traveler.travel(), 5.0);
    }

    #[test]
    fn update_calls_both_hooks_once_at_the_new_center() {
        let mut world = empty_world();
        world.set_traveler_position((3.0, 4.0));
        let mut hook = RecordingHook::default();
        let output = world.update(&InlineExecutor, &mut hook);

        assert_eq!(hook.before, 1);
        assert_eq!(hook.after, 1);
        assert_eq!(hook.pre_position, Some((3.0, 4.0)));
        assert_eq!(hook.post_travel, Some(5.0));
        assert_eq!(hook.effective_count, 1);
        assert_eq!(output.update_serial, 1);
        assert_eq!(output.travel, 5.0);
        assert_eq!(world.traveler().previous_position, (3.0, 4.0));

        let second = world.update(&InlineExecutor, &mut hook);
        assert_eq!(second.update_serial, 2);
        assert_eq!(second.travel, 0.0);
        assert_eq!(hook.before, 2);
        assert_eq!(hook.after, 2);
    }

    #[test]
    fn permanent_and_derived_anchor_storage_stay_separate() {
        let mut world = empty_world();
        world.anchors_mut().push(Anchor {
            world_pos: (0.0, 0.0),
            target: PossibilityVector::neutral(),
            mask: 1,
            kind: AnchorKind::Suppress,
            strength: 0.2,
            falloff_radius: 20.0,
            source: AnchorSource::Manual,
        });
        let mut hook = RecordingHook::default();
        let output = world.update(&InlineExecutor, &mut hook);
        assert_eq!(output.stats.anchors_active, 2);
        assert_eq!(hook.effective_count, 2);
        assert_eq!(world.anchors().len(), 1);
    }
}
