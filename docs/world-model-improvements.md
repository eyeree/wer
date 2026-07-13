## Questionable aspects and opportunities for improvement

This document separates likely correctness or contract violations from
deliberate model simplifications. Some issues are unlikely under the generous
default limits, but the corresponding public configuration claims are still
stronger than the implementation. Resolved findings remain below for
auditability and are explicitly labeled; unlabeled findings remain open.

### Prioritized improvement roadmap

The ordering below is **category-major**: correctness and contract work comes
before performance or memory optimization, and performance work comes before
expanding the model. Within each category, items are ordered by expected impact
and by how many later changes depend on them. Finding numbers refer to the
detailed analyses later in this section; closely related findings are bundled
into one implementation unit.

#### A. Correctness and contract integrity

1. **Completed: Make dependency integration and cache eviction self-healing**
   ([Improvement A.1](plans/prototype/improvement_A_1_self_healing.md); findings
   2, 3, and 10). The runtime now demand-repairs missing declared inputs,
   protects and rebuilds resident roster entries, and gates completed work on
   its dispatch and current dependency keys. Tight macro/roster ceilings,
   every declared edge, every result shape, cell inspection, and realization
   recovery are covered by focused runtime regressions.

2. **Completed: Give preserves deterministic ownership and revision semantics**
   ([Improvement A.2](plans/prototype/improvement_A_2_preserve_ownership_revision.md);
   finding 26). The runtime retains all preserve contributors and selects the
   lowest content id, reveals successors after deletion, and advances revision
   plus retires organisms on a material resident snap while bucket flips alone
   govern tile dirtiness. Focused runtime, preserve integration, and vault
   harness regressions cover resident batch-order reversal, winner/non-winner
   deletion, same-bucket normalization, cancellation, session restore, and
   eviction/reload recovery.

3. **Completed: Make persistence failures explicit and durable**
   ([Improvement A.3](plans/prototype/improvement_A_3_persistence_failures_durability.md);
   finding 28). Flush and delete failures now carry structured operation, key,
   progress, and retry information; explicit saves succeed only when clean,
   valid import results contribute to and raise a lagging live sequence
   immediately, with typed no-mutation exhaustion when no newer value is
   representable, and retained diagnostics are deduplicated and capped. Native
   writes and removals cross file and directory durability barriers, with
   staged fault tests and the expanded vault harness covering retry order and
   recovery.

4. **Completed: Separate authoritative regional history from disposable field
   memory**
   ([Improvement A.4](plans/prototype/improvement_A_4_authoritative_regional_history.md);
   findings 4 and 5). The ordered region map now retains bounded authority
   while capacity parks only derived fields; loading and regional evolution are
   ceiling-independent, every-frame geometry pins near crossings immediately,
   and reactivation rebuilds from retained current/revision. Focused queued,
   preserve, late-result, and session regressions plus a per-frame tight-versus-
   roomy `wer-scale` history gate cover the corrected lifecycle.

5. **Completed: Restore resource-tier invariance for gameplay and shared
   records**
   ([Improvement A.5](plans/prototype/improvement_A_5_resource_tier_invariance.md);
   finding 6). Organisms now carry explicit density slots; a fixed one-region
   pre-resonance pass publishes canonical slot 0, while tier-budgeted expansion
   adds visual slots only. Capture and fixed-cap 64-node resonance read slot 0,
   and focused plus `wer-scale` gates require exact Low/Mid/High capture,
   resonance, actual route records, and encoded route bytes.

6. **Completed: Canonicalize all anchor reductions and signatures**
   ([Improvement A.6](plans/prototype/improvement_A_6_canonical_anchor_reductions_signatures.md);
   findings 1 and 21). A complete raw-bit key now sorts every duplicate-
   retaining occurrence before reduction and supplies the cardinality/full-
   field signature. Suppress-final priority is explicit, runtime reorder-only
   changes remain amortized, and native route recording signs the exact
   explicit-plus-derived effective slice.

7. **Completed: Enforce the intended semantics of route and suppress
   influences**
   ([Improvement A.7](plans/prototype/improvement_A_7_route_suppress_influences.md);
   findings 7 and 17). Deterministic selection now precedes one common-scale,
   fixed-iteration cap over the complete selected route channel, while
   compatibility weights authoritative current-versus-final-target differences
   with the same canonical center-evaluated influence profile. Dense
   multi-route, Suppress-final, exact permutation, native recording, tier, and
   an additive native/wasm-executed parity sample covers the corrected
   contracts.

8. **Completed: Make stable topology and ordinary region boundaries satisfy
   their stated guarantees**
   ([Improvement A.8](plans/prototype/improvement_A_8_topology_boundaries.md);
   findings 9 and 19). Drainage routing elevation is now an entirely integer
   Q30/i128 function with field-aware keys and executed native/wasm topology
   probes. Terrain samples an exact 3 by 3 realized-current/fallback P/G halo,
   emits centered ghost-derived Slope atomically, and invalidates every
   affected neighbor closure across authority lifecycle changes. Exact
   ordinary history-divergent border tests cover Terrain through Biome.

9. **Completed: Separate stable organism identity, placement, succession, and
   expression**
   ([Improvement A.9](plans/prototype/improvement_A_9_organism_identity_placement_succession_expression.md);
   finding 12). L8 aggregate Ecology now reads Ecology only, while runtime
   realization tracks typed identity, expression, and presentation keys. M/B/A
   changes refresh bucket-center genome expression without changing presence,
   id, species, trophic role, density slot, local cell, or jittered placement;
   aggregate/habitat provenance and the explicit succession epoch are the
   identity-grade inputs.

10. **Completed: Harden content equality and canonical set encoding**
    ([Improvement A.10](plans/prototype/improvement_A_10_content_equality_canonical_sets.md);
    findings 24 and 25). Same-id discovery, route, and preserve records now
    compare typed immutable bodies before mutable fields merge. Atlas bundles
    are keyed sets by record id, equal-body duplicates collapse by the normal
    merge law, conflicts are rejected, and `wer-atlas check` reports a
    SHA-256 digest over canonical bundle bytes. Preserve coordinates are
    coordinate-keyed sets, and route discovery refs are sorted unique ids.
    Tombstones and per-replica route-usage counters remain separate future
    work before deletion and usage can be called fully CRDT-compatible.

11. **Completed: Make executor failure and shutdown bounded**
    ([Improvement A.11](plans/prototype/improvement_A_11_executor_failure_shutdown.md);
    finding 30). LaneExecutor shutdown now clears queued work before waking
    workers, workers exit before taking more work once shutdown starts, and
    submit-after-shutdown drops closures. Worker panics are caught as telemetry
    while runtime generation panics become structured failed dispatch results
    that retire matching in-flight entries, dirty the affected closure, and
    retry deterministically. Fairness, bounded queues, and proactive removal of
    cancelled queued closures remain future backpressure work.

12. **Completed: State and encode the truth of snapshots and route samples**
    ([Improvement A.12](plans/prototype/improvement_A_12_snapshot_route_truth.md);
    findings 22 and 29). Record format v2 stores session runtime metadata,
    resident targets, active recorder state, and active tracker legs; load
    paths compare metadata before claiming exact continuation. Route nodes now
    distinguish target from visible current, carry segment distance, and
    sample every crossed interval with retained remainder. Migrated v1 records
    remain readable with explicit unknown-current/zero-distance semantics.
    Ordered route traversal remains a separate roadmap concern.

13. **Completed: Expand the verification surface alongside these fixes**
    ([Improvement A.13](plans/prototype/improvement_A_13_verification_surface.md);
    finding 33). `wer-scale` now includes frame-slicing, all-cache-ceiling,
    and cross-tier persistence scenarios. The shared settled-state hash covers
    organism position and expression fields, route tests cover multi-node
    softness, SIMD differential coverage enumerates every biome id, and wasm
    parity remains an executed Node suite. Executor queues and in-flight
    closures remain outside settled equality and must be empty before hashing;
    logical cache byte ceilings remain separate from full heap accounting
    (finding 32), and ordered route traversal remains separate roadmap work.

#### B. Performance and memory usage

1. **Index spatial and possibility-space queries** (finding 23). Replace full
   route, route-node, and resonance scans with bounded top-$k$ selection and
   spatial indexes; use a metric or quantized index for nearest searches in the
   eight-dimensional signature space.

2. **Make the vault lazy, paged, and locality-aware** (finding 27). Avoid
   loading every record at startup or walking the whole native store once per
   namespace. Page authored records and load seen chunks and route partitions
   around the active area.

3. **Turn logical payload budgets into honest memory budgets** (finding 32).
   Account for vector capacity, map nodes, `Arc`s, structs, in-flight snapshots,
   pools, and organism storage, then calibrate ceilings against allocator- or
   capacity-observed memory rather than element payload alone.

4. **Add executor backpressure and fairness** (the performance half of finding
   30). Bound queues, remove cancelled work before it reaches a worker, and add
   aging or weighted service so sustained Critical traffic cannot starve the
   far field.

5. **Replace closure-shaped background jobs with serializable descriptors**
   (finding 31). This makes ordinary Web Workers possible without relying on
   shared-memory wasm deployment and also reduces the amount of state captured
   by each queued job.

6. **Introduce adaptive spatial detail only after cache correctness is secure**
   (Section 4.5). A clipmap, quadtree, or multi-LOD field hierarchy can reduce
   far-field tile count and bandwidth, but it should preserve the same
   authoritative sampling and identity contracts before becoming an
   optimization target.

#### C. Functional and model improvements

1. **Make transition mode implement the advertised travel fantasy** (finding
   8). Ordinary movement should be mostly neutral; deliberate transition should
   enable convergence while changing movement speed or cost in a visibly
   meaningful way.

2. **Reconcile aggregate ecology with realized organisms** (finding 11). Derive
   entity trophic roles and biomass from L8 pressures, include Ecology in
   realization abundance, and keep authoritative biomass constant as visual
   organism density changes.

3. **Connect genome niches to ecological roles** (finding 13). Let trophic
   tendency, diet breadth, temperature tolerance, moisture tolerance, form, and
   sociality affect role assignment, habitat suitability, expression, or
   behavior; make capture report the role actually expressed by the organism.

4. **Make dominance, diversity, and food-web structure informative** (findings
   14 and 15). Add local species fitness, expose richness separately from
   evenness, choose prey by ecological affinity, retain meaningful absolute
   biomass, and filter pruned species and edges from survivor-facing readouts.

5. **Add geographic lineage and smooth habitat turnover** (finding 16). Combine
   broad biome archetypes with stable provinces, isolation, and continuous
   suitability so a band boundary does not replace the entire roster and
   distant identical bands do not always share the same species.

6. **Make trait capture local and observable** (finding 18). Enforce a capture
   radius, report which nearby feature supplied each trait, and add actual
   atmospheric or planetary observables before Planetary capture is presented
   as discovery-derived.

7. **Make routes represent traversal rather than unordered proximity** (the
   functional half of finding 22). Track ordered segment progress, direction,
   continuous corridor coverage, and distance-weighted difficulty; clearly
   distinguish remembered visible conditions from desired possibility targets.

8. **Increase environmental fidelity where it improves exploration** (finding
   20). Prioritize cross-window catchment flux, depression handling, and
   coherent channel geometry before adding latitude, prevailing wind, rain
   shadows, seasons, erosion, or soil history.

9. **Keep refined presentation anchored to authoritative data** (finding 34).
   Share shader constants with Rust, test CPU/GPU presentation parity, and make
   refinement residuals exactly coarse-sample- or mean-preserving if that is the
   intended visual contract.

10. **Deepen possibility space and world variability** (Section 4.5). Add a
    user-visible world seed and replace the eight scalar proxies with richer
    trait spaces only after their identity, quantization, projection, and
    persistence rules are specified.

11. **Promote life from markers to dynamics** (Section 4.5). Migration,
    reproduction, competition, succession, evolutionary lineage, and behavior
    should build on a reconciled aggregate/entity model rather than becoming a
    second independent ecology.

12. **Build the player-facing platform in dependency order** (Section 4.5).
    Complete the terrain/vegetation/organism renderer, then the browser runtime
    and asynchronous storage, and only then networking, trust, multiplayer, and
    community sharing.

### 4.1 Correctness and architectural contract concerns

#### 1. Resolved: anchor combination was not bitwise order-independent

**Status:** Resolved by
[Improvement A.6](plans/prototype/improvement_A_6_canonical_anchor_reductions_signatures.md).

The steering equations are symmetric over real numbers, but the code accumulates
weighted sums, denominators, and products sequentially in `f32`. Float addition
and multiplication are not associative. Permuting a modest set of valid anchor
weights can change the result by one or more ULPs, potentially crossing a
possibility bucket at a boundary. The existing three-anchor test uses values
that happen to compare equal.

Canonicalize anchors by a total, bitwise key before every reduction and test
many permutations with adversarial values. If polarity is meant to be fully
symmetric, also replace the fixed "Emphasize, then Suppress" application order
with one simultaneous solve; otherwise document that Suppress has the final
blend priority.

The related runtime steering signature folded anchors in slice order, so a mere
reorder forced an unnecessary whole-window retarget even when steering output
was unchanged.

ADR 0025 now projects every occurrence to one complete integer key, sorts the
multiset once before all domain reductions, and shares that projection with a
counted ordered signature. Source and unmasked target storage normalize out;
duplicates remain meaningful. Emphasize blends first and Suppress deliberately
blends last. Exhaustive 720-permutation exact-bit tests and runtime deferred-
counter regressions enforce both output and invalidation behavior.

#### 2. Macro-cache capacity eviction can strand Hydrology forever

**Status:** Resolved by
[Improvement A.1](plans/prototype/improvement_A_1_self_healing.md).

The capacity path formerly could remove a Drainage macro while its covered
regions retained clean Drainage hints. A later Hydrology invalidation then
deferred forever because readiness observed the missing macro but did not
request it.

Dependency repair now restores a current-key Drainage request whenever a dirty
consumer needs a missing or stale macro, without eagerly rebuilding unused
macros. A tight-macro-ceiling regression evicts the input, invalidates
Hydrology alone, settles the complete downstream chain, and compares its keys
and content with a roomy-cache run.

#### 3. Roster-cache capacity eviction can make life permanently disappear

**Status:** Resolved by
[Improvement A.1](plans/prototype/improvement_A_1_self_healing.md).

Roster eviction formerly could remove signatures still referenced by fresh
Ecology tiles. Lookup-only cell inspection then failed, while near-field
realization could publish an incomplete vector and record the unchanged L8 key
without retrying.

The runtime now ensures and protects the union of field-active signature sets,
evicts only disposable entries, and permits that required floor to exceed its
logical byte target. Realization preflights the complete set and defers without
advancing its key if an entry is absent. Tight roster-ceiling tests cover
required-entry repair, settled-cell inspection, realization retry, roomy-cache
content equality, and continued eviction of unprotected signatures.

#### 4. Resolved: amortized retargeting can violate the geometric near-field pin

**Status:** Resolved by
[Improvement A.4](plans/prototype/improvement_A_4_authoritative_regional_history.md).

Previously, Mid and High tiers round-robined the combined stability-and-target
pass. Player movement was not part of the steering-change hash and there was no
near-first exception. A resident region that moved physically inside
`near_radius` could retain an old `stability < 1` for several frames and
continue converging while visible. The replay defined "pinned" using the stored
stability, so it did not detect a geometrically near region whose stored value
was stale.

Stability is cheap and safety-critical; compute it for all regions every frame,
or at least for every near/crossing region, and amortize only the target
calculation.

The runtime now refreshes geometric stability for every authoritative resident
before resonance and convergence; `max_retarget_regions` budgets target
calculation only. Continuity pin checks derive near status from geometry rather
than trusting stored stability, and a focused one-target-per-frame regression
moves a far coordinate into the near radius with positive travel and proves its
current/revision remain unchanged while every resident's stability is current.

#### 5. Resolved: field-capacity eviction removes authoritative history, not just cache

**Status:** Resolved by
[Improvement A.4](plans/prototype/improvement_A_4_authoritative_regional_history.md).

Previously, when the field-byte ceiling was exceeded, the implementation called
`drop_region`, which discarded `RegionState` as well as tiles. If that region
was reloaded inside the load radius, it started with `current = target`; its
prior convergence history and revision were gone. Thus a field "cache" ceiling
could change the mid-journey world rather than merely cause deterministic
recomputation.

Retain the small authoritative `RegionState` while evicting only derived tiles,
or explicitly describe capacity eviction as world-state eviction and include
it in schedule/continuity guarantees.

Capacity pressure now changes `GenerationStatus` to `Unloaded` and tears down
only tiles, signatures, organisms, and obsolete jobs. Authoritative loading,
target refresh, stability, convergence, revision, preserves, and snapshots
continue independently until the radius sweep removes the coordinate.
Reactivation recomputes live target/geometry and rebuilds every layer without
resetting history. Full-payload reservations prevent partial generations from
over-admitting a logical target, and `wer-scale` compares ordered regional
history after every changing-bias travel frame under tight and roomy ceilings,
requiring both capacity parking and observed parked-state evolution.

#### 6. Resolved: Resource tiers fed gameplay and persistent identity

**Status:** Resolved by
[Improvement A.5](plans/prototype/improvement_A_5_resource_tier_invariance.md).

Previously, High tier added organism slots and increased the resonance-node
cap. Resonance read realized organism count and species entropy, so hardware
tier changed the convergence rate during travel. Extra slots could also change
which organism was nearest during capture. Route nodes persisted
`1 - resonance` as part of their content id, so the same physical expedition
could produce different shared route bytes on Low and High hardware.

The effect was not even monotone after density saturated at eight nodes: adding
farther or less evenly distributed organisms could lower the mean-distance or
diversity term while density stayed one.

That behavior contradicted the description of organism density as
presentation-only and the claim that shared surfaces are tier-invariant.

Resolution: every organism now records its density slot, slot 0 is the sole
gameplay sample, and higher slots remain additive presentation. Canonical
publication admits one nearest fresh roster-complete region per frame before
resonance, independently of visual realization budgets; stale or incomplete
inputs retire both canonical and presentation currency before gameplay reads.
Resonance always selects the exact first 64 canonical nodes under its total
sort. Non-vacuous focused tests probe an extra organism whose species differs
from the nearest canonical specimen, and the scale harness settles equal ready
Low/Mid/High inputs under one explicit anchor before requiring bit-exact
capture/resonance and byte-identical actual `RouteRecord` encodings. L8/executor
readiness may still differ by frame, and live float capture remains
presentation-grade across native/wasm; neither caveat permits visual density to
change gameplay once authoritative prerequisites match.

#### 7. Resolved: route attraction is globally capped after candidate selection

**Status:** Resolved by
[Improvement A.7](plans/prototype/improvement_A_7_route_suppress_influences.md)
and [ADR 0026](adr/0026-route-attraction-is-globally-bounded.md).

Each route node is capped below 0.35, but overlapping nodes combine through

$$
W=1-\prod_i(1-w_i).
$$

At the default 32-node cap, 32 fresh nodes with $w=0.1225$ already produce
$W\approx0.9847$; at usage four, $W\approx0.9998$. A dense or overlapping
route can therefore almost force its weighted target and overwhelm a player
anchor, contrary to the stated soft-attraction contract.

The implementation keeps deterministic nearest-first selection, then applies
one common scale to all selected occurrences across every route. Exactly 32
safe `f32` bisection trials evaluate ADR 0025's canonical product, retaining raw
bits when already safe and otherwise the greatest tested vector with aggregate
peak at most 0.35. Peak normalization is position-independent, so falloff can
only reduce it. Core and integration tests cover co-located multi-route worst
cases, every returned node center and corridor probes, singleton bits, usage
saturation, output order, and route-iterator permutations. The vault harness
reports the dense peak, the native recording test covers normalized effective
inputs, and an additive quantized route sample is golden-tested natively and
executed as wasm in Node.

#### 8. Transition mode currently reverses the high-level movement fantasy

The project overview says ordinary movement is fast exploration and only slow,
deliberate transition movement should significantly change reality. Native
movement speed is identical in both modes, while transition mode multiplies
convergence by 0.35 and free movement uses 1.0. Ordinary travel therefore
changes reality about 2.86 times more per unit than explicit transition mode.

Either make free travel nearly neutral and transition mode enable convergence
while reducing physical speed, or revise the product description and controls
to match the implemented mechanic.

#### 9. Resolved: integer drainage topology depended on float thresholds

**Status:** Resolved by
[Improvement A.8](plans/prototype/improvement_A_8_topology_boundaries.md) and
[ADR 0027](adr/0027-fixed-point-drainage-and-halo-sampled-terrain.md).

Routing elevation now uses one integer-only Q30/i128 evaluator from the
control-point stream through P/G bucket sampling, hashed-gradient fBm, signed
ties-away rounding, and final centimeters. Scalar and macro paths share it;
the macro key includes the raw stored field spacing and both layer revisions.
Known answers cover signed/custom/extreme coordinates and CI executes a broad
multi-macro topology fold as real wasm in Node.

#### 10. Integration does not revalidate the dependency hash

**Status:** Resolved by
[Improvement A.1](plans/prototype/improvement_A_1_self_healing.md).

Integration formerly accepted a tile when its job id matched and dirty
bookkeeping remained clear, without comparing `result.dep_hash` with current
authoritative provenance. A missed dirty hint could therefore admit stale
content.

Each in-flight entry now records its dispatch key. Macro and region results
integrate only when job id matches and the result key equals both that dispatch
key and the recursively recomputed current key. Rejection leaves cached
channels untouched, reclaims owned buffers, dirties the dependent closure, and
requeues through normal dispatch. The stale-result matrix exercises every
result shape and verifies atomic rejection followed by a correct retry.

### 4.2 Ecological and steering-model inconsistencies

#### 11. Aggregate pressure and realized consumers do not conserve the same model

Ecology fields scale consumer pressure by $d_vE$, but realization creates an
organism with probability $d_v$ and samples its trophic tier from the fixed web
shares. It ignores $E$ and the herbivore/predator tiles. As $E$ approaches
zero, aggregate herbivore and predator pressure approach zero while consumers
can still be realized. (Generated layers receive a positive bucket center, so
the lowest realizable value is tiny rather than exactly zero.) Increasing slots
multiplies entity counts without changing aggregate pressure.

Sample tiers from the L8 pressure fields, or give realized organisms explicit
biomass weights whose sum remains constant across resource tiers. Tests should
include the pure $E=0$ function case and the lowest quantized Ecology bucket,
then compare entity totals with both pressure channels.

#### 12. Resolved: appearance-only changes can re-roll identity and placement

Previously, Morphology, Behavior, and Aesthetics made the L8 dependency hash
change even though aggregate L8 values did not use them. Rebuilt realization
also used the incremented region revision in feature ids, so an
Aesthetics-only bucket flip could re-roll presence, species choice, and
positions rather than merely changing expression. Raw M/B/A floats could also
diverge from a bucket-keyed cache after near exit and re-entry.

Resolution: aggregate Ecology now declares only Ecology as its direct
possibility-domain input. Runtime realization uses typed keys for stable
identity, expression, and presentation completion. Stable identity folds fresh
L8 provenance, field resolution, sorted habitat-signature/roster content, and
an explicit succession epoch; it excludes M/B/A and `RegionState::revision`.
Expression is rebuilt from M/B/A bucket centers, so same-bucket drift and
near-window re-entry reproduce the same vector. M/B/A-only bucket changes can
refresh expressed genome values but keep presence, id, species, trophic role,
density slot, local cell, and jittered world position stable.

#### 13. Several genes are unused or contradict assigned trophic roles

Appearance form, behavior sociality, and both environmental tolerances are
generated and fingerprinted, but have no effect on expression, role assignment,
food-web construction, suitability, or realization. Trophic role is assigned
from roster position before the genome is considered; trophic-tendency genes do
not influence it. A Producer can therefore carry a strongly predator-like gene,
and Ecology capture uses that gene rather than the organism's actual role.

Generate candidates first, then assign or filter trophic roles using their
niche genes within habitat-level quotas. Habitat tolerances should affect
suitability and abundance. Capture should use actual role/food-web position or
make the gene and role consistent. Morphology capture should also consider
expressed size, rather than raw size class alone.

#### 14. The dominant-species tile is degenerate

Under the current biomass ratios, roster index 0 is always dominant. Producers
come first; each has more per-species mass than any consumer or decomposer, and
producer-only ties choose the lowest index. Habitat changes still change the
global species id for index 0, which can hide this constant local index in the
visualization.

Species-specific local suitability, deterministic patch variation, or niche
fitness would make dominance informative. Until then, storing a full `u16`
dominant tile provides no per-cell information.

#### 15. Food-web and diversity outputs are coarser than their names imply

Prey are the first admissible roster entries, not the best ecological matches;
omnivores often exhaust their diet on early producers. Productivity largely
cancels when tier biomass is normalized, except at body-size/tier-presence
thresholds. Herbivores are not size-pruned, and representative band-center
productivity can admit a web poorly matched to an extreme cell in the same
signature.

Pruned species remain in the roster with zero biomass. The inspector's trophic
counts still include them, and feeding edges are not cleaned if a prey species
is marked pruned later in construction. The reported roster and edge graph are
therefore not a closed graph over the surviving population. Filter edges and
report survivor counts (or both raw and surviving counts) after pruning.

Likewise, normalized entropy measures evenness, not richness: a uniform
two-species and twelve-species roster both score 1.0. Expose richness and
evenness separately or multiply entropy by a bounded richness term. Scored
prey affinity, absolute biomass, and cell-level suitability would make the web
more responsive without requiring a dynamic simulation.

#### 16. Habitat zoning is globally repetitive and discontinuous

Only 1,440 signatures can exist, and a fixed signature has exactly the same
roster everywhere in the infinite plane. Crossing a hard temperature,
moisture, or fertility band can replace the entire roster, while distant
continents with the same bands share every species.

A hierarchical identity could combine broad biome archetypes with stable
geographic province, isolation, and smoothly weighted habitat fitness. That
would preserve recognizable families across boundaries while permitting local
speciation.

#### 17. Resolved: Suppress compatibility scores the final desired state

**Status:** Resolved by
[Improvement A.7](plans/prototype/improvement_A_7_route_suppress_influences.md)
and [ADR 0026](adr/0026-route-attraction-is-globally-bounded.md).

Anchor compatibility rewards the local world for being close to every
anchor's literal target, irrespective of polarity. For Suppress, the desired
direction is away from that target, so the current formula makes a world rich
in the suppressed trait resonate more strongly.

Compatibility now compares the covering authority's current vector with its
already-refreshed final projected target. Canonical active-domain weights are
evaluated at the region center from the same effective multiset; they describe
relevance only and do not reconstruct polarity. Missing authority, no active
domain, and effective preserves are neutral, while an ordinary pinned region
with a different target remains meaningful. Focused tests distinguish final
Suppress desire from the rejected literal trait, cover mixed polarity,
duplicates, center geometry, and exact permutations; `wer-anchor` exposes the
polarity-correct comparison and the native route-record test carries it through
target, cost, id, and encoded bytes.

#### 18. Capture is weakly localized and Planetary capture is only a baseline

The nearest-organism search accepts any organism in the 256-unit covering
region; there is no maximum capture distance. M/B/A capture can also return a
baseline anchor when no organism was found, despite the documentation's
"nothing capturable" language. Planetary has no atmospheric observable at all.

Add a capture radius and an explicit result describing which feature supplied
each trait. Weather, cloud, ocean, or atmospheric fields are needed before a
Planetary capture can be distinctive.

#### 19. Resolved: Terrain and downstream fields were not border-identical

**Status:** Resolved by
[Improvement A.8](plans/prototype/improvement_A_8_topology_boundaries.md) and
[ADR 0027](adr/0027-fixed-point-drainage-and-halo-sampled-terrain.md).

Terrain now snapshots nine absolute realized-current/fallback P/G center
samples, bilinearly evaluates them at every core and ghost position, and folds
all 18 buckets into its key. It emits Elevation and centered ghost-derived
Slope under one provenance key; Hydrology and Soils consume the stored Slope.
Central lifecycle invalidation reaches every affected neighbor, while parked
authority remains halo input. Ordinary divergent-history, reverse-order,
parking, downstream-oracle, and queued-stale-result regressions pin the
boundary contract.

#### 20. Drainage accumulation and physical rules are acknowledged approximations

Direction is cell-local and consistent across macro windows, but accumulation
is apron-truncated. The same overlap cell can have different catchment size in
two tiles, creating macro-edge river-width steps. Hierarchical accumulation or
deterministic boundary flux would improve this.

Other fidelity limits are substantial but intentional: local minima are never
filled, channel geometry has only region-scale support and is represented by a
bilinearly smeared accumulation scalar, Climate has no latitude/wind/seasons,
Geology is a hardness Voronoi field, and Soils has no history. A few comments
also overstate formulas: slope 0.4 does not eliminate soil because wetness can
still add 0.25 depth, and as vegetation density approaches zero, canopy still
approaches half its root/temperature-adjusted biome maximum.

### 4.3 Routes, records, and distributed-state concerns

#### 21. Resolved: anchor-set signatures did not describe actual steering sets

**Status:** Resolved by
[Improvement A.6](plans/prototype/improvement_A_6_canonical_anchor_reductions_signatures.md).

The route-node anchor signature omits falloff radius even though it changes
steering, includes unmasked target components that do not change steering, and
XORs per-anchor hashes. Two identical anchors cancel to the empty signature
even though steering makes the pair stronger; cardinality is not encoded.
Native recording also signs only the player's explicit anchors: route-derived
anchors can alter the saved target and resonance cost while remaining absent
from this summary.

Use a canonical sorted fold over count and every steering-relevant field. If
duplicates are intended to collapse, deduplicate before both steering and
signing rather than only in the signature.

The corrected signature folds exact cardinality and every occurrence's shared
steering key in canonical order, including falloff and only masked target bits;
duplicate occurrences never cancel. Native `World::update` keeps its effective
explicit-plus-selected-route vector alive through `RouteRecorder::observe`.
Field-sensitivity/multiplicity tests and a real native update/recording gate
prove the summary matches the inputs that produced target and resonance.

#### 22. Partly resolved: route recording now states sample truth

**Status:** Recording/sample truth resolved by
[Improvement A.12](plans/prototype/improvement_A_12_snapshot_route_truth.md);
ordered traversal semantics remain open.

Previously, a frame that crossed several 192-unit intervals emitted at most one
node, reset accumulated travel to zero, and discarded overshoot. The node's
possibility signature was the covering region's target, while difficulty was
measured from currently realized resonance; in pinned near regions those can
describe an unseen aspiration and the visible world respectively.

RouteRecorder now stores its previous observed position, carries distance
remainder, interpolates every crossed interval up to the node cap, and leaves a
missing due interval pending until its covering authority is resident. V2 route
nodes encode target signature, optional visible-current signature, segment
distance, stability, cost, position, and anchor-set signature. New records set
current signature and nonzero distance for non-initial interval nodes; migrated
v1 nodes explicitly carry unknown current and zero distance while preserving
legacy ids. Route difficulty is distance-weighted when distance metadata
exists and keeps the v1 arithmetic mean fallback otherwise.

Traversal still requires 60% of nodes in one broad corridor leg but not ordered
progress, direction, or continuous path coverage; clustered nodes can be
credited together. Track route-segment progress if usage is meant to represent
following an expedition.

#### 23. Route queries and per-frame scans do not scale with an atlas

Route attraction and traversal scan every node of every route before
truncation. Resonance similarly scans and sorts every near organism.
`RouteGraph` scans and sorts all nodes for every nearest-possibility query; its
signature-seed ordering does not accelerate the L1 metric.

A bounded top-$k$ heap is an immediate improvement. Larger stores need a
physical spatial index for corridors and a metric tree or quantized spatial
index for eight-dimensional possibility signatures.

#### 24. Resolved: A 64-bit content fold is not proof of immutable equality

Previously, merge logic assumed equal ids implied equal immutable fields "by
construction." A 64-bit hash collision is unlikely but possible, and the mixer
is not intended to authenticate untrusted internet bundles. On an id collision,
merge did not compare immutable bodies.

**Resolution (Improvement A.10):** Discovery, route, and preserve records now
have typed immutable-body predicates, and checked merge returns a structured
error on id mismatch or same-id immutable conflict before any mutable metadata
is applied. Vault open/import and atlas canonicalization validate `id ==
content_id`, canonical inner sets, and immutable equality before accepting or
merging records. Public atlas tooling reports a SHA-256 digest over canonical
encoded bundle content; it is a collision/tamper check for a digest obtained
through a trusted channel, not an author signature. Authored signatures remain
future work. Deletion is still not a CRDT operation without tombstones, and
`usage = max` still loses independent traversal increments that a per-replica
grow-only counter could retain.

#### 25. Resolved: Canonical "sets" preserve duplicate multiplicity

Previously, atlas canonicalization sorted but did not deduplicate record ids,
and preserve construction sorted but did not deduplicate repeated region
coordinates. Byte encoding and preserve identity could therefore depend on
duplicates despite set language.

**Resolution (Improvement A.10):** Bundle record vectors are canonical keyed
sets by id. Equal-id records with equal immutable bodies collapse by the normal
mutable merge law; equal-id records with unequal immutable bodies are rejected
as collisions/tamper. Preserve region membership is a true coordinate-keyed set:
exact duplicate coordinate/signature pairs collapse at construction, and
duplicate coordinates with different signatures are invalid. Route discovery
references are sorted and deduplicated by discovery id, documenting them as an
ordered set rather than multiplicity-bearing journal entries. `wer-atlas check`
reports duplicate ids, non-canonical route refs, duplicate preserve
coordinates, empty public routes/preserves, and canonicalization failures.

#### 26. Resolved: Overlapping preserves lacked ownership and conflict semantics

Previously, the runtime stored only one override signature per region. Applying
overlapping preserves made application order decide the winner. Deleting one
preserve could clear a region even if another preserve still covered it;
applying a foreign preserve to a resident near region could also snap its
realized vector and regenerate the supposedly pinned landscape.

The required correction was to track override contributors per region, define
a deterministic conflict rule, and recompute the effective override after
add/delete. Applying a foreign preserve only offscreen or through an explicit
transition was also considered as a possible presentation policy.

There was also a revision-accounting bug at this boundary. `set_override` could
materially replace a resident region's realized vector without bumping that
region's revision. Near-field realization could then reuse an old feature-id
epoch for a different roster or possibility state. A material override change
needed to advance the revision (with same-bucket center snapping explicitly
defined) before dependent organisms were rebuilt.

**Resolution (Improvement A.2):** `RegionMap` now retains an ordered
content-id-to-signature contributor map for every covered coordinate and uses
the lowest content id as its effective owner. Winner deletion recomputes and
applies the successor; non-winner deletion is inert, and final deletion keeps
the no-snap release contract. Any material resident winner snap advances
revision once, while only domain bucket flips dirty the ADR 0007 reader
closure. A.9 later split organism identity from region revision, so
same-bucket center normalization now preserves organism identities and
placement while keeping tile hashes and in-flight tile work intact. Runtime
unit tests, including resident forward/reverse
atomic batches and session restore, the native effective-owner deletion seam,
end-to-end overlap and evicted-deletion tests, and the `wer-vault` sign-off
scenario exercise these contracts. Separate UI calls remain distinct material
history; only canonical synchronization batches reconcile once. Duplicate-
coordinate canonicalization is resolved by Improvement A.10; durable local
delete failure handling is resolved by Improvement A.3 and finding 28.

#### 27. Vault loading is eager and its interface is not browser-shaped

`Vault::open` loads every discovery, route, preserve, and seen chunk into
memory. Native prefix enumeration walks the whole root once per namespace.
This contradicts comments about partial loading and makes startup proportional
to the complete atlas. The synchronous `Storage` methods also do not map
naturally to IndexedDB.

Use asynchronous, paged namespace iteration and lazy indexes. Load seen chunks
and route spatial partitions near the player rather than the whole store.

#### 28. Resolved: Persistence failures and sequence handling are explicit and durable

**Status:** Resolved by
[Improvement A.3](plans/prototype/improvement_A_3_persistence_failures_durability.md).

Previously, `remove_preserve` dropped the in-memory record and ignored backend
remove errors, so a failed delete could silently resurrect after reopen.
Repeated flush failures appended duplicate issue strings. The native shell
could report a save even if dirty data remained. Imported high sequence values
did not advance the local metadata counter until reopen, allowing imported
mutable text to dominate subsequent local edits.

Return and surface delete/flush failures, bound retry reporting, treat a save as
successful only when clean, and advance/dirtify local sequence metadata during
import. Temp-write plus rename also lacks file and directory `fsync`, so it
prevents ordinary torn writes but does not provide the strongest claimed
power-loss durability.

**Resolution (Improvement A.3):** Flush and delete callers now receive
structured operation/key failures and progress, `flush_all` succeeds only when
the vault is clean, and dirty data cannot be retired or overtaken by metadata
after an error. Preserve and route deletion use commit-after-durable-remove, so
a failed action leaves the record and its runtime contribution visible for a
retry. Every valid resulting import record contributes to the live local
sequence maximum; import raises and dirties it immediately when it lags. A
valid `u64::MAX` record remains accepted/exportable/reimportable, while all four
local authoring paths return typed exhaustion without partial mutation when no
newer sequence is representable. Stable issue identities deduplicate into at
most 64 retained diagnostics plus a suppressed counter. Native writes
synchronize a complete temp file before atomic rename and then synchronize the
containing directory; durable ancestor creation and remove/not-found retries
use directory barriers as well. Neutral scripted-storage tests, staged native
file-operation tests, caller regressions, and the expanded `wer-vault` scenario
exercise the ordering, bounded-reporting, retry, deletion, and sequence
contracts. This does not add CRDT tombstones or resolve the synchronous eager
interface/browser backend limitation in finding 27.

#### 29. Resolved: session exactness preconditions are encoded

**Status:** Resolved by
[Improvement A.12](plans/prototype/improvement_A_12_snapshot_route_truth.md).

Previously, the snapshot omitted resource tier/configuration, target vectors,
unfinished route recording, route-tracker leg state, executor queue state, and
caches. Exact restoration was demonstrated for the same algorithm, field,
configuration, platform, anchors, and scripted follow-up, not arbitrary builds
or hardware modes.

SessionSnapshot now records runtime metadata, resident `current` and `target`
vectors, active recorder state, and active tracker leg state. Load paths
compare metadata before claiming exact continuation. Executor queues,
in-flight jobs, disposable caches, rosters, realized organism vectors, renderer
state, and GPU resources remain outside persistence by design; exactness means
save, load, then zero-travel settle under matching metadata, not replaying
transient worker or presentation state.

### 4.4 Scheduling, portability, and verification gaps

#### 30. Native executor shutdown drains work it says it discards

Correctness resolved by
[Improvement A.11](plans/prototype/improvement_A_11_executor_failure_shutdown.md):
`LaneExecutor::Drop` now sets `shutdown` and clears all queued lanes under the
mutex before waking workers, worker loops check shutdown before selecting new
work, and submit-after-shutdown drops the closure. Shutdown can still wait for
already-running Rust closures, but it no longer drains the queued backlog.

Worker/job panics are also bounded. The native worker loop catches and counts
panics so the configured worker count remains live, and runtime generation
closures catch panics separately so `RegionMap` receives structured failed
macro/tile dispatch results. Matching current failures retire their in-flight
entry, dirty the failed layer and dependents, and retry through ordinary
budgeted dispatch; obsolete failed results are dropped like stale successes.

Strict Critical-over-Normal-over-Background priority also has no fairness or
aging, so continuous nearby work can starve the far field indefinitely. The
queue is unbounded, and cancelled no-op closures remain queued. Bounded queues,
weighted aging, and cancellation-aware removal would improve backpressure.

#### 31. The executor abstraction is not directly portable to Web Workers

A boxed Rust closure carrying `Arc` tile snapshots and an `mpsc::Sender` cannot
be serialized into an ordinary isolated browser worker. A browser runtime must
either use wasm shared-memory threads with COOP/COEP deployment constraints, or
replace this boundary with serializable job descriptors and results. The same
redesign pressure applies to synchronous storage.

#### 32. Cache "byte ceilings" are payload estimates, not heap ceilings

Tile accounting uses vector length times element size and roster accounting
counts selected vector elements. It omits `BTreeMap` nodes, `Arc`s, struct
overhead, spare vector capacity, in-flight snapshots, organisms, and most pool
overhead. The limits are useful logical budgets but should not be presented as
hard process-memory caps. Track allocator-observed or capacity-inclusive bytes
when enforcing a true memory tier.

The published macro-tile estimate is also stale: a 48-by-48 `u8 + u32` tile is
11,520 bytes, not roughly 4.5 KiB.

#### 33. Some advertised verification is absent or narrower than stated

**Status:** Resolved by Improvements A.1, A.4, A.8, A.12, and A.13. The
verification surface now covers the advertised contracts directly enough for
the current prototype.

`wer-scale` checks executor choice, worker count, budget scale, cancellation,
retarget amortization, alternate frame slicing, simultaneous field/macro/roster
cache pressure with realization/pool churn, tier identity, cross-tier session
persistence compatibility, and density additivity. The all-cache case proves
that logical cache ceilings can evict or rebuild disposable caches while the
return-trip near-window output is reproduced; it does not claim full
process-memory accounting, which remains finding 32.

The shared settled-state hash covers regional authority, field/biome/dominant
tiles, macro cache, roster cache, and realized organisms including trophic
role, slot, cell, exact world position bits, and expressed presentation fields.
Executor queues, worker closures, and GPU presentation are not part of settled
state; schedule and persistence harnesses settle first and require no in-flight
work before comparing hashes.

Focused runtime tests cover field/macro/roster recovery, stale-result rejection
for every layer shape, ordinary divergent-history borders through Terrain,
Slope, Hydrology, Soils, and Biome, and preserve/radius-drop edge paths. Route
tests now include a recorded multi-node path that attracts softly near distinct
nodes and segments, stays corridor-bounded, respects `ROUTE_PULL_CAP`, and
does not replace the target outright. SIMD differential tests enumerate all 12
biome ids, including Ice. Wasm parity is still an executed
`wasm-pack test --node crates/platform-web` suite, not only a compile check.

Remaining limitations are explicit: ordered route traversal is not implemented
by the route-softness tests, and cache byte ceilings are logical payload
budgets rather than complete heap ceilings.

#### 34. GPU refinement is safely derived, but its parity story can improve

The WGSL shader duplicates hash constants, algorithm version, habitat banding,
and species hashing from Rust. Because there is no readback this cannot corrupt
world state, but a core change can silently make CPU and GPU visuals disagree.
Generate shared constants or add a presentation-parity test.

Gradient noise is zero-mean statistically; adding it does not guarantee zero
detail at every authoritative cell center or preserve each local sample's exact
average. If exact coarse anchoring is visually important, subtract the octave
value at the cell center or construct an explicitly mean-preserving residual.

### 4.5 Intentional scope limitations worth keeping visible

The following are not implementation mistakes, but they strongly shape what
the current prototype demonstrates:

* There is one fixed canonical base world; no user world seed is exposed.
* The eight possibility domains are single proxies, not rich trait spaces.
* The active representation is a uniform sparse grid, not an adaptive
  quadtree, clipmap, or multi-LOD environmental hierarchy.
* Unpreserved regions forget their realized history after geometric radius
  unload; field-capacity parking inside the window retains it.
* Organisms are sampled markers, not agents with behavior or local simulation.
* Ecology has no migration, reproduction, competition, succession clock, or
  evolutionary history.
* The renderer is a debug map, not the planned terrain/vegetation/organism 3D
  renderer.
* The web target is a compile/parity shell, not a browser game runtime.
* Atlas exchange is file-based; there is no networking, server, trust model,
  multiplayer, or collaborative service.

These constraints are sensible for proving the architecture, but product-facing
descriptions should not imply that the richer systems already exist.
