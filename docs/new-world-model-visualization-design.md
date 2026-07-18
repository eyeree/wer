# New World Model Visualization Design

## Status and scope

This document defines a basic, concrete Visualization architecture for the
Realization boundary in
[`new-world-realization-interface.md`](new-world-realization-interface.md).
It supports the World Loom in
[`new-world-model-option-4.md`](new-world-model-option-4.md), including its
typed constitutions, projective planet, chunk-backed snapshots, and transition
events. The architecture is intentionally usable by another Model that
negotiates the same semantic profile; it does not make Loom packet types or
solver internals renderer inputs.

The normative Visualization requirements are in
[`new-world-model-visualization-requirements.md`](new-world-model-visualization-requirements.md).
Terms and responsibility boundaries follow
[`conceptual-model.md`](conceptual-model.md). Proposal-selection context comes
from [`new-world-model-comparison.md`](new-world-model-comparison.md). This
design preserves the accepted one-world, shared-host decision in
[ADR 0028](adr/0028-shared-viewer-host-and-one-world-multi-view.md).

This is an architecture and implementation design, not an implementation plan.
It does not specify milestones, staffing, estimates, or an exhaustive work
breakdown. Concrete sizes below define a useful first profile or a bounded API
shape; they are subject to measurement and profile versioning rather than being
delivery estimates.

## Decision summary

The Visualization is one shared, platform-neutral host over one immutable
Realization snapshot. It owns one Traveler, one canonical model-time selector,
one deterministic Visualization simulation clock, one query broker, one Map
presenter, one POV presenter, and one presentation-only transition history.
Each logical tick updates that state once. Map and POV packets are then built
from the same post-update `FrameSemanticView`; Split changes only layout and
records both panes into the same surface frame.

The principal decisions are:

- `viewer-host` is generalized from concrete `RegionMap` access to the
  object-safe snapshot/query contract. It remains the authority for normalized
  actions, one Traveler, one snapshot, query scheduling, semantic Map/POV
  interpretation, inspection, the information document, layout, and focus.
- `pov-host` remains a pure, platform-neutral presentation-preparation crate.
  It consumes copied semantic geometry/material/organism DTOs, produces CPU
  terrain and primitive geometry, and retains the CPU lattices used for
  grounding and picking. It does not query the Model independently.
- `renderer` remains world- and Model-agnostic. It consumes bounded upload
  values, owns only GPU resources, exposes no live readback, and records Map,
  POV, or Split through one acquire, one submission, and one present.
- native and web compile the same Model adapter, realization API, shared host,
  POV preparation, and renderer modules into one executable or one wasm module.
  Platform crates own I/O, immutable chunk acquisition, executor/Worker policy,
  surfaces, lifecycle, and recovery.
- the CPU Map raster and CPU presentation geometry are the test, headless,
  inspection, collision, and picking truth. GPU Map composition, shadows,
  detail normals, water wobble, fog, and rasterized pixels are derived
  presentation only.
- Option 4's scalar channels, categorical mixtures, typed measures, typed
  relations, canonical observations, forcing, and transition event pages stay
  distinct through the host. The adapter does not flatten them into a generic
  float array merely because some views ultimately choose colors or primitive
  dimensions.
- `Complete`, `Partial`, `Unresolved`, and `NeedsInput` are ordinary streaming
  states. Continuations are value tokens, immutable chunks are injected by the
  host, and no placeholder is labeled canonical.
- the first visual style has ambition comparable to the present POV: a
  deterministic triangulated surface, simple material colors, translucent
  water, box/sphere primitive organisms, one directional light, analytic sky,
  distance fog, and small deterministic presentation motions. It does not
  require procedural texture synthesis, complex mesh grammar, skeletal
  animation, or runtime learned content.

These choices implement the consumer side of `VIZ-R01`–`VIZ-R160`. Builds are
kept as a negotiated optional overlay under `VIZ-R161`–`VIZ-R168`; a Build
renderer is not required by the basic natural-world profile.

## 1. Goals and non-goals

### 1.1 Goals

The design has the following goals.

1. Present one complete canonical world through coherent top-down Map,
   embodied POV, and fixed-ratio Split experiences (`VIZ-R05`–`VIZ-R10`,
   `VIZ-R74`–`VIZ-R91`).
2. Support Option 4 without weakening its semantic distinctions: exact point
   versus cell address, continuous versus categorical values, measures versus
   samples, relation couplings versus scalar fields, endpoint identity versus
   transition-local correspondence, and missing input versus unresolved work
   (`VIZ-R13`–`VIZ-R16`, `VIZ-R116`–`VIZ-R140`).
3. Preserve intimate movement precision on a finite oblate planet while also
   supporting regional and whole-planet overview (`VIZ-R18`–`VIZ-R33`,
   `VIZ-R141`, `VIZ-R149`).
4. Keep canonical observation independent of pixels and use one observation
   path for Map inspection, POV inspection, and Impression confirmation
   (`VIZ-R62`–`VIZ-R73`, `VIZ-R90`).
5. Make incomplete, approximate, uncertain, stale, unsupported, and failed
   information visibly and programmatically distinct (`VIZ-R14`,
   `VIZ-R116`–`VIZ-R126`).
6. Stream bounded work without making priority, cancellation, cache capacity,
   completion order, or tier part of settled meaning (`VIZ-R127`–`VIZ-R152`).
7. Reconcile continuous changes, matches, births, deaths, splits, merges, and
   topology changes without creating false persistent identity or a global
   reload (`VIZ-R101`–`VIZ-R115`).
8. Preserve the current native/web module boundary and one-surface frame while
   allowing truthful reduced experiences after unsupported capabilities or GPU
   loss (`VIZ-R153`–`VIZ-R160`).

### 1.2 Non-goals

The basic Visualization does not:

- choose an Egress destination, calculate Reachable Possibility, interpret a
  Yearning, or compute Resonance;
- mutate a Model State, an Option 4 State Packet, a canonical organism, or
  canonical model time as a side effect of rendering;
- reproduce photorealistic geology, atmosphere, water, vegetation, or animals;
- generate caves, overhangs, volumetric interiors, or multiple traversable
  surfaces when the negotiated geometry profile is a height surface;
- infer canonical facts from a color, mesh, animation pose, visibility result,
  or GPU depth value;
- promise that a coarse or missing query has exact fine-scale geometry;
- make transition blending, cached old meshes, or simulation-only organism
  paths portable Model identity;
- specify the authored Build graph, social services, storage protocol, or
  network transport; or
- define a stable Rust dynamic-library ABI. Separately distributed Models use
  the wire/component binding described by the shared interface.

If a Model advertises required non-height-field geometry, an embodied profile
that only implements this basic surface design is refused or explicitly reduced
rather than silently flattening it (`VIZ-R17`, `VIZ-R26`, `VIZ-R155`).

## 2. Supported experience and capability profile

### 2.1 `basic-planet-map-pov-v1`

The full profile is named `basic-planet-map-pov-v1`. Compatibility is decided
before a snapshot is used. It requires semantic agreement, not equal labels,
on the following capability families:

| Capability | Required meaning in the full profile | Reduced behavior |
|---|---|---|
| World domain | finite planetary surface, point and nested cell schemas, metric, orientation, gravity/local-frame rule, extent, seam rule | no reduction; refusal |
| Surface geometry | one traversable height/displacement surface, bounds, solidity/traversability, discontinuities, projective refinement | Map-only if collision-grade geometry is absent |
| Material | substrate/material category or mixture with units/applicability and aggregation law | neutral untextured geometry, visibly marked optional loss |
| Water | surface/depth or canonical absence, coastline/connectivity, flow where claimed, physical-domain semantics | dry visual profile only when water is explicitly unsupported, never when unresolved |
| Environment | climate, soil, biome, and vegetation semantics needed by the selected composite style | terrain-only reduced style |
| Ecology | typed populations or trait measures, abundance support, habitat constraints, representative manifestation rule, and typed relations | ecology-hidden profile; never “canonical extinction” |
| Observation | subject resolution, attributes, relations, quality, provenance, capture eligibility | inspection-limited refusal for ordinary play |
| Model time and forcing | canonical time schema plus illumination/climate response inputs | declared timeless lighting profile if the Model is timeless |
| Transition | endpoint/path provenance, continuous bounds, correspondence/events, risk, status, continuations | static-snapshot profile; no Egress continuity claim |

The profile accepts `Preview`, `Interactive`, and `Canonical` grades but assigns
them different authority. Preview may fill distant Map context. Interactive is
the ordinary visible geometry grade. Canonical is required for Impression
confirmation and any exact subject claim. A Model that offers only same-platform
results cannot satisfy the profile's cross-platform canonical-observation claim.

Compatibility also pins:

- the semantic vocabulary and every consumed capability revision;
- unit conversions or explicit semantic adapters;
- identity scope and refinement guarantees;
- aggregation/restriction behavior for quantities, categories, measures, and
  relations;
- the time basis and forcing interpretation;
- required quality/error forms;
- maximum address, page, token, scratch, and immutable-input sizes; and
- whether a lower complete grade is a valid conservative source for collision.

The resulting report separately states Map, POV, Split, inspection, ecology,
time, transition, and Build compatibility (`VIZ-R153`–`VIZ-R157`).

### 2.2 Presentation capability profile

The first renderer/presenter profile is deliberately small:

- top-down composite and thematic Map layers;
- triangular height-surface terrain with simple vertex/material colors;
- opaque land, translucent sea/lake/river surfaces, and no underwater volume
  scattering;
- deterministic box and icosphere organism primitives with non-uniform scale;
- one directional light, ambient hemisphere light, an analytic sky, optional
  simple clouds, directional shadows, and distance fog;
- static organisms plus small deterministic bob/turn/wander presentation
  motions that remain inside canonical habitat support;
- CPU ray picking and height-surface collision; and
- semantic panel rows for fields, entities, measures, relations, quality,
  provenance, and transition state.

Texture atlases, authored meshes, skeletal rigs, audio, particles, volumetric
clouds, and procedural mesh synthesis are optional later presentation profiles.
Their absence does not reduce canonical meaning.

## 3. Architectural boundaries and ownership

### 3.1 Dependency direction

The dependency and data-flow shape is:

```text
                         platform-native / platform-web
                  I/O, chunks, executors, DOM/winit, surfaces,
                         durable services, lifecycle/recovery
                                      |
                                      v
                                viewer-host
                  one snapshot + Traveler + tick + query broker
                   Map/POV semantics, layout, inspection, panel
                         |                         |
                         v                         v
                      pov-host                 renderer DTOs
             pure geometry/simulation prep          |
                         |                           v
                         +----------------------> renderer
                                      GPU resources and one surface frame

        Option adapter ----------------------+
                 |                            |
                 v                            v
         Model implementation ---------- realization-api
          canonical pure work          identities/descriptors/queries/status
```

Dependency arrows never reverse:

- `realization-api` is platform-neutral and contains semantic types and
  object-safe query traits only.
- an Option 4 adapter/runtime depends on `realization-api`; it has no renderer,
  window, filesystem, socket, clock, or platform-thread dependency.
- `viewer-host` depends on the semantic API and on value/upload types from
  `pov-host` and `renderer`. It never imports winit, DOM, filesystem, socket,
  or native thread-creation APIs.
- `pov-host` accepts copied presentation inputs. It does not own the canonical
  snapshot and cannot issue a second semantic query stream.
- `renderer` accepts only model-agnostic upload/frame values. It does not
  depend on `viewer-host`, `realization-api`, Option 4, or a Model crate.
- platforms implement execution, immutable input acquisition, storage,
  surface creation, and device recovery. They do not reinterpret fields or
  construct independent Map/POV observations.

This keeps the neutral-crate rule in [`AGENTS.md`](../AGENTS.md), preserves
[ADR 0002](adr/0002-workspace-crate-boundaries.md), and retains ADR 0028's
parallel rule for `viewer-host` and `renderer`.

The existing prototype path may coexist behind a `RegionMap` adapter while it
is supported. The generalized host does not redefine the prototype's
`PossibilityField`, region identities, or persistence records as Loom values.

### 3.2 `realization-api`

The shared semantic crate owns:

- Model, package, State, vocabulary, snapshot, capability, semantic, entity,
  transition, query, and continuation identities;
- World-domain, point/cell address, metric, topology, orientation, time,
  field, measure, relation, observation, and quality descriptors;
- object-safe `RealizationModel`, `RealizationSnapshot`, and Loom profile
  traits;
- caller-owned bounded request/output records;
- `Complete`, `Partial`, `Unresolved`, and `NeedsInput` envelopes;
- per-datum availability, authority, quality, support, and provenance; and
- compatibility and evolution reports.

It owns no presentation palette, camera, cache, simulation, renderer handle,
or platform service.

### 3.3 Option 4 adapter

The adapter validates the supplied State envelope and immutable chunk closure,
opens one snapshot, publishes the icosahedral domain descriptor, and maps Loom
law nodes into negotiated semantic capabilities. It preserves first-class Loom
measure, coupling, and transition-event pages through the Loom extension
traits.

The adapter is the only component that knows such details as State Packet law
ids, motif atoms, sparse coupling forms, program strata, or solver revisions.
It may expose those concepts through typed descriptors, but it does not leak a
Loom allocation, cache pointer, or mutable solver object into presentation.

### 3.4 `viewer-host`

The shared host owns:

- the selected compatibility report and exact semantic bindings;
- one open immutable current snapshot and its complete identity;
- one canonical Traveler World address and one controller path accumulator;
- one model-time selector and one Visualization simulation clock;
- normalized input, ordered actions, view mode, focus, and fixed Split ratio;
- the bounded query broker, immutable-input requests, continuation queues,
  result caches, and publication barriers;
- Map projection, semantic styles, CPU composition, atlas preparation, and Map
  inspection;
- POV interest selection, semantic-to-presentation DTO construction, camera
  intent, canonical collision policy, and observation requests;
- presentation-only transition continuity state;
- canonical observation and Impression-confirmation state machines; and
- the semantic information document and diagnostics snapshot.

Only this host may replace the current snapshot, advance the Traveler, or
publish a completed semantic page into the current frame view. A platform may
deliver query/chunk job results, but those results enter an ordered tick queue
before they become visible.

### 3.5 `pov-host`

`pov-host` generalizes the useful patterns in
[`crates/pov-host/src/lib.rs`](../crates/pov-host/src/lib.rs): pure meshing,
provenance-keyed handles, cancellation, bounded integration, CPU surface
lattices, primitive-instance construction, and CPU ray tests.

It owns:

- tessellation of `SurfacePatchInput` into renderer vertices and indices;
- simple material-color and organism-primitive mapping selected by the
  Visualization profile;
- presentation handle lifecycles and the CPU copy paired with each upload;
- camera-relative transform preparation;
- the fly/walk camera and pure local-frame movement, grounding, and picking
  calculations whose accepted result `viewer-host` applies to the one
  canonical Traveler;
- deterministic fixed-step transient organism motion; and
- terrain/water/primitive ray intersection helpers.

It does not decide whether a value is canonical, resolve a subject, confirm an
Impression, request chunks, or advance Model time.

### 3.6 `renderer`

The renderer retains the staged transaction in
[`crates/renderer/src/lib.rs`](../crates/renderer/src/lib.rs): resource uploads
are prepared before surface acquisition, the surface is acquired once, one
encoder records all visible panes and decorations, the queue is submitted once,
and the surface is presented once.

The renderer owns opaque Map atlas slots, surface-patch buffers, water-index
buffers, primitive-instance buffers, shadow/depth/offscreen targets, pipelines,
bind groups, and the presentation surface. It has no semantic query, cache
authority, canonical hash, observation, persistence, Egress, or readback API.

## 4. Option 4 semantic binding

### 4.1 Binding rule

The profile uses stable semantic ids only after the compatibility report binds
each id to an exact capability descriptor, vocabulary, unit, support,
aggregation law, quality policy, and semantics digest. The strings below are
readable names for those bindings, not permission to match by text.

The adapter may bind one Loom law node to several semantics or combine several
law nodes through a declared exact/quality-bounded semantic adapter. Every
derived binding records that adapter in provenance. A presentation convenience
computed by `viewer-host` uses `PresentationOnly` authority and a Visualization
derivation id.

### 4.2 Terrain, material, and water

| Profile semantic id | Option 4 source | Canonical payload | Map use | POV use |
|---|---|---|---|---|
| `surface.elevation` | terrain variational section | signed length at point/cell plus interval, footprint, projective level | elevation ramp, hillshade input | vertex displacement and CPU ground lattice |
| `surface.slope` | declared terrain observation or bounded derivative | dimensionless slope/vector with support and error | slope theme | normals and traversability; never inferred as canonical from shading |
| `surface.landform` | active-set/observation node | categorical or mixture value plus classifier margin | named landform theme | inspection only in basic style |
| `material.substrate` | lithology measure/field | material category or nonnegative mixture with restriction law | geology/material colors | simple albedo and roughness class |
| `material.strength` | terrain/material law | typed intensive quantity and interval | stability/affordance theme | collision affordance when negotiated |
| `water.surface` | ocean/lake level and geometry | surface support, height/shape, connectivity, quality | ocean/lake fill and coastline | separate water triangles/patches |
| `water.depth` | inventory/geometry solve | nonnegative length or canonical absence | depth theme | opacity/color class; fluid entry CPU test |
| `water.flow` | hydrology discrete form | directed flux with unit, edge/curve support, and bounds | river/flow overlay | river ribbons and inspection |
| `water.wetness` | hydrology/soil relation | continuous fraction or capacity-defined quantity | wetness theme | material gloss input |

Elevation remains a canonical geometry value. Hillshade, vertex normals not
provided by the Model, skirt geometry, mesh decimation, water glint, and fine
normal wobble are presentation-only. A missing water page never means dry land;
only `CanonicalAbsence` does (`VIZ-R14`, `VIZ-R25`, `VIZ-R38`).

### 4.3 Climate, soil, biome, and vegetation

| Profile semantic id | Option 4 source | Required semantics | Basic presentation |
|---|---|---|---|
| `climate.temperature.mean` | reduced climate section | temperature unit, averaging interval/phase, support, residual bound | thematic color and inspection |
| `climate.moisture.mean` | climate/moisture section | quantity kind, interval, support, model-time basis | thematic color and sky/weather envelope |
| `climate.precipitation.envelope` | climate response modes | rate/statistical envelope, period, uncertainty | simulation constraint; never a live rain event |
| `climate.wind.prevailing` | circulation form | tangent vector/flux, time phase, support | deterministic vegetation sway direction and panel |
| `soil.composition` | soil balance | typed mixture with exact/interval totals | soil color mixture |
| `soil.depth` | soil balance | nonnegative length, footprint, uncertainty | terrain tint and inspection |
| `soil.moisture_capacity` | soil/hydrology coupling | intensive/capacity meaning and unit | wet-ground response constraint |
| `soil.fertility` | soil/productivity block | declared synthetic index or physical unit | soil/vegetation tint |
| `biome.membership` | fuzzy observation node | full category membership vector and margins | mixture-aware biome color; no forced argmax at ecotones |
| `vegetation.cover` | productivity/ecology output | fraction and footprint | ground tint and density cue |
| `vegetation.structure` | trait/height measure | measure over height/form classes | simple billboard-free box/sphere plant primitives near the Traveler |
| `vegetation.biomass` | ecology inventory | extensive mass with area/time support | honest density theme and inspection |

Map aggregation follows each descriptor: intensive quantities use a declared
weighted average, extensive quantities sum, categorical memberships restrict
as mixtures, and vectors use the declared frame/flux rule. The Visualization
does not apply one arithmetic average to all channels (`VIZ-R37`, `VIZ-R47`,
`VIZ-R48`, `VIZ-R147`).

### 4.4 Ecology and typed relations

| Profile semantic id | Loom profile source | Preserved meaning | Basic presentation |
|---|---|---|---|
| `ecology.trait_measure` | typed measure page | motif domain, mass kind/unit, support, total interval, atoms, restriction | Map diversity/biomass summaries; POV representative sampling |
| `ecology.species_mode` | resolved ecology entity page | exact state-local manifestation id, matching key, traits, niche, margin | species theme, panel, primitive mapping |
| `ecology.abundance` | measure/observable | count, density, biomass, occupancy, or expectation plus denominator support | density surface and manifestation budget label |
| `ecology.habitat` | relation/field page | suitability versus presence versus absence, constraints, support | habitat theme and placement constraint |
| `ecology.trophic_flow` | typed coupling page | source/target roles, energy-per-time unit, marginals/submarginals, efficiency, slack | relationship panel; optional flow overlay |
| `ecology.representative` | canonical manifestation query | sample address, exact manifestation/species id, epoch, observable traits | deterministic box/icosphere instance |

Trait measures are not converted into fake species counts. Trophic couplings
are not converted into an untyped adjacency matrix. The host retains their
measure/relation schemas and exposes them to the information document and
Impression observation. A Map style may derive diversity, dominant mass, or
role balance only through a semantic adapter whose aggregation and uncertainty
are recorded.

At broad scale the Map normally presents population measures, not individuals.
At close Map scale and in POV, the representative query yields a deterministic,
bounded sample. The panel always reports both manifested count and canonical
abundance meaning so a low tier cannot look like extinction (`VIZ-R49`–
`VIZ-R61`, `VIZ-R148`).

An exact Option 4 manifestation id is endpoint-state identity. A lineage-basin
key is a matching feature. A renderer primitive, presentation instance handle,
and simulation actor id are Visualization identities. None is silently
substituted for another.

### 4.5 Forcing and observations

| Profile semantic id | Option 4 source | Consumer behavior |
|---|---|---|
| `forcing.illumination` | orbital/stellar forcing | drives light direction/intensity and sky palette at explicit model time |
| `forcing.tide` | lunar/ocean forcing | constrains water surface phase if the water schema supports it |
| `forcing.season` | canonical cycles | selects climate/ecology phase and simulation envelope |
| `forcing.response_mode` | climate/ecology response output | constrains transient weather and vegetation/organism motion |
| `observation.subject` | canonical observe query | resolves subjects, ambiguity, attributes, relations, quality, capture eligibility |
| `observation.attachment` | attachment query | supplies tangent/gravity/feature frame for optional Builds |

Forcing is canonical Model input to presentation. A cloud, gust, ripple,
footstep, pose, and transient organism path are Visualization simulation. The
current model time, simulation time, and wall time remain separate values.

### 4.6 Transition semantics

| Transition content | Option 4 meaning | Visualization use |
|---|---|---|
| continuous channel bounds | certified endpoint/path interval on named support | bound interpolation/cross-fade and determine refinement need |
| law coupling | transport in motif/law space | trait/material continuity evidence only; never assumed spatial deformation |
| feature match | transition-local correspondence between endpoint entities | retain a presentation thread while endpoint ids remain distinct |
| birth/death | no endpoint predecessor/successor | stage independent appearance/disappearance |
| split/merge | transition-local many-to-many ancestry event | branch/converge presentation threads without forging identity |
| topology event | persistence/active-set event with support and path interval | local dual-geometry handoff, risk cue, and collision cutover |
| unresolved event | bounded work cannot resolve correspondence/topology | independent fade or neutral concealment with explicit unresolved status |
| recommended step | Model-provided maximum presentation path step | upper bound on continuity sampling; not a simulation time step |

Every transition page retains from/to State ids, path/checkpoint provenance,
World support, parameter interval, quality, margin, event identity, page order,
and continuation. Transition-local ids expire with the named transition and
are never persisted as global entities (`VIZ-R103`–`VIZ-R115`,
`VIZ-R132`).

## 5. One Traveler, one snapshot, one tick, one frame

### 5.1 Authoritative host state

At any completed logical tick the host has exactly one authoritative current
snapshot:

```rust
pub struct VisualizationWorld {
    current: Box<dyn RealizationSnapshot>,
    compatibility: CompatibilityReport,
    traveler: PlanetTraveler,
    model_time: TimeSelector,
    simulation: VisualizationSimulation,
    transition: Option<ContinuityState>,
    queries: QueryBroker,
    map: PlanetMapPresenter,
    pov: PlanetPovPresenter,
    layout: ViewLayout,
}
```

`ContinuityState` may retain copied presentation data and an immutable handle
to the former endpoint for bounded reconciliation, but it does not become a
second current snapshot. Every copied old datum carries `CanonicalTransition`
or `PresentationOnly` authority plus its source State. Canonical observation,
collision after a transition cutover, Impression confirmation, current model
time, and new queries always use `current`.

The Traveler's canonical state is a schema-tagged World point, not the Map
center, a camera-relative vector, a GPU coordinate, or a resident tile. Its
View camera may move relative to the Traveler only where the input profile
explicitly permits a free camera; Egress credit consumes the collision-resolved
canonical Traveler path, never camera orbit or Map pan.

### 5.2 Logical tick order

One logical tick applies this fixed order:

1. Drain sequenced platform service results: verified immutable chunk
   injections, completed bounded query invocations, storage results, and GPU
   capability notifications.
2. Validate each query result against its snapshot/request/page key. Publish
   complete pages atomically, merge valid `Partial` prefixes, and discard stale
   or canceled invocations without side effects.
3. Reduce queued semantic actions in total order, including view/focus changes,
   Map projection/scale, model-time controls, and an explicitly selected
   Egress-plan result from the Traveler layer.
4. Sample continuous input once. Resolve the intended Exploration segment
   against CPU canonical collision data. The Traveler records the resulting
   exact World path and invokes its travel-to-Egress rule outside
   Visualization simulation.
5. If the Model/Traveler commits a new State checkpoint, validate/open its
   snapshot, replace `current` once, and initialize one bounded
   `ContinuityState` from the named Transition Plan. No pane performs its own
   replacement.
6. Advance canonical model time only through a supported explicit action or
   gameplay policy. Advance Visualization simulation in deterministic fixed
   ticks according to the recorded model-time/simulation-time mapping.
7. Compute Map and POV interest from the same post-update Traveler, snapshot,
   model time, transition, simulation tick, and layout. Update the query broker
   once and emit platform job/input effects.
8. Advance Map and POV presentation lifecycles once. Construct one immutable
   `FrameSemanticView` and the semantic information document.
9. Build zero, one, or both render panes from that view. Submit one
   `MultiViewFrame` when a GPU surface is used.

The order generalizes the current
[`ViewerController::tick`](../crates/viewer-host/src/controller.rs) contract.
It prevents action or result races from producing a Map of State A and a POV of
State B. A query completion arriving after frame construction waits for the
next tick; a platform callback never mutates a displayed page in place.

### 5.3 Frame semantic view

```rust
#[derive(Debug, Clone)]
pub struct FrameSemanticView {
    pub logical_tick: u64,
    pub snapshot: SnapshotIdentity,
    pub traveler: WorldAddress,
    pub model_time: TimeSelector,
    pub simulation_tick: u64,
    pub transition: Option<TransitionFrameRef>,
    pub map_revision: u64,
    pub pov_revision: u64,
    pub observation_revision: u64,
}
```

The value is small and copied into pane-build inputs. It does not borrow a
mutable cache. Each pane asserts that its packet key contains the same
`logical_tick`, `snapshot`, `traveler`, and `model_time`. Split uses the
existing fixed-ratio semantics from
[`layout.rs`](../crates/viewer-host/src/layout.rs): one resolved physical layout,
one focus owner, a fitted Map rectangle, one POV aspect, and nonoverlapping
pane rectangles.

### 5.4 Surface transaction

The renderer's device-free pass plan remains:

```text
surface clear
  -> optional Map
  -> optional POV shadow
  -> optional POV offscreen color/depth
  -> optional POV composite
  -> optional semantic information surfaces
  -> optional focus decoration
  -> submit once -> present once
```

POV renders into a pane-sized offscreen target so its color/depth clear cannot
erase Map. A CPU Map in Split is uploaded into that same transaction. A
Map-only CPU fallback may use a platform 2-D canvas and no GPU surface attempt,
as the browser does today; this is still one visible Map presentation, not a
second world update. A successful GPU Map, POV, or Split frame has exactly one
acquire, one surface clear, one ordered command submission, and one present
(`VIZ-R10`, ADR 0028).

## 6. Bounded, status-aware query and streaming design

### 6.1 Query broker

`viewer-host::QueryBroker` owns semantic demand and publication; it does not
execute threads or fetch bytes. It converts Map/POV/inspection interests into
normalized bounded requests against the one snapshot. Each invocation carries:

- snapshot, generation, package, vocabulary, capability, and algorithm
  revision;
- explicit World support and model time;
- accuracy/tolerance, item, byte, scratch, and work caps;
- normalized semantic selectors and aggregation/refinement request;
- optional transition identity and direction;
- optional continuation token; and
- caller-owned output buffers whose capacity is checked before mutation.

The broker emits declarative jobs to an injected `TaskExecutor` or invokes the
same bounded function inline. Native may use Critical, Normal, and Background
lanes. Browser may use `InlineExecutor` or a Worker adapter. The semantic call
and publication checks are identical.

No result borrows a Model cache. A worker returns owned/caller-buffer bytes and
metadata. Cancellation before publication makes the invocation observationally
absent.

### 6.2 Priority classes

The shared host assigns priority from semantic need, not Model-specific layer
names:

| Priority | Demand | Examples |
|---|---|---|
| Critical | required for safe current interaction | collision support immediately ahead, canonical Impression confirmation, explicit inspection continuation |
| Visible | affects a currently visible pane | POV surface/water patches, Map cells in the physical viewport, current-subject observation |
| Near | likely to become visible or needed for continuity | adjacent POV rings, Map pan margin, transition events near the Traveler |
| Background | improves richness without blocking meaning | finer projective children, distant ecology, relation detail, shadow-only geometry |

The three-lane executor maps `Critical` directly, folds `Visible` and `Near`
into stable Normal subqueues, and maps `Background` directly. Within a queue,
the broker orders by logical deadline, geodesic distance, coarser level first,
canonical address, capability id, and request digest. This ordering improves
latency and repeatability of telemetry; it is not Model identity.

Changing focus may reprioritize Map and POV work but cannot cancel collision,
observation confirmation, or already committed page prefixes. Split requests
both visible sets from one budget; it does not double the world tick.

### 6.3 Cancellation and stale work

A request receives a host-owned cancellation generation. It is canceled when:

- its snapshot is no longer current and it is not retained by the active
  Transition Plan;
- its Map projection or POV interest leaves the bounded retention margin;
- a strictly stronger request supersedes the same support;
- model time changes outside the capability's validity interval;
- the user changes a thematic semantic binding; or
- a resource-tier change removes optional presentation demand.

Pure Model work checks a supplied token only at declared safe points and
publishes nothing itself. The broker accepts a completion only if snapshot,
request digest, page cursor, and cancellation generation still match. A stale
completion may populate an immutable content cache if its full key is valid,
but it cannot alter current coverage, observation, collision, or transition
state.

### 6.4 Completion-status handling

The four query statuses have concrete consumer behavior.

#### `Complete`

The broker publishes the whole requested page at the achieved grade. Per-datum
statuses still apply; a complete page may contain canonical absence,
approximation, or uncertainty. Coverage and provenance become visible together
in one tick.

#### `Partial`

The returned `PageInfo` must describe a complete canonical prefix. The broker
copies that prefix into a page set, records its coverage and quality, and
queues the value continuation under the same request. Unwritten output remains
uninitialized and invisible. Already published cells/features retain their
meaning while later pages arrive.

Map paints only covered supports and shows the remaining support as incomplete.
POV admits only complete patch units whose boundary inputs are valid; it never
constructs half a collision mesh. Resolved transition components may animate
while other components remain pending.

#### `Unresolved`

`Unresolved` is not absence and not failure. With a continuation, the broker
may schedule more work under the applicable priority and budget. Without one,
the status remains attached to its support until the request or Model inputs
change.

If a lower complete projective parent or lower requested grade is semantically
valid, it may remain visible with that exact quality/provenance. Otherwise Map
uses the unresolved style and POV leaves a visible neutral gap/veil with a CPU
collision barrier. A plausible invented terrain, biome, species, or relation is
forbidden.

#### `NeedsInput`

The broker validates and deduplicates the named immutable `ChunkId`s, emits a
bounded `InputNeed` effect, and retains the continuation. A platform adapter
loads bytes from its explicit package/bundle/storage policy, verifies length
and digest, and injects `ChunkView`s at a later tick. Neutral Model code never
opens a file, URL, IndexedDB database, or socket and never blocks through a
callback into JavaScript.

Missing input, corrupt input, incomplete solver work, and semantic ambiguity
remain separate. Corrupt data becomes `ModelError`; unavailable optional input
may reduce the negotiated capability; ordinary missing input stays
`NeedsInput`.

### 6.5 Per-datum status presentation

The host maps `DatumStatus` to a shared status style independent of thematic
color:

| Status | Map treatment | POV treatment | Inspection wording |
|---|---|---|---|
| `Present` | thematic color | normal geometry/material | value plus quality |
| `CanonicalAbsence` | declared empty/zero style | omit phenomenon, retain supporting geometry | “canonically absent” |
| `Inapplicable` | hatched not-applicable mask | omit phenomenon | reason/applicability |
| `Uncertain` | bounded stipple or uncertainty overlay | conservative geometry plus subtle tint when geometry exists | interval/source |
| `Approximate` | ordinary color plus quality indicator | geometry at achieved bound | achieved grade/error |
| `UnresolvedRefinement` | coarse cell outline | retain valid parent, suppress invented child detail | unresolved level |
| `StaleTransitionContent` | provenance tint | old presentation ghost only | old State/transition source |
| `Unsupported` | disabled layer pattern | omit optional phenomenon or reduce experience | unsupported capability |
| `OutOfDomain` | domain background | hard boundary/no geometry | invalid World support |
| `GenerationFailed` | failure crosshatch | local failure veil/barrier | typed error id |

Status texture/patterns are accessibility-redundant: the semantic panel and
optional outline/symbol language do not rely on hue alone. Styling never
changes the underlying status.

### 6.6 Cache layers and keys

The design has four bounded cache classes:

1. immutable semantic pages copied from query outputs;
2. CPU Map rasters and projective coverage tables;
3. CPU POV surface/water meshes and primitive lists; and
4. renderer-owned upload resources keyed by opaque presentation handles.

A semantic page key is complete enough to reject accidental reuse:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SemanticPageKey {
    pub snapshot: SnapshotIdentity,
    pub capability: CapabilityId,
    pub semantic: SemanticId,
    pub query_revision: u32,
    pub model_time: TimeSelector,
    pub support_digest: [u8; 32],
    pub request_digest: [u8; 32],
    pub accuracy: AccuracyRequestKey,
    pub page_cursor: PageCursorKey,
    pub dependency_digest: DependencyDigest,
}
```

A continuation token is stored beside, not replaced by, `page_cursor`; it is a
bounded value committed to this key and remains valid after cache eviction.
Presentation keys add Visualization version/style, projection or local-frame
revision, tier-dependent density, transition-presentation state, and consumed
semantic-page digests. None of those values enters canonical Model identity.

Caches use byte ceilings. Eviction preference is optional detail first, then
farthest geodesic support, then oldest presentation use, with canonical key as
a stable tie-break. Current collision cells, an active observation, and the
bounded continuity near-zone may be pinned within declared caps. If pins exceed
a cap, the host degrades optional density/refinement and reports pressure; it
does not evict the one current snapshot or reinterpret a query.

Eviction changes latency only. Reissuing the same snapshot/request/token
reproduces the same canonical page (`VIZ-R133`–`VIZ-R145`).

### 6.7 Projective refinement publication

The icosahedral hierarchy is treated as a semantic projective hierarchy, not a
screen-space mip chain. A child page is admitted only after:

- its State/time/capability key matches the parent;
- all declared parent/child restriction identities or quality enclosures pass;
- shared edge traces agree at the required fixed-point or certified bound;
- identities are either preserved or linked through the declared refinement
  resolution relation; and
- the full presentation patch and CPU collision data can swap atomically.

The valid parent stays visible while children compute. After admission, a
short deterministic presentation cross-fade may conceal tessellation change;
canonical geometry and inspection switch at the tick boundary, and collision
uses one grade for the entire resolved path segment. Cross-fade weights never
enter observation or grounding.

## 7. Projective icosahedral planet and local frames

### 7.1 Canonical addresses

The Option 4 planetary binding uses separate point and cell schemas.

A canonical surface point contains:

```text
base face 0..19
barycentric b0 and b1 in exact Q2.46
b2 = 1 - b0 - b1 exactly
signed normal altitude in centimetres
```

Validation enforces nonnegative barycentric components and exact sum bounds.
On a base edge or vertex, the lowest numbered incident face wins and the
manifest's oriented vertex map converts coordinates before hashing. The host
never selects a face from floating-point proximity.

A hierarchical cell contains a base face, level, and canonical child path in
the nested four-way triangular refinement. It denotes aggregate support and is
never substituted for a point address. Stable edge and vertex addresses name
shared boundaries for restrictions and crack checks.

The host retains opaque `WorldAddress`/`CellAddress` values in public state and
uses a negotiated fast-form decoder internally. An Impression stores canonical
address bytes, not an ECEF approximation or Map projection coordinate.

### 7.2 Planet geometry and metric

The domain descriptor supplies equatorial/polar radii, reference ellipsoid,
orientation, normal-altitude convention, gravity direction, and the product
metric combining ellipsoid arclength with altitude. All long-range distance,
interest-ring, cache-distance, and Traveler-path calculations call that metric
or a certified bound supplied by the Model. Chord distance and screen distance
are never treated as physical arclength.

The Visualization may convert an exact point to double-precision planet-fixed
Cartesian coordinates for presentation. That conversion is versioned by the
spatial adapter and carries a bounded error. It is not a new World address.

### 7.3 Canonical tangent frame query

For a point or small cell, the host requests/derives a right-handed local frame:

```rust
pub struct LocalSurfaceFrame {
    pub anchor: WorldAddress,
    pub origin_planet: [f64; 3],
    pub east: [f64; 3],
    pub north: [f64; 3],
    pub up: [f64; 3],
    pub frame_revision: u32,
    pub error_m: f64,
}
```

`up` is the declared ellipsoid normal or gravity-opposed direction. `east` is
the oriented tangent chosen by the domain manifest; at coordinate singularities
the containing face's oriented edge supplies a deterministic reference.
`north = up × east` under the declared handedness. A frame never relies on an
arbitrary cross product that can flip at a pole.

The Traveler controller integrates a short intended movement in the current
frame, resolves collision, then converts the path endpoints and required
boundary crossings back through the domain address operation. It subdivides at
canonical cell/frame boundaries, so the same normalized input path is not
frame-rate dependent.

### 7.4 Camera-relative precision and rebasing

The camera and CPU queries retain `f64` planet-fixed or local values. Each POV
patch has its own `LocalSurfaceFrame`; vertex positions are small `f32` offsets
from that patch anchor. Before upload/draw, `pov-host` computes the patch-anchor
offset and basis relative to the camera frame in `f64`, then narrows only the
small transform to `f32`.

Primitive positions use the same patch-local representation or the renderer's
existing high/low split. No shader receives a planet-radius absolute `f32`
position. Lighting directions are transformed into the camera frame once per
frame.

When the Traveler crosses a rebase threshold or canonical frame boundary, the
host selects a new frame anchor. Rebase changes presentation transforms and
renderer uploads only. Snapshot, World address, entity identity, simulation
identity, inspection support, and semantic cache keys remain unchanged
(`VIZ-R23`, `VIZ-R89`, `VIZ-R149`).

### 7.5 Interest cover on the triangular hierarchy

Map and POV request sets use deterministic hierarchical covers:

- begin from the 20 base faces;
- reject cells outside a geodesic/projection/frustum bound;
- refine a cell while its declared footprint exceeds the current physical
  pixel, collision, or geometry tolerance and the resource profile allows it;
- keep adjacent visible POV cells within one refinement level; and
- output cells in `(level, base_face, child_path)` order.

The cover is a request optimization, not world partition identity. A different
cover that samples the same support and grade must yield the same canonical
meaning within declared quality (`VIZ-R31`, `VIZ-R32`, `VIZ-R133`,
`VIZ-R142`).

## 8. Top-down Map design

### 8.1 Map state and projections

The Map owns a canonical World focus point, scale in physical meters per
destination pixel, orientation, projection id, thematic layer, and overlays.
Map panning changes the focus point through World Space; it does not move the
Traveler. Selecting “center on Traveler” copies the Traveler address into the
focus.

The basic profile implements two disclosed projections:

1. `LocalAzimuthalEquidistant` for local and regional scales. The focus is the
   projection center. Bearing is measured in its canonical tangent frame and
   radial distance uses the declared surface metric. Distance and bearing are
   faithful from the center; scale distortion away from it is available to the
   panel. The antipodal singularity is out of domain.
2. `IcosahedralNet` for whole-planet overview. All 20 faces are laid out as a
   versioned interrupted triangular net. Within a face, barycentric coordinates
   map affinely; duplicated net edges carry the same canonical edge id. Seams
   are visibly drawn and picking applies the lowest-face ownership rule.

The projection id/version, center, orientation, scale, destination rectangle,
and distortion descriptor are part of a `MapProjectionKey`. They are View
Space state, not canonical World identity (`VIZ-R75`, `VIZ-R82`).

### 8.2 Query-to-raster flow

For each frame revision, the Map presenter:

1. builds the projective cell cover for the viewport and a small pan margin;
2. requests the selected semantics and required dependencies at footprints
   matched to destination pixels;
3. requests entity/measure aggregates appropriate to scale rather than every
   individual;
4. converts valid typed values through a versioned `MapStyle` into encoded
   color, symbols, and status overlays;
5. rasterizes canonical cell/feature supports through the selected projection
   into a CPU RGBA8 image plus a CPU pick table; and
6. prepares either that image or generic atlas deltas for the renderer.

The pick table stores, per raster run or compact cell index, the canonical
World support, semantic-page key, achieved quality, candidate entity refs, and
status. It contains no screen-derived fact. Physical-to-map picking inverts the
projection, obtains an exact/normalized World point, and issues the common
observation request.

### 8.3 Thematic styles

A `MapStyleDescriptor` declares required semantic bindings, scale range,
aggregation expectations, palette/symbol version, uncertainty treatment, and
whether a layer is categorical, continuous, vector, entity, measure, relation,
or provenance. Basic styles include:

- composite surface/material/water/biome/vegetation;
- elevation and slope;
- substrate/material mixture;
- water depth and directed flow;
- temperature and moisture;
- soil depth/fertility;
- biome membership mixture;
- vegetation cover/structure/biomass;
- ecology diversity, abundance kind, habitat, and trophic balance;
- forcing phase/illumination;
- query quality/status/refinement;
- transition provenance, correspondence risk, and topology events; and
- Traveler, Impression/Attractor dual-space destinations, routes, preserves,
  and optional Builds as explicitly non-natural overlays.

The composite style is a Visualization definition. It may combine canonical
inputs for legibility but reports their ids and does not become a canonical
“composite biome” field. Thematic magnitude never encodes certainty; a separate
status/quality treatment carries certainty (`VIZ-R77`, `VIZ-R81`,
`VIZ-R123`).

### 8.4 Aggregation and scale

When several projective cells cover one pixel, the host uses descriptor-defined
restriction:

- totals and inventories sum exactly or within declared bounds;
- densities and other intensive values use the declared weighted mean;
- probabilities and category memberships remain normalized mixtures;
- identity sets cluster without minting a cluster entity;
- relation couplings aggregate their typed source/target marginals and slack;
- vector/flux fields transform into the projection frame before aggregation;
  and
- representative symbols are selected by a deterministic presentation hash
  over canonical candidates, never by completion order.

A feature retains its entity ref across scale where meaningful. When a coarse
aggregate refines into several subjects, the pick table retains the aggregate-
to-child resolution relation. Omission or symbol clustering cannot mean
canonical absence (`VIZ-R76`–`VIZ-R79`, `VIZ-R121`, `VIZ-R147`).

### 8.5 CPU-authoritative composition

The CPU composer generalizes
[`viewer-host/src/map.rs`](../crates/viewer-host/src/map.rs). It retains:

- one canonical layer order;
- exact physical layout/projection shared by drawing and picking;
- deterministic raster traversal;
- stable presentation-pixel hashes for upload suppression;
- a complete RGBA8 output for headless tests and screenshots; and
- status, transition provenance, routes, preserves, organisms, and Traveler
  overlays in declared order.

The CPU image is authoritative only for this Visualization's Map presentation,
not for Model truth. Canonical observation still uses `observe`. Nevertheless,
the CPU raster is the pixel reference for base-level GPU comparison and the
truthful fallback on unsupported/lost GPU hardware.

The reference projection and raster path fixes operation order, portable
transcendental approximations where the azimuthal projection needs them, edge
ownership, and subpixel quantization before coverage tests. That contract is
what makes the CPU image bit-stable across native and wasm; an alternative
accelerated projection may be used only when it produces the same quantized
coverage and bytes.

### 8.6 GPU-derived Map composition

The current atlas delta pattern in
[`viewer-host/src/atlas.rs`](../crates/viewer-host/src/atlas.rs) is retained but
made semantic-agnostic. `viewer-host` uploads presentation tiles, not Loom law
arrays:

- base encoded RGBA8 or linear RGBA16F color produced from valid semantic
  pages;
- a compact status/quality plane;
- feature/symbol and pre-grid/post-grid sparse overlay planes;
- a slot lookup for the visible projective cover; and
- projection-local triangle transforms or an index raster.

The presentation key contains consumed semantic-page dependency digests,
projection/style versions, coverage, and transition presentation revision.
Steady state produces no tile uploads. WGSL may apply destination-aware grid,
status patterns, projection masking, lighting-only relief, and zero-mean
style detail, but it does not decode Option 4 semantics, choose an aggregation
law, resolve an entity, or generate an observation.

Unsupported styles or GPU limits select the complete CPU image with an explicit
`MapBackendFallback`. No GPU Map output is read back or used for picking,
collision, cache keys, persistence, Egress, or Impression capture.

## 9. Embodied point-of-view design

POV is a second presentation of the same `FrameSemanticView`, not a second
world runtime. It consumes the same surface, water, environment, ecology,
observation, time, and transition pages as Map (`VIZ-R10`, `VIZ-R15`,
`VIZ-R83`–`VIZ-R91`).

### 9.1 Surface patches and materials

`pov-host` generalizes its pure mesh preparation from square `RegionMap`
chunks to projective triangular patches:

1. Select a nested triangular cover around the Traveler in metric distance.
2. Request surface samples, discontinuities, materials, and water at the
   patch's declared footprint.
3. Convert exact projective addresses through the current local frame.
4. Build indexed CPU geometry with normals derived from canonical samples.
5. Retain the exact CPU triangles and semantic pick table beside the upload.
6. Publish only if snapshot, time, transition, patch, and dependency tokens
   still match.

Adjacent patches share canonical edge addresses and sample order. Stitch
strips join different refinements; a short visual skirt may hide an unresolved
neighbor but is never collidable or observable. A valid parent remains visible
until all replacement children are publishable, preventing refinement holes.
Cliffs and other declared discontinuities use explicit faces instead of a
smoothed height-field bridge (`VIZ-R26`, `VIZ-R27`, `VIZ-R32`, `VIZ-R150`).

Materials are a deterministic basic palette plus scalar modulation driven by
canonical substrate, landform, soil, biome, and wetness. Mixtures stay mixtures;
no texture label becomes a canonical category. Optional seeded detail is
zero-mean, bounded by the declared footprint/error, keyed by canonical
provenance, and excluded from interaction (`VIZ-R35`–`VIZ-R48`, `VIZ-R86`).

### 9.2 Water

Sea, lake, wetland, and channel meshes come from canonical surface elevation,
water-surface elevation, depth, shoreline, and flow. Channel width and bed
placement are queried or bounded rather than guessed. Water animation may
perturb normals and color, but not the CPU fluid boundary. Grounding, entry,
and picking use the paired CPU water geometry and current canonical endpoint.
Unresolved water is patterned or omitted with its status; it is never replaced
by plausible blue geometry (`VIZ-R25`, `VIZ-R43`, `VIZ-R85`, `VIZ-R125`).

### 9.3 Organism manifestation

The basic profile manifests plants and animals as deterministic combinations
of boxes and fixed-topology icospheres. Size, color family,
orientation, mobility class, habitat, and placement bounds come from canonical
species/representative traits. A manifestation id is explicitly
Visualization-local and distinct from a canonical individual, species id, or
simulation handle (`VIZ-R50`–`VIZ-R60`).

Candidates are selected by a stable presentation hash from the aggregate's
declared representative distribution. Density is proportional to abundance
kind and support; tier limits thin representatives without asserting scarcity
or extinction. Deterministic simulation may add idle motion within habitat and
behavior constraints. It cannot change canonical biomass, relations, life
phase, or observation values. An inspection resolves through the candidate's
canonical representative reference, not its primitive (`VIZ-R58`–`VIZ-R61`,
`VIZ-R87`, `VIZ-R148`).

### 9.4 Movement, collision, and grounding

The camera retains the current fly and walk concepts. The Traveler's durable
address is projective; the camera is a local tangent-frame offset/orientation.
Frame rebasing changes only View Space. Walk mode resolves the current
canonical CPU surface grade, applies declared slope/affordance and water-entry
rules, and then commits the Traveler address. Fly mode still uses metric
planet distances and cannot mutate Model State.

Collision uses the best complete interaction-grade CPU page, never transition
blend geometry or GPU displacement. If required geometry is not interaction
grade, movement stops at a conservative resolved boundary and reports
`InteractionNeedsData`; the broker promotes that request. This is an honest
temporary barrier, not invented terrain (`VIZ-R17`, `VIZ-R30`, `VIZ-R85`).

### 9.5 Picking, lighting, sky, fog, and far field

CPU picking transforms the ray into the local frame and tests retained terrain
triangles, water triangles, organism primitives, and optional Build bounds.
The hit becomes an `ObservationTarget`; `observe` decides the subject and
values. GPU depth, colors, and object ids are never read back (`VIZ-R62`,
`VIZ-R67`, `VIZ-R90`).

Canonical forcing determines sun direction, intensity, sky palette, fog range,
and water highlight when available. A model without forcing uses a declared
timeless neutral rig. The far field uses coarser surface/water pages, the same
ellipsoid horizon, and status-aware atmospheric fading. Shadows, tone mapping,
fog noise, and sky gradients are presentation only (`VIZ-R46`, `VIZ-R88`).

## 10. Model time and deterministic simulation time

The host carries three separate clocks:

- `ModelTimeAddress`: canonical state time or declared timeless value;
- `SimulationTick(u64)`: fixed-step Visualization behavior time; and
- wall time: platform pacing only, never deterministic input.

The Visualization definition records the mapping from model-time movement to
simulation reset, pause, continuation, or declared resampling. Pause/rate/jump/
reverse controls appear only when the model capability supports them. Cycles
are evaluated from canonical temporal descriptors, not inferred from wall time
(`VIZ-R92`–`VIZ-R100`).

Each fixed simulation step consumes the immutable snapshot, model time,
simulation tick, input action, and versioned Visualization seed. Scheduling,
frame count, cache state, and GPU tier do not enter the result. Exact replay is
identified by Model/State/semantic-vocabulary ids, Visualization and simulation
versions, tier policy, initial Traveler/camera/simulation state, model time,
transition history, and normalized action log. Pixel comparison remains
tolerance-based where GPU rasterization is not portable (`VIZ-R127`–`VIZ-R140`).

## 11. Egress continuity and nearby state changes

Egress replaces the authoritative snapshot at one tick boundary. A bounded
`ContinuityState` retains only old presentation pages needed near the Traveler,
plus transition path, provenance, correspondence, risk, and event records. The
new snapshot is immediately authoritative for collision, observation,
Impressions, navigation, and new queries (`VIZ-R101`–`VIZ-R115`).

The domain-specific visual rules are:

| Change evidence | Basic treatment |
|---|---|
| Continuous bound/law coupling | Interpolate only within the supplied bound and validity interval; otherwise cut/fade. |
| Stable or explicit match | Keep one visual thread and morph bounded geometry/attributes. |
| Birth or death | Fade/appear independently at the supplied event support. |
| Split | One old presentation branches into explicitly matched new subjects; picks resolve only to current branches. |
| Merge | Explicit old branches converge to the current subject; old ids remain provenance only. |
| Topology event | Show bounded old/new local meshes together, mark the event, and cut collision to current topology. |
| Unresolved correspondence | Never guess identity; independently fade old and reveal new with uncertainty treatment. |

Fields use bounded interpolation; terrain, water, ecology, atmosphere, Builds,
and overlays each use their own reconciliation policy. One global alpha is not
sufficient. Transition work obeys continuation tokens and chunk injection just
like steady queries. A missing old continuation shortens presentation history;
it cannot delay authority of the new state. Unvisited areas show only current
state. Retained history has deterministic byte/time bounds and a stable
eviction order, so replay either reproduces it or records an explicit degraded
continuity outcome (`VIZ-R108`, `VIZ-R111`, `VIZ-R115`).

Any retained old geometry is visibly non-interactive: it is ghosted or paired
with a legible current interaction surface. Opaque old terrain or water may not
appear collidable after collision has cut over to the new endpoint. If current
interaction-grade geometry is unavailable, the conservative movement barrier
from Section 9.4 remains in force.

During a transition, Map selection and POV picking resolve an old visual thread
through an explicit current match. Otherwise it is labeled historical and is
not capturable. Reachability and Resonance are displayed from canonical
transition data only; presentation smoothness never authorizes Egress.

## 12. Canonical inspection and Impression capture

Map and POV share one path:

```text
CPU hit/address -> ObservationTarget -> observe(Interactive)
                -> ObservationEnvelope -> semantic panel document
```

The panel presents canonical value, unit/category vocabulary, subject/entity
scope, Model State and model time, spatial/scale support, provenance, quality,
status, relationships, and eligibility. Presentation-derived facts are labeled
separately. Repeating the same request against the same snapshot produces the
same document independent of view (`VIZ-R62`–`VIZ-R73`).

Impression capture reissues `observe` with `Canonical` purpose. Capture is
enabled only when the response is `Complete`, current, closure-sufficient, and
eligible, with required support/provenance/quality fields. `Partial` follows a
continuation, `NeedsInput` requests named chunks, and `Unresolved` explains why
capture is unavailable. No rendered color, manifested individual count, GPU
hit, stale transition value, or locally smoothed value enters an Impression.

## 13. Tiers, degradation, platforms, and device loss

Tiers alter presentation density and latency, never canonical meaning:

| Policy | Low | Mid | High |
|---|---:|---:|---:|
| Map presentation edge | 512 px | 768 px | 1024 px |
| POV refinement rings | 4 | 6 | 8 |
| Organism representatives/cell | 1 | 2 | 4 |
| Shadow map | 1024 | 2048 | 2048 |
| Presentation cache target | 128 MiB | 256 MiB | 512 MiB |

Platforms may clamp a policy and disclose the effective tier. Query quality
required for interaction/inspection does not fall with tier. Over budget, the
host first reduces shadows/detail/representative count, then refinement radius,
then POV availability; Map and canonical inspection remain (`VIZ-R141`–
`VIZ-R152`).

Missing optional capabilities remove their styles/passes with a disclosure.
Missing required capabilities make the profile incompatible before entry.
Localized query or mesh failure leaves the last valid parent, marks the support,
and preserves other regions (`VIZ-R116`–`VIZ-R126`, `VIZ-R153`–`VIZ-R160`).

Native and wasm run the same host, DTOs, simulation, Map composer, mesh
preparation, and shaders. Platform crates supply only normalized events,
executor/chunk transport, surfaces, lifecycle, and allowed persistence. Browser
worker or IndexedDB availability is not a semantic capability.

On device loss the shell declares GPU unavailable before the next tick,
discards renderer objects and upload-publication mirrors, and forces Map CPU
fallback. It retains snapshot, Traveler, model/simulation time, query/semantic
caches, transition state, and CPU pick geometry. Recovery recreates the renderer
and performs complete visible uploads; it never silently re-enters POV or
changes the world (`VIZ-R91`, `VIZ-R137`, `VIZ-R159`).

## 14. Host and renderer data contracts

Representative host DTOs are owned above `renderer`:

```rust
struct PresentationDatum<T> {
    value: Option<T>,
    status: DatumStatus,
    quality: QualityRecord,
    provenance: ProvenanceRecord,
}

struct SurfacePatchInput {
    key: SurfacePatchKey,
    frame: LocalSurfaceFrame,
    samples: Vec<SurfaceSample>,
    edges: [SharedEdge; 3],
    discontinuities: Vec<Discontinuity>,
    transition: Option<PatchContinuity>,
}

struct SurfaceSample {
    address: ProjectivePoint,
    elevation: PresentationDatum<Length>,
    material: PresentationDatum<MaterialMix>,
    water: PresentationDatum<WaterColumn>,
    pick_subjects: Vec<ObservationTarget>,
}

struct MultiViewPresentation {
    semantic: FrameSemanticView,
    traveler: TravelerState,
    map: MapPresentation,
    pov: PovPresentation,
    panel: PanelDocument,
    continuity: Option<ContinuityState>,
}
```

Uploads are presentation-only and contain no Option 4 query handles:

```rust
enum RendererDelta<T> { Keep, Replace(T), Remove }

struct PlanetMapTileUpload { slot: u32, rgba: ImagePlane, status: ImagePlane,
    transform: ProjectionTriangle, presentation_key: Hash128 }
struct SurfacePatchUpload { key: RenderPatchKey, vertices: Vec<PovVertex>,
    indices: Vec<u32>, material_slots: Vec<u16> }
struct WaterPatchUpload { key: RenderPatchKey, vertices: Vec<WaterVertex>,
    indices: Vec<u32>, style: WaterStyle }
struct PrimitiveBatchUpload { key: RenderBatchKey,
    instances: Vec<PrimitiveInstance> }
```

Every key includes snapshot/time support, projective coverage, refinement,
semantic dependency digest, transition presentation revision, and relevant
Visualization versions. `Keep/Replace/Remove` preserves the existing tri-state
publication rule and makes stale removal explicit.

WGSL remains model-agnostic. Map shaders sample prepared color/status/overlay
planes and projective transforms. POV shaders transform local-frame vertices,
shade palette material slots, water, primitives, sky, and fog, and composite
Map/POV/Split. They do not interpret semantic vocabulary ids, aggregate fields,
resolve correspondence, advance simulation, or produce canonical values.

One `MultiViewFrame` retains the existing pass discipline: acquire once, clear
once, record optional Map; record POV shadow, offscreen color/depth, and
composite; draw information/focus; submit once; present once. A resize or pane
change changes View Space only (`VIZ-R89`).

## 15. Observability and headless evidence

The shared characterization record includes snapshot/time/transition ids,
Traveler/local-frame address, layout/focus, effective tier, query queue by
priority/status, unresolved supports and requested chunks, continuation count,
cache bytes/evictions, publication tokens, visible patch/page keys, transition
match/risk/event counts, simulation tick, frame/pass counters, and device state.

F12/debug dumps pair this record with the CPU Map reference and panel document.
Headless tests inspect CPU map pixels, CPU POV geometry/pick tables, renderer
pass plans, and upload deltas. Native GPU capture remains a file-bound debug
facility; browser diagnostics expose state rather than relying on headless GPU
screenshots. No live renderer readback API is introduced.

## 16. Verification and acceptance evidence

The basic profile is acceptable when automated fixtures demonstrate:

- capability negotiation accepts Option 4 and refuses a required semantic
  mismatch before viewer construction;
- one normalized action advances one Traveler/snapshot/tick and Map, POV,
  Split, panel, collision, and picking cite that same frame identity;
- Map placement, local-frame POV geometry, geodesic distances, seam ownership,
  horizon, and cross-face travel agree for pole/seam/large-coordinate cases;
- restriction/refinement conserves declared quantities and preserves stable
  subjects; parent-child publication has no holes or completion-order drift;
- every `Complete`, `Partial`, `Unresolved`, and `NeedsInput` fixture has honest
  Map, POV, panel, continuation, and Impression behavior;
- CPU Map output is bit-stable; GPU base composition matches its defined
  tolerance; CPU picking resolves independently of GPU output;
- deterministic query reordering, cancellation, chunk arrival, cache eviction,
  worker count, and tier yield the same settled semantic/presentation hashes;
- organism thinning preserves declared aggregate ecology and does not change
  observations or simulation results;
- fixed-step simulation replay is frame-rate/schedule independent and model
  time never aliases simulation or wall time;
- continuous, match, birth, death, split, merge, topology, and unresolved Egress
  fixtures preserve current-state authority and reproduce continuity history;
- Map/POV inspection returns identical canonical envelopes, and only eligible
  complete envelopes produce bit-identical Impression payloads;
- GPU unavailability, device loss/recovery, local realization failure, and
  optional-capability absence degrade as declared without moving the Traveler;
- native and actual wasm parity fixtures agree on DTO codecs, projective math,
  CPU maps, mesh/pick hashes, simulation, and transition decisions; and
- renderer traces prove one acquire/submit/present for Map, POV, and Split.

These checks extend the existing deterministic, viewer, renderer, scale, and
web sign-off harnesses; they do not relax the repository's native/wasm, lint,
or warning gates (`VIZ-R96`, `VIZ-R135`, `VIZ-R137`, `VIZ-R144`, `VIZ-R160`).

## 17. Risks, open decisions, and ADR needs

The principal risks are semantic-page memory pressure, cracks or precision
loss at projective seams, collision lag behind presentation, misleading
interpolation across discontinuities, representative organisms being mistaken
for canonical individuals, and platform GPU differences. The design contains
them through byte ceilings, exact addresses/local frames, interaction-grade CPU
authority, event-specific transitions, explicit manifestation identity, and
CPU/status observability.

Before implementation, focused decisions remain for the exact global Map net,
interaction-grade quality threshold, default transition durations, cache
budgets, Build profile inclusion, and which Option 4 chunks may be requested by
Interactive observation. These are policy choices, not permission to change
canonical semantics.

ADR 0028 remains governing. A successor/new ADR is required for the generalized
`viewer-host` snapshot/query boundary and projective one-world frame; separate
ADRs should freeze the renderer upload/WGSL contract, deterministic
Visualization simulation/replay identity, and Egress presentation-history
policy. Accepted ADRs are superseded, never edited.

## 18. Requirements traceability

This table covers every normative requirement range; earlier section-level
citations identify finer-grained obligations.

| Requirement range | Design evidence |
|---|---|
| `VIZ-R01`–`VIZ-R10` | §§1–5 separate canonical, presentation, spaces, one state, and one shared experience. |
| `VIZ-R11`–`VIZ-R17` | §§2, 6, 13 negotiate capabilities/status and prohibit invented fallback. |
| `VIZ-R18`–`VIZ-R33` | §§7, 9 define projective addresses, ellipsoid metric, local frames, geometry, affordances, and scale. |
| `VIZ-R34`–`VIZ-R48` | §§4, 8–9 bind typed environmental semantics and honest mixtures/aggregation. |
| `VIZ-R49`–`VIZ-R61` | §§4, 8–9 define ecology relations, representatives, manifestation identity, and aggregate consistency. |
| `VIZ-R62`–`VIZ-R73` | §§4, 8–9, 12 provide one canonical observe/inspect/Impression path. |
| `VIZ-R74`–`VIZ-R82` | §8 provides faithful multi-scale Map, themes, status, history, and dual-space overlays. |
| `VIZ-R83`–`VIZ-R91` | §9 provides metric POV, interaction, organisms, far field, picking, and continuity. |
| `VIZ-R92`–`VIZ-R100` | §10 separates canonical, simulation, and wall time with deterministic mapping. |
| `VIZ-R101`–`VIZ-R115` | §11 handles nearby states, provenance, correspondence, risks, events, and authority. |
| `VIZ-R116`–`VIZ-R126` | §§6, 8–9, 13 preserve status/quality/refinement and contain failures. |
| `VIZ-R127`–`VIZ-R140` | §§5–6, 10–11, 14 define identity, provenance, versioned keys, and replay. |
| `VIZ-R141`–`VIZ-R152` | §§6–9, 13 define bounded streaming, conservation, tiers, and long-travel stability. |
| `VIZ-R153`–`VIZ-R160` | §§2, 13, 16 define compatibility, evolution boundaries, parity, and evidence. |
| `VIZ-R161`–`VIZ-R168` | §§8–9, 11, 13 keep Builds optional, authored, dual-anchored, distinct, and versioned. |
