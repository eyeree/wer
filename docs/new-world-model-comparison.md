# New World Model Comparison

## Status and scope

This document compares the four proposed replacements for the current world
model:

- [Option 1](new-world-model-option-1.md): a 32-dimensional latent manifold of
  complete procedural planets;
- [Option 2](new-world-model-option-2.md): an attribute-first latent manifold
  with a per-tile continuity wake;
- [Option 3](new-world-model-option-3.md): an exponential family of statistical
  world-laws with Fisher geometry; and
- [Option 4](new-world-model-option-4.md): the World Loom, a typed causal
  constitution navigated by transport and grammar rewrites.

The comparison uses [the conceptual model](conceptual-model.md) as the
requirements baseline. It evaluates the proposals as written, not their own
claims about one another. This matters because Options 3 and 4 were edited in
parallel and a few cross-references are stale.

All four are designs rather than measured implementations. Consequently,
"meets" below means that the design supplies a credible, testable contract. It
does not mean that native/wasm determinism, real-time performance, memory
plateaus, or player comprehension have been demonstrated.

## Executive assessment

**Option 1 is the lowest-regret implementation baseline.** It has the most
complete, concrete, and testable planet model among the less ambitious options,
the strongest portable determinism contract short of Option 4, and the best
risk-adjusted chance of producing varied but coherent worlds. Its cold global
work and whole-coordinate cache turnover must be benchmarked before it can be
called real-time.

**Option 4 has the highest ceiling and the highest risk by a wide margin.** It is
the only proposal whose state vocabulary can grow beyond a fixed latent axis or
sufficient-statistic set. It also has the strongest account of conservation,
relational structure, structural change, deterministic failure, and transition
correspondence. It should be treated as a staged research program through its
own Stage 0A/0B kill gates, not as an unconditional production commitment.

**Option 3 is the strongest fixed-vocabulary research alternative.** Its convex
reconciliation and Fisher geometry are elegant and comparatively compact, but
the fitted statistical law is only approximately tied to what is realized on
the ground. It also adds several advanced numerical systems and makes live
cross-platform navigation deterministic only in Canonical mode.

**Option 2 should not be selected as written.** Its direct attribute-space
steering is easy to understand and worth preserving as an experimental
baseline, but the proposal has unresolved contradictions in its coordinate
representation, projection, continuity state, cache independence, portable
Egress, and claimed tile-regeneration bound.

### Decision matrix

The ratings describe confidence in the present proposal, not only its intended
end state.

| Criterion | Option 1 | Option 2 | Option 3 | Option 4 |
|---|---|---|---|---|
| Requirements coverage | **Strongest practical world-generation/runtime core**; Build and Attractor integration remain partial | **Mixed**; covers the conceptual pipeline, but the planet, time, canonical entities, accuracy, and much of realization remain schematic | **Strong core**; rigorous navigation and ecology concepts, but planet, canonical time, Builds, and parts of realization remain open | **Broadest specification**; covers the full reference planet and optional integration profiles, but much is explicitly research-stage |
| Portable determinism | **Strong** Canonical native/wasm contract; manifest still uninstantiated | **Weakest**; identity is portable, while Realization and navigation are largely same-platform only | **Strong only in Canonical mode**; fixed addresses are portable, ordinary live navigation may diverge | **Strongest contract**; fixed-point, checked intervals, canonical operation paths, and fail-closed `Unresolved` results |
| Real-time confidence | **Moderate at best**; bounded local work, but important cold global costs are omitted from the budget | **Low despite light algebra**; realization is under-specified and the wake/regeneration bound is not valid as written | **Medium-low**; compact navigation is plausible, but Newton/CG, calibration queries, and continuity transport are unmeasured | **Lowest**; explicit latency and resolved-rate targets are good gates, not evidence that the gates can be met |
| Attribute extensibility | **Low-medium**; optional derived channels are natural, new independent controls require refitting/versioning | **Low-medium and overclaimed**; attributes, chart, metric, and steering are tightly coupled | **Low**; the fixed sufficient-statistic schema and archetype bank define the ceiling | **Highest**; typed motifs, relations, strata, and adapters can be added under explicit package and kernel versioning |
| Implementation risk | **Medium-high, but comparatively contained** | **High foundational ambiguity** despite apparent simplicity | **High mathematical, calibration, and numerical risk** | **Very high**, explicitly acknowledged and staged |
| Runtime compute/memory | **Medium to high** | **Lowest intended**, with potentially severe tile churn/cache fragmentation | **Medium-high** | **Largest planned surface; likely highest** |
| Intuitive steerability | **Best near-term confidence** if observable probes and Scope are calibrated | **Most direct mental model**, but weakest guarantee that requested attributes match realized outcomes | **Strongest formal fixed-schema reconciliation**, weakened by calibration and Hold semantics | **Highest semantic ceiling**, with the greatest risk of opaque modes or `Unresolved` gameplay |
| Variety plus coherence | **Strongest likely deliverable** | **Weakest evidence** | **Potentially rich but visibly bounded by one fitted statistical family** | **Highest theoretical ceiling and strongest typed interconnection; least implementation evidence** |
| Novelty | High within a conventional procedural-planet architecture | Moderate; the continuity wake is the main differentiator | High; world-law/Fisher navigation plus WFR continuity | Highest; typed causal programs, transport navigation, projective realization, and explicit correspondence |

## Important common ground

The proposals are not four unrelated generators. All four intend to provide:

- one global canonical state for one complete world;
- lazy deterministic realization rather than stored generated planets;
- a non-Euclidean relationship between nearby worlds;
- global-prevalence Scope rather than spatial falloff;
- simultaneous, order-independent reconciliation of weighted Yearnings;
- Egress gated by physical travel at the Traveler layer;
- portable integer-derived permanent identity;
- immutable, dependency-keyed work that is intended to be schedule- and
  cache-independent; and
- a platform-neutral Model boundary with presentation history outside the
  canonical state.

Those are baseline requirements, not selection differentiators. The important
differences are the ontology of a world state, the strength of the canonical
numeric contract, how directly Yearnings control realized outcomes, how nearby
worlds share procedural innovation, and how much runtime/compiler machinery is
needed.

## 1. Requirements, determinism, and real-time compute

### Option 1

Within the world-generation and runtime core, Option 1 is the strongest
practical match to the conceptual requirements. It specifies a finite oblate
planet, exact cube-map addresses, canonical forcing time, geology, water
inventory, drainage, climate, soil, ecology, entity identity, immutable queries,
accuracy grades, continuity-risk reporting, and a testable Yearning solve.

Its integration coverage is thinner. Builds receive a clean Model-boundary
paragraph rather than a reproduction contract, and community Attractor evidence
is external to the Model without a comparable abuse-resistance and removal
design. Those gaps matter for full conceptual conformance, though not for the
core generator decision.

Its three determinism grades are appropriately separated: portable integer
identity, bit-identical Canonical realization on native and wasm, and bounded
but potentially low-bit-different Interactive realization. Canonical results
specify traversal, iteration, rounding, and portable transcendental
approximations before an Impression is committed.

The contract is still incomplete until the parameter manifest fixes every
coefficient and numeric operation. There is also a versioning contradiction to
resolve: `minor` is described as capable of adding optional capabilities without
changing old results, while hashes are described as including the full Model
identity, apparently including `minor`.

Portable polynomial approximations are necessary but not sufficient for the
claimed native/wasm bit identity. The eventual manifest and kernels must also
freeze FMA contraction, subnormal handling, conversions, intermediate rounding,
and reduction semantics.

The real-time design is locally bounded, but the published operation table is
not a complete cold-path budget. In particular, it omits or does not isolate:

- the 16,384-sample, 16-step global sea-level solve for a new coordinate;
- the 1,536-cell, six-level climate work and monthly outputs;
- the dependencies and cold work beneath the stated sub-2-ms, 256-probe metric
  and Yearning budget; and
- possible population or refresh of coordinate-keyed preference buckets over
  the 4,096-lineage species pool.

Because cache keys include the complete Possibility coordinate, continuous
travel may get little reuse from per-coordinate global caches. Option 1 is
structurally plausible at real-time rates, but only a cold-snapshot and
continuous-Egress benchmark can establish that.

### Option 2

Option 2 satisfies the requirements cleanly only at the level of its intended
pipeline. Its realization is a planar noise-stack sketch, not a comparably
specified world model. Global sea fraction and prevalence are not well-defined
over its infinite plane without a canonical sampling measure; canonical time,
planetary structure, stable entities, accuracy tiers, and error policy are not
fully integrated.

Its determinism contract protects integer identities, but explicitly leaves
tiles, the metric, Egress direction, Resonance, and organism expression at
same-platform float exactness. The navigation subset—metric, direction, and
Resonance, with canonical ecological tiles feeding support—chooses the next
permanent coordinate, so cross-platform low-bit differences can select
different worlds. The proposal also treats mathematical summation as proof of
order-independence without specifying exact accumulation or canonical finite
precision reduction.

It also gives the coordinate two incompatible roles. The fixed-point integer
vector is the portable canonical world address, yet the text says sub-bucket
Egress changes the canonical coordinate while `W` consumes only the bucketed
value. Either a second continuous navigator state is missing from the contract,
or sub-bucket movement does not change the canonical world.

The continuity field `Xi` creates a more fundamental reproducibility problem.
The world experienced at a tile is `W(Xi(x,t), x)`, but `Xi` depends on travel
history and is stored per resident tile. The stated canonical inputs and
Impression payload omit it. Evicting `Xi` either changes revisit behavior or
requires history whose memory grows beyond the resident window. Adjacent
quantized `Xi` values can also produce incompatible terrain, drainage, climate,
or ecology at tile boundaries.

The float ODE for `Xi` also lacks a fixed integration cadence, exact integrator,
and canonical rounding schedule. Even with an integer-recorded Traveler path,
frame subdivision can therefore change bucket-crossing times and settled cache
keys.

The advertised approximately 50 microsecond navigation and thin-annulus
regeneration behavior are estimates, not evidence. The proposed bucket-count
bound is not valid in a multidimensional, path-dependent wake: a Traveler can
loop or travel tangentially while a tile remains in the band; different tiles
have different histories; and dividing one metric spread by a scalar quantum
does not count vector lattice buckets. Q32 makes the churn concern more acute.

### Option 3

Option 3 gives a fixed coordinate a precise statistical meaning and makes
reconciliation a strictly convex problem with a unique target. A fixed
coordinate, dependency keys, and entity ids are portable. Tile generation is
lazy and intended to be independent of schedule and cache state.

The next coordinate nevertheless comes from a float pipeline involving the
reconciliation solve, inversion of the mean map, matrix-free metric solves, and
rounding. The document correctly says cross-platform live navigation is
portable only when that pipeline runs in Canonical mode. The stronger
quantization-cell enclosure and cold-operation-path rules stated by Option 4
would be needed to make this contract robust near rounding boundaries.

The estimated navigation cost of hundreds of microseconds is credible enough
to prototype, but not measured. Fixed-count Newton/CG work, canonical support
quadrature, tile regeneration, WFR/Bures transition buffers, and optional
Sinkhorn work all need to appear in the same native/wasm ledger. Its initial
World Space is a plane, with a planet deferred, and canonical time and Build
reproduction are explicitly left open.

### Option 4

Option 4 has the strongest requirements and determinism specification. Its
Canonical contract includes normalized packet bytes, checked integer totals,
fixed-point costs and coefficients, canonical sparse ordering and balanced
reductions, portable tables for transcendental operations, bounded active-set
selection, complete dependency closures, and explicit interval results. If a
result cannot be certified under the work cap, the API returns `Partial` or
`Unresolved`; it never returns a platform-dependent Canonical best effort.

That semantic strength does not make the runtime feasible. A representative
Egress probe may inspect eight modes, two rewrites, four levels, twelve
transport blocks with as many as 256 atoms, path feasibility intervals, and
separate free/valid Resonance probes. The proposed target is below 4 ms native
and 10 ms wasm at 10 Hz while at least 99% of ordinary probes complete. The
target is an appropriately strict kill gate and an aggressive research bet.

Option 4 is the only proposal that treats latency and resolution rate as one
product metric. A solver that is fast because it frequently returns
`Unresolved` does not meet the requirement. Until the Stage 0A/0B gates pass,
Option 4 has the best contract on paper and the lowest confidence of meeting the
interaction envelope.

### Requirements conclusion

No option currently proves real-time operation or native/wasm parity. For a
near-term implementation, Option 1 has the best balance of specification
strength and feasibility. Option 4 is stronger semantically but cannot be ranked
as production-feasible before its early gates. Option 3 is a plausible R&D
prototype. Option 2 requires contract repair before a performance comparison is
meaningful.

## 2. Flexibility and extensibility

It is important to distinguish adding a query from adding a new independent way
for worlds to vary.

| Change | Option 1 | Option 2 | Option 3 | Option 4 |
|---|---|---|---|---|
| Add an optional derived observable | Relatively easy through a new capability/channel | Easy only if it does not participate in the chart, metric, or steering | Easy if it remains a function of existing means | Native through a capability and semantic adapter |
| Make a new attribute directly steerable | May fit existing latent directions; otherwise refit decoder/metric and version | Changes the attribute chart, Jacobian, metric, and usually coordinate schema | Changes the sufficient-statistic set, free energy, bank, metric, and calibration | Add a typed observation and sensitivity path; still requires compiler evidence and versioning |
| Add a new independent physical/ecological degree of freedom | Usually a new major model or latent redesign | Usually a new dimension and address format | A new family/refit; the fixed schema is an explicit ceiling | Add a motif, relation, law level, or stratum if the grammar and kernel can certify it |
| Add a new causal regime or relationship topology | Awkward discrete exception around the smooth decoder | Poor fit for the triangular chart | Poor fit for one convex exponential family | First-class through zero-mass introduction, typed rewrites, and relation measures |

Option 4 therefore wins extensibility decisively, but it is not cost-free or
unbounded. New opcodes or canonical operations require a kernel/application
version; some phenomena still require a new stratum or major package; packet,
grammar, compiler, and proof sizes have hard ceilings.

Among the compact options, Option 1 has the cleanest separation between latent
addressing, derived channels, capabilities, and observations. Option 2's claim
that attributes can simply be appended is incompatible with using the same
attribute vector as an invertible chart, metric source, and steering language.
Option 3 is the most mathematically coupled: in a minimal exponential family,
adding a sufficient statistic changes the coordinate dimension and the object
that generates nearly every navigation operation.

## 3. Risks and ambiguity

### Option 1

- The frozen decoder and fitted corpus can be perfectly deterministic while
  producing implausible, repetitive, or unintentionally correlated worlds.
- A 256-location probe summary can miss localized or rare features, especially
  when Scope claims to represent tails near 0.1% prevalence.
- The global cold work and whole-coordinate dependency keys may cause bursts or
  broad cache turnover and regeneration during continuous Egress.
- Exact species genomes and entity ids hash the complete coordinate and can
  churn at every quantum. The proposal does distinguish q-specific
  manifestations from persistent lineage slots, but every downstream ecology,
  identity, and correspondence consumer must use the correct grade or a smooth
  world step can still look like wholesale replacement.
- Topology margins expose river, lake, biome, and entity discontinuities; they
  do not by themselves make those transitions visually acceptable.

### Option 2

- The Q32 coordinate requires values through `2^32 - 1`, but the Rust sketch
  stores `i32`; the prose uses floor while the sketch says round.
- The projection mixes coordinate and attribute spaces and claims that a
  triangular clamp is the nearest point under a generally non-diagonal metric.
  That does not follow.
- The proposal does not supply a periodic, globally smooth decoder for its
  cyclic axes that is also compatible with the claimed global inverse. Assigning
  an ordinary interval-valued affine attribute to such an axis would create a
  wrap discontinuity; the document never declares which outputs have cyclic
  semantics that could avoid it.
- The per-tile wake is path-dependent, lacks defined eviction/revisit semantics,
  and can create cross-tile physical seams.
- Hold names the current value but does not specify whether that value is
  captured at activation or recomputed on every reconciliation. The latter
  reading behaves as drag and can ratchet instead of preserving the activation
  value.
- Nominal prevalence attributes are not demonstrated to equal prevalence in
  the realized terrain, species, or ecology.

### Option 3

- The archetype bank is a systemic quality risk: it defines validity, means,
  geometry, reachable combinations, and much of steering, but is validated by
  a corpus rather than a mathematical identity.
- The free-energy law and the separately decoded realization are not the same
  object by construction. The ecology prevalence link is only a calibrated,
  asymptotic equality, and the same bridge is not fully specified for every
  abiotic statistic.
- The scalar and prevalence blocks of the stated free energy are independent,
  limiting the claimed cross-block physical correlations unless the decoder
  adds a separate, verified coupling.
- The Bregman projection is defined in mean space but later applied to natural
  coordinates; a target on the closed hull boundary may require an infinite
  natural parameter.
- The sample coupling across nearby coordinates is ambiguous. If the coordinate
  enters the hashed field seed, adjacent canonical laws can receive unrelated
  ground samples even though their statistics are close.
- Stationary/isotropic Matérn-like fields and a fitted convex archetype hull may
  create a recognizable house style and weak tectonic/nonstationary structure.
- The LGCP thinning rule divides by `lambda_max`, but an ideal
  log-Gaussian-Cox intensity has no declared finite global maximum. A bounded
  candidate process or certified envelope is missing.
- Repress is only described as "biased below," and Hold retargets to the moving
  current mean.
- Summed natural-parameter Attractors do not preserve multimodal destinations.
  Even repeated evidence at one nonzero coordinate produces `n * theta_i`, not
  `theta_i`, driving the target toward an extreme (unboundedly in a scalar
  direction) rather than merely shrinking uncertainty around the visit.
- The WFR/Bures annulus defines a transition for one old/new pair but not how to
  rebase or compose it when another Egress commit arrives before that morph
  completes.
- Its Egress monotonicity language has a sign conflict: Section 8.4 describes
  descent of the reconciliation free energy, while Appendix C asks for a
  non-decreasing objective.

### Option 4

- Runtime transport, rewrite, active-set, correspondence, and path-feasibility
  searches can exceed their caps. Semantically correct `Unresolved` results may
  still yield an unplayable product.
- Projective restriction creates one coherent synthetic hierarchy, not a proof
  that a continuum physical model converges. Hard coarse constraints can create
  visible or systemic coarse-grid artifacts.
- Reduced bases can fail near bifurcations; nonconvex active sets can grow
  combinatorially; compiler parameter-cell subdivision can explode.
- Generated certificates, bounds, and proof artifacts can be wrong. Independent
  checking reduces but does not remove the trusted implementation surface.
- Strong convexity and uniqueness can sterilize worlds; relaxing them brings
  back candidate explosion and ambiguity.
- A global Scope request may legitimately invalidate most chunks. Structural
  sharing is a benchmark hypothesis, not a guaranteed local-update theorem.
- A State root is not self-decoding. Large packet/chunk closures make standalone
  Impressions and package distribution materially heavier.
- Typed ground costs and law weights determine what feels intuitively causal;
  type safety and solver certificates cannot prove that those choices are fun.

Option 4 exposes and contains its risks more honestly than the other proposals,
but it also has many more independent subsystems in which unknown unknowns can
hide. Option 2 has fewer components yet more foundational contradictions. Option
1 has the smallest unresolved research surface; Option 3 lies between Options 1
and 4.

## 4. Complexity, compute, and memory

| Dimension | Option 1 | Option 2 | Option 3 | Option 4 |
|---|---|---|---|---|
| Canonical coordinate before metadata | 32 `i32`s, about 128 bytes | 24–48 words, about 96–192 bytes if represented consistently | About 40 `i32`s, about 160 bytes | Up to 64 KiB and 4,096 entries, plus required chunk closure |
| Navigation core | 256 realized probes, Jacobian/metric, fixed trust-region solve | Attribute Jacobian, at most 48-by-48 Cholesky, KDE | About 256 archetypes, convex target, several Newton/CG solves | Bounded multimode route search, typed transport, rewrites, path certification, multiple Resonance probes |
| World realization | Global sea/climate work plus local terrain-to-ecology stack and 4,096 lineage pool | Mostly unspecified parameterized fields | Matérn-like fields, LGCP/marked processes, integer hydrology | Projective planet, global/coarse sections, reduced bases, variational physics/ecology, certified refinement |
| Continuity work | Transition descriptor; Visualization owns retention/blending | One full coordinate per resident tile plus multi-coordinate tile cache | WFR/Bures annulus and optional coarse Sinkhorn | Transport couplings, persistence, exact bounded feature matching, split/merge event pages |
| Package/tooling | Frozen numeric manifest and canonical kernels | Smallest intended coefficient/schema surface | Fitted statistic/archetype/field bank | Compiler-generated operators, bases, restriction maps, interval bounds, certificates, fixtures, and optional AOT kernels |
| Primary memory risk | Per-state global caches and broad cache turnover | `Xi` history, fragmented coordinate buckets, undefined revisit state | Bank, solver scratch, transport buffers, and regenerated endpoint tiles | Package/startup closure, packets/chunks, solver state, transitions, certificates, and caches |

The likely engineering-complexity ordering is:

```text
Option 2 < Option 1 < Option 3 << Option 4
```

That is not a reliable total runtime ordering. Option 2's wake may erase its
nominal advantage; Option 1's unbudgeted global work may be expensive; and
Option 3's navigation can be cheaper than Option 1's probes while its
continuity/ecology path is heavier. Option 4 has the largest planned runtime
surface and is likely the heaviest; it is unambiguously the largest memory,
package, and compiler commitment.

## 5. Intuitive steerability

### Option 1: measured outcomes, indirect controls

Option 1 defines each steerable attribute through an observation function,
population measure, membership function, applicability, uncertainty, and
derivative. Scope targets a soft global quantile, and the trust-region solve
balances fit, distance, continuity risk, and Hold terms. This is indirect for a
player but grounded in measured world output. It has the best near-term chance
of doing what a Yearning appears to ask, provided the fixed probes and decoder
are calibrated against actual player observations.

The main UX risk is proxy error: the metric and objective may say a rare trait
changed while the player never encounters it, or may miss a localized phenomenon
that dominates the player's experience.

### Option 2: direct controls, weak outcome grounding

Option 2 is easiest to explain: weighted requests form an attribute target and
a natural gradient moves toward it. Designers can inspect the chart directly.
However, a nominal prevalence coordinate is not shown to equal realized
prevalence, chart couplings can create opaque side effects, Repress and scalar
Scope lack complete formulas, ambiguous Hold timing may ratchet, and ecological
support can throttle an otherwise well-understood request in a barren location.

It is the best cheap UI/math spike, not the best final steering contract.

### Option 3: unique statistical target, approximate visible result

Option 3 gives Scope the cleanest fixed-schema mathematical meaning: it targets
a mean of the world-law, and all conflicts have one unique convex compromise.
The KL proximal term makes retaining the current law explicit, while Resonance
can report ecological support, susceptibility, and disagreement.

The player sees the deterministic sample, not the abstract law. Finite ecology,
field approximation, and calibration can make visible prevalence disagree with
the target mean. A unique convex answer can also average genuinely distinct
causal modes into a compromise. Repress, activation-time Hold, and multimodal
Attractors need repair before the formal elegance translates to predictable
gameplay.

### Option 4: causally legible surprise, if it resolves

Option 4 has the best semantic definitions: absolute one-sided Scope,
complement-based Repress, Hold against the activation snapshot, explicit
applicability denominators, exact hierarchical weighting, distinct causal modes,
and reported compromise/error intervals. It can tell the UI whether gliding
became prevalent through canopy, cliffs, atmosphere, or another certified
route, and its Transition Plan can explain births, deaths, splits, and merges.

That expressive power raises the UX burden. Observables are deliberately not
control coordinates; least-cost causal consequences may be non-obvious; modes
need selection; and partial or unresolved results must be communicated without
making the Yearning feel broken. Option 4's proposed playtest target—most
players identify the requested direction while still finding consequences
surprising but coherent—is exactly the right kill gate and is currently only a
target.

### Steering conclusion

For a fixed vocabulary, Option 1 has the best risk-adjusted outcome grounding
and Option 3 has the cleanest mathematical target. Option 2 is simplest to
prototype. Option 4 has the highest long-term ceiling for intuitive semantic
requests and explanatory surprise, but the lowest confidence until ordinary
requests resolve quickly and pass blinded playtests.

## 6. Unique and novel aspects

### Option 1

- A finite procedural planet with canonical spatial/time addressing.
- Shared spherical harmonics and smoothly transformed hashed residuals, giving
  nearby worlds a common source of detail.
- Water-volume-derived sea level, deterministic hierarchical drainage, and
  contractive climate/soil/ecology blocks.
- A pullback metric over realized observable probes rather than raw latent axes.
- Explicit classifier/topology margins and transition-risk descriptors.

### Option 2

- A torus-times-box latent topology.
- One attribute vector serving as chart, public contract, metric source, and
  steering language.
- A compact least-squares/natural-gradient Egress pipeline.
- The per-tile effective-coordinate continuity wake.
- KDE Attractors with shrinking bandwidth and dual-space rate matching.

### Option 3

- A world defined as an exponential-family probability law over observations.
- One free energy generating the mean map, Fisher metric, KL/Bregman comparison,
  and convex reconciliation geometry.
- Natural/mean dual coordinates that make prevalence a coordinate of the law.
- Information geometry for navigation and WFR/Bures only for presentation
  continuity.
- LGCP and marked-point-process ecology tied, approximately, to global means.

### Option 4

- A Possibility Coordinate that is a typed causal program plus multiscale
  measures, coefficients, and relational couplings.
- Structural regime changes through typed zero-mass introduction and grammar
  rewrites rather than hidden discontinuities in a smooth decoder.
- A state-independent universal innovation thread transformed by each
  constitution, preserving common procedural chance across worlds.
- Projective planetary realization with explicit conservation, restriction,
  residual, and failure contracts.
- Transport navigation plans reused as correspondence evidence for entities,
  species, and topology events.
- A compiler-generated, certificate-carrying data package with independent
  runtime checking.

## 7. Variety, uniqueness, plausibility, and interconnection

The number of addressable states is not the limiting factor in any option; even
the fixed-vector spaces contain far more coordinates than can be visited. The
real issue is semantic rank: how many independent kinds of world change exist,
how well the generator uses them, and whether constraints produce coherent
consequences rather than repetitive interpolation.

### Option 1

Option 1 is the strongest risk-adjusted proposal. Global harmonics, shared
multiscale residuals, plates, ocean inventory, terrain, drainage, climate,
soil/productivity feedback, a 4,096-lineage pool, and bounded local ecological
assembly provide a concrete path to diverse worlds with causal links.

Its limits are Earth-like bounds, 32 latent dimensions, a fixed decoder, fixed
probe bias, and the possibility that all worlds reveal a common procedural
skeleton. Validity-by-construction guarantees bounds and solver properties, not
that every decoded world is interesting or scientifically plausible.

### Option 2

Option 2 offers many numeric coordinates and potentially broad stylization, but
only terrain receives a full spatial generative equation. Integer hydrology
topology is outlined, while most other physical and ecological layers are said
to be analogous. A triangular sequence of ceilings is a weak model of global
feedback, and per-tile world coordinates threaten seams and interconnected
hydrology/climate/ecology. It supplies the least evidence for this criterion.

### Option 3

Option 3 can generate enormous sample-level variety and encode plausible
statistical correlations through its archetype hull. Its ecology and living
continuity could be unusually evocative. However, every world remains inside
one fitted statistic vocabulary and convex family. Stationary/isotropic field
assumptions, approximate law-to-sample calibration, and weak explicit
cross-block physics may produce a recognizable house style or statistically
valid worlds that do not feel causally connected.

### Option 4

Option 4 has the highest theoretical ceiling. Different causal programs,
inventories, phase regimes, relation graphs, motif measures, multiscale patches,
and ecology modes can vary independently while typed units, conservation,
couplings, global sections, and refinement contracts keep them interconnected.
The common innovation thread should also make nearby changes feel like
transformations of one world rather than reseeding.

Those are guarantees only relative to the synthetic laws in the package, not
proof of Earth science or fun. Strong convex closures may sterilize outcomes;
nonconvex modes may exceed search bounds; bounded packets may exclude important
variation; and no world-quality corpus yet demonstrates the claimed range.

### Variety conclusion

- **Best likely deliverable:** Option 1.
- **Highest eventual ceiling:** Option 4.
- **Most distinctive statistical/ecological family:** Option 3.
- **Weakest current evidence:** Option 2.

## 8. Other material differences

| Dimension | Option 1 | Option 2 | Option 3 | Option 4 |
|---|---|---|---|---|
| Initial World Space | Finite oblate planet, cube map | Infinite plane | Generic plane; sphere deferred | Finite oblate planet, nested icosahedron |
| Canonical time | Orbital/environmental forcing | Not integrated into realization | Explicitly left open | Orbital, tidal, seasonal forcing plus replay boundary |
| Nearby-world coupling | Same residual gradients, smoothly transformed | Same parameterized field family, but path-dependent per-tile coordinates | Nearby laws are close; common sample innovation is ambiguous | Explicit shared innovation thread transformed by state |
| Continuity | Model reports risk/descriptors; Visualization blends | Model pipeline uses a lagged per-tile effective coordinate | Visualization transports living mass and spectrally reshapes abiotic fields | Model emits law/feature/topology correspondences; Visualization owns presentation history |
| Multiple plausible consequences | Up to three local compromise directions in flat cases | One least-squares direction | One unique convex target, tending to average modes | Bounded, structurally distinct path modes with explicit selection |
| Failure model | Errors, bounds, unresolved identities, acceptance gates | Mostly assumes one answer | Fixed iteration/approximation, limited partial-result protocol | `Complete`, deterministic continuation, `Partial`, `Unresolved`, or certified unreachable |
| Shareable state | Compact coordinate and Impression record | Compact coordinate, but wake history is not addressed | Compact coordinate and means | Merkle-addressed packet; root requires a self-contained chunk closure |
| Offline maintenance | Hand-designed/fitted manifest | Hand-designed chart and weights | Fitted archetype/statistic/field bank | Compiler-generated operators, bases, bounds, certificates, and fixtures |
| Delivery shape | One substantial but conventional clean-slate model | Small concept sketch needing redesign | Research-heavy fixed-family model | Multi-stage kernel, planet, ecology, records, and integration program |

## Cross-reference reconciliation

The current files should be interpreted as Option 3 and Option 4 according to
their filenames and titles.

- Option 3's opening statement that the World Loom is styled internally as
  another "third option" is stale. Option 4 now consistently calls itself the
  fourth option and V4.
- Option 3's statement that both designs abandon a latent vector is imprecise:
  Option 3 still uses an approximately 40-dimensional fixed-point
  natural-parameter vector. It abandons a generic latent decoder, not
  fixed-dimensional coordinates.
- Option 3 overstates that the Loom can add physics without re-versioning.
  Current Option 4 permits compatible optional extensions using existing
  opcodes, but new canonical operations require kernel/application versioning
  and some new phenomena require a new stratum or major package.
- Option 3's shorthand that optimal transport is the Loom's navigation
  mechanism remains directionally correct but incomplete. Current Option 4
  combines multiscale balanced/unbalanced transport with rewrite lengths,
  directed Finsler/control paths, bounded active-set search, and a
  JKO-inspired (not classical JKO) proximal probe.
- Option 3's claim that its prevalence bridge is the tightest of "the three
  proposals" predates the current four-option set and should not be used in the
  decision. Option 4 defines canonical prevalence through typed applicable
  measures and certified bounds.

Option 2 has separate source-hygiene issues. The leading `bwq` in its title
appears to be an editing artifact. Its implementation comparisons describe the
current runtime as work-stealing and repeat an incorrect climate-before-geology
dependency order; the repository instead uses the three-lane `LaneExecutor` and
the declared Terrain/Geology/Drainage/Climate dependency graph. Its
"GPU-parallel" realization statement should be read as a possible future
optimization, not as reusable current authority: current canonical generation
is CPU-side and GPU work is derived presentation.

The central substantive contrast remains current: Option 3 uses a fixed
statistical family and Fisher geometry for navigation, with optimal transport
confined to presentation continuity; Option 4 makes typed measures and programs
the state and uses transport plus rewrites to choose and explain paths through
Possibility.

## Recommendation and decision gates

### Recommended default

Adopt **Option 1 as the implementation baseline**, conditional on two early
spikes:

1. benchmark cold snapshot creation and sustained Egress on native and wasm,
   including sea level, climate, metric probes, visible-tile regeneration, and
   cache/memory plateaus; and
2. run blinded Yearning/Scope playtests against actual realized prevalence,
   including rare traits, conflicts, Hold, barren regions, and topology changes.

Useful ideas from the other proposals can improve an Option 1 successor without
adopting their entire ontology: Option 4's precisely specified activation-time
Hold semantics (which would also clarify Option 1's `z_0` lifetime), absolute
one-sided Scope, shared innovation/version separation, interval/`Unresolved`
discipline, and explicit transition events are particularly valuable. Doing so
would still require a new coherent design and appropriate versioning; it should
not be described as implementing Option 1 unchanged.

### When to choose Option 4 instead

Choose Option 4 only if open-ended causal regimes, relationship topology, and
explainable structural change are the product's defining durable advantage.
Even then, commit first only to Stage 0A and 0B. Continue to the planetary
family only if both of these hold:

- at least 99% of the frozen ordinary-intent corpus returns a complete default
  within the native/wasm interaction budgets; and
- blinded players reliably identify the requested direction while rating the
  non-obvious consequences coherent, with no visible global reload.

Failure of either gate should select the cheaper baseline rather than defer the
question to later stages.

### Role of Options 2 and 3

Use Option 2 only as a cheap direct-steering experiment after fixing its numeric
coordinate, canonical reduction, Hold, and continuity-state contracts. Do not
build the final world model around the current `Xi` wake.

Use Option 3 as an R&D branch if the project wants to test Fisher geometry,
convex mean-space steering, or living-mass transport. It should not become the
core until the law-to-Realization calibration, common innovation coupling,
mean/natural projection types, Hold/Repress semantics, multimodal Attractors,
and Canonical navigation enclosure are repaired.

### Common evidence required before final selection

Whichever option proceeds must demonstrate:

1. bit-identical Canonical addresses, Egress commits, fields, topology, and
   entities on native and wasm, including adversarial rounding boundaries;
2. a single cold and warm performance ledger covering navigation, global work,
   visible realization, transition work, and Canonical Impression confirmation;
3. fixed memory under long, fast, turning, and revisiting travel with multiple
   simultaneous Yearnings and pervasive Scope;
4. schedule, cancellation, cache-capacity, resource-tier, and frame-subdivision
   independence;
5. novice, intermediate, and expert blind playtests of Accentuate, Repress,
   activation-time Hold, Disable, conflict, and Scope monotonicity;
6. a held-out world-quality corpus measuring diversity, repeated motifs,
   conservation/plausibility failures, ecological structure, and nearby-world
   correspondence; and
7. visible continuity tests across coordinate quanta, river/coast topology,
   species birth/death/split/merge, cache eviction, and unfinished transitions.

Until that evidence exists, the most defensible decision is Option 1 for a
production-oriented path and Option 4 only for a separately kill-gated research
path.
