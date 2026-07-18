# The World Loom Explained for Software Developers

This document explains [New World Model Option 4: The World Loom](new-world-model-option-4.md) for experienced software developers without assuming a background in advanced mathematics.

Option 4 models a world as a small, deterministic, strongly typed "world program," then derives an entire planet from it.

Instead of representing Possibility as something like:

```rust
struct World {
    wetness: f32,
    ruggedness: f32,
    biodiversity: f32,
    // 20–40 fixed knobs
}
```

it proposes something closer to:

```rust
struct WorldConstitution {
    causal_program: TypedGraph,
    inventories: TypedMeasures,
    relationships: TypedCouplings,
    multiscale_parameters: SparsePatches,
}
```

That difference drives almost everything in the proposal.

## The core mental model

The proposal divides world generation into three parts:

- **Constitution:** the declarative specification of the world's laws, inventories, distributions, and relationships.
- **Thread:** a deterministic, addressable source of procedural randomness shared across worlds.
- **Weave:** the complete planet produced by solving the constitution against that thread.

A useful software analogy is:

```text
constitution ≈ immutable typed configuration + DSL program
thread       ≈ deterministic counter-based input data
weave        ≈ reproducible materialized view
```

The materialized view is lazy: the engine computes only the locations and detail levels requested. Logically, however, one constitution denotes one entire planet.

The constitution never contains cached tiles, individual simulated animals, transition history, or renderer state.

## Why not use a vector of world parameters?

Options 1–3 define a fixed global vocabulary: perhaps 32 or 48 numeric coordinates whose meaning is decided when the Model is designed.

That is simple and compact, but it permanently limits what kinds of changes are natural. You can turn biodiversity up or humidity down, but it becomes awkward to express:

- introducing a new ecological role;
- splitting one ecological guild into two;
- adding a methane cycle;
- changing from one ocean-circulation regime to another;
- changing relationships in a food web; or
- preserving water or energy budgets while those changes happen.

Option 4 makes the vocabulary extensible. Its state can change both numerical values and program structure.

The cost is substantial complexity. A state might be up to 64 KiB rather than tens of scalars, and the runtime needs transport solvers, constraint checkers, transition matching, generated operator packages, and explicit error certificates.

## A typed causal constitution

The constitution is a directed graph written in a restricted declarative language.

Its nodes represent things such as:

- inventories: stocks of water, atmospheric nitrogen, or crustal material;
- constitutive laws: permeability as a function of rock and porosity;
- equilibrium solvers: climate or ecology balance;
- topology extraction: rivers, coastlines, or species modes;
- forcing: seasons, tides, or illumination;
- observations: aridity, body plans, or habitat connectivity; and
- refinement: how coarse and fine representations relate.

"Typed" means more than Rust-style nominal types. Ports carry dimensions and semantic meanings. The compiler should reject operations such as:

```text
water mass + habitat suitability
```

It should also reject a food-web flow whose output energy exceeds available supply.

This resembles a combination of:

- a typed dataflow DSL;
- a build graph;
- a constraint model; and
- a database schema with strong invariants.

The graph must be acyclic between solver blocks. A block may internally solve simultaneous equations, but undeclared feedback cycles are rejected.

It is not arbitrary scripting: there are no loops, I/O, clocks, native calls, or unbounded recursion.

## Canonicalization is fundamental

Equivalent inputs must normalize to exactly the same bytes. Normalization includes operations such as:

- sorting commutative inputs;
- combining identical terms;
- removing optional nodes whose mass is zero; and
- applying a fixed set of semantics-preserving rewrites.

Consequently:

```text
state identity = canonical constitution bytes
```

State identity is not whatever result a floating-point solver happened to produce.

This is analogous to canonical serialization of an AST, except the rewrite system must be terminating and confluent:

- **Terminating:** normalization always finishes.
- **Confluent:** different valid rewrite orders lead to the same normal form.

If those properties cannot be established for some grammar fragment, the design refuses to pretend those programs are equivalent.

## Measures: distributions with real meanings

Much of the source document uses the word *measure*. For practical understanding, think:

> A measure is a weighted distribution whose total weight has a declared meaning.

For example:

```text
body-plan distribution:
    glider         0.10
    climber        0.25
    ground-runner  0.65
```

Totals are not always probabilities. They may represent:

- a fraction;
- kilograms of inventory;
- energy per unit time;
- expected abundance; or
- carrying capacity.

The type system keeps those meanings distinct.

Relationships are distributions too. A food web is not merely a vector saying how many producers and consumers exist. It includes weighted edges:

```text
consumer trait × resource trait → energy flow
```

This lets a Yearning alter relational structure, not just scalar values.

## Multiscale consistency

The constitution describes values at multiple levels:

```text
planet
  └── coarse regions
       └── finer regions
            └── tiles
```

Every fine level must preserve the promises made by its parent. If a parent declares 1,000 units of water, its children must sum to exactly 1,000. If a parent reports a boundary flux, its children must reproduce that flux.

The inverse-limit notation in the source document essentially means:

> A theoretically complete state is an infinitely refinable hierarchy in which every level agrees with the coarser level above it.

The actual packet is finite. It stores sparse deviations over a deterministic procedural default, with hard limits such as:

- 64 KiB;
- 4,096 patch entries;
- 16 active levels; and
- fixed-point mass values.

This is much like a sparse overlay or a Git tree: the package supplies defaults, while a state records only meaningful differences.

## State packet, address, and envelope

The proposal carefully separates three concepts:

- `StatePacket`: the complete normalized state value.
- `StateAddress`: the Model generation plus the packet's Merkle root.
- `StateEnvelope`: the address plus the chunks needed to decode the packet.

A root is only an identifier. It is not magically self-decoding.

The neutral runtime therefore never responds to a missing chunk by fetching it from some hidden server. The caller must explicitly supply the data or an explicit bounded `ChunkSource`. Otherwise the result is a deterministic missing-data error or continuation.

This preserves offline reproducibility.

## The shared innovation thread

Normally, procedural worlds are seeded by their complete world state. Changing one parameter can then replace all random details, causing unrelated terrain and organisms to jump.

Option 4 instead uses one shared counter-addressed innovation field:

```text
random_value(channel, scale, spatial_address, ordinal)
```

The value depends on stable integer inputs, not evaluation order or mutable RNG state.

Different constitutions transform the same underlying innovation. Changing rainfall might therefore make the same underlying terrain pattern wetter rather than replacing it with unrelated noise.

This resembles using a stable deterministic fixture across A/B configurations. It makes changes attributable and helps establish correspondence between neighboring worlds.

## Navigating Possibility means transporting distributions

Suppose a world has:

```text
10% gliding organisms
90% non-gliding organisms
```

and the player requests pervasive gliding.

A scalar system might simply increase a `gliding` number. The Loom instead asks how trait and habitat mass can move:

- transform existing body plans;
- create new ecological roles;
- expand cliff habitats;
- create taller forest canopies;
- change atmospheric density; or
- introduce or remove lineages.

The transport cost says which changes are semantically near or far.

### Wasserstein transport

For conserved quantities, Wasserstein transport is roughly the cheapest way to redistribute mass.

A developer-oriented analogy is a min-cost flow problem:

```text
source distribution → destination distribution
```

Moving weight between similar traits is cheap; moving it between very different traits is expensive.

### Unbalanced transport

Some things can be created or destroyed. Hellinger–Kantorovich/Wasserstein–Fisher–Rao transport extends the cost model with two operations:

- move or transform mass; and
- create or remove mass.

The parameter κ controls how expensive creation or destruction is relative to transformation.

Typed conservation still wins. The birth/death mechanism cannot delete an inventory declared exactly conserved.

## Structural changes are grammar rewrites

Some world changes cannot be expressed by moving numeric mass. The program itself must change.

Examples include:

- splitting one ecological guild into two;
- adding a methane cycle; or
- replacing a single-ocean circulation block with a two-basin model.

These are licensed grammar rewrites with:

- preconditions;
- a defined embedding;
- a path cost; and
- possibly an inverse.

New structure begins with zero mass and grows continuously. Existing structure must reach zero mass before being removed. This avoids teleporting between unrelated program structures.

If the runtime cannot find a path within its bounded rewrite search, the result is not "impossible." It is:

```rust
Unresolved(SearchHorizonExhausted)
```

Only a genuine proof that the grammar components are disconnected may return `ProvenUnreachable`.

That distinction is one of the proposal's strongest design principles.

## Egress planning as constrained optimization

A Yearning describes desired observations, not implementation knobs.

For example:

```text
Make gliding pervasive.
Hold the current river topology.
Repress arid habitats.
```

The planner searches for a short, feasible path through constitutions whose endpoint best satisfies those requests.

Conceptually, it minimizes:

```text
cost of changing the world
+ dissatisfaction with Yearnings
- attraction toward community Attractors
```

subject to:

- conservation laws;
- valid intermediate states;
- licensed rewrites;
- packet-size limits;
- a maximum path length; and
- certified solver error.

The path cost prevents enormous world changes merely because they satisfy an intent slightly better.

The runtime searches only a bounded number of modes—initially eight modes and at most two rewrites ahead. It returns:

- one deterministic default;
- up to two structurally distinct alternatives; or
- `Partial` or `Unresolved` when it cannot prove the ranking.

This makes alternative causal explanations a gameplay feature. "More gliding" might produce a canopy world, a cliff world, or a dense-atmosphere world.

## Scope is prevalence, not geographic radius

Scope answers:

> In what fraction of applicable cases should this trait be present?

It does not mean "within 10 km of where the organism was captured."

Examples include:

- singular: around one in ten thousand applicable cases;
- common: an intermediate share; and
- pervasive: approaching 85%.

The denominator is defined by each observable:

- percentage of species;
- percentage of biomass;
- percentage of land area; or
- percentage of applicable habitats.

Those are intentionally not interchangeable.

Accentuate and Repress are monotonic:

- Accentuate never lowers an already-high prevalence.
- Repress never raises an already-low prevalence.
- Hold compares against the world where Hold was activated, not the moving current state.

Yearnings are normalized hierarchically so that one Yearning containing ten captured attributes does not automatically outweigh another containing one. Inputs are sorted and reduced canonically, making results independent of insertion order.

## Physical travel powers world change

World-space travel and Possibility-space travel remain separate metrics:

- **World Space:** meters traveled over, on, or above the planet.
- **Possibility:** semantic length of the constitution change.

The Traveler/controller converts physical path length into credit along the selected Egress path:

```text
possibility_credit =
    physical_arclength
    × conversion_rate
    × resonance
    × optional local opportunity
```

Important details:

- It uses actual path arclength, not endpoint displacement.
- Camera movement does not count.
- Standing still gives zero Egress.
- Frame subdivision does not change the result.
- Credit belongs to one exact selected plan.
- Changing the intent or plan clears leftover credit.

The Model does not read input, velocity, clocks, or renderer state. The controller owns this coupling.

## Resonance in plain language

Resonance estimates how workable the requested change is within the current bounded planning horizon.

It combines four factors:

- **Fit:** how much of the requested improvement survives real physical constraints compared with a relaxed fantasy solve.
- **Conditioning:** whether the requested observations are controllable without extreme sensitivity.
- **Work:** how long the valid route is.
- **Safety:** how risky the route is near topology changes such as ocean mergers or habitat collapse.

The geometric mean combines them into one rate factor.

Resonance affects how quickly travel advances the selected plan. It does not redefine what is reachable.

A low result means "this bounded, certified route is difficult or unresolved," not "mathematically impossible forever."

## Realization is constraint solving, not ordinary noise layering

The complete planet is generated by solving coupled systems for:

- terrain and material;
- ocean level;
- hydrology;
- climate;
- soils and productivity;
- biome distributions; and
- ecology and food webs.

The design prefers problems with unique solutions, especially convex optimization. In software terms, uniqueness is important because it eliminates dependence on initialization, thread scheduling, and solver history.

Where a subsystem is genuinely nonconvex, the Model must:

1. enumerate a bounded candidate set;
2. solve each candidate deterministically;
3. choose using certified objective bounds; and
4. return `Unresolved` if candidates cannot be distinguished.

It must not disguise a heuristic result as canonical merely because it is repeatable.

## Projective refinement

This is one of the hardest but most important ideas.

Ordinarily, a coarse simulation can later be replaced by a finer solve that disagrees with it. A river may move or an ocean summary may change after refinement.

The Loom instead makes the hierarchy authoritative:

```text
solve coarse level
then solve fine level subject to:
    restrict(fine) == coarse promises
```

The fine result must preserve declared parent values, inventories, boundary flows, and summaries.

This is not a claim that real continuum physics behaves this way. It deliberately defines a synthetic hierarchical physics Model in which refinement cannot contradict already-canonical coarse facts.

For outputs that cannot support exact preservation, the coarse query must return an interval. Refinement can narrow that interval rather than silently changing a supposedly exact value.

## Local-to-global consistency and sheaves

The sheaf terminology can be understood as a strongly checked tiling protocol.

Every tile exposes boundary summaries. Neighboring tiles must agree when their data are restricted to the shared boundary:

```text
tile A east boundary == tile B west boundary
```

The glue residual measures disagreement across all those overlaps.

A zero glue residual means the pieces fit together. It does not prove that each tile's internal physics is correct; each local solver needs a separate certificate for that.

This gives clean cancellation behavior: a partially computed tile cannot become authoritative because it does not yet form part of a valid global result.

## Transition plans

A new world state does not merely replace the old one. The Model generates an explicit semantic diff:

- which features persist;
- which species correspond;
- which entities are born or die;
- which entities split or merge;
- where topology changes occur; and
- how confident each match is.

The engine uses bounded exact min-cost-flow matching for authoritative correspondence. Heuristics may propose candidates, but they cannot mint canonical matches.

This distinction is crucial:

```text
entity identity belongs to an endpoint world
correspondence belongs to a named transition
```

A species in world B is not literally the same entity as a species in world A. The Transition Plan provides evidence that one succeeds, splits from, or corresponds to the other.

The renderer may use that plan to stage succession, migration, morphing, births, or deaths without a global reload.

## Canonical world versus presentation history

At every moment there is exactly one authoritative State Packet.

The renderer may retain old meshes or animals locally to make transitions look smooth, but that retained state is presentation history, not a regional alternative world.

Therefore:

- revisiting after cache eviction may look somewhat different;
- different Visualizations may conceal transitions differently;
- canonical queries and captures must still refer to the current packet; and
- old blended entities cannot be captured as if they were current canonical entities.

Exact presentation replay requires a separate `VisualizationReplayBundle` containing the simulator version, assets, seed, parameters, event log, and simulation state.

## Canonical, Interactive, and Preview grades

Queries have three accuracy levels:

- **Preview:** fast approximation, conservative error bounds, and no portable identities.
- **Interactive:** sufficient for display or gameplay tolerance; threshold-crossing classifications remain unresolved.
- **Canonical:** fixed operation order, fixed-point inputs, deterministic tables, certified convergence, and exact tie rules.

Only Canonical results can confirm portable Impressions.

This separates "good enough to render" from "safe to persist as world identity."

## `Complete`, `Partial`, and `Unresolved`

The API makes bounded computation explicit:

```rust
enum QueryStatus<T> {
    Complete(T),
    Partial {
        value: T,
        continuation: ContinuationToken,
    },
    Unresolved {
        reason: UnresolvedReason,
        continuation: Option<ContinuationToken>,
    },
}
```

This is not ordinary error handling.

- `Complete`: the requested claim was certified.
- `Partial`: a valid prefix or page is available and can be continued deterministically.
- `Unresolved`: the bounded computation cannot currently justify an answer.
- `ModelError`: corrupt input, missing required data, an unsupported version, or a broken integrity contract.

A central rule is:

> Never convert resource exhaustion or numerical ambiguity into a guessed canonical result.

The product risk is obvious: if ordinary gameplay frequently returns `Unresolved`, the design is unusable. That is why the proposal requires at least 99% of normal Egress probes to complete within the interaction latency budget.

## Determinism and versioning

The proposal distinguishes several identities:

- `GenerationId`: the meaning of the world Model.
- `PackageId`: compatible optional extensions.
- `ArtifactId`: the exact compiled package bytes.
- per-channel revisions: procedural innovation semantics.
- packet, schema, simulation, and Build versions.

Improving compiler metadata or certificates must not reidentify every world if semantics remain unchanged. Conversely, any change that alters canonical output must change the appropriate semantic identity.

Canonical behavior must be independent of:

- thread count;
- scheduling;
- cancellation;
- cache state;
- resource tier;
- scalar versus SIMD execution; and
- native versus wasm execution.

The CPU is authoritative. GPU work is presentation-only.

## What the offline compiler does

The proposed runtime package is immutable data interpreted by a bounded neutral kernel. It does not contain arbitrary dynamically loaded code and performs no machine learning.

An offline compiler may generate:

- sparse operators;
- restriction maps;
- reduced bases;
- fixed-point ranges;
- interval and error bounds;
- optimized expression forms;
- parity fixtures; and
- optional ahead-of-time scalar and SIMD kernels.

Think of this as shader compilation plus schema compilation plus numerical code generation, with a much stronger verification pipeline.

Generated output is not trusted automatically. Independent checkers validate it before release.

## The proposed planet and ecology

The reference family uses a subdivided icosahedron—20 triangular base faces refined recursively—rather than an infinite plane.

One state is therefore one finite planet. "Infinite exploration" moves into Possibility: there is an enormous space of possible planets, rather than one spatially infinite planet.

Its ecology represents distributions over traits and trophic relationships. Species are stable modes or clusters found in that global trait distribution, not a mutable list of individually simulated lineages.

A canonical organism is a deterministic representative sample. Its live pose, behavior, and location belong to the Visualization simulation.

## What remains from the current prototype

The proposal preserves several engineering principles already used by the repository:

- integer-derived permanent identity;
- explicit versioning;
- dependency-keyed derived data;
- schedule and cache independence;
- native/wasm parity;
- neutral runtime crates;
- CPU-authoritative gameplay;
- GPU-only derived presentation; and
- sparse persisted records.

It does not extend the current regional possibility vector. This would be a clean-slate Model major, likely coexisting with the current implementation during development.

Old captures could be migrated only as attribute-by-value observations used to search for a similar Loom state. They would not retain exact world identity.

## The real tradeoff

Option 4 is betting that the project's defining feature should be:

> Worlds can change their causal and relational structure in rich, explainable ways.

Its advantages are:

- an extensible world vocabulary;
- native conservation and units;
- meaningful birth, death, split, and merge operations;
- explicit cross-world correspondence;
- coherent coarse-to-fine refinement;
- clearer failure semantics; and
- world changes driven by desired observations rather than exposed generator knobs.

Its disadvantages are:

- much larger state packets;
- very difficult solver and compiler work;
- heavy certificate infrastructure;
- bounded route search that can miss real paths;
- risk of combinatorial explosion;
- risk that strict uniqueness makes worlds sterile;
- risk that frequent `Unresolved` results make the game frustrating; and
- no evidence yet that the architecture is performant or fun.

The proposal is unusually honest about this: it is a staged research design, not implemented behavior or a claim that all the mathematics will compose successfully.

## The implementation strategy

The stages are deliberately kill-gated:

1. Build only packets, typed distributions, one rewrite, transport, and certificates.
2. Build a tiny 20-face playable world and test whether players understand and enjoy its changes.
3. Add the projective planet and primitive fields.
4. Add inventories, ocean, hydrology, and transition matching.
5. Add climate, soil, ecology, and species modes.
6. Add full navigation, records, Impressions, Resonance, and Attractors.
7. Integrate with native and browser hosts.

The most important point is that the fun test happens before expensive climate and ecology work. Failure is intended to push the project toward a simpler option.

## Summary

In one sentence:

> World Loom treats each possible world as a canonical, typed, content-addressed program; navigation edits that program through constrained transport and licensed rewrites, while deterministic multiscale solvers materialize one coherent planet and explicit Transition Plans explain how the old world corresponds to the new one.
