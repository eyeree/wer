# New World Realization / Visualization Interface

## Status and purpose

This document is an architecture input for the boundary between a Model's
Realization and a Visualization. It compares the four candidate Models and
defines the largest interface they can honestly share. It is not an
implementation plan, an ABI specification, a rendering design, or a commitment
to one Model.

The normative source for this analysis is
[`new-world-model-visualization-requirements.md`](new-world-model-visualization-requirements.md).
The earlier shorthand `new-world-model-visualization.md` refers to that file.
Terminology and responsibility boundaries follow
[`conceptual-model.md`](conceptual-model.md). The compared designs are:

- [Option 1](new-world-model-option-1.md), a latent manifold of procedural
  planets;
- [Option 2](new-world-model-option-2.md), an attribute-first manifold with a
  per-tile effective-coordinate wake;
- [Option 3](new-world-model-option-3.md), a statistical manifold of
  world-laws with a deterministic spatial sample; and
- [Option 4](new-world-model-option-4.md), the World Loom's typed,
  chunk-backed causal constitutions.

The comparative baseline is the
[`New World Model Comparison`](new-world-model-comparison.md). This document
uses that comparison's reconciled reading of the proposals, then returns to
the detailed proposal documents wherever interface semantics require more
precision.

This document cites requirement ranges where a design element discharges a
group of related obligations and cites individual IDs where a distinction is
especially important. The range matrix near the end covers every requirement
from `VIZ-R01` through `VIZ-R168`.

## Decision summary

The largest honest common interface is an **immutable semantic snapshot plus
bounded, typed queries**. It includes:

- explicit Model, semantic-vocabulary, Model State, snapshot, spatial-domain,
  and model-time identities;
- capability and semantic descriptors rich enough to negotiate meaning rather
  than names;
- geometry, field, tile, entity, ecology, relationship, observation, forcing,
  and transition-summary queries;
- accuracy, error, uncertainty, provenance, per-datum availability, stable
  ordering, pagination, and deterministic continuation;
- caller-owned, capacity-bounded output and scratch storage; and
- execution policy, storage, networking, rendering, and simulation kept
  outside the semantic Model interface.

That contract is large enough for a map, embodied presentation, canonical
inspection, Impression capture, and continuity planning without assuming a
particular coordinate system, field representation, planet topology, or model
ontology. It does **not** claim that the four proposals share a state encoding,
Possibility geometry, generator DAG, ecology representation, or transition
mechanism.

Three proposal-specific profiles remain necessary:

1. **Option 2 wake-session profile.** The history-dependent `Xi` field cannot be
   represented as an ordinary immutable snapshot without making its history,
   integration, checkpoint, revisit, and eviction semantics explicit. As
   written, Option 2 cannot promise both wake-dependent presentation and
   retention-independent reproducibility.
2. **Option 3 law-and-transport profile.** A sample query alone loses the
   proposal's canonical distinction between a world-law and the deterministic
   ground sample. Its WFR/Bures transition data also needs a typed transport
   contract and an overlapping-transition policy.
3. **Option 4 Loom profile.** Typed measures, typed relation couplings,
   projective restriction laws, chunk closure, deterministic continuations,
   and rich split/merge/topology events cannot be flattened into generic scalar
   fields without erasing canonical meaning.

The preferred in-process boundary is ordinary Rust compiled into the **same
native executable or the same WebAssembly module** as the viewer host. Rust
trait objects and borrowed buffers are appropriate inside that compilation
unit; they are not a stable plugin ABI. A separately distributed Model should
instead use an optional versioned byte protocol or WIT-style component boundary
whose wire version is explicitly separate from semantic Model versions.

These decisions primarily address `VIZ-R01`–`VIZ-R17`, `VIZ-R116`–`VIZ-R160`,
and provide the information foundation for `VIZ-R18`–`VIZ-R115`. Builds remain
optional Visualization content under `VIZ-R161`–`VIZ-R168` and do not become a
Model query side effect.

## 1. Evaluation method

### 1.1 Fit vocabulary

The proposal tables use four fit labels:

| Fit | Meaning |
|---|---|
| **Direct** | The proposal already defines enough semantics to map into the common contract without changing its authority model. |
| **Adapter** | The proposal can map into the common contract through a mechanical descriptor or query adapter, but the proposal does not spell out the interface completely. |
| **Extension** | A proposal-specific profile is required to preserve meaning that the common contract cannot express faithfully. |
| **Gap** | The proposal is missing, contradictory, or too under-specified to claim the requirement range without a semantic design change. |

“Direct” is an assessment of the written design, not evidence that performance,
native/wasm parity, or correctness has been demonstrated. Presentation
requirements are marked according to whether the Model makes enough information
available; the Visualization still owns their satisfaction.

### 1.2 What counts as common

A feature belongs in the common contract only when all four proposals can give
it the same broad authority:

- the value is attributable to one canonical Model State or to an explicitly
  named transition between states;
- its semantic kind, support, time, quality, and provenance can be described;
- absence, approximation, incompleteness, unsupported meaning, and failure are
  distinguishable;
- cache residency and execution order are not part of its settled meaning; and
- a Visualization can consume it without knowing the proposal's internal
  coordinate mathematics.

Commonality does not mean identical fidelity. A model may negotiate a reduced
experience or omit an optional capability. It does mean that a capability it
claims must not silently acquire a different interpretation in a different
view (`VIZ-R11`–`VIZ-R17`).

### 1.3 Semantic contract versus transport

The **semantic contract** defines identities, meanings, queries, statuses,
quality, and invariants. It is independent of language and transport as required
by `VIZ-R03`.

The **Rust surface** later in this document is one representative binding of
that contract for the preferred same-module deployment. The **wire/component
surface** is a second binding for separately distributed plugins. Neither
binding is the semantic contract itself, and changing a codec without changing
meaning must not reidentify a world.

## 2. Proposal-by-proposal requirement fit

### 2.1 Option 1 — latent procedural planet

| VIZ range | Fit | Realization/Visualization interface assessment |
|---|---|---|
| `VIZ-R01`–`VIZ-R10` | **Direct** | One global `(M, q)` snapshot, World/View/Possibility separation, and Visualization-owned transition history map cleanly to an immutable snapshot. |
| `VIZ-R11`–`VIZ-R17` | **Direct** | Versioned capabilities, three accuracy grades, canonical versus presentation meaning, and explicit missing/error behavior are already central to the proposal. |
| `VIZ-R18`–`VIZ-R33` | **Direct** | The oblate planet, cube-map point and quadtree cell addresses, metric, altitude, seam rules, surface geometry, water, topology margins, and cross-state descriptors are concrete. Non-height-field geometry would simply be unsupported in V1. |
| `VIZ-R34`–`VIZ-R48` | **Direct** | Terrain, geology, drainage, climate, soil, biome, and ecological fields have defined dependencies and mostly clear physical meaning. Descriptors still need explicit unit, aggregation, and exceptional-value records. |
| `VIZ-R49`–`VIZ-R61` | **Direct** | Global lineage slots, exact state-local species ids, local population equilibrium, food webs, and representative organism samples map to ecology/entity queries. The adapter must preserve the lineage-slot versus exact-manifestation identity distinction. |
| `VIZ-R62`–`VIZ-R73` | **Direct** | Canonical attributes, margins, Canonical confirmation, and attribute-by-value fallback support presentation-independent observation and Impression capture. |
| `VIZ-R74`–`VIZ-R82` | **Adapter** | The Model supplies projection-independent planetary data and aggregates; map projection, symbology, history display, and dual-space UI remain Visualization work. |
| `VIZ-R83`–`VIZ-R91` | **Adapter** | Geometry, materials, affordances, organisms, and inspection are sufficient inputs, but collision-grade geometry and far/near presentation policies must be negotiated rather than inferred from tiles. |
| `VIZ-R92`–`VIZ-R100` | **Direct** | Integer canonical time, orbital forcing, intervals, and climate envelopes are defined. A transition query must carry the time mapping explicitly to satisfy `VIZ-R100`. |
| `VIZ-R101`–`VIZ-R115` | **Direct** | `TransitionDescriptor`, lineage/entity matching, channel bounds, topology margins, and risk locations fit the common transition-summary surface. Exact retained-presentation replay remains Visualization state. |
| `VIZ-R116`–`VIZ-R126` | **Direct** | Accuracy grades, componentwise bounds, unresolved identities, and dependency keys fit directly. The common status vocabulary makes implicit error cases explicit. |
| `VIZ-R127`–`VIZ-R140` | **Direct** | Model, state, entity, lineage, dependency, and determinism identities are strong. The stated `minor` behavior conflicts with hashes that include full `M`; the adapter must use separate semantic-generation and optional-package identities. |
| `VIZ-R141`–`VIZ-R152` | **Direct** | Lazy bounded queries, immutable caches, refinement error, and schedule independence fit. Cold global work is a performance risk, not an interface mismatch. |
| `VIZ-R153`–`VIZ-R160` | **Direct** | Capability/version negotiation and native/wasm canonical grades are explicit enough to populate a compatibility report. Verification evidence remains unimplemented. |
| `VIZ-R161`–`VIZ-R168` | **Adapter** | The Model validates the attachment state/address and supplies terrain frames; Build schema, authored semantics, loading, continuity, and rendering remain a separate Visualization profile. |

**Overall fit.** Option 1 is the cleanest direct implementation of the common
snapshot/query contract. It needs no Model-specific Visualization interface for
ordinary realization. Sensitivity and latent derivatives may be exposed as an
optional diagnostic profile, but a compatible Visualization must not depend on
latent coordinates to understand canonical fields.

### 2.2 Option 2 — attribute manifold with `Xi` wake

| VIZ range | Fit | Realization/Visualization interface assessment |
|---|---|---|
| `VIZ-R01`–`VIZ-R10` | **Gap** | The proposal names one canonical `theta_star`, but the experienced tile is `W(Xi(x,t), x)`. Unless `Xi` is explicitly classified as transition presentation with provenance, the same view contains different effective Model States and weakens the one-world interpretation. |
| `VIZ-R11`–`VIZ-R17` | **Gap** | Attribute groups are versioned, but fields, units, quality, canonical absence, unsupported meaning, and realization fidelity are mostly schematic. `Xi` also makes materialization/history affect the returned world. |
| `VIZ-R18`–`VIZ-R33` | **Gap** | The infinite plane is clear, but location precision, boundary behavior, geometry support, water geometry, physical affordances, resolution footprints, and cross-state correspondence are not specified to Visualization-contract depth. Adjacent tiles using different `Xi` buckets may create physical seams. |
| `VIZ-R34`–`VIZ-R48` | **Adapter/Gap** | A field stack is named and terrain has an equation; most other layers are “analogous.” The attribute chart is not a sufficient substitute for field semantics, dependencies, units, mixtures, and error bounds. |
| `VIZ-R49`–`VIZ-R61` | **Gap** | Ecology and organism density are asserted without a stable species/entity/observation contract, abundance support, or canonical individual boundary. |
| `VIZ-R62`–`VIZ-R73` | **Gap** | Impressions capture quantized attributes, but canonical subject resolution and presentation-independent observation are under-specified. A mid-wake observation has no defined rule choosing canonical `theta_star` versus local `Xi`. |
| `VIZ-R74`–`VIZ-R82` | **Adapter/Gap** | Planar fields can drive a map, but coverage, aggregation, uncertainty, stable subjects, and current-versus-wake history would need new descriptors. |
| `VIZ-R83`–`VIZ-R91` | **Adapter/Gap** | A streaming field stack could drive an embodied view, but seam-free collision, near/far identity, and wake-transition inspection are not established. |
| `VIZ-R92`–`VIZ-R100` | **Gap** | Canonical time is not integrated. `Xi` uses travel arclength but its integration cadence and relationship to simulation time are unspecified. |
| `VIZ-R101`–`VIZ-R115` | **Extension/Gap** | The wake is the proposal's continuity mechanism, but it lacks a reproducible history payload, fixed integrator, frame-subdivision rule, revisit policy, cross-tile reconciliation, and composition semantics. A dedicated wake-session interface is mandatory. |
| `VIZ-R116`–`VIZ-R126` | **Gap** | Accuracy grades, uncertainty sources, refinement meaning, unresolved classifiers, and failure policy are not fully defined. Wake staleness is not distinguishable from canonical state. |
| `VIZ-R127`–`VIZ-R140` | **Gap** | Integer ids are portable, but fields, navigation, and `Xi` are same-platform floats. The Q32 representation is internally inconsistent (`i32` cannot hold the stated unsigned range), and sub-bucket canonical-state versus bucketed-realization meaning is contradictory. |
| `VIZ-R141`–`VIZ-R152` | **Gap** | The claimed bucket bound does not hold for a multidimensional, path-dependent wake. Eviction or looped travel can change revisit results or require unbounded history, conflicting with retention and long-travel invariants. |
| `VIZ-R153`–`VIZ-R160` | **Gap** | Attribute-group matching is too weak for semantic compatibility; cross-platform canonical realization and navigation are not promised. A reduced experimental profile can still be declared honestly. |
| `VIZ-R161`–`VIZ-R168` | **Adapter/Gap** | Builds are declared external but have no attachment, compatibility, or continuity contract. |

**Overall fit.** Option 2 can expose `W(hat(theta_star), x)` through the common
immutable snapshot contract after its numeric address and field semantics are
repaired. That is not, however, the experienced realization described by the
proposal. Exposing `W(Xi, x)` requires the unique wake-session profile in
Section 10.2, and the proposal cannot claim reproducible eviction/revisit
semantics until it chooses one of that profile's explicit history policies.

### 2.3 Option 3 — world-law plus deterministic sample

| VIZ range | Fit | Realization/Visualization interface assessment |
|---|---|---|
| `VIZ-R01`–`VIZ-R10` | **Direct** | One global law coordinate is authoritative; transition morph state stays in the Visualization. The adapter must keep law, deterministic sample, and transient transport distinct. |
| `VIZ-R11`–`VIZ-R17` | **Extension** | Snapshot capabilities fit, but semantic completeness requires a law/sample authority descriptor and calibrated bridge rather than treating a ground sample as the law itself. |
| `VIZ-R18`–`VIZ-R33` | **Adapter** | The generic plane is addressable and local fields are lazy, but planetary topology, water geometry, physical affordances, and feature correspondence are deferred or schematic. |
| `VIZ-R34`–`VIZ-R48` | **Adapter/Extension** | Matérn-like law parameters and deterministic fBm samples need separate descriptors. Stationarity, asymptotic spectral matching, calibration error, and sample coupling are material quality semantics. |
| `VIZ-R49`–`VIZ-R61` | **Adapter/Extension** | LGCP intensity, marked samples, and trait laws support ecology, but finite samples only approximate the canonical prevalence law. `lambda_max`, stable species/entity identity, and abundance support require repair or explicit unresolved status. |
| `VIZ-R62`–`VIZ-R73` | **Adapter** | Canonical mean values and sample subjects can support observation if every value says whether it is a law statistic, deterministic sample fact, or calibrated population estimate. |
| `VIZ-R74`–`VIZ-R82` | **Adapter** | Fields, population intensities, and statistics can drive thematic maps; the Visualization must not display expected law mass as exact local presence. |
| `VIZ-R83`–`VIZ-R91` | **Adapter** | Sample fields and representative organisms can drive an embodied view, provided manifestation density and law/sample error remain inspectable. |
| `VIZ-R92`–`VIZ-R100` | **Gap** | Canonical time is explicitly left open. Simulation time and transport progress can be represented, but no canonical temporal forcing contract exists. |
| `VIZ-R101`–`VIZ-R115` | **Extension/Gap** | WFR living-mass and Bures abiotic transition data require a transport profile. The proposal does not define how a second commit rebases or composes with an unfinished morph, which blocks full transition replay. |
| `VIZ-R116`–`VIZ-R126` | **Direct/Extension** | Three grades and bounds fit; law/sample calibration, finite-population error, Sinkhorn error, and transport uncertainty need profile-specific quality records. |
| `VIZ-R127`–`VIZ-R140` | **Direct/Gap** | Fixed addresses and dependency keys are portable; live Egress is portable only in Canonical mode. Shared innovation coupling is ambiguous if `theta` changes field seeds. Law identity and sample identity must be separate. |
| `VIZ-R141`–`VIZ-R152` | **Direct/Extension** | Lazy tiles and bounded annulus transport fit scale requirements, but aggregation must distinguish probability-law means, mass intensities, and realized counts. |
| `VIZ-R153`–`VIZ-R160` | **Adapter** | Observable and accuracy descriptors can negotiate a reduced plane/time-limited experience; exact law/sample and transport profile versions must participate in compatibility. |
| `VIZ-R161`–`VIZ-R168` | **Gap** | Build reproduction is explicitly left open; only the common external Build boundary can be claimed. |

**Overall fit.** Option 3 maps cleanly to the common snapshot for ordinary
sample fields and observations, but doing only that would erase its defining
canonical object. A conforming Option 3 adapter therefore exposes both the
common snapshot and the statistical-law profile. A continuity-capable
Visualization also needs the transport profile and a resolved plan-composition
policy.

### 2.4 Option 4 — World Loom

| VIZ range | Fit | Realization/Visualization interface assessment |
|---|---|---|
| `VIZ-R01`–`VIZ-R10` | **Direct** | One normalized State Packet is authoritative; old presentation and path sidecars remain noncanonical. Model, Traveler, and Visualization responsibilities are unusually explicit. |
| `VIZ-R11`–`VIZ-R17` | **Direct/Extension** | Capability descriptors are strong. Typed measures, relation couplings, and chunk availability require the Loom profile rather than generic field names. |
| `VIZ-R18`–`VIZ-R33` | **Direct** | Oblate icosahedral planet, exact point/cell distinction, 3-D metric, topology, projective refinement, water, spatial relations, and discontinuity events are concrete. |
| `VIZ-R34`–`VIZ-R48` | **Direct/Extension** | Typed channels, units, inventories, discrete forms, restriction laws, mixtures, and residuals are strong. The general field API conveys values; the Loom profile preserves conservation and law structure. |
| `VIZ-R49`–`VIZ-R61` | **Direct/Extension** | Trait measures, trophic couplings, species modes, manifestation ids, matching keys, representative samples, and transition-local ancestry are detailed. Typed ecology queries are required to avoid flattening measures and flows. |
| `VIZ-R62`–`VIZ-R73` | **Direct** | Canonical observables, applicability, intervals, subject margins, transition-aware capture, and attribute-by-value fallback fit directly. |
| `VIZ-R74`–`VIZ-R82` | **Adapter** | Projective fields, measures, coverage, history provenance, and uncertainty can drive a faithful map; projection and symbology remain Visualization concerns. |
| `VIZ-R83`–`VIZ-R91` | **Adapter** | Geometry, attachment frames, physical channels, entities, ecology, and event data support an embodied view; current host DTOs would need planetary/model-neutral adaptation. |
| `VIZ-R92`–`VIZ-R100` | **Direct** | Integer canonical time, orbital/tidal/seasonal forcing, and an explicit complete Visualization replay tuple provide the strongest time separation. |
| `VIZ-R101`–`VIZ-R115` | **Direct/Extension** | Law couplings, bounded feature correspondence, births/deaths/splits/merges, topology events, margins, and path provenance are rich enough, but require the Loom transition-event profile and pagination. |
| `VIZ-R116`–`VIZ-R126` | **Direct** | `Complete`, `Partial`, `Unresolved`, continuations, intervals, residuals, ambiguity, and fail-closed behavior are first-class. |
| `VIZ-R127`–`VIZ-R140` | **Direct** | Generation, package, artifact, packet, capability, entity, matching, and transition identities are carefully separated. Canonical numeric operation paths are explicit. |
| `VIZ-R141`–`VIZ-R152` | **Direct** | Projective summaries, hard caps, pagination, structural sharing, cache independence, and exact aggregation laws fit. Feasibility and resolved rate remain research risks. |
| `VIZ-R153`–`VIZ-R160` | **Direct** | Semantic capability negotiation, profile-scoped conformance, native/wasm rules, and explicit reduced/unsupported outcomes are comprehensive. |
| `VIZ-R161`–`VIZ-R168` | **Direct/Adapter** | The optional Build profile defines semantic structure, attachment, collision authority, loading, and stylistic variability while keeping Builds out of Model State. |

**Overall fit.** Option 4 is a superset of most common query mechanics, and its
status discipline strongly influences the shared design. Its state closure and
typed ontology are not merely larger encodings of common scalar fields; the
Loom profile is required for a Visualization that claims to expose
constitution, conservation, relational ecology, projective refinement, or
transition events faithfully.

## 3. Shared semantic contract

### 3.1 Contract boundary

The common interface answers this question:

> For one immutable canonical Model State, or for one explicitly named
> transition between canonical states, what canonical world meaning is
> available at a requested World Space support and model time, with what
> quality, identity, and provenance?

It does not answer how the Model stores its coordinate, schedules its solvers,
chooses an Egress path, simulates weather, constructs meshes, or renders a
frame. This preserves the responsibility split in `VIZ-R04`, `VIZ-R09`, and
`VIZ-R38`.

The shared contract has five conceptual layers:

1. **Identity and vocabulary** identify the Model generation, canonical State,
   semantic definitions, capability revisions, and optional implementation
   artifact.
2. **Domain descriptors** define World Space topology, addresses, metrics,
   model time, and refinement semantics.
3. **Immutable snapshots** bind all canonical queries to one complete Model
   State while remaining independent of cache contents and presentation
   history.
4. **Typed queries** return canonical geometry, fields, entities,
   relationships, ecology, observations, forcing, and transition information.
5. **Quality and control envelopes** report availability, approximation,
   uncertainty, provenance, pagination, bounded work, and failure without
   changing semantic meaning.

This division supports `VIZ-R05`–`VIZ-R08`, `VIZ-R13`–`VIZ-R16`,
`VIZ-R116`–`VIZ-R140`, and `VIZ-R141`–`VIZ-R145`.

### 3.2 Canonical and presentation authority

Every returned datum declares one authority class:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthorityClass {
    /// Meaning is part of the canonical Realization of one Model State.
    CanonicalState,
    /// Meaning is canonical only for a named transition/path.
    CanonicalTransition,
    /// Deterministic Model-derived estimate with declared approximation.
    ModelDerived,
    /// Deterministic representative sampled from canonical aggregate meaning.
    RepresentativeSample,
    /// Visualization-owned history, simulation, or stylistic derivation.
    PresentationOnly,
}
```

The common Model query surface normally returns the first four. It may describe
the inputs or constraints for `PresentationOnly` work, but it must not return a
presentation fallback labeled canonical. A Visualization attaches its own
provenance to presentation-only values. This is the enforceable form of
`VIZ-R04`, `VIZ-R05`, `VIZ-R17`, `VIZ-R38`, `VIZ-R62`, and `VIZ-R67`.

Option 3 uses `CanonicalState` for its law and either `ModelDerived` or
`RepresentativeSample` for ground realizations according to the channel's
declared contract. Option 2's `Xi` result cannot be `CanonicalState` unless the
Model State identity includes the complete history that determines it; the
preferred classification is `PresentationOnly` or `CanonicalTransition` in a
named wake session. Option 4's transition matches are
`CanonicalTransition`, never endpoint entity identity.

### 3.3 Snapshot invariants

An immutable snapshot obeys all of the following:

- it denotes one complete Model State, not one tile or resident window;
- its identity does not include cache residency, worker count, resource tier,
  current camera, or Visualization simulation state;
- a query result is a pure function of the snapshot identity, normalized
  request, explicit continuation, and semantic package closure;
- model time is an explicit query coordinate unless the snapshot descriptor
  declares a fixed time view;
- refinements narrow quality or reveal previously unresolved subjects without
  silently changing already confirmed canonical identity;
- a snapshot may be unavailable because required immutable closure is missing,
  but missing bytes do not change the Model State it denotes; and
- transition presentation never mutates either endpoint snapshot.

These rules discharge `VIZ-R06`, `VIZ-R07`, `VIZ-R10`, `VIZ-R16`,
`VIZ-R32`, `VIZ-R121`, `VIZ-R129`, `VIZ-R133`, `VIZ-R134`, and
`VIZ-R145`.

### 3.4 Minimum common capability set

No universal planet or ecology is mandatory. The minimum interface vocabulary
is mandatory; individual capabilities are negotiated. A usable experience
profile declares which of these are required:

| Common capability family | Minimum semantic promise when claimed |
|---|---|
| World domain | Stable point/cell addresses, topology, metric or neighborhood meaning, extent, precision, and boundary behavior. |
| Geometry | Canonical physical support, representation kind, material/affordance meaning, resolution footprint, and error. |
| Fields | Typed continuous, categorical, probability, count, identity, mixture, or structured values with spatial/time support. |
| Entities | Canonical subjects with identity scope, defining support, attributes, margins, and optional correspondence hints. |
| Ecology | Species/guild/trait actors, distribution or abundance meaning, habitat constraints, representative-sample rules, and relationships. |
| Relationships | Directed or undirected typed edges/couplings that cannot be inferred safely from shape or co-location. |
| Observations | Presentation-independent canonical facts suitable for inspection and, at Canonical grade, Impression capture. |
| Forcing | Canonical model-time conditions, intervals, cycles, and response envelopes; never live Visualization weather. |
| Transition summary | Endpoint/path provenance, channel change bounds, correspondence status, risk, and change classification. |
| Attachment | Canonical frame and feature information used by optional Builds without mutating the Realization. |

The first seven support both map and embodied experiences. Forcing and
transition may be absent in a declared static or non-Egress reduced experience.
Attachment may be absent when Builds are unsupported. The compatibility report
must say so before normal use (`VIZ-R12`, `VIZ-R153`–`VIZ-R157`).

## 4. Proposed neutral crate boundaries

### 4.1 Dependency shape

The interface should be introduced as a small platform-neutral semantic crate,
shown here as `realization-api`. Proposal implementations and adapters depend
on it; it does not depend on them.

```text
                         platform-native / platform-web
                         (I/O, async loading, executors,
                          windows/DOM, surfaces, lifecycle)
                                  |
             +--------------------+--------------------+
             |                                         |
             v                                         v
        viewer-host                              model adapter
   (queries + presentation DTOs)       (Option 1/2/3/4 implementation)
             |                                         |
             +--------------------+--------------------+
                                  v
                          realization-api
                   (neutral semantic types/traits)

        viewer-host / pov-host  ---> presentation DTOs ---> renderer
                                                        (GPU only)

        optional realization-wire / WIT component binding
                    depends on realization-api semantics
```

The arrows show dependency or data flow, not permission to reverse-call a
host. In particular:

- `realization-api` performs no filesystem access, networking, thread creation,
  clock reads, DOM calls, or GPU work;
- Option adapters and their canonical runtime crates remain neutral under the
  same rule as `world-core` and `world-runtime`;
- `viewer-host` remains environment-neutral and owns the cross-view semantic
  interpretation;
- `renderer` does not depend on `viewer-host` or on a Model implementation and
  does not perform canonical inspection;
- `pov-host` or `viewer-host` converts canonical query results into bounded
  presentation DTOs before rendering;
- native and web platform crates own asynchronous chunk/package acquisition,
  durable storage, execution, and lifecycle; and
- an offline package compiler such as Option 4's `loom-compiler` remains a tool
  crate outside the neutral wasm runtime dependency graph.

This preserves the existing boundary in
[`AGENTS.md`](../AGENTS.md), [ADR 0002](adr/0002-workspace-crate-boundaries.md),
and [ADR 0028](adr/0028-shared-viewer-host-and-one-world-multi-view.md).

### 4.2 Suggested crate responsibilities

| Crate or layer | Responsibility | Explicit exclusions |
|---|---|---|
| `realization-api` | IDs, descriptors, addresses, snapshot/query traits, statuses, quality, provenance, compatibility records, profile IDs | Model math, caches, files, threads, sockets, DOM, GPU, UI |
| Option adapter crate | Convert one proposal's state and results into common semantics; expose proposal profiles | Rendering, platform I/O, hidden semantic fallback |
| `realization-wire` | Optional canonical encoding, WIT operation/schema ids, bounds validation, codec fixtures | Changing Model semantics, dynamic Rust ABI |
| `viewer-host` | Negotiate profiles, issue canonical queries, retain one Traveler/state/time interpretation, produce semantic panel and presentation DTOs | Files, sockets, thread creation, DOM/winit, Model mutation |
| `pov-host` | Camera-independent physical presentation preparation, collision/picking inputs derived from canonical queries | Canonical identity generation, storage, environment APIs |
| `renderer` | Consume presentation DTOs and record Map/POV/Split GPU passes | Model queries, live readback authority, viewer-host dependency |
| Platform adapters | Load package/chunks asynchronously, schedule bounded calls, store records, expose windows/DOM/surfaces | Reinterpreting canonical meaning |

The exact crate names are not a decision. The dependency direction and
responsibility split are.

### 4.3 Execution-policy separation

The semantic API is synchronous and bounded per call. “Synchronous” means a
call performs one explicitly capped unit of pure work; it does not mean the UI
thread must invoke it. The host may invoke the same call:

- inline in a browser without worker support;
- on a browser Worker using copied or transferred request/output storage;
- in one of the native lane executor's priority lanes; or
- in a deterministic headless harness.

The request carries semantic work and item caps. If the model cannot finish
within them, it returns `Partial` or `Unresolved` with a deterministic
continuation. It never spawns a thread or waits for I/O. Cancellation is a host
decision: a canceled invocation publishes no output, and retrying the same
request/continuation has the same meaning (`VIZ-R137`, `VIZ-R144`).

Model implementations may internally expose declarative job graphs to a host
adapter, following the existing `TaskExecutor` pattern. That graph is an
optimization interface below the Realization semantics. Integration still
checks immutable dependency keys, and settled output cannot depend on job
order.

### 4.4 Asynchronous immutable input

Option 4 demonstrates why the Model must not perform an implicit network fetch.
The preferred control flow is:

1. the host supplies a bounded in-memory State/package closure;
2. validation or a query returns `NeedsInput` with content ids for missing
   immutable chunks;
3. the platform adapter obtains them asynchronously under its own policy;
4. the host validates their digests and retries with the same semantic request;
5. the canonical result depends on chunk content, not fetch timing or source.

An in-memory `ChunkView` trait may be passed to neutral code, but its methods
must only read bytes already made available for this call. A trait method that
silently blocks on disk or network would violate both the boundary and wasm
portability.

## 5. Identity and semantic descriptors

### 5.1 Identity axes

The common identity model adopts Option 4's useful separation without forcing
its State Packet format on other proposals:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GenerationId(pub [u8; 32]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackageId(pub [u8; 32]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtifactId(pub [u8; 32]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StateId(pub [u8; 32]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VocabularyId(pub [u8; 32]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CapabilityId(pub [u8; 16]);
```

- `GenerationId` changes when an existing canonical address or output changes
  meaning or deterministic value incompatibly.
- `PackageId` identifies the semantic capability closure actually needed to
  decode a state; an unused optional extension must not reidentify old results.
- `ArtifactId` identifies concrete package/code bytes and evidence. A rebuild
  with identical semantics may change it without changing `GenerationId`.
- `StateId` identifies the complete canonical Model State, independent of
  regional materialization and transition history.
- `VocabularyId` versions units, categories, relation kinds, value schemas, and
  observation meanings.
- `CapabilityId` identifies one negotiated semantic capability and is paired
  with a schema/revision descriptor.

Options 1–3 may initially set `PackageId` to a digest of their frozen manifest
and `ArtifactId` to the build artifact. They must not repeat Option 1's ambiguity
where an allegedly optional `minor` participates in every canonical hash.

These axes implement `VIZ-R127`–`VIZ-R130`, `VIZ-R138`, and `VIZ-R140`.

### 5.2 Snapshot identity

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotIdentity {
    pub generation: GenerationId,
    pub package: PackageId,
    pub state: StateId,
    pub vocabulary: VocabularyId,
    pub used_capability_revisions: BoundedCapabilitySet,
    pub state_encoding: StateEncodingId,
}
```

`SnapshotIdentity` excludes model time when time is a query coordinate. A
Model family that defines a snapshot as a fixed temporal slice may include the
canonical time in `StateId`, but must declare that choice. It always excludes:

- `ArtifactId` unless the artifact changes semantics;
- cache keys that are not semantic dependencies;
- current refinement residency;
- current transition-blend state;
- Traveler location;
- View Space and camera state; and
- Visualization resource tier or simulation time.

An Option 4 `StateAddress` maps to `(GenerationId, StateId)` and its envelope
maps to an explicit state closure. Option 1's `(M, q)`, Option 2's repaired
fixed-point global coordinate, and Option 3's `(M, hat(theta))` each receive a
canonical encoding whose digest is `StateId`.

### 5.3 Capability descriptor

```rust
#[derive(Clone, Debug)]
pub struct CapabilityDescriptor {
    pub id: CapabilityId,
    pub schema_version: u32,
    pub algorithm_revision: u32,
    pub vocabulary: VocabularyId,
    pub family: CapabilityFamily,
    pub requirement: RequirementLevel,
    pub determinism: DeterminismGradeSet,
    pub spatial_support: SpatialSupportDescriptor,
    pub temporal_support: TemporalSupportDescriptor,
    pub refinement: RefinementDescriptor,
    pub quality: QualityPolicyDescriptor,
    pub dependencies: BoundedCapabilitySet,
    pub value_schema: ValueSchemaId,
    pub semantics_digest: [u8; 32],
}
```

The `semantics_digest` covers the material interpretation, not only a display
name. Its referenced schema defines:

- units and dimensional kind;
- valid range and exceptional values;
- continuous, categorical, ordered-band, probability, count, identity,
  mixture, measure, or relation semantics;
- intensive/extensive and aggregation behavior;
- applicability and denominator meaning;
- canonical absence and missing-value meaning;
- spatial and temporal support;
- identity scope and lifetime where applicable;
- refinement invariants and possible newly resolved detail;
- accuracy/error forms; and
- whether the value may guide a Yearning or be captured in an Impression.

This is the shared answer to `VIZ-R11`, `VIZ-R13`–`VIZ-R15`,
`VIZ-R34`–`VIZ-R48`, `VIZ-R64`, `VIZ-R72`, `VIZ-R130`, and
`VIZ-R147`.

### 5.4 Semantic vocabulary records

Names are human-facing metadata, not compatibility keys. A semantic vocabulary
contains stable records such as:

```rust
#[derive(Clone, Debug)]
pub struct FieldDescriptor {
    pub capability: CapabilityId,
    pub semantic_id: SemanticId,
    pub value_kind: ValueKind,
    pub unit: UnitDescriptor,
    pub range: RangeDescriptor,
    pub aggregation: AggregationRule,
    pub boundary: BoundarySemantics,
    pub steerability: SteerabilityDescriptor,
}

#[derive(Clone, Debug)]
pub struct RelationDescriptor {
    pub semantic_id: SemanticId,
    pub direction: RelationDirection,
    pub endpoint_roles: BoundedRoleSet,
    pub magnitude_schema: Option<ValueSchemaId>,
    pub identity_scope: IdentityScope,
    pub spatial_support: SpatialSupportDescriptor,
    pub temporal_support: TemporalSupportDescriptor,
}
```

Examples of incompatible meanings that must receive different descriptors are:

- biomass density versus organism count;
- habitat suitability versus likely presence versus canonical occupancy;
- mean climate temperature versus an instantaneous simulated temperature;
- species identity versus lineage matching key versus rendered asset id;
- probability of a category versus mixture mass of categories;
- per-area flow accumulation versus river entity membership; and
- Option 3 law expectation versus one deterministic sample value.

### 5.5 Capability and semantic adapters

A semantic adapter is an explicit, versioned mapping:

```rust
#[derive(Clone, Debug)]
pub struct SemanticAdapterDescriptor {
    pub id: AdapterId,
    pub from: SemanticBinding,
    pub to: SemanticBinding,
    pub guarantee: AdapterGuarantee,
    pub error_policy: QualityPolicyDescriptor,
    pub identity_policy: AdapterIdentityPolicy,
}
```

An adapter may rename units exactly, project a richer value into a declared
coarser meaning, or map a Model-specific observable into a shared semantic id
with bounded loss. It may not infer a canonical category from pixels or map two
unrelated concepts merely because their labels match. Lossy adapters force a
reduced compatibility result and preserve their error in observations and
Impressions (`VIZ-R124`, `VIZ-R154`, `VIZ-R158`).

## 6. World Space, addresses, and time

### 6.1 Spatial-domain descriptor

No common `x, y` or planet face enum can represent all four proposals honestly.
World Space is therefore defined by a descriptor and canonical, schema-tagged
address bytes.

```rust
#[derive(Clone, Debug)]
pub struct WorldDomainDescriptor {
    pub id: SpatialDomainId,
    pub address_schema: AddressSchemaId,
    pub cell_schema: Option<AddressSchemaId>,
    pub dimension: u8,
    pub topology: TopologyDescriptor,
    pub extent: ExtentDescriptor,
    pub metric: MetricDescriptor,
    pub orientation: OrientationDescriptor,
    pub precision: PrecisionDescriptor,
    pub boundary: DomainBoundaryDescriptor,
    pub refinement: Option<SpatialHierarchyDescriptor>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldAddress {
    pub schema: AddressSchemaId,
    pub len: u8,
    pub bytes: [u8; 63],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellAddress {
    pub schema: AddressSchemaId,
    pub level: u8,
    pub len: u8,
    pub bytes: [u8; 62],
}
```

The fixed capacities are illustrative profile limits, not semantic limits.
Wire and in-process profiles publish their actual maximum before use.

The descriptor states whether the domain is planar, planetary, bounded,
periodic, layered, disconnected, volumetric, or model-specific; how distance
and neighborhoods work; which axes and units addresses represent; where seams,
poles, or invalid addresses occur; and whether a cell is an aggregate support
rather than another point encoding. This supports `VIZ-R18`–`VIZ-R23` and
prevents Option 4's hierarchical cell from being confused with its barycentric
surface point.

Adapters map addresses as follows:

| Proposal | Point address | Aggregate/refinement address |
|---|---|---|
| Option 1 | Cube face + two Q2.46 coordinates + centimetre altitude | Cube-map quadtree cell, level, halo |
| Option 2 | Signed fixed-point planar coordinate; exact format must be added | Planar tile/region index and resolution |
| Option 3 | Fixed-point coordinate in the declared plane/domain | Field tile/region and sample footprint |
| Option 4 | Icosahedron face + exact barycentric Q2.46 coordinates + centimetre altitude | Separate nested-icosahedron cell address and level |

### 6.2 Spatial support

Every value refers to an explicit support:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpatialSupport {
    Point(WorldAddress),
    Cell(CellAddress),
    Footprint(FootprintRef),
    Volume(VolumeRef),
    Curve(CurveRef),
    Feature(EntityRef),
    DomainAggregate(SpatialDomainId),
}
```

A point sample may still carry a nonzero filter footprint. A cell value declares
whether it is an average, total, bound, category summary, mixture, sample, or
representative. Geometry outputs declare whether they describe a height
surface, implicit solid, multiple surfaces, a volume, or a topology graph.
This is required by `VIZ-R24`–`VIZ-R32`, `VIZ-R36`, `VIZ-R47`,
`VIZ-R55`, `VIZ-R69`, `VIZ-R119`, and `VIZ-R143`.

### 6.3 Model time

```rust
#[derive(Clone, Debug)]
pub struct ModelTimeDescriptor {
    pub schema: TimeSchemaId,
    pub support: ModelTimeSupport,
    pub epoch: Option<EpochDescriptor>,
    pub unit: UnitDescriptor,
    pub cycles: BoundedCycleSet,
    pub reversible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeSelector {
    Timeless,
    Instant { schema: TimeSchemaId, tick: i64 },
    Interval { schema: TimeSchemaId, start: i64, end: i64 },
    Phase { schema: TimeSchemaId, phase_q32: u32 },
}
```

Visualization simulation time and wall-clock time never appear in a canonical
Model query. A Visualization records their mapping separately. Option 1 and
Option 4 can advertise canonical forcing time. Options 2 and 3 must advertise
`Timeless` or a temporally limited reduced profile until their designs add
canonical time. This makes partial compatibility visible under `VIZ-R92`–
`VIZ-R100` and `VIZ-R157`.

## 7. Representative object-safe Rust façade

### 7.1 Model and snapshot traits

The following sketches the preferred same-compilation-unit binding. Names and
capacities are illustrative; the object-safety and ownership properties are
architectural.

```rust
pub trait RealizationModel: core::fmt::Debug {
    fn descriptor(&self) -> &ModelDescriptor;

    fn negotiate(
        &self,
        request: &CompatibilityRequest,
        out: &mut CompatibilityReport,
    ) -> Result<(), ModelError>;

    fn validate_state(
        &self,
        input: &StateInput<'_>,
        scratch: &mut ScratchSpace<'_>,
        needs: &mut InputNeedOutput<'_>,
    ) -> Result<QueryStatus<ValidatedState>, ModelError>;

    fn open_snapshot(
        &self,
        state: &ValidatedState,
        continuation: Option<&ContinuationToken>,
        scratch: &mut ScratchSpace<'_>,
    ) -> Result<QueryStatus<Box<dyn RealizationSnapshot>>, ModelError>;

    fn transition(
        &self,
        request: &TransitionRequest<'_>,
        out: &mut TransitionOutput<'_>,
        scratch: &mut ScratchSpace<'_>,
    ) -> Result<QueryStatus<PageInfo>, ModelError>;
}

pub trait RealizationSnapshot: core::fmt::Debug {
    fn identity(&self) -> &SnapshotIdentity;
    fn world_domain(&self) -> &WorldDomainDescriptor;
    fn model_time(&self) -> &ModelTimeDescriptor;
    fn capabilities(&self) -> &[CapabilityDescriptor];

    fn geometry(
        &self,
        request: &GeometryRequest<'_>,
        out: &mut GeometryOutput<'_>,
        scratch: &mut ScratchSpace<'_>,
    ) -> Result<QueryStatus<PageInfo>, ModelError>;

    fn fields(
        &self,
        request: &FieldRequest<'_>,
        out: &mut FieldOutput<'_>,
        scratch: &mut ScratchSpace<'_>,
    ) -> Result<QueryStatus<PageInfo>, ModelError>;

    fn samples(
        &self,
        request: &SampleRequest<'_>,
        out: &mut SampleOutput<'_>,
        scratch: &mut ScratchSpace<'_>,
    ) -> Result<QueryStatus<PageInfo>, ModelError>;

    fn tile(
        &self,
        request: &TileRequest<'_>,
        out: &mut TileOutput<'_>,
        scratch: &mut ScratchSpace<'_>,
    ) -> Result<QueryStatus<PageInfo>, ModelError>;

    fn entities(
        &self,
        request: &EntityRequest<'_>,
        out: &mut EntityOutput<'_>,
        scratch: &mut ScratchSpace<'_>,
    ) -> Result<QueryStatus<PageInfo>, ModelError>;

    fn ecology(
        &self,
        request: &EcologyRequest<'_>,
        out: &mut EcologyOutput<'_>,
        scratch: &mut ScratchSpace<'_>,
    ) -> Result<QueryStatus<PageInfo>, ModelError>;

    fn relationships(
        &self,
        request: &RelationshipRequest<'_>,
        out: &mut RelationshipOutput<'_>,
        scratch: &mut ScratchSpace<'_>,
    ) -> Result<QueryStatus<PageInfo>, ModelError>;

    fn observe(
        &self,
        request: &ObservationRequest<'_>,
        out: &mut ObservationOutput<'_>,
        scratch: &mut ScratchSpace<'_>,
    ) -> Result<QueryStatus<PageInfo>, ModelError>;

    fn forcing(
        &self,
        request: &ForcingRequest<'_>,
        out: &mut ForcingOutput<'_>,
        scratch: &mut ScratchSpace<'_>,
    ) -> Result<QueryStatus<PageInfo>, ModelError>;

    fn attachment(
        &self,
        request: &AttachmentRequest<'_>,
        out: &mut AttachmentOutput<'_>,
        scratch: &mut ScratchSpace<'_>,
    ) -> Result<QueryStatus<PageInfo>, ModelError>;
}
```

The traits have no associated types, generic methods, `async fn`, `impl Trait`,
`Self` return, or `Sized`/`Clone` supertrait, so they are object-safe. The
long-lived `Box<dyn RealizationSnapshot>` allocation is outside hot paths.
Hot query data and scratch space are caller-owned. `Send + Sync` is deliberately
not a base requirement: a native binding may add those bounds, while a
single-threaded wasm binding need not pretend to support them.

Snapshot opening uses the same status envelope as queries because validating
state bytes does not guarantee that every Model can finish its bounded
canonical initialization. In particular, Option 4 may need a deterministic
continuation or may remain unresolved after its immutable closure has been
validated.

### 7.2 State input and missing chunks

```rust
pub struct StateInput<'a> {
    pub generation: GenerationId,
    pub state_schema: StateEncodingId,
    pub address_bytes: &'a [u8],
    pub chunks: &'a [ChunkView<'a>],
    pub input_policy: InputPolicy,
}

pub struct ChunkView<'a> {
    pub id: ChunkId,
    pub bytes: &'a [u8],
}

#[derive(Clone, Debug)]
pub struct ValidatedState {
    pub snapshot: SnapshotIdentity,
    pub validation: ValidationDigest,
    pub opaque_handle: BoundedStateHandle,
}
```

`opaque_handle` is meaningful only to the model instance that validated it. It
is not a wire identity or persisted state. Persisted identity remains the
canonical state address and its required immutable closure.

### 7.3 Common request header

```rust
pub struct QueryHeader<'a> {
    pub time: TimeSelector,
    pub accuracy: AccuracyRequest,
    pub max_items: u32,
    pub max_work: WorkLimit,
    pub continuation: Option<&'a ContinuationToken>,
    pub provenance_level: ProvenanceLevel,
}

pub struct SampleRequest<'a> {
    pub common: QueryHeader<'a>,
    pub positions: &'a [WorldAddress],
    pub channels: &'a [CapabilityId],
    pub footprint: SamplingFootprint,
}

pub struct TileRequest<'a> {
    pub common: QueryHeader<'a>,
    pub cell: CellAddress,
    pub resolution: [u16; 3],
    pub halo: u8,
    pub channels: &'a [CapabilityId],
}
```

Entity, ecology, relationship, observation, forcing, geometry, and transition
requests use the same header and add their typed selector. Inputs are
normalized before hashing. A continuation is valid only for the same snapshot,
request digest, semantic package, and query revision.

### 7.4 Caller-owned outputs

```rust
pub struct SampleOutput<'a> {
    pub rows: &'a mut [SampleRow],
    pub values: &'a mut [ValueCell],
    pub variable_bytes: &'a mut [u8],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PageInfo {
    pub rows_written: u32,
    pub values_written: u32,
    pub bytes_written: u32,
    pub canonical_order: OrderingId,
}
```

Every output type reports capacity requirements or returns a typed
`OutputCapacityInsufficient` error before mutation. On `ModelError`,
`NeedsInput`, or value-less `Unresolved`, buffers remain unchanged. `Partial`
commits only the complete canonical prefix described by `PageInfo`. A caller
may safely discard any invocation that it cancels before publication.

No result may borrow model cache storage. This prevents eviction from
invalidating a page and supports both native and wasm memory models.

## 8. Status, availability, quality, and provenance

### 8.1 Query completion status

```rust
#[derive(Clone, Debug)]
pub enum QueryStatus<T> {
    Complete(T),
    Partial {
        value: T,
        continuation: ContinuationToken,
    },
    Unresolved {
        reason: UnresolvedReason,
        continuation: Option<ContinuationToken>,
    },
    NeedsInput {
        need: InputNeed,
        continuation: ContinuationToken,
    },
}
```

- `Complete` means the normalized request is complete at the achieved declared
  grade, not necessarily exact.
- `Partial` means a stable complete prefix has been returned. It is not a
  synonym for low quality.
- `Unresolved` means bounded work or available evidence cannot choose or
  enclose a semantically valid answer. It is not canonical absence and not
  proof of global unreachability.
- `NeedsInput` requests explicitly identified immutable data. It never performs
  an implicit fetch.

Options 1–3 may return `Complete` for most ordinary queries, but they still use
the same status language. Option 4 relies on all four cases.

### 8.2 Per-datum availability

Query completion is orthogonal to each value's information state:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatumStatus {
    Present,
    CanonicalAbsence,
    Inapplicable,
    Uncertain,
    Approximate,
    UnresolvedRefinement,
    StaleTransitionContent,
    Unsupported,
    OutOfDomain,
    GenerationFailed,
}
```

A complete observation page can therefore say “no organisms canonically
present,” “habitat suitable but presence uncertain,” or “ecology unsupported”
without conflating them. NaN, zero, an empty batch, `Option::None`, and a generic
error are not acceptable substitutes (`VIZ-R14`, `VIZ-R17`,
`VIZ-R116`–`VIZ-R126`).

### 8.3 Accuracy and quality

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccuracyGrade {
    Preview,
    Interactive,
    Canonical,
}

#[derive(Clone, Debug)]
pub struct AccuracyRequest {
    pub grade: AccuracyGrade,
    pub absolute_tolerance: Option<CanonicalNumber>,
    pub relative_tolerance_q32: Option<u32>,
    pub spatial_tolerance: Option<CanonicalLength>,
    pub topology_resolution: TopologyResolutionPolicy,
}

#[derive(Clone, Debug)]
pub struct QualityRecord {
    pub achieved_grade: AccuracyGrade,
    pub bound: QualityBound,
    pub source: UncertaintySource,
    pub support: QualitySupport,
    pub refinement: RefinementLevel,
    pub category_stability: CategoryStability,
}
```

`QualityBound` is a tagged union supporting exact, absolute/relative interval,
confidence, probability, covariance, residual-plus-stability, count sampling,
category margin, and explicitly unbounded forms. A model chooses the bound
appropriate to the declared value semantics; it does not force every field
into one confidence percentage.

Canonical grade means the model's reference semantic/numeric contract, not
that an approximate physical model has zero modeling error. Option 3's
law-to-sample calibration and Option 4's approximate SPDE ensemble remain
declared approximations even when evaluated bit-identically.

### 8.4 Provenance

```rust
#[derive(Clone, Debug)]
pub struct ProvenanceRecord {
    pub snapshot: SnapshotIdentity,
    pub authority: AuthorityClass,
    pub capability: CapabilityId,
    pub algorithm_revision: u32,
    pub spatial_support: SpatialSupport,
    pub temporal_support: TemporalSupport,
    pub transition: Option<TransitionProvenance>,
    pub dependencies: DependencyDigest,
    pub quality: QualityRecord,
    pub page: PageProvenance,
}
```

Transition provenance identifies endpoint states and, when material, path,
checkpoint, or retained-history origin. It never changes the authoritative
current state. This supports `VIZ-R65`, `VIZ-R80`, `VIZ-R100`,
`VIZ-R103`–`VIZ-R110`, `VIZ-R115`, `VIZ-R124`, and `VIZ-R138`.

### 8.5 Continuation tokens

A continuation is a bounded, versioned value, not a pointer into a cache. It
commits to:

- Model generation and package;
- source snapshot or transition endpoint roots;
- normalized request digest;
- canonical cursor and page ordering;
- accumulated certified bounds or search frontier needed for correctness;
- query algorithm revision; and
- token codec version and integrity digest.

Replaying a token after cache eviction yields the same next page. A model may
declare tokens session-local for size reasons, but then they still cannot rely
on cache identity and cannot be persisted as portable transition history.

### 8.6 `ModelError`

`ModelError` is reserved for invalid or corrupt input, unsupported versions,
missing required immutable closure under a fail policy, output/scratch contract
violations, or an internal integrity failure. Expected ambiguity, approximation,
and work exhaustion are statuses. Panics or unwinding never cross a wire or
host boundary.

## 9. Common query semantics

### 9.1 Geometry

`geometry` returns canonical physical support, material-domain references,
affordances, discontinuity classification, coverage, and error. It does not
return a prescribed render mesh. A result may describe height samples,
piecewise surfaces, implicit solids, volumes, or topology graphs according to
the capability descriptor. This supports `VIZ-R24`–`VIZ-R33` and
`VIZ-R83`–`VIZ-R91` without selecting tessellation.

### 9.2 Fields, samples, and tiles

`fields` queries coverage or structured field support, `samples` evaluates typed
channels at stable addresses and explicit footprints, and `tile` evaluates the
same semantic channels over one canonical cell and resolution/halo request.
Tile partitioning is an access optimization: a value's identity and meaning
cannot change because a different tile layout was used. Channels retain
canonical order and structure-of-arrays output is allowed, but memory layout is
not semantic (`VIZ-R31`, `VIZ-R32`, `VIZ-R133`, `VIZ-R142`,
`VIZ-R150`).

### 9.3 Entities and features

Entity results include exact identity or stable identifying context, identity
scope, defining support, canonical attributes, classifier/topology margin, and
relationships needed to interpret the feature. They distinguish global,
state-local, region-local, time-local, transition-local, and
representative-only identity. A lineage key or matching signature is not
silently promoted to exact cross-world identity (`VIZ-R50`, `VIZ-R58`,
`VIZ-R63`, `VIZ-R66`, `VIZ-R121`, `VIZ-R131`–`VIZ-R133`).

### 9.4 Ecology and relationships

`ecology` returns population or trait meaning, distributions, abundance kind,
support area/volume/time, habitat and tolerance constraints, representative
sampling rules, and canonical actor identities. `relationships` exposes typed
direction, endpoint roles, magnitude/flow meaning, and temporal/spatial support.
Option 4's full measure/coupling structure remains a richer profile, but common
food-web and habitat relationships cannot be omitted merely because a field
tile exists (`VIZ-R49`–`VIZ-R61`).

### 9.5 Canonical observation

`observe` is the only common source for inspection and Impression-eligible
facts. Given a location/support, time, subject filters, and accuracy, it returns:

- resolved subject(s) or explicit ambiguity;
- canonical attributes and relationships;
- identity and identity scope;
- spatial/temporal support;
- applicability, quality, margin, and provenance; and
- eligibility for capture and for Yearning use.

Map picking and embodied picking normalize to this request. Screen color,
visible mesh, animation pose, and transient presence do not substitute for it.
This directly addresses `VIZ-R62`–`VIZ-R73` and `VIZ-R90`.

### 9.6 Forcing

`forcing` returns canonical cycles, phase, boundary conditions, moments,
envelopes, or response modes. It never returns live clouds, organism movement,
or wall-clock-driven weather. A Visualization records the mapping from model
time into its simulator separately (`VIZ-R42`, `VIZ-R46`,
`VIZ-R92`–`VIZ-R100`).

### 9.7 Transition summary

A common `TransitionRequest` identifies:

- from/to validated State identities;
- an ordered canonical path or path id when the Model defines one;
- requested World Space bounds and model-time relation;
- channel, entity, ecology, and relationship interests;
- accuracy, event/correspondence policy, caps, and continuation; and
- the Visualization history context only when a proposal-specific profile
  explicitly consumes it.

The common result can contain channel change bounds, change classification,
continuity risk, correspondence candidates or explicit non-correspondence,
affected supports, topology-risk summaries, recommended maximum presentation
step, and provenance. Correspondence is transition-local evidence. The common
summary does not imply a spatial deformation, transport velocity, or generic
blend rule; those belong to profiles (`VIZ-R101`–`VIZ-R115`).

### 9.8 Build attachment

The Model may expose an attachment query returning canonical tangent/normal or
gravity frame, attachment feature, geometry interval, and conflict status at an
Impression address. Build content and loading remain a separate Visualization
profile. Querying an attachment never mutates terrain, ecology, Egress, or
observation (`VIZ-R161`–`VIZ-R168`).

## 10. Proposal adapters and unique profiles

### 10.1 Option 1 adapter

Option 1 needs only a mechanical common adapter:

| Option 1 concept | Common binding |
|---|---|
| `(M, q)` | `SnapshotIdentity` with canonical encoded `StateId` |
| Cube-map point / quadtree cell | `WorldAddress` / `CellAddress` under separate schemas |
| `Planet` | `WorldDomainDescriptor` plus planet/forcing capabilities |
| `sample` / `tile` | Common sample/tile query with explicit status and bounded outputs |
| Rivers, lakes, plates, lineages, organisms | Entity/ecology queries with identity scope and classifier margin |
| Food web and habitat membership | Relationship query |
| Canonical attribute record | Observation query at Canonical grade |
| `Sensitivity` | Common transition risk plus optional sensitivity diagnostic profile |
| `TransitionDescriptor` | Common transition summary |

The adapter must add transactional output, item/work caps, pagination, and
semantic descriptors that the proposal's short Rust sketch omits. It must also
separate `GenerationId` from optional package/artifact identity and encode
exact-species identity separately from persistent lineage-slot correspondence.
No latent coordinate or Jacobian is required by a Visualization.

### 10.2 Option 2 wake-session profile

The immutable common snapshot for Option 2 is the canonical field
`W(hat(theta_star), x)`. The experienced wake field `W(Xi(x,s), x)` is a
different, history-dependent view. It needs a profile such as:

```rust
pub trait WakeRealizationSession: core::fmt::Debug {
    fn identity(&self) -> &WakeSessionIdentity;

    fn advance(
        &mut self,
        request: &WakeAdvanceRequest<'_>,
        out: &mut WakeDeltaOutput<'_>,
        scratch: &mut ScratchSpace<'_>,
    ) -> Result<QueryStatus<PageInfo>, ModelError>;

    fn effective_samples(
        &self,
        request: &SampleRequest<'_>,
        out: &mut SampleOutput<'_>,
        scratch: &mut ScratchSpace<'_>,
    ) -> Result<QueryStatus<PageInfo>, ModelError>;

    fn checkpoint(
        &self,
        out: &mut [u8],
    ) -> Result<QueryStatus<CheckpointInfo>, ModelError>;
}

#[derive(Clone, Debug)]
pub struct WakeSessionIdentity {
    pub model_state: SnapshotIdentity,
    pub transition_id: TransitionId,
    pub integrator_revision: u32,
    pub initialization_revision: u32,
    pub bucket_rule_revision: u32,
    pub seam_policy_revision: u32,
    pub path_prefix_digest: [u8; 32],
}
```

The profile must define all of the following before it can claim reproducible
wake presentation under `VIZ-R108` and `VIZ-R115`:

1. `Xi` initial conditions for resident, unseen, revisited, and evicted cells;
2. an exact analytic update or a fixed, frame-subdivision-independent integrator;
3. canonical Traveler path/arclength input, cell-boundary events, rounding, and
   coordinate bucket rules;
4. spatial coverage and per-tile `Xi` provenance;
5. halo/joint-boundary rules for terrain, drainage, climate, water, and ecology
   when adjacent tiles use different effective coordinates;
6. checkpoint serialization and replay identity;
7. what a canonical inspection reads during the wake; and
8. how a new Egress step composes with an unfinished wake.

There are only three honest eviction policies:

```rust
pub enum WakeEvictionPolicy {
    /// Recompute bit-identically from a bounded checkpoint and canonical path suffix.
    Reconstructible,
    /// Retain every authoritative wake cell/history needed for exact revisit.
    RetainedAuthority,
    /// Discard it because wake state is explicitly presentation-only.
    PresentationForgetful,
}
```

`Reconstructible` requires a bounded compaction theorem or explicit memory cap;
an ever-growing travel log is not a cache-independent solution.
`RetainedAuthority` grows with explored area unless another bound is supplied.
`PresentationForgetful` is compatible only when canonical observation and
identity always use `W(theta_star, x)` and exact wake replay is not claimed.

As written, Option 2 selects none of these. Eviction can therefore change
revisit output, contradicting `VIZ-R134` and `VIZ-R145`; frame cadence can
change integration and bucket crossings, contradicting `VIZ-R137`; and
different resident-tile histories can create physical seams, contradicting
`VIZ-R27`, `VIZ-R32`, and `VIZ-R150`. The compatibility report must mark the
full wake profile unsupported until those choices are versioned.

The proposal must also separate a continuous navigator accumulator from the
canonical fixed-point State type. The State consumed by `W`, the value stored in
an Impression, and the value said to change under sub-bucket movement cannot
remain three different interpretations of one `Coord`.

### 10.3 Option 3 statistical-law profile

Option 3's canonical object is a law, while the Traveler encounters one
deterministic sample. The profile makes both queryable without conflation:

```rust
pub trait StatisticalLawSnapshot: core::fmt::Debug {
    fn law_descriptor(&self) -> &LawDescriptor;

    fn statistics(
        &self,
        request: &StatisticRequest<'_>,
        out: &mut StatisticOutput<'_>,
        scratch: &mut ScratchSpace<'_>,
    ) -> Result<QueryStatus<PageInfo>, ModelError>;

    fn law_sample_bridge(
        &self,
        request: &LawSampleBridgeRequest,
        out: &mut LawSampleBridgeOutput<'_>,
    ) -> Result<QueryStatus<PageInfo>, ModelError>;

    fn transport(
        &self,
        request: &LawTransportRequest<'_>,
        out: &mut LawTransportOutput<'_>,
        scratch: &mut ScratchSpace<'_>,
    ) -> Result<QueryStatus<PageInfo>, ModelError>;
}
```

`LawDescriptor` identifies the sufficient-statistic schema, sample/applicability
universe, base-measure semantics, natural and mean coordinates, free-energy
revision, and which statistics are scalar magnitudes, probabilities,
prevalences, intensities, or derived values. A `LawSampleBridge` records:

- law statistic and sample channel ids;
- whether the relationship is an identity, deterministic transform,
  calibration, asymptotic claim, or estimator;
- calibration corpus/manifest identity;
- finite-domain and finite-population support;
- bias, variance, confidence/error bounds, and ergodic assumptions;
- sample innovation id/channel revision and common-innovation rule; and
- whether the value is eligible for canonical observation or only aggregate
  interpretation.

This prevents `mu_a` from being reported as an exact local organism fact and
makes the LGCP prevalence tolerance visible under `VIZ-R54`–`VIZ-R61`,
`VIZ-R69`, and `VIZ-R117`–`VIZ-R124`. The deterministic sample must declare a
state-independent innovation coupling across nearby laws. If `theta` is folded
into the field seed, statistically close laws may have unrelated ground
samples; that fact must be exposed as high transition risk rather than hidden.

The transition payload is a tagged plan:

```rust
pub enum LawTransportBlock {
    WfrSpatialMeasure(WfrBlock),
    BuresGaussianField(BuresBlock),
    FeatureCorrespondence(CorrespondenceBlock),
}
```

A WFR block carries endpoint State ids, nonnegative measure and units, World
Space support, ground metric, `delta_W`, transport/reaction parameterization,
mass creation/destruction semantics, endpoint/error bounds, deterministic grade,
and annulus/pinning applicability. A Bures block carries field-law ids,
spectral basis and normalization, endpoint means/covariances or spectra,
common sample phase/innovation, interpolation revision, support, and error. A
transport block never claims feature identity; that remains a separate
correspondence result.

The profile also requires an overlap policy:

```rust
pub enum TransitionChainPolicy {
    FinishThenStart,
    RestartFromCurrentCanonical,
    ComposeInDeclaredRepresentation,
    UnresolvedOverlap,
}
```

The policy states how a new canonical commit interacts with an unfinished WFR
or Bures morph, what transient state must be recorded for replay, and how
completion/progress is parameterized. Option 3 currently defines one old/new
annulus but not this rebase/composition rule; full conformance to
`VIZ-R100`, `VIZ-R103`, `VIZ-R108`, `VIZ-R111`, and `VIZ-R115` remains open.

### 10.4 Option 4 Loom profile

The common status and snapshot APIs deliberately accommodate Option 4's
`Partial`, `Unresolved`, and closure semantics. A Loom-aware Visualization needs
additional ontology queries:

```rust
pub trait LoomConstitutionSnapshot: core::fmt::Debug {
    fn constitution(&self) -> &ConstitutionDescriptor;

    fn measures(
        &self,
        request: &MeasureRequest<'_>,
        out: &mut MeasureOutput<'_>,
        scratch: &mut ScratchSpace<'_>,
    ) -> Result<QueryStatus<PageInfo>, ModelError>;

    fn relation_couplings(
        &self,
        request: &CouplingRequest<'_>,
        out: &mut CouplingOutput<'_>,
        scratch: &mut ScratchSpace<'_>,
    ) -> Result<QueryStatus<PageInfo>, ModelError>;

    fn law_nodes(
        &self,
        request: &LawNodeRequest<'_>,
        out: &mut LawNodeOutput<'_>,
        scratch: &mut ScratchSpace<'_>,
    ) -> Result<QueryStatus<PageInfo>, ModelError>;
}
```

A typed measure descriptor includes motif domain, scale/level, unit, mass kind
(fraction, inventory, capacity, expected abundance, or rate), conservation or
creation policy, applicability, exact total or interval, atom schema,
parent/child restriction, procedural-tail meaning, and aggregation law.

A relation-coupling descriptor includes typed source/target/hyperedge roles,
direction, physical unit, sparse/factor representation, marginal or
submarginal maps, conversion efficiency, capacities, slack/loss channels,
scale, and balance bounds. Flattening feeding energy flow into an untyped
matrix of floats would violate `VIZ-R13`, `VIZ-R39`, `VIZ-R53`,
`VIZ-R66`, and `VIZ-R147`.

Option 4 transition pages preserve event semantics:

```rust
pub enum EntityTransitionKind {
    Match,
    Birth,
    Death,
    Split,
    Merge,
    Unresolved,
}

pub struct EntityTransitionEvent {
    pub kind: EntityTransitionKind,
    pub from: BoundedEntitySet,
    pub to: BoundedEntitySet,
    pub transition_identity: TransitionEventId,
    pub parameter_interval: CanonicalInterval,
    pub world_support: SpatialSupport,
    pub cost_interval: CanonicalInterval,
    pub margin: CanonicalInterval,
    pub provenance: ProvenanceRecord,
}
```

Topology events additionally carry persistence, filtration/complex revision,
affected World Space bounds, and whether an inter-complex map resolves the
event. Law-space couplings are transition evidence, not spatial deformation.
Exact manifestation ids remain endpoint identities; lineage-basin keys are
matching features; split/merge ancestry is transition-local.

State opening has four separate axes: syntactically representable bytes,
semantic validity, locally available verified closure, and reachability from a
source. A Merkle root alone is not an open snapshot. Missing chunks produce
`NeedsInput` or the declared missing-required-data error; corrupt chunks are an
integrity error; bounded solver ambiguity is `Unresolved`. Continuations remain
valid across cache eviction and bind the State root, request, manifest, cursor,
and accumulated bounds.

## 11. Compatibility negotiation

```rust
#[derive(Clone, Debug)]
pub struct CompatibilityRequest {
    pub experience: ExperienceProfileId,
    pub required: BoundedCapabilityRequirements,
    pub optional: BoundedCapabilityRequirements,
    pub accepted_domains: BoundedDomainRequirements,
    pub accepted_time: TemporalCompatibility,
    pub required_determinism: DeterminismGrade,
    pub accepted_accuracy: AccuracyGradeSet,
    pub known_adapters: BoundedAdapterSet,
    pub hard_limits: ConsumerLimits,
}

#[derive(Clone, Debug)]
pub enum CompatibilityDisposition {
    Full,
    Reduced { profile: ReducedExperienceId },
    Refused,
}
```

The report binds every requested semantic to an exact capability/version or
adapter, records units/domain/time/identity/quality agreement, lists missing or
degraded optional meanings, and explains every refusal. It separately reports
map, embodied, inspection, temporal, transition, ecology, and Build support.
Matching names never establishes compatibility (`VIZ-R153`–`VIZ-R159`).

Thus Options 2 and 3 may negotiate static map/embodied profiles while declaring
canonical time unsupported; Option 2 may support canonical inspection while
refusing exact wake replay; Option 3 must refuse a consumer that treats law
prevalence as exact local occupancy; and Option 4 may expose common fields while
reducing away constitution inspection when the Loom profile is absent.

Every update also publishes an evolution record covering State addresses,
observations, Impressions, transition histories, capability semantics,
adapters, and exact-presentation replay. An old record is reproducible,
explicitly migrated as a new observation, or unsupported—never silently
reinterpreted (`VIZ-R140`, `VIZ-R158`).

## 12. Determinism and versioning

| Grade | Promise |
|---|---|
| Identity | Canonical addresses, ids, schemas, dependency digests, and encoded records are bit-identical. |
| Canonical | The reference query operation path, result, and ambiguity/status are bit-identical on conforming native and wasm targets. |
| Bounded interactive | Values may differ within declared bounds but cannot mint canonical identity or an Impression without Canonical confirmation. |
| Same-platform | Repeatability is limited to one target contract and is not cross-platform canonical. |
| Presentation | Reproducible only under the full Visualization/simulator/history tuple, or explicitly non-reproducible. |

Canonical dependencies include generation, exact State, relevant model time,
capability and algorithm revisions, normalized request, accuracy, and semantic
package closure. They exclude schedule, warm caches, worker count, resource
tier, frame cadence, and hardware (`VIZ-R134`–`VIZ-R137`,
`VIZ-R144`–`VIZ-R152`).

Option 1 may claim Canonical fields only after its numeric manifest fixes every
operation. Option 2 currently claims identity and same-platform content; its
wake is weaker. Option 3's live navigation is cross-platform only in Canonical
mode. Option 4 returns `Unresolved` rather than a platform-dependent Canonical
best effort.

Exact presentation replay additionally requires Visualization and simulator
identities, assets, preferences, tier, parameters, seed, canonical initial
simulation state, action/event log or replacement snapshot, simulation time,
and continuity state. A Model snapshot promises canonical meaning, not pixels
(`VIZ-R94`–`VIZ-R96`, `VIZ-R115`, `VIZ-R128`, `VIZ-R135`, `VIZ-R139`).

## 13. Native, wasm32, and stable plugin boundaries

The preferred deployment statically links the Model adapter and `viewer-host`
into one native executable or one wasm module. That permits ordinary object-safe
Rust calls, borrowed caller buffers, and one implementation exercised by native
and Node wasm parity. Browser execution may be inline or host-scheduled on a
Worker; the semantic call is unchanged. JavaScript does not retain pointers
across wasm memory growth.

Rust trait objects are not a stable plugin ABI. A separately distributed Model
uses a versioned byte or WIT-style component protocol with opaque model/snapshot
handles and bounded operations resembling:

```text
describe -> descriptor bytes
negotiate(request bytes) -> compatibility bytes
validate-state(state + supplied chunks) -> status + needs/handle
open-snapshot(validated handle) -> snapshot handle
query-into(snapshot, opcode, request, output capacity)
transition-into(model, request, output capacity)
drop-handle(handle)
```

The wire schema fixes byte order, integer widths, tags, ordering, lengths and
caps, allocation ownership, initialized prefixes, and panic/trap conversion.
WIT resources may represent handles; large pages remain bounded. The plugin has
no ambient files, sockets, clock, entropy, DOM, or GPU. The host asynchronously
injects verified immutable chunks; `NeedsInput` drives retry rather than a
synchronous JavaScript callback.

Wire/WIT version, semantic generation, package, artifact, State encoding, and
capability schema remain independent. `#[repr(C)]` does not make `Vec`,
`String`, references, generic enums, fat pointers, or trait objects stable
across modules. Native distributed plugins should preferably use the same
sandboxed component contract.

## 14. VIZ requirement-range traceability matrix

| Requirement range | Shared contract element | Proposal-specific qualification |
|---|---|---|
| `VIZ-R01`–`VIZ-R10` | Source links, semantic/wire separation, authority classes, one-State snapshots, independent spaces | Option 2 wake requires explicit transition/presentation authority. |
| `VIZ-R11`–`VIZ-R17` | Capability/vocabulary descriptors, negotiation, datum status, no canonical fallback | Option 3 law/sample and Option 4 typed profiles preserve extra meaning. |
| `VIZ-R18`–`VIZ-R33` | Domain/address descriptors, point/cell distinction, topology, metric, support, geometry | Option 2 geometry/seams remain gaps; Options 1/4 supply planet schemas. |
| `VIZ-R34`–`VIZ-R48` | Field schemas, units, aggregation, dependencies, mixtures, refinement quality | Option 3 separates law/sample; Option 4 adds inventories/restrictions. |
| `VIZ-R49`–`VIZ-R61` | Ecology/entity/relationship queries, abundance support, representative identity | Option 2 is incomplete; Options 3/4 require richer profiles. |
| `VIZ-R62`–`VIZ-R73` | Canonical observation, ambiguity, capture eligibility, provenance | Wake/transport presentation cannot mint false current subjects. |
| `VIZ-R74`–`VIZ-R82` | Projection-independent multi-resolution data, coverage and history provenance | Enables but does not itself satisfy map behavior. |
| `VIZ-R83`–`VIZ-R91` | Geometry/affordance/ecology inputs, stable addresses, shared observation | Enables but does not itself satisfy embodied behavior. |
| `VIZ-R92`–`VIZ-R100` | Model-time descriptor, forcing, simulation-time exclusion, transition time | Options 2/3 are temporally reduced; Option 3 overlap remains open. |
| `VIZ-R101`–`VIZ-R115` | Endpoint/path transition query, bounds, risk, correspondence and provenance | Option 2 needs wake history; Option 3 transport/composition; Option 4 events. |
| `VIZ-R116`–`VIZ-R126` | Query/datum status, typed bounds, uncertainty source/support, refinement identity | Partial, unresolved, input needs, absence and failure stay distinct. |
| `VIZ-R127`–`VIZ-R140` | Generation/package/artifact/State/vocabulary ids, identity scope and grades | Option 2 and ordinary Option 3 navigation are not cross-platform Canonical. |
| `VIZ-R141`–`VIZ-R152` | Bounded pages/work, caller buffers, continuations, aggregation, cache independence | Option 2 lacks bounded wake eviction; Option 4 preserves projective laws. |
| `VIZ-R153`–`VIZ-R160` | Pre-use semantic report, reduced/refused profiles, evolution, native/wasm binding | Evidence remains separate from a design claim. |
| `VIZ-R161`–`VIZ-R168` | Optional attachment query and separate Build negotiation | Only Option 4 specifies a rich Build profile. |

The matrix traces interface obligations, not proof of consumer behavior.
Cross-view, cross-scale, cross-time, cross-transition, cross-tier, and
cross-platform evidence remains required by `VIZ-R160`.

## 15. Unresolved architectural decisions

1. Registry and collision rules for semantic, unit, capability, value-schema,
   and extension-profile ids.
2. Exact address, output, scratch, chunk, and continuation caps and whether
   opaque addresses have built-in fast forms.
3. Snapshot ownership (`Box`, arena handle, or other) and native-only
   `Send + Sync` policy.
4. Minimum collision-grade geometry needed for embodied compatibility,
   especially future non-height-field Models.
5. Whether canonical model time is optional in the base product profile.
6. Minimum common transition correspondence versus a bounds-only reduced
   transition capability.
7. Option 2's authority classification, integrator, seam, and bounded
   eviction/revisit policy.
8. Option 3's common innovation, finite LGCP envelope, stable subject identity,
   and unfinished-transition composition.
9. Option 4's chunk/token caps and whether continuations persist across
   semantically identical artifacts.
10. Whether typed measures/relations become a general optional profile beyond
    Option 4.
11. Exact byte codec and WIT shared-buffer/resource design.
12. Authored Build schema and shared semantic collision representation.
13. Conformance-evidence format binding advertised capabilities to
    `VIZ-R160` without making evidence bytes world identity.

These open choices do not weaken the core decision: commonality lives in the
immutable semantic snapshot, explicit query meaning, and honest status envelope.
Proposal-specific history, statistical laws, transport, constitutions, and
typed relations remain explicit profiles rather than being erased to make the
interface look universal.
