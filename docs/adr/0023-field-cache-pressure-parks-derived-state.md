# 23. Field cache pressure parks derived state; regional history follows streaming geometry

Date: 2026-07-12

## Status

Accepted

Builds on [ADR 0006](0006-travel-fueled-convergence.md),
[ADR 0008](0008-tiles-are-functions-of-their-dependency-hash.md),
[ADR 0018](0018-settled-state-is-schedule-independent.md),
[ADR 0019](0019-dependency-hashes-gate-integration.md), and
[ADR 0020](0020-preserve-overlaps-use-lowest-content-id.md).

## Context

`RegionMap` previously used one deletion path for two different events. Crossing
the geometric unload boundary removed a region and its generated fields, while
field-cache pressure called that same path. The latter therefore discarded the
small `RegionState` containing realized and target possibility vectors,
stability, and revision. Reloading inside the same streaming window created a
fresh `current = target` epoch, so a memory ceiling changed regional history
instead of changing only derived residency and recomputation.

The performance tiers also amortized stability and target calculation as one
retarget pass. Player movement was absent from the steering-input signature. A
coordinate could cross inside the geometric near radius but retain stale
stability for several frames and continue travel-fueled convergence while
visible. The continuity replay trusted that stored stability, so it could not
detect the violated geometric pin.

Generated fields, macro drainage, habitat rosters, and near-field organisms are
reproducible derived working sets. Regional `current`, `target`, stability, and
revision are the bounded authoritative history of the streaming window. These
two lifecycles need distinct policies without adding a second authority map or
changing generation formulas, persisted bytes, or stable identities.

## Decision

1. **The ordered region map is the sole authority.** A coordinate is absent
   only when it is not in `RegionMap::regions`. `GenerationStatus::Unloaded`
   means that authoritative state exists but its disposable field working set
   is parked. `Generating` and `Ready` are field-active states. Public region
   lookup, length, authoritative iteration, frame resident counts, convergence,
   and session snapshots include parked entries.

2. **Streaming geometry bounds authority.** Missing coordinates inside
   `load_radius` are created nearest-first under `max_loads`, independent of
   field capacity. Crossing `unload_radius` removes every complete regional
   authority and its derived state; sparse preserve contributor bookkeeping
   survives separately so a later load can reconstruct its winner. Capacity-
   parked entries remain subject to that ordinary radius sweep, so the
   authoritative window stays bounded.

3. **Field pressure parks only derived state.** The capacity pass parks
   farthest field-active, non-near, non-preserved coordinates first. Parking
   removes region tiles, region-signature bookkeeping, organisms, realization
   keys, and level-0 in-flight dispatch identities, while retaining `current`,
   `target`, stability, and revision. Cancellation tokens are an optimization;
   a late cancellation-off result lacks a live dispatch identity and is
   reclaimed without recreating fields.

4. **Admission reserves the full eventual payload.** Every field-active,
   disposable coordinate reserves
   `resolution² × (13 × sizeof(f32) + sizeof(u8) + sizeof(u16))`, including
   partially generated regions. Near and contributor-covered regions are
   admitted as explicit exemptions above the disposable target. Reactivation
   recomputes target and geometric stability from live inputs, marks every
   layer dirty, and never resets realized state or revision. A preserve instead
   remains self-targeted and fully stable.

5. **Authoritative evolution does not depend on field residency.** Every
   authoritative coordinate participates in target refresh and convergence in
   the same deterministic coordinate/distance order. Parked dirty hints never
   dispatch work. Macro and roster allocations are protected only by
   field-active consumers; parked authority can compute expected dependency
   keys but cannot pin derived inputs it cannot consume.

6. **Geometry is current every frame; only targets are amortized.** Before
   resonance and convergence, the runtime refreshes stability for all
   authoritative coordinates in coordinate order. `max_retarget_regions`
   budgets only unchanged-steering target calculation. A bias or anchor
   signature change refreshes all targets immediately. Retarget deferral
   telemetry therefore never describes deferred geometric stability.

7. **Preserves and sessions operate on parked authority.** Effective preserve
   changes retain ADR 0020's exact-vector revision, bucket-closure, organism,
   and final-owner no-snap rules without waking a parked field set. Session
   snapshots include parked entries; restore inserts their exact current,
   stability, and revision as parked authority, and live admission reconstructs
   target and fields without changing the record schema.

8. **Regional history has a field-independent comparison surface.** The tools
   fold ordered coordinate (including level), current, target, stability, and
   revision into `regional_history_hash`. The full replay hash incorporates
   that fold before derived caches. Status is excluded because field residency
   is deliberately allowed to differ across ceilings.

## Alternatives considered

- **Describe field capacity as world-state capacity:** rejected because a
  derived-cache tuning knob would still alter exploration history and violate
  the schedule/capacity contract.
- **Keep a second parked-history map:** rejected because duplicated authority
  can drift and makes preserve, snapshot, and dependency-key ownership
  ambiguous.
- **Reserve only integrated cache bytes:** rejected because multiple partial
  generations can all appear cheap and over-admit their eventual payload.
- **Recreate a parked region with `current = target`:** rejected because
  reactivation is not a new streaming epoch and must retain its journey.
- **Refresh only near coordinates' stability:** rejected because an all-region
  geometry pass is cheap, ordered, and makes boundary transitions and telemetry
  unambiguous.
- **Persist target or field-residency status:** rejected because target is
  reconstructed from live steering and field inputs, while field admission is
  derived configuration state. The broader mid-frame snapshot precondition is
  unchanged.

## Consequences

- Tight and roomy field ceilings create and evolve identical authoritative
  coordinates for the same player script and load budget, while their field,
  macro, roster, organism, and pool residency may differ.
- Field pressure can increase deterministic regeneration work but cannot erase
  current possibility state or revision. Only the unload radius forgets
  ordinary unpreserved history.
- Stability now costs one cheap geometry calculation per authoritative region
  per frame. The more expensive steering target calculation remains amortized.
- Logical field targets remain payload accounting rather than allocator or
  process-memory caps; allocator-inclusive accounting is a separate concern.
- ADR 0018's allowance for executor-paced mid-flight resonance remains. This
  decision removes capacity as a direct cause of authority loss or paused
  regional evolution; it does not make all gameplay surfaces tier-invariant.
- No generation equation, dependency-hash fold, algorithm revision, world
  version, record format, or golden fixture changes.
