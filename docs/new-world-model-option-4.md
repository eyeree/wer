# New World Model Option 3: The World Loom

## Status and purpose

This document proposes a third complete Model for
[`conceptual-model.md`](conceptual-model.md). It is a design, not a description of
landed behavior. The implemented prototype remains documented in
[`world-model.md`](world-model.md).

[Option 1](new-world-model-option-1.md) and
[Option 2](new-world-model-option-2.md) are variations on one strong idea: a small global latent vector
is smoothly decoded into procedural parameters, observable derivatives induce a
Riemannian metric, and a gradient-like solve moves the Traveler through that
manifold. This proposal deliberately explores a different family.

> **Central departure.** A point in Possibility is a normalized, typed
> **causal constitution**: a content-addressed program plus compatible measures
> of material, process, and ecological traits at every modeled scale. Possibility
> is a glued, scale-extensible typed space of those constitutions, not a fixed
> 32- or 48-dimensional latent manifold. Egress transports one constitution into
> another. Realization solves the constitution's local-to-global constraints
> against one shared, addressable source of procedural innovation. The transport
> solve also supplies explicit correspondence between successive worlds.

The proposal is called **The World Loom** because it separates three things:

- the **constitution**, which says what kinds of processes and distributions a
  complete world has;
- the **thread**, a fixed counter-addressed innovation field shared by every
  world in one Model family; and
- the **weave**, the unique, lazily queried Realization obtained by reconciling
  the constitution and thread on a planetary cell complex.

This design intentionally spends more complexity on authoring and verification
than Options 1 and 2. That is a feature, not an accidental cost. A versioned
Model package may contain thousands of generated restriction maps, reduced
bases, sparse operators, interval bounds, and conformance fixtures. Agents can
build and maintain that artifact offline. The runtime remains finite,
deterministic, inspectable, and independent of any online model or service.

This is a staged research architecture, not a claim that its most ambitious
pieces already compose into a production system. The typed packet, measure,
transport, and bounded-query kernel is intended to be directly implementable.
The projective planetary solver, certified ecological modes, and transition
correspondence machinery must earn their place at explicit gates; the design
requires an `Unresolved` result wherever those gates cannot yet be met.

The main claims of the proposal are:

1. one canonical constitution denotes one complete planet;
2. measure transport expresses prevalence, birth, death, split, and merger
   without forcing all change through a smooth Euclidean chart;
3. typed conservation laws and certified residuals give a stronger notion of
   plausibility than bounded generator knobs;
4. projective refinement makes declared coarse summaries and their fine queries
   agree by construction, while other outputs carry narrowing intervals;
5. a transition is a first-class correspondence plan rather than only a blend
   amount; and
6. the shipped Model can be much too large and intricate for a person to tune
   manually while remaining tractable for automated maintenance.

## 1. System overview

One Model family provides a public family key `K`, a versioned causal grammar,
motif dictionaries, constraint operators, and a compiler manifest. A Model State
selects and weights pieces of that grammar at every scale. It never contains
resident tiles, simulated organisms, cache state, or transition history.

The principal flow is:

```text
Impressions + Influence + Scope + weight
                 |
                 v
       canonical intent measures
                 |
                 v
  constrained minimum-length transport  <---- Attractor distribution
                 |
                 +----> Resonance + alternate path modes
                 |
                 v
      continuous constitution path
                 |
       Traveler supplies path-length budget
                 v
       new canonical State Packet
                 |
     +-----------+------------------+
     |                              |
     v                              v
fixed innovation thread       Transition Plan
     |                    (matches/births/deaths/risk)
     v                              |
typed multiscale constraints        v
     |                         Visualization continuity
     v
certified, lazy Realization
```

Egress planning is a pure Model operation. The Traveler/controller decides how
much of a returned path may be committed from physical distance traveled. A
Visualization receives immutable snapshots and transition descriptions. It may
retain and blend old presentation near the Traveler, but that history never
becomes a Model State.

## 2. The mathematical object

### 2.1 Model identity and package

A stable `GenerationId` is

$$
M_g=(\text{family},\text{major},h_{\rm core},K).
$$

A loadable `PackageId` is

$$
P=(M_g,\text{minor},h_{\rm extensions}).
$$

Here:

- `family` names the World Loom contract;
- `major` or $h_{\rm core}$ changes the meaning of an existing base address or
  canonical result;
- `minor` and $h_{\rm extensions}$ may append optional capabilities without
  changing an old address, field, counter, or id;
- $h_{\rm core}$ identifies every base grammar rule, atom dictionary, unit,
  ground cost, solver constant, rounding point, and test vector; and
- $K$ is a public 128- or 256-bit family key used by the universal innovation
  field.

The names `GenerationId` and `PackageId` are normative API types, not two
spellings of a generic `ModelId`. The package contains no secret weights and performs no learning at runtime. It
is an immutable compilation product. Package equality requires identity and
canonical manifest bytes; a digest is an index, not a magical substitute for
the data it names. A State Packet records $M_g$ plus only those extension ids it
actually uses. Installing an unused optional extension cannot change its root.

Every primitive has a typed domain tag. A permanent hash or random counter folds
the stable domain identity

$$
(\text{family},\text{major},K,\text{semantic channel id},
\text{channel algorithm revision}),
$$

plus scale, canonical spatial address, and ordinal in a specified order. It does
not blindly fold `minor` or the extension-package digest. Floating-point bits
are never sources of identity.

### 2.2 A typed causal constitution

A constitution has a normalized typed program $G$ and numeric laws $q$:

$$
\omega=(G,q).
$$

$G$ is a directed hypergraph in a small declarative language. Its ports carry
quantities with explicit dimensions and semantic kinds. Initial node families
are:

| Node family | Meaning | Example |
|---|---|---|
| `Inventory` | a conserved or bounded stock | water, atmospheric nitrogen, crustal mass |
| `Constitutive` | a local relation | permeability from lithology and porosity |
| `Variational` | a unique equilibrium or constrained optimum | climate balance, trait assembly |
| `Topology` | an integer active-set extraction with margins | river support, species mode, coastline |
| `Forcing` | canonical time-dependent boundary data | insolation, tide phase, seasonal envelope |
| `Observation` | a versioned canonical attribute | aridity, body plan, habitat connectivity |
| `Refinement` | parent/child restriction and prolongation | coarse flux to child boundary traces |

Feedback such as soil-productivity coupling is represented as one declared
solver block. The graph of blocks is acyclic even when equations inside a block
are simultaneous. Each solver block declares whether it is strongly convex,
strongly monotone with an existence/coercivity witness, contractive on a complete
declared domain, or an explicitly finite active-set search. Mere monotonicity is
not a uniqueness claim. An undeclared cycle is a compile error.

“Causal” has a limited, testable meaning here. Replacing an input law or
inventory is an intervention: only downstream blocks in the condensation DAG
may change. A simultaneous block is one constitutive macro-node; the Model makes
no causal-direction claim among variables inside it. The program is a
generative process model, not an empirically identified structural-causal model
of a real planet.

Program normalization sorts commutative ports, combines equal terms, removes
zero-mass optional nodes, and applies only versioned semantics-preserving
rewrites. Thus a constitution has one canonical byte representation. Distinct
canonical programs are distinct Model States even if a finite set of
observations happens to be equal.

This is not arbitrary user code. There are no loops, clocks, I/O operations,
native calls, dynamic allocation policies, or unbounded recursion in the
language. Every node lowers to a bounded neutral-core kernel plus a dependency
declaration.

### 2.3 Laws as compatible measures

For each type $c$ and scale $\ell$, the package defines a compact motif space
$A_{c,\ell}$. Examples include lithology mixtures, storm-track modes, leaf
strategies, body plans, trophic roles, and disturbance regimes. A world assigns
a finite positive measure

$$
\mu_{c,\ell}\in\mathcal M_+(A_{c,\ell})
$$

and a vector of constitutive coefficients $a_{c,\ell}$. Mass has a declared
meaning: fraction, inventory, capacity, expected abundance, or rate budget. It
is never an untyped probability merely because a solver wants one.

Relation nodes carry coupling measures. For example, a physical feeding-flow
relation may have

$$
\phi_{\rm feed}\in
\mathcal M_+(A_{\rm consumer}\times A_{\rm resource}),
$$

with energy-per-time units. Its resource projection is bounded by typed
available-energy supply, its consumer projection by typed intake capacity, and
declared conversion/respiration/detritus slack closes the balance. It is not
required to have two differently unitized biomass and energy budgets as literal
marginals; any normalized role-correspondence coupling is a separate law. A
climate relation may couple heat-source, moisture-carrier, and seasonal-mode
measures under analogous typed balance maps. These couplings let a state
describe relational structure, not only a bag of scalar prevalence values.

State measures are **laws, priors, capacities, and inventories**. They are not
automatically the realized population measures shown to a player. The ecology
block, for example, derives $\nu_x^*$ and a food-web flow from state priors and
budgets. Canonical prevalence integrates those derived outputs. Egress transports
the constitutive measures; a Yearning on derived prevalence is differentiated or
bounded through the realization solve and is generally nonlinear. Sinkhorn is a
solver for typed transport subproblems, not a claim that the entire Model is one
linear transport problem.

Scale compatibility is explicit. Let $r_\ell$ coarsen level $\ell+1$ into
level $\ell$. A valid tower obeys

$$
r_\ell(q_{\ell+1})=q_\ell.
$$

For one fixed program $G$, theoretical states form the inverse limit

$$
\Theta_G=\varprojlim
\left(
\Theta_{G,0}\xleftarrow{r_0}\Theta_{G,1}
\xleftarrow{r_1}\Theta_{G,2}\leftarrow\cdots
\right).
$$

Intuitively, every finer law must preserve the masses, moments, and boundary
semantics already promised by its parent. Signed coefficient fields may use
zero-parent-moment lifting details. Positive measures instead store nonnegative
child allocations whose integer sum equals the parent mass; the signed
difference from a default prolongation is a derived coordinate, not a positive
measure. A positive measure is never described as a nonzero zero-mass detail.

Program topology may also change. The whole Possibility is

$$
\Theta=
\left(\coprod_{G\in\mathfrak G}\Theta_G\right)/\sim,
$$

where $\sim$ identifies normalized programs at shared zero-mass boundaries. A
new causal motif is introduced with zero mass and can then grow continuously; a
motif can disappear only after its mass reaches zero. This glues typed pieces
into a proposed **stratified state complex**. Phase changes and categorical
regimes are real parts of the topology, not singularities hidden behind a
decoder.

“Stratified” is a compiler obligation, not a consequence of the quotient
notation. The compiler refines program cells by measure support, factor rank,
cone face, and active set, then must establish the declared local cone/manifold
models, frontier incidence, local finiteness, and separation properties for the
bounded grammar fragment. A fragment lacking those witnesses is merely a glued
candidate quotient navigated through explicit cells; no stratified-space
theorem is invoked for it.

A rewrite may use this gluing only when the compiler proves its zero-mass
embedding is semantically neutral: every new output is gated to zero and every
old restriction map is unchanged at the boundary. Merely setting one displayed
coefficient to zero is not enough.

The normalization rewrite system must also be terminating and confluent, and
its equivalence relation must be transitive. Until the compiler verifies those
properties for a grammar fragment, its programs remain separate strata rather
than being asserted to form a quotient or moduli space.

### 2.4 Possibility Coordinate and Model State

The canonical **State Packet** is the Possibility Coordinate. It contains:

```text
stable generation identity, core manifest digest, and sorted used extension ids
normalized program/rewrite signature
sorted fixed-point measure atoms and relation couplings
sorted fixed-point constitutive coefficient patches
procedural-tail rule and packet-format version
```

The initial implementation bounds a packet to 64 KiB, at most 4,096 explicit
patch entries, at most 16 active law levels, and 24-fractional-bit nonnegative
mass quanta. A fractional mass is stored in a `u32` whose valid range is
`0..=2^24`, representing $[0,1]$; larger typed inventories declare a separate
scaled integer format rather than overloading that fraction encoding.
Each declared inventory has an exact integer total. A canonical entropy coding
may shrink the record, but equality is defined on the decoded normalized packet.

Measure patches are absolute nonnegative child allocations. Coefficient patches
are signed deltas. A relation law declares one canonical representable form:
bounded sparse support or bounded-rank nonnegative factors, plus exact
correction for its declared marginal/submarginal/slack balances. The total
4,096-entry ceiling covers atoms, relation support/factors, and coefficients.
Support/rank selection is a finite active-set problem; a dense
entropic transport plan is a transition sidecar, not automatically a valid
endpoint packet.

The packet is a finite sparse patch over a versioned procedural tail. The tail
is a fixed part of the Model package, so omitting a fine coefficient never means
“whatever happens to be in cache.” Removing all finite complexity ceilings would
make finite patches a natural approximation family for the theoretical inverse
limit. V1's fixed 64 KiB/4,096-entry subset is finite and is not claimed to be
dense by itself.

A 256-bit Merkle root makes structural sharing and lookup cheap. The root does
not make a state self-decoding. An Impression or atlas bundle must carry the
immutable chunks it references, directly or through a content-addressed bundle
whose availability is part of the record validation result.

The root is computed over the normalized payload and child-chunk digests,
excluding the root field itself. Packet validation recomputes it before any
lookup or identity use.

Validation and reachability certificates are sidecars excluded from packet
equality and the root. Two proof witnesses or tighter interval enclosures for
the same normalized $(G,q)$ cannot create two Possibility Coordinates. A
certificate names the State root, manifest/checker version, and claim it proves.

The **Model State** is the fully interpreted $(G,q)$ tower denoted by that
packet. Compiled operators, factorizations, tiles, and transition plans are
derived caches. There is no second hidden state.

This coordinate is deliberately larger than a latent vector. It is still tiny
relative to a planet, portable, diffable, and structurally shared. More
importantly, it does not force every future concept of geology or life into a
permanent handful of axes.

### 2.5 Theoretical, Representable, and Reachable Possibility

The three sets from the conceptual model have precise meanings:

- **Theoretical Possibility** is $\Theta$, including real-valued, infinitely
  refined compatible constitutions admitted by the grammar.
- **Representable Possibility** is the set of canonical State Packets obeying
  the finite complexity ceiling, fixed-point formats, supported grammar, and a
  valid certificate.
- **Reachable Possibility** from $q_0$ is the set of packets at the end of a
  finite certified path whose directed transport length and grammar rewrites obey the
  directional controls in Section 3 and whose hard feasibility residual is zero.

Representable coordinates form a finite quantized subset at one Model
version, while Egress maintains a continuous internal path of measures and
coefficients. Error-feedback quantization commits canonical packets without
losing sub-quantum motion.

The package must bound the largest change in every declared **continuous
channel or moment** caused by one packet quantum. That bound must fall below its
declared continuity tolerance, or the format needs more precision and the
package is unsupported. Discrete ids, active sets, and topology cannot satisfy
such a Lipschitz promise; they instead require margins, explicit events,
`Unresolved` status, and a Transition Plan. The certified continuous path
remains available between packet crossings; quantization is not permission for
a visible global step.

Sub-quantum path length is a `NavigationAccumulator` owned by the Traveler. It
is movement credit along a named Egress Plan, not a partially realized local world
or a second canonical coordinate. It belongs in an exact run-session snapshot
but not in an Impression. Until the next packet boundary is crossed, canonical
Realization remains at the current packet. Replanning transports or clears the
credit by a specified deterministic rule.

Reachability is generally directed. Adding atmospheric oxygen through a slow
biogeochemical control need not cost the same as consuming it; acquiring a
symbiosis may require an intermediate ecology even if the endpoint coefficients
look close. The symmetric neighborhood distance and directed admissible length
are therefore separate concepts.

### 2.6 World Space and canonical time

V1 World Space is an oblate planet addressed on a recursively subdivided
icosahedron. A canonical surface point is

$$
x=(f,b_0,b_1),
$$

where $f\in\{0,\ldots,19\}$ is a base face, $b_0,b_1$ are signed fixed-point
barycentric coordinates at one fixed Q2.46 precision. Signed centimetre altitude
along the reference normal completes a three-dimensional address. Tie rules on
edges and vertices choose one canonical face. A hierarchical **cell** address
has a separate level $L$; it is not another encoding of the point. The World
Space path metric is the declared product metric

$$
ds_W^2=ds_{\rm ellipsoid}^2+dh^2,
$$

where $h$ is normal altitude. Thus vertical flight is not zero-distance and
surface travel at fixed altitude reduces to the ellipsoid term. The Traveler
credits the arclength of its full canonical 3-D controller path under this
metric, never camera/View motion. This is not a distance in Possibility.

The hierarchy supplies stable cell, edge, and vertex addresses for refinement
and discrete calculus. World Space is finite in extent but resolution is
scale-extensible. A planar development Model could implement the same contract,
but the first World Loom target should be planetary so astronomy, seams, and
global inventories are not deferred architectural problems.

Canonical model time is an integer number of SI seconds from a versioned epoch.
The Model exposes orbital, lunar, tidal, illumination, and seasonal forcing, as
well as time-independent geological age labels and initial-condition envelopes.
It does not advance weather, move an animal, grow a plant, or mutate simulation
state. Those operations belong to the Visualization.

An Impression includes canonical time only when its subject or attribute depends
on forcing phase. It may separately include or reference a self-contained
`VisualizationReplayBundle` carrying the complete tuple in Section 6.4. A
simulation timestamp by itself never claims exact presentation replay.

View Space is entirely a Visualization concept: cameras, projections, screen
coordinates, and display transforms never enter a State Packet or World Space
address. The Traveler may therefore move independently along three axes:
Exploration in World Space, Egress in Possibility, and temporal movement through
canonical forcing and/or Visualization simulation time.

## 3. Geometry and paths in Possibility

### 3.1 Transport distance inside a stratum

Changing a distribution is different from changing a scalar. Moving trait mass
from “small nocturnal grazer” to “large diurnal grazer” should cost according to
the semantic distance between those traits. Creating or eliminating a lineage
should also be possible, but not free.

For conserved measures, the Model uses quadratic Wasserstein transport. For
mass that may appear or disappear, it uses a versioned Hellinger--Kantorovich /
Wasserstein--Fisher--Rao ground metric. On a smooth Riemannian motif space, one
dynamic form is

$$
HK_\kappa^2(\mu_0,\mu_1)=
\inf_{\rho,v,\alpha}
\int_0^1\!\int
\left(\|v\|_c^2+\kappa_c^2\alpha^2\right)d\rho_t\,dt
$$

subject to

$$
\partial_t\rho+\nabla\!\cdot(\rho v)=\alpha\rho,
\qquad \rho_0=\mu_0,\quad \rho_1=\mu_1.
$$

$v$ transports mass through a motif space; $\alpha$ creates or removes it.
$\kappa_c>0$ states how expensive birth/death is relative to transformation. The
ground cost $\|\cdot\|_c$ is part of the manifest and is expressed in canonical
observable units, not learned from a player's current Yearnings.
HK/WFR normalizations differ in the literature; the displayed dynamic energy and its
$\kappa_c$ convention are part of the versioned manifest.

Not every compact motif space has a velocity or divergence. A finite/categorical
runtime block instead declares either a graph incidence operator and discrete
continuity equation, or a static finite Kantorovich/entropy-transport cost table
with the same creation penalty. Its integer cost matrix and normalization are
canonical data; the continuum PDE is motivation, not an undefined operation on
category labels.

For two constitutions in one stratum,

$$
D_G(q,p)^2=
\sum_{\ell=0}^{\infty}2^{-2s\ell}
\left[
(\sum_c \lambda_c
\mathcal T_c(\mu_{c,\ell}^q,\mu_{c,\ell}^p)^2)
+
(a_{\ell}^q-a_{\ell}^p)^T
R_{\ell}(a_{\ell}^q-a_{\ell}^p)
\right].
$$

$\mathcal T_c$ is $W_2$ for an exactly conserved measure and the declared
unbalanced $HK_{\kappa_c}$ metric for a measure whose typed laws admit
creation/destruction. An exact inventory cannot disappear through a convenient
unbalanced penalty. The coefficient quadratic is also summed over its declared
channels; the abbreviated display assumes $a_\ell$ and $R_\ell$ are the
concatenated block vector and SPD block matrix. Every $\lambda_c$ is positive,
motif ground spaces are metrics after their declared semantic quotient, and
$R_\ell$ is SPD. If a schema deliberately identifies distinct atoms, the result
is named a pseudometric rather than silently called a metric.

This is also a unit contract. $\lambda_c$ and the entries of $R_\ell$
nondimensionalize heterogeneous channel costs into one declared squared
Possibility-length unit $L_P^2$ before they are added. A manifest that adds
meters, kilograms, and trait dissimilarity as bare numbers is ill typed.

$s>0$ alone does not prove convergence. The manifest supplies a level bound
$B_\ell$ for the bracket, including growth in atom and coefficient dimension,
and must prove

$$
\sum_{\ell\ge0}2^{-2s\ell}B_\ell<\infty.
$$

For example, $B_\ell\le C2^{d\ell}$ requires $s>d/2$. Within that contract,
coarse continental or biospheric changes dominate tiny texture changes, while
no finite scale is declared semantically nonexistent.

Relation couplings may use a manifest-defined fused pairwise transport cost: one
term compares node attributes and another compares relation-graph structure.
This lets two food webs be close when roles correspond despite different exact
species counts. A higher-order hyperedge term requires its own explicit bounded
loss and checker; pairwise fused transport does not imply one.
Such a relation cost enters the symmetric neighborhood metric only when the
compiler verifies its metric/pseudometric conditions on the declared quotient;
otherwise it is labeled a transition or control penalty, not folded into $D_G$.

The solver returns the transport plans $\pi_{c,\ell}$, not only the scalar
distance. They are reusable semantic correspondences.

The dynamic optimal-transport displays define a **quadratic energy**. Egress
budgeting instead needs an additive prefix coordinate. For each path mode the
manifest therefore defines a positively one-homogeneous, possibly asymmetric
control norm $F_m(q,\dot q)$ and directed length

$$
\mathscr L_m^\rightarrow[\Gamma]
=\int_0^1 F_m(\Gamma(t),\dot\Gamma(t))\,dt
+\sum_{r\in\Gamma}\ell_r.
$$

Licensed rewrites have additive lengths $\ell_r$. In a symmetric stratum the
compiler must choose $F_m$ as the metric derivative/Finsler norm induced by the
displayed $W_2$/HK/coefficient product action, so $F_m^2$ is its action density;
only under that obligation does minimizing length give $D_G$ and minimizing the
corresponding fixed-time energy give $D_G^2$. Asymmetric modes may add directed
control constraints and costs. Length is invariant under monotone
reparameterization and additive under concatenation. It—not squared energy—is
the Egress horizon and `NavigationAccumulator` coordinate.

### 3.2 Crossing program strata

A grammar rewrite is an explicitly typed morphism

$$
r:G_a\rightsquigarrow G_b
$$

with preconditions, a zero-mass embedding, an inverse when one exists, and a
nonnegative path length. Examples are splitting one ecological guild into two,
adding a methane cycle, or changing a single-ocean circulation block into a
two-basin block after the connecting strait closes.

A cross-stratum path embeds both endpoints in a common refinement graph, moves
optional mass continuously away from zero, and then normalizes. There is no
instantaneous portal. If no common refinement is licensed, the states are in
different reachable components even if a renderer could cross-fade them. Only a
complete grammar/rewrite connectivity proof may establish that fact; failure of
the bounded runtime search establishes only
`Unresolved(SearchHorizonExhausted)`.
The symmetric neighborhood distance is treated as an extended metric with
$D=\infty$ between certified disconnected components.

The Model exposes:

- a symmetric topology for “nearby world” queries, obtained as the induced
  shortest-path metric on the normalized rewrite graph with within-stratum
  transport lengths and symmetric rewrite edges; zero-cost edges identify the
  same quotient point and every other rewrite edge has positive length;
- a directed length for Egress, obtained from admissible controls and directed
  rewrite lengths; and
- a rewrite-stratum signature recording which licensed rewrites a route crosses.

This can admit branches, one-way controls, and phase boundaries. Calling two
rewrite sequences homotopic would additionally require versioned 2-cells and
path-equivalence laws, which V1 does not assume. A single local Jacobian is not
expected to describe all of Possibility.

### 3.3 Finite route search

An Egress probe does not enumerate every program. The compiler emits, for each
normalized stratum, a bounded neighborhood of applicable rewrites and lower
bounds on their length. The initial runtime examines at most eight path modes,
at most two rewrites ahead, and at most four active law levels. Within each mode,
the measure problem is solved by fixed-count proximal/Sinkhorn blocks. A
deterministic branch-and-bound order rejects a mode only when a certified lower
bound on its **entire remaining lexicographic key**—primary
length/Yearning/Attractor objective first, then the most favorable closure
prior, then path signature—already exceeds the best certified upper tuple. The
prior is not numerically folded into the primary objective. A length-only lower
bound is not safe pruning.

The other packet levels and procedural tail are not ignored. A mode either
declares them frozen, in which case their path length is exactly zero, or includes
certified lower/upper bounds obtained from parent moments and the tail envelope.
If omitted-level intervals can change feasibility, the length limit, pruning, or
mode selection, the planner activates more levels up to the 16-level packet
ceiling. Remaining ambiguity returns
`Unresolved(SearchHorizonExhausted)`, never a false canonical winner.

Twenty-four scaling iterations are a hard work ceiling, not a convergence
promise. Each transport block must return a feasible primal upper bound and a
dual lower bound; failure to meet the requested gap returns `Unresolved` for
that mode.

The Model may return up to three non-dominated modes when intent is genuinely
multimodal. The Traveler chooses explicitly or a versioned policy chooses by
objective, then lexicographic path signature. An arbitrary iteration order can
never decide which world is reached.
If the certified non-dominated frontier exceeds three, the query returns a
`Partial` page with a continuation or `Unresolved(ModeFrontierTooLarge)`; it
never silently discards a mode that could change the choice.

Selection is an explicit pure Model operation. Choosing a returned `mode_id`
mints a `SelectedEgressPlan` whose id commits to source root, normalized intent
and Attractor digest, mode-specific certified path, Resonance/rate inputs, and
length horizon. Only that selected plan can be advanced; a collection containing
three alternatives is never itself an ambiguous path. Selection is enabled only
after the frontier is `Complete`, or when a certificate proves omitted pages
cannot dominate the explicitly chosen mode.

Long travel replans from committed states in model-predictive-control fashion.
This does not make endpoint realization path-dependent: a State Packet always
realizes identically. The path affects which endpoint is selected, just as two
physical routes can reach different destinations.

Runtime discoverability is therefore weaker than mathematical Reachability. A
positive segment certificate proves a discovered admissible path; only a
complete grammar cut/certificate can produce `ProvenUnreachable`. Exhausting the local
mode, rewrite, level, or iteration ceiling means
`Unresolved(SearchHorizonExhausted)`, not `ProvenUnreachable`.

### 3.4 Reachability certificates

A committed segment may emit a compact certificate containing:

```text
source and destination State roots plus required chunks
normalized rewrite sequence
quantized directed path length and length limit
hard-constraint primal residuals
dual lower bound / optimality interval
canonical solver and manifest versions
```

A future Attractor service can validate a reported route by replaying bounded
segments and checking certificates. Verification is bounded **per segment**;
replaying an uncheckpointed expedition remains linear in its segment count.
Long routes therefore use signed checkpoint roots or separately specified
recursive aggregation proofs before claiming bounded whole-route validation.
The service need not solve an existential path problem over arbitrary Yearnings.
A destination without a valid source chain may still be representable but is
not accepted as proven reachable from that source.

## 4. Plausibility by compilation and constraint

### 4.1 Static validity

The compiler rejects a constitution before runtime if:

- a port's units or semantic type do not match;
- a conserved inventory has no balanced source/sink relation;
- a relation measure violates its declared marginal, submarginal, conversion, or slack balance;
- a refinement fails its parent-moment identities;
- a solver block lacks a uniqueness or finite-selection rule;
- a coefficient leaves its certified parameter cell;
- a capability exposes an observation without a canonical error policy; or
- a dependency is cyclic outside a declared simultaneous block.

This is validity by typed construction, but it does not pretend types alone
prove a planet physically coherent. Numeric feasibility is a separate solve.

### 4.2 Numeric feasibility

Packet feasibility and derived-solver accuracy are separate. The exact packet
set is

$$
\mathcal F_G^{\rm packet}=
\{q:Cq=d,\ q\in\mathcal K,\ E_k(q)\le0,
\operatorname{complexity}(q)\le B_G\},
$$

where $Cq=d$ expresses exact inventories and declared balance equalities, $\mathcal K$ is a
product of nonnegative/simplex/positive-semidefinite cones, $E_k$ are exact
fixed-point inequalities, and $B_G$ includes support/rank/packet ceilings. These
claims are checked exactly on packet integers.

Let $y$ collect a derived global section or query result. Its equations and
inequalities are evaluated with certified residual and output-error intervals;
“inside tolerance” is not renamed exact equality. Every representable packet
must support a Canonical **base grade** for required terrain, forcing, and
ecological-summary channels. Higher accuracy, optional classifiers, or topology
resolution may return `Unresolved` with a deterministic continuation.

The authoritative design prefers unique convex or strongly monotone blocks. A
typical block is

$$
y^*(q)=\arg\min_{y\in C(q)}
\left[
\tfrac12\|A(q)y-b(q)\|^2+
\sum_j\lambda_j\Psi_j(y)+
\tfrac\epsilon2\|y\|^2
\right],
\qquad \epsilon>0.
$$

This block has a unique canonical answer only when $C(q)$ is convex, each
$\Psi_j$ is convex, and the compiler certifies a positive strong-convexity bound
on the feasible tangent space. Adding the final quadratic term does not rescue a
nonconvex feasible set. A query is resolved only when its residual and stability
bound enclose the requested output error.

Some valuable natural structures are genuinely nonconvex. They are isolated
behind finite active sets: enumerate a bounded canonical candidate set, solve a
unique problem for each candidate, choose the least certified objective, and
break a tie by integer candidate id only when canonical fixed-point objectives
are exactly equal. Overlapping objective intervals at the refinement ceiling
return `Unresolved`; they are not an exact tie. The Model reports the certified
objective margin. It never labels a discontinuous classifier “smooth” merely to
simplify navigation.

### 4.3 Local-to-global consistency

Each spatial tile, layer, and refinement level has a local state space. Typed
restriction maps send its boundary values, fluxes, and parent moments to shared
edges and coarser cells. They form a cellular sheaf only when the compiler
checks the functorial composition laws on the declared incidence/refinement
poset. A valid global section is a collection of local results whose
restrictions agree.

For finite-dimensional linear stalks, let $\delta_{\mathcal S}^0$ stack those
restriction differences. Then

$$
L_{\mathcal S}^0=(\delta_{\mathcal S}^0)^*\delta_{\mathcal S}^0,
\qquad
\chi_{\rm glue}(s)=\langle s,L_{\mathcal S}^0s\rangle
=\|\delta_{\mathcal S}^0s\|^2.
$$

Its kernel is the space of global sections. Nonlinear local state spaces use a
sheaf of sets plus a declared local linearization for diagnostics; the linear
spectral theorem is not applied to them without that reduction. Canonical
integration accepts a tile only when the relevant restrictions match exactly in
fixed point and the remaining certified residual is within its declared bound.

This is stronger than hoping neighboring procedural evaluations happen to
match. It also gives cancellation semantics: an incomplete local result is not
a global section and cannot become authoritative. A zero glue residual proves
compatibility of overlaps, not correctness of each local physical solve; that
requires its separate residual/error certificate.

### 4.4 The certificate-carrying manifest

The package includes machine-readable evidence for every numeric claim:

- interval bounds on coefficient cells;
- contraction, monotonicity, convexity, or active-set declarations;
- exact parent/child restriction identities;
- conservation vectors and unit derivations;
- reduced-basis residual estimators;
- canonical operation counts and rounding modes; and
- native/wasm golden fixtures.

“Certificate-carrying” does not imply a formally verified theorem prover for the
entire planet. It means every optimization relied upon by the runtime has a
small checkable witness or conservative declaration, and the conformance harness
can reject a stale or overstated artifact. Golden fixtures are regression
evidence, not mathematical proofs.

## 5. Realization: weaving one deterministic planet

### 5.1 The universal innovation thread

All worlds sharing one stable generation identity use one addressable innovation
tower. For semantic channel $c$, let

$$
\zeta_{c,\ell,k}=R(D_c,\ell,k),
\qquad
D_c=(\text{family},\text{major},K,c,\text{algorithm revision}).
$$

where $k$ is an integer cell, edge, vertex, mode, or sample address and $R$ is a
specified counter-based integer generator. There is no mutable random stream.
Evaluation order, worker count, cancellation, and cache history cannot change a
sample.

Raw counters at different levels are not assumed compatible. The manifest
constructs

$$
\xi_{c,0}=Q_{c,0}\zeta_{c,0},
\qquad
\xi_{c,\ell+1}=P_{c,\ell}\xi_{c,\ell}
+Q_{c,\ell+1}\zeta_{c,\ell+1},
$$

with exact fixed-point identities

$$
r_{c,\ell}P_{c,\ell}=I,
\qquad
r_{c,\ell}Q_{c,\ell+1}=0.
$$

Thus restricting an innovation exactly recovers its parent. When an SPDE-like
block wants a finite-element approximation to correlated or white forcing, the
compiler declares its load covariance explicitly; iid vertex counters are not
mislabeled finite-element white noise. Exact Galerkin white-noise loads have
covariance equal to the finite-element mass matrix $M$. Because
$\mathcal L^TM^{-1}\mathcal L$ is generally dense, V1 gets a sparse canonical
coefficient precision only through a diagonal mass-lumped $\widetilde M$ or
another specifically verified construction with a versioned error certificate.
A retained auxiliary forcing variable may give a sparse **joint** system; its
marginal precision after elimination is not called sparse without a separate
proof. A small exact-$M$ base solve may instead be dense and bounded; it may not
simultaneously claim exact covariance and generic sparse precision.

The innovation is transformed by the constitution rather than reseeded by the
State root. Nearby constitutions therefore encounter the same underlying
“chance”: the same large-scale forcing mode may become wetter, steeper, or more
biologically productive instead of being replaced by unrelated noise. This is
the common-random-numbers coupling made part of the Model contract.

Counter values are converted to canonical fixed-point innovations through
versioned tables. This is a deterministic SPDE-inspired field, never an exact
Gaussian draw: a finite fixed-point table has a discrete finite-support law. A
package may call it an exact deterministic realization of its versioned
**approximate** SPDE/GMRF ensemble and certify distribution/covariance
discrepancy from a mathematical Gaussian reference. Interactive float kernels
may approximate the canonical field but may not mint identities.

### 5.2 Planetary complex and discrete forms

World Space uses nested icosahedral complexes

$$
X_0\prec X_1\prec\cdots.
$$

Physical quantities live where their conservation law belongs:

- potentials and some scalar attributes are 0-cochains on vertices;
- tangential circulation lives as primal 1-cochains, while normal flux in the
  surface mesh lives as dual 1-cochains or is mapped through a declared Hodge
  star;
- mass, heat, water, and biomass inventories live as 2-cochains on faces; and
- cross-layer relations live on typed hyperedges in the causal program.

Discrete exterior calculus supplies incidence matrices with exact topological
identities such as $d_1d_0=0$. Metric Hodge-star operators are versioned numeric
data with certified SPD bounds; mesh orientations are globally consistent. A
circumcentric star is used only on a mesh with positive suitable dual geometry,
otherwise a barycentric or other certified SPD star is required. This separates
topology from geometry and makes seam-free divergence, gradient, circulation,
and inventory accounting natural on a sphere.

Refinement uses lifting steps. Restricting the **represented coefficient and
innovation towers** exactly recovers their parents. That fact alone says nothing
about independently solving a nonlinear PDE on two grids.

World Loom therefore makes a deliberate modeling choice. Its authoritative
projective solver is recursive: solve $u_0$, then solve each $u_{\ell+1}$ with
hard constraints

$$
R_\ell u_{\ell+1}=u_\ell
$$

for the parent values, boundary flux sums, inventories, and other summaries the
capability declares invariant. This is a synthetic hierarchical Model, not a
claim that ordinary fine-grid continuum physics commutes with restriction. A
block that cannot impose those constraints instead returns a coarse interval
that refinement may narrow; it may not promise an exact coarse value. High-
fidelity offline solves measure modeling error but do not retroactively redefine
the shipped hierarchy.

### 5.3 Correlated primitive fields from sparse SPDE-inspired operators

Primitive geology, material, and environmental forcing fields use coupled local
elliptic operators evaluated against the frozen pseudorandom forcing $\xi$. A
representative supported diagonal block is

$$
\sum_d\mathcal L_{cd}(q)u_d=\xi_c,
\qquad
\mathcal L_{cc}u_c=
\left(\kappa_c(q,x)^2-
\nabla\!\cdot H_c(q,x)\nabla\right)^{r_c}
(\tau_c(q,x)u_c),
\qquad r_c\in\mathbb N.
$$

V1 permits only integer local powers, lowered through auxiliary variables to
sparse operators. A fractional power is nonlocal and is unsupported at Canonical
grade unless a separately versioned rational approximation has its own error
certificate. Positive bounds on $\kappa,\tau,H$, the ordering shown above, the
cross-channel covariance factor, and the coupled block's coercivity/invertibility
are compiler obligations. Nonstationary coefficients can follow continent,
latitude, lithology, or climate regimes.

After finite-element/discrete-form projection, the local elliptic operator is
sparse. The coefficient precision is sparse only under the explicitly declared
mass-lumped or separately verified construction above; an auxiliary construction
guarantees only its retained joint sparsity unless more is proved. The inverse
remains global; sparsity does not justify an exact bounded-halo solve by itself.

A canonical coarse level is solved for every queried snapshot. The compiler may
precompute symbolic sparsity, parameter-cell reduced bases, and boundary
layouts, but numeric Schur data are State-dependent and computed under the
relevant dependency subroot. Fine tiles use those bounded boundary summaries and
projective corrections. If a dense boundary exceeds the declared cap, a
required base query falls back to its bounded global/coarse solve; an optional
higher grade may return `Unresolved`. A local solve is conforming only when its
primal, boundary, and truncation bounds certify the declared hierarchical Model.

### 5.4 Astronomy, planet, and terrain

The constitution carries bounded distributions and inventories for stellar
luminosity, orbital elements, moons, atmosphere, hydrosphere, and crust. A typed
orbital block rejects unstable configurations under the V1 horizon and exposes
integer-time illumination and tide forcing.

Terrain is a coupled variational material problem, not an octave sum. One V1
form is

$$
E(z,m)=
\tfrac12\langle z,L_{\rm bend}(q)z\rangle
+\lambda_f\operatorname{TV}_\epsilon(m)
+\lambda_c E_{\rm compression}(z,m)
+\lambda_e E_{\rm erosion\ prior}(z,m)
-\langle \xi_z,z\rangle,
$$

subject to exact crustal-volume and material-simplex constraints. $z$ is
elevation and $m$ is a multiphase lithology field. Diffuse phase boundaries
allow faults, cratons, volcanic arcs, and basins to emerge from one coupled
equilibrium. Integer active-set extraction names robust plate/fault features and
reports the objective margin to the next candidate.

The displayed ingredients do not automatically make this energy convex. A V1
instantiation must either certify a positive Hessian lower bound within each
finite material active set or use the bounded candidate rule of Section 4.2.
Uncertified coefficients are not valid State cells.

Ocean level is the canonical monotone volume solve for the declared free-water
inventory over canonical quadrature. Uniqueness is specified at plateaus by the
least quantized level satisfying the inventory interval. Zero free water selects
a manifest lower bound below minimum terrain; inventory above the declared
surface capacity is invalid unless the constitution explicitly contains an ice,
subsurface, or atmospheric reservoir that receives the excess. Projective
terrain refinement must preserve the parent water-volume and coastline-summary
constraints (or narrow a previously returned interval); the generic lifting
identity alone is not enough. Elevation, slope, material, age, strength,
permeability, and uncertainty are canonical channels. Formation chronology is a
pure derived label or precompiled response mode; the Model does not run a live
tectonic simulation.

### 5.5 Hydrology as shared convex transport

For canonical source basins $k$, macro flow solves

$$
\min_{f^k\ge0}
\sum_e c_e(q,z)
\sqrt{\epsilon^2+\sum_k(f_e^k)^2}
+\frac\eta2\sum_{e,k}(f_e^k)^2
$$

subject to

$$
Bf^k=r^k-s^k
$$

and exact storage, capacity, and ocean-outlet constraints. Candidate edges are
oriented by quantized hydraulic head after a canonical depression/spill solve;
flat ties use an integer order, and the resulting directed graph admits no
unlicensed uphill or cyclic flux. Sharing an edge is cheaper than duplicating
nearby paths, so branching river networks emerge. The quadratic term gives a
unique flux field only under the manifest obligations $c_e\ge0$, $\epsilon>0$,
$\eta>0$, and a nonempty convex affine/capacity feasible set; otherwise the
packet is invalid or the block is unresolved. This is a deterministic convex surrogate for branched
transport, not a claim to reproduce geomorphological history exactly.

River, lake, watershed, and coastline entities are extracted from quantized
flux and level sets. Each reports its threshold margin and a persistent feature
signature. Local high-resolution runoff is a constrained refinement of the
macro flux, so it cannot create or destroy water at a tile seam.

### 5.6 Climate and canonical temporal modes

The authoritative climate block solves periodic energy, moisture, and coarse
circulation balance on discrete forms. Agents build a reduced basis offline
from a much higher-resolution structure-preserving solver. Runtime solves

$$
A_r(q)c_r=b_r(q,z,I_t),
$$

typically with 32--96 basis coefficients, then evaluates a certified residual
against the full operator. The certificate combines that residual with a
manifest lower bound on operator stability/coercivity; a residual alone is not
an error bound. A basis cell that cannot enclose the requested state falls back
to a larger basis or returns `Unresolved`; it never extrapolates silently.

The outputs are monthly or harmonic means, covariances, extrema envelopes,
prevailing fluxes, and deterministic response modes. They are initial and
boundary data for Visualization weather. A cloud at 14:03, a gust, and a storm
track realization are not Model entities.

### 5.7 Soil, biome, and disturbance

Soil is a constrained material balance over mineral fractions, water capacity,
organic matter, nutrients, pH, and disturbance response. Coupled soil and
productivity blocks use one simultaneous strongly monotone solve with declared
existence/coercivity bounds, so layer order cannot create a fictitious causal
priority.

Biome names are fuzzy observational labels over continuous canonical channels.
The full membership vector is authoritative; `argmax` is only a convenient
entity classifier with a returned margin. Fire, flood, and succession regimes
are canonical distributions and response modes. Individual events are
Visualization simulation unless an Impression explicitly addresses a canonical
forcing phase.

### 5.8 Ecology as transport over trait space

Ecology is a positive measure $\nu_x(a)$ over a versioned continuous trait
space, with local density represented on a finite adaptive atom dictionary. A
canonical equilibrium is

$$
\nu_x^*=\arg\min_{\nu\ge0}
\left[
\operatorname{Ent}(\nu\mid\nu_0(q,x))
+\tfrac\beta2\|R\nu-s_x\|^2
+\tfrac12\langle\nu,K_x\nu\rangle
\right]
$$

under exact energy, resource, abundance, and applicability constraints.
$\operatorname{Ent}$ is the generalized finite-measure entropy defined in
Section 8.1, including its zero/support conventions; ordinary probability KL is
not applied to an unnormalized biomass measure. At Canonical grade the feasible
set is nonempty and convex, $K_x$ is symmetric positive semidefinite on its
tangent space, and the compiler proves the total objective strongly convex or
uses the finite-candidate rule of Section 4.2. The entropy term is a
closure principle that selects one distribution from incomplete ecological
constraints; it is not asserted to be a universal law of nature.

Food-web flow uses entropy-regularized typed energy-flow couplings between
producer, consumer, scavenger, and decomposer roles. Their submarginals obey
supply/intake capacities derived from canonical biomass, while explicit
efficiency transforms and loss/slack channels close the energy budget in one
unit system. Optional normalized role couplings describe correspondence, not
physical energy. This makes relationship structure and trophic efficiency
queryable without simulating individual predation.

The local solves do not require a scan of every habitat cell to define a global
species. Each projective ecology tile restricts to a fixed summary of trait
mass, habitat membership, and trophic moments. The 1,280-face base complex plus
certified tail bounds therefore defines one bounded global trait/habitat measure
using at most 256 atoms per declared trait block. A canonical scale-space
clustering rule first extracts modes on that coarse atom grid. Refinement may
resolve a mode whose interval was previously ambiguous, but may not rename a
mode already reported canonical; a tail bound crossing a split/merge threshold
returns `Unresolved`.

Species are those persistent modes of the bounded global trait/habitat measure.
Each has:

- an **ancestry key** derived from the family innovation basin and motif path,
  stable while that mode persists across nearby constitutions;
- an **exact manifestation id** derived from State root, ancestry key, and mode
  ordinal;
- a trait distribution, tolerance, niche, trophic relations, and classifier
  margin; and
- split/merge ancestry emitted in a Transition Plan.

A canonical organism manifestation is an addressable representative sample

$$
o=(\text{habitat cell},\text{manifestation id},\text{sample ordinal},
\text{canonical epoch}).
$$

It supplies age/size/sex or reproductive mode where applicable, health envelope,
and observable traits. Its live position, pose, behavior, and interactions
belong to the Visualization.

## 6. The Realization contract

### 6.1 Immutable query surface

The neutral API is snapshot-oriented:

```rust
pub enum QueryStatus<T> {
    Complete(T),
    Partial { value: T, continuation: ContinuationToken },
    Unresolved {
        reason: UnresolvedReason,
        continuation: Option<ContinuationToken>,
    },
}

pub enum EgressOutcome {
    Planned(EgressPlan),
    Idle,
    ProvenUnreachable(UnreachableCertificate),
}

pub trait LoomModel {
    fn validate_state(
        &self,
        packet: &StatePacket,
    ) -> Result<QueryStatus<StateCertificate>, ModelError>;
    fn open(&self, packet: &StatePacket) -> Result<QueryStatus<Snapshot>, ModelError>;
    fn plan_egress(
        &self,
        request: &EgressRequest,
    ) -> Result<QueryStatus<EgressOutcome>, ModelError>;
    fn select_egress_mode(
        &self,
        plan: &EgressPlan,
        mode: ModeId,
    ) -> Result<QueryStatus<SelectedEgressPlan>, ModelError>;
    fn advance(
        &self,
        plan: &SelectedEgressPlan,
        cumulative_path_length_q32: u64,
    ) -> Result<QueryStatus<AdvanceResult>, ModelError>;
    fn transition(
        &self,
        request: &TransitionRequest,
    ) -> Result<QueryStatus<TransitionPlanPage>, ModelError>;
}

pub trait LoomRealization {
    fn planet(&self) -> PlanetDescriptor;
    fn sample(&self, request: &SampleRequest, out: &mut SampleBatch)
        -> Result<QueryStatus<PageInfo>, ModelError>;
    fn tile(&self, request: &TileRequest, out: &mut Tile)
        -> Result<QueryStatus<PageInfo>, ModelError>;
    fn features(&self, request: &FeatureRequest, out: &mut FeatureBatch)
        -> Result<QueryStatus<PageInfo>, ModelError>;
    fn ecology(&self, request: &EcologyRequest, out: &mut EcologyBatch)
        -> Result<QueryStatus<PageInfo>, ModelError>;
    fn forcing(&self, request: &ForcingRequest, out: &mut ForcingBatch)
        -> Result<QueryStatus<PageInfo>, ModelError>;
    fn sensitivity(&self, request: &SensitivityRequest)
        -> Result<QueryStatus<SensitivityReport>, ModelError>;
}
```

Requests carry sorted World Space addresses, canonical time or interval,
channel/capability ids, accuracy, tolerance, and optional transition direction.
Every collection request also carries an explicit item/work cap and optional
continuation token. A token commits to the source root, normalized request,
manifest, canonical cursor, and accumulated certified bounds; replaying it is
deterministic. No API silently truncates. `SearchHorizonExhausted` is an
unresolved reason with a continuation, whereas `ProvenUnreachable` is a
complete result backed by a grammar-cut certificate. All results depend only on
the snapshot, request, continuation, and manifest. Caller-provided scratch
storage and a declarative job graph keep execution policy outside the neutral
crates.

`ModelError` is reserved for invalid/corrupt inputs, missing required immutable
data, unsupported versions, or violated internal integrity contracts. Expected
approximation, classifier ambiguity, and bounded-work exhaustion are typed
`QueryStatus` values, not exceptional control flow.

### 6.2 Capability negotiation

Every capability descriptor includes:

```text
semantic id and schema version
required input channels and dependency revisions
units, ranges, applicability, and missing-value meaning
canonical / interactive determinism grade
spatial support and refinement behavior
model-time semantics
error and topology-resolution policy
required or optional status
```

A Visualization declares the capability versions and error grades it consumes.
The pair is compatible only if all required capabilities match. Optional
capabilities may be absent or rendered by a declared fallback. A Visualization
may reject the Model rather than reinterpret an unknown ecological relation.

Presentation assets, meshes, textures, animation, audio, input, UI, hardware
tiers, and live simulation do not appear in a Model capability. Builds have a
separate negotiated presentation schema (Section 11).

### 6.3 Accuracy and error grades

Three query grades are defined:

1. **Preview** may use low-rank float approximations and coarse refinement. It
   returns conservative error bounds and no portable identities.
2. **Interactive** meets a requested screen/world tolerance, may use portable
   SIMD approximations, and marks any classifier or topology whose interval
   crosses a threshold as unresolved.
3. **Canonical** uses fixed-point inputs, specified operation order, canonical
   transcendental tables, fixed or certified-convergent solver rules, and exact
   active-set tie breaks. It is the only grade that confirms an Impression.

Refinement must narrow a returned interval and preserve all already-resolved
coarse moments. It may resolve a formerly unknown identity; it may not silently
change one that was reported canonical.

### 6.4 Model time versus simulation time

The Model returns deterministic forcing and statistical response. The
Visualization chooses a versioned simulator that turns those into transient
weather, motion, organism behavior, sound, and other activity. Its complete
simulation seed, parameters, resource tier, and time are explicit Visualization
inputs.

The versioned `VisualizationReplayBundle` for exact replay names the complete
tuple: Visualization and simulator definitions
and versions; backend/hardware class wherever it can affect an observable;
assets, preferences, resource tier, and parameters; Realization snapshot and
World Space location; canonical initial simulation snapshot; deterministic
action/event log (or a replacement full snapshot); and simulation time. The
same tuple must produce bit-identical declared observable state and output.
Backend variation cannot be waived behind a vague determinism grade: it is
either eliminated from the declared output or represented in the tuple's
identity. A different tuple may present the same Model differently, but it
cannot change canonical Model observations, Egress, Resonance, or Reachability.

## 7. Observables, Impressions, and Yearnings

### 7.1 Observables are not control coordinates

The Model publishes a versioned catalog of canonical observables. An observable
$O_a$ declares:

- subject and applicability universes;
- units and a normalized comparison metric;
- a membership kernel for captured values or traits;
- a population measure over the whole planet or applicable population;
- spatial, species, relation, and temporal aggregation semantics;
- Preview/Interactive/Canonical evaluators and error bounds; and
- sensitivity support, which may be an analytic derivative, a dual solution,
  or certified finite differences.

Unlike Option 2, the observable vector is not also the coordinate chart. The
constitution may contain causes that have no direct player-facing attribute,
and two different causal arrangements may produce similar observables. This is
where surprise can live without making navigation meaningless.

Cross-Model compatibility uses semantic ids plus explicit adapters. A generic
attribute such as `organism.body_plan.limb_count` may be shared; a Model-specific
attribute such as `photosynthesis.electron_acceptor` may have no adapter. An
Impression term is usable only when the destination Model declares a compatible
membership and prevalence measure. Unknown semantics are disabled, never
silently mapped by name.

### 7.2 Scope is measure mass

Let $h_I(a)\in[0,1]$ be the membership kernel around an attribute captured by an
Impression, and let $\nu_q$ be the applicable canonical population measure. Its
applicable mass and world-level prevalence are

$$
M_I(q)=\int 1_{\rm applicable}(a)\,d\nu_q(a),
\qquad
p_I(q)=\frac{\int h_I(a)1_{\rm applicable}(a)\,d\nu_q(a)}{M_I(q)}.
$$

There is no numeric epsilon in this semantic ratio. If the certified lower
bound on $M_I$ is zero, the term is `Inapplicable` when the mass is exactly zero
and otherwise `Unresolved`; in either case it contributes no steering energy.
Resolved division uses the manifest's canonical rational/interval arithmetic.

For environmental attributes, $\nu_q$ is the exact hierarchical area/time
measure; for organism traits, it is the species or biomass measure named by the
attribute schema. The schema must say which denominator it uses. “Most species”
and “most biomass” are not interchangeable.

A continuous Scope $s\in[0,1]$ maps to

$$
\tau(s)=10^{-4(1-s)}0.85^s.
$$

Thus singular begins near a one-in-ten-thousand exceptional share, common moves
through intermediate prevalence, and pervasive approaches 85% of applicable
cases. Named UI bands are intervals over $s$, not different algorithms.

The mass tower carries exact parent totals and bounded tail mass, so prevalence
queries do not scan the planet. A query sums canonical coarse atoms plus a
certified bound for unresolved fine detail.

Scope never uses distance from the source Impression's World Space location.
That location tells the Model what was observed; it does not define a radial
zone in the destination world.

### 7.3 The four Influence intentions

Each active Yearning produces canonical terms from its source Impressions.
Scope always denotes the **absolute destination prevalence of the predicate
requested by the Influence**, not a fraction of the remaining possible change:

| Influence | Model term |
|---|---|
| **Accentuate** | request the captured membership predicate at at least the Scope prevalence, plus any schema-declared magnitude target |
| **Repress** | request the complement of the captured membership predicate at at least the Scope prevalence |
| **Hold** | penalize transport of selected moments/couplings away from their value when the Yearning was activated |
| **Disable** | emit no term and no constraint |

If $p_0$ is the current canonical prevalence when the Yearning is activated, V1
uses the monotone one-sided prevalence targets

$$
p_{\rm accentuate}\ge \max(p_0,\tau(s)),
\qquad
p_{\rm repress}\le \min(p_0,1-\tau(s)).
$$

The second inequality is exactly the request
$\operatorname{prevalence}(1-h_I)\ge\tau(s)$. Thus higher Scope means broader
expression for Accentuate and broader absence for Repress. If the activation
world already exceeds the absolute target, prevalence supplies no pressure in
the opposite direction. Attribute schemas may also target magnitude or a
relation moment, but must publish an analogous absolute, monotone interpretation.
The captured value defines the membership kernel; $p_0$ comes from its
prevalence in the activation world.

Hold captures its reference packet and observable interval once. It does not
retarget to the moving current state every tick, which would ratchet and turn a
hold into viscous drag. Hold is finite unless the schema explicitly defines a
hard safety invariant. Ordinary Hold weight can be overcome by stronger
conflicting intent or by validity; it can never make an invalid world valid.

Accentuate and Repress are relative to the captured membership kernel, not an
assumption that the one observed organism represents its species. Applicability
and capture uncertainty widen the penalty interval. A term whose canonical
interval is too wide is reported unresolved and contributes only its certified
lower-confidence weight.

### 7.4 Order-independent intent

Each active Yearning $y$ has a positive unsigned top-level weight $W_y$; a
zero-weight Yearning is disabled before normalization. Each enabled
semantic request inside it becomes a term

$$
t_{y i}=(k_{y i},u_{y i},\Phi_{y i}),
$$

where $k_{y i}$ is its canonical semantic key, $u_{y i}$ is its positive unsigned
within-Yearning weight (equal by default), and $\Phi_{y i}(q)$ is a convex or
bounded finite-mode penalty over moments, measures, or relation couplings.

The aggregate Yearning energy is

$$
\mathcal Y(q)=
\sum_y\frac{W_y}{\sum_zW_z}
\left(
\sum_i\frac{u_{y i}}{\sum_j u_{y j}}\Phi_{y i}(q)
\right).
$$

Empty, zero-weight, or fully disabled Yearnings are removed before the outer normalization.
If none remain, $\mathcal Y=0$ exactly. This hierarchy prevents a Yearning with
many captured attributes from gaining more total influence merely because it
contains more terms. Both normalization denominators are checked positive before
division.

Inputs are a canonical multiset sorted by semantic key and Impression content
id. Duplicate references retain multiplicity; removing a duplicate removes its
weight. Equal terms are combined by checked integer weight addition only within
the same owning Yearning before inner normalization. Normalized
weights remain exact rationals at both hierarchy levels, and all remaining
numeric reductions use a fixed balanced tree. Scaling every top-level weight,
or all within-Yearning weights of one Yearning, by a common factor changes
nothing; relative weights express compromise. Duplicate multiplicity is
preserved inside its owning Yearning. Permuting Yearnings, Impressions, or
attributes therefore changes neither bits nor meaning.

Conflicts are simultaneous terms in one objective. There is no “accentuate
pass” or winner based on insertion order. When several path modes are nearly
equal, the Model returns the modes and their objective intervals instead of
pretending one local gradient expresses the full intent.

### 7.5 Impressions

A portable Impression contains or references a self-contained bundle with:

```text
GenerationId, used extension ids, State root, and required immutable State chunks
exact fixed-point World Space address and attachment frame
optional canonical model time / forcing phase
canonical subject id and margin, or a canonical attribute-by-value record
attribute schema versions, applicability, and error intervals
optional self-contained/referenced VisualizationReplayBundle from Section 6.4
optional Build payload/content id
optional Reachability certificate chain
```

The State chunks make the root reproducible without an online registry. Atlas
bundles may deduplicate them across many Impressions.

An Impression may remain private, be shared directly as a bundle, or be
published to a library. Publication and discovery policy do not change its
canonical payload.

Robust canonical ids are preferred for stable species, river reaches, and other
features. An attribute-by-value record is mandatory when a classifier margin is
small, an entity is unresolved, or the desired Yearning does not require entity
identity. An Impression is an observation and address only; none of its fields
affects Egress until the Traveler places it in a Yearning.

During a continuity blend, the Visualization may show an old manifestation that
has no exact current-world counterpart. Capture then uses the Transition Plan:

- if a correspondence to a current canonical subject is resolved, the
  Impression records that current subject and may attach a Visualization-only
  snapshot of the blended appearance;
- if no correspondence is resolved, canonical capture is delayed or records
  attributes by value with `transition_unresolved`; and
- it never records the old local effective world as though it were the
  Traveler's current Possibility point.

This restriction is visible to the UI, but it preserves the stronger guarantee:
another compatible Visualization can reproduce the same canonical Realization
and subject from the Impression.

## 8. Egress as constrained minimizing movement

### 8.1 One navigation probe

Let $q_n$ be the current constitution, $\mathcal Y$ the aggregate Yearning
energy, and $\mathcal A$ an optional Attractor potential. The manifest declares
a hard per-plan directed-length horizon $\Delta L_{\rm model}$; the caller may
request a smaller horizon but never a larger one. For bounded path mode $m$, let
$\mathcal C_m(q_n)$ be the class of paths that:

- start at $q_n$;
- remain in the continuous law-feasible set for every path parameter, with
  exact inventory/declared-balance constraints and certified derived-solver bounds;
- obey the mode's directed controls, measure continuity equations, and rewrite
  preconditions; and
- include a bounded sequence of committable checkpoints whose constrained
  rounding satisfies the packet support/rank cap, exact integer balances, and
  continuous-channel jump bound, ending in a certified representable packet.

The primary generalized proximal-control problem is

$$
\Gamma_m^\dagger\in
\arg\min_{\substack{\Gamma\in\mathcal C_m(q_n)\\
\mathscr L_m^{\rightarrow}[\Gamma]\le\Delta L}}
\left[
\frac{\mathscr L_m^{\rightarrow}[\Gamma]^2}{2\tau}
+\mathcal Y(\Gamma(1))
-\gamma\mathcal A(\Gamma(1))
\right].
$$

Here $\Delta L=\min(\Delta L_{\rm requested},\Delta L_{\rm model})$ and
$\mathscr L_m^{\rightarrow}$ is the additive asymmetric control/rewrite length
from Section 3.1. In a single geodesically convex stratum with symmetric length,
its infimum is transport distance and the displayed proximal term reduces to
$D^2/(2\tau)$. In general it does not. This is a constrained
minimizing-movement scheme **inspired by** the JKO
construction, not an assertion that classical JKO existence or convergence
theorems apply to the mixed glued state complex.

$\tau>0$ has units converting $L_P^2$ to the manifest's canonical intent-energy
unit. $\mathcal Y$ is normalized into that unit, $\mathcal A$ has a declared
potential unit, and $\gamma\ge0$ converts it to intent energy. These terms are
never added before dimensional type checking.

Among candidates whose certified primary objective is exactly equal, the Model
then minimizes $\mathcal P_{\rm law}(\Gamma(1))$ and finally the canonical path
signature. This is an actual lexicographic selection, not a small additive term
that could secretly trade against intent. The closure rule is reached only
after a non-idle Yearning/Attractor objective leaves directions underdetermined;
with neither, planning returns `Idle`. It cannot make ordinary unsteered walking
drift toward a prior. For positive finite measure channels it may use
generalized relative entropy

$$
\operatorname{Ent}(\mu\mid\nu)=
\sum_i\left[\mu_i\log\frac{\mu_i}{\nu_i}-\mu_i+\nu_i\right]
$$

on a declared common discrete support, with canonical zero conventions.
Coefficients use declared quadratic priors, and grammar rewrites use explicit
closure/complexity costs. Entropy is not applied to an opaque program byte
string and never overrides hard feasibility or primary intent.

Entropic regularization turns individual finite transport blocks into bounded
matrix-scaling iterations. The whole problem can still be nonconvex because
derived observables, support choices, physical guards, and rewrites are
nonconvex. V1 enumerates their bounded active sets and reports the best
**certified candidate within the searched horizon**, together with primal upper
bounds, dual/lower bounds where available, and a horizon status. It does not
claim a global optimum over unsearched programs. The mathematical distance, the
regularized subproblem, iteration count, and approximation interval are separate
versioned quantities; a finite-regularization cost is not called exact
Wasserstein distance.

Runtime path variables use a fixed collocation grid, then interval-check every
segment. A segment is subdivided up to a manifest ceiling if feasibility or a
rewrite guard cannot be enclosed over its whole parameter interval. Failure
returns `Unresolved`; a feasible endpoint never licenses an uncertified
interpolation. The result therefore contains the actual certified piecewise
transport/control path

$$
\Gamma:[0,L_{\max}]\to\Theta,
\qquad \Gamma(0)=q_n,
$$

parameterized by cumulative directed path length, plus canonical packet crossing
points. A dense transport sidecar between checkpoints is allowed, but it is not
a State Packet. `advance` at an arbitrary cumulative length commits the greatest
certified checkpoint not beyond that prefix and reports consumed length plus
unspent credit. If no bounded checkpoint schedule satisfies representability
and continuity, the mode is `Unresolved`; path feasibility alone cannot mint an
over-cap packet.

### 8.2 Traveler-owned travel gating

The Model does not read velocity, input, a clock, or the Visualization. The
shared Traveler/controller measures canonical accumulated World Space arclength
$\Delta s_W$ along the normalized Traveler path, not endpoint displacement. It
computes new path-length credit

$$
L_{\rm credit}=\beta\,\rho_{\rm plan}
\int_{\text{new path segment}} r_{\rm local}(x(s))\,ds.
$$

Without a declared local factor this is
$\beta\rho_{\rm plan}\Delta s_W$. With one, the integral uses versioned
fixed-point quadrature split at canonical cell and plan boundaries, never one
sample per render frame.
$\beta>0$ converts World Space length to Possibility length; both Resonance
factors are dimensionless and cannot create negative progress.
The selected plan commits to the exact $\rho_{\rm plan}$, local-factor
capability revision, quadrature rule, and normalized request digest used by this
calculation.

The `NavigationAccumulator` stores the mode-specific `selected_plan_id`,
cumulative consumed path length, and unspent fixed-point credit.
`advance(plan, cumulative_length)` is idempotent and
uses a cumulative prefix coordinate, never a frame-local delta. The controller
consumes arclength in canonical packet/plan-boundary order; if a path segment
crosses a boundary, it replans there and applies the remaining arclength to the
new plan. This event-driven rule makes subdivision of the same input path into
frames irrelevant.

Unspent sub-quantum credit persists only while the same intent/Attractor digest
and path mode remain active. An explicit intent or mode change clears it; credit
cannot be banked under one desire and spent under another. Zero Exploration
supplies zero path-length credit and therefore zero Egress. Build-only
collision is resolved before the Traveler supplies its normalized canonical
World Space path to this accumulator. Walking into a loaded Build wall therefore
supplies zero arclength; walking around it supplies the actual detour length.
The Traveler, not the Visualization or Model, applies the Egress conversion.

This preserves the gameplay coupling while keeping it out of both the Model
mathematics and Visualization simulation.

The controller may coordinate a longer route, pause Egress, or apply a gameplay
threshold. None of those policies changes the State Packet denoted by a point,
the Model's distance, or the canonical path returned for explicit inputs.

### 8.3 Resonance

Resonance reports how much **certified continuation within this probe's bounded
horizon** the current intent has. Core Resonance uses only canonical Model
queries, the current State, normalized request, and directed-length horizon.

Let $L_{\rm req}=\mathcal Y-\gamma\mathcal A$. Resonance uses two diagnostic
probes distinct from the proximal choice in Section 8.1. Both have identical
directed control/length limits, packet complexity ceiling, and search horizon;
both **first maximize certified decrease in $L_{\rm req}$**, then minimize
directed length and the same closure keys only to choose among equally improving
paths. The **free** probe relaxes only the coupled physical/projective
constraints that can frustrate the requested observables; the **valid** probe
enforces them. Feasible-set inclusion therefore orders their best requested
improvements instead of relying on a clamp to conceal incomparable proximal
optima.

A relaxed point is not a Realization, so ordinary derived-observable queries are
not called on it. Every observable schema and Attractor mode admitted to the
free probe must provide a typed slack-augmented diagnostic extension
$\widetilde L_{\rm req}$ over its relaxed
control/moment space, with certified intervals and
$\widetilde L_{\rm req}=L_{\rm req}$ on valid global sections. Without that
extension, fit is `Unresolved`. A free diagnostic point can never be normalized
as a State Packet, mint an entity, or be captured in an Impression. Define

$$
I_{\rm free}=L_{\rm req}(q_n)-\widetilde L_{\rm req}(q^R_{\rm free}),
\qquad
I_{\rm valid}=L_{\rm req}(q_n)-L_{\rm req}(q^R_{\rm valid}),
$$

and

$$
r_{\rm fit}=\operatorname{clamp}_{[0,1]}
\frac{I_{\rm valid}}{I_{\rm free}}.
$$

The ratio is emitted only when its interval is certified and the lower bound of
$I_{\rm free}$ is positive. If the request is exactly satisfied, the result is
`Idle`; if the improvement interval straddles zero or bounded search found no
certified descent, the probe is `Unresolved` or reports zero fit with that
horizon status. It never turns failure of a finite probe into a proof that no
valid descent exists.

Let $C$ be the certified local control-to-requested-observable operator. The
manifest fixes Hilbert norms for controls and observables, so the normalized
singular values $\hat\sigma_i$ are dimensionless and invariant under a mere
unit/basis change. Requested schemas are canonically orthonormalized after equal
semantic terms are combined; duplicating an observable cannot manufacture an
extra direction. For $m$ independent requested directions,

$$
r_{\rm condition}^{\rm raw}=
\left[\prod_{i=1}^{m}
\frac{\hat\sigma_i}{\hat\sigma_i+\delta}\right]^{1/m}.
$$

This is a conditioned-controllability diagnostic, not proof of a globally broad
valley. For a valid Attractor-only plan with no requested observable basis,
$m=0$ and $r_{\rm condition}^{\rm raw}=1$. With neither an active Yearning nor an
Attractor the result is `Idle`, not a zero-dimensional optimization.

Rank deficiency is reported per requested direction, but it cannot veto a
certified compromise that improves other weighted requests or follows a
nonlinear/rewrite detour with zero local derivative. Whenever
$I_{\rm valid}>0$, the rate factor is

$$
r_{\rm condition}=r_{c,\min}+
(1-r_{c,\min})r_{\rm condition}^{\rm raw},
\qquad 0<r_{c,\min}<1.
$$

Thus poor local conditioning slows a certified plan but only absent/uncertified
fit stops it. Unmet directions remain visible in the objective intervals.

The other state/path factors are

$$
r_{\rm work}=\exp[-\mathscr L^\rightarrow[\Gamma^R_{\rm valid}]/\sigma_L],
\qquad
r_{\rm safe}=\exp[-\chi_{\rm topo}/\sigma_T],
$$

$\delta>0$ and $r_{c,\min}$ are dimensionless manifest constants,
$\sigma_L>0$ has Possibility-length units, and
$\sigma_T>0$ has the same unit as $\chi_{\rm topo}$. Also
$\hat\sigma_i\ge0$ and $\chi_{\rm topo}\ge0$ by construction.

Core Resonance is

$$
\rho_{\rm plan}=(r_{\rm fit}r_{\rm condition}r_{\rm work}r_{\rm safe})^{1/4}.
$$

A family may separately report a local opportunity multiplier
$r_{\rm local}\in[r_{\min},1]$, $r_{\min}>0$, from a fixed World Space
quadrature of canonical habitat/process channels. It reads no simulated
organism. The recommended travel rate is
$\rho_{\rm rate}=\rho_{\rm plan}r_{\rm local}$, but local opportunity is never
a path precondition and cannot change mathematical Reachability or a route
certificate. Families without it set it to one.

All factors and interval/status information are reported separately. A gameplay
threshold is optional. Zero fit stops committing this unresolved/current plan;
it proves neither global unreachability nor the absence of a path under a
different request. Cache readiness, resident tile count, frame time, worker
count, simulated animal density, Visualization, and hardware do not occur in
the definition.

### 8.4 Why the result can be surprising

Yearnings constrain observations and measures, not generator knobs. A request
for pervasive gliding body plans may be satisfied by forest canopies, cliff
ecologies, dense-atmosphere floaters, or several combinations. The constrained
transport solve pays for causal changes and preserves unrequested structure;
the maximum-entropy closure fills what the Traveler did not specify.

The bounded mode search can expose materially different compromises. Selection
is deterministic, but the relationship between an observed trait and the least-
directed-length complete world need not be obvious to a player.

## 9. Continuity as explicit correspondence

### 9.1 Transition Plan

For two canonical packets $q_0,q_1$ and requested World Space bounds, the Model
returns a `TransitionPlan` containing:

- the constitution-level law-space transport couplings $\pi_{c,\ell}$;
- fixed-point bounds on every requested continuous channel;
- bounded candidate correspondences for robust endpoint spatial features;
- ancestry mapping for species and ecological modes;
- matches, births, deaths, splits, and merges for canonical entities;
- topology events with World Space bounds, persistence, and event parameter;
- old/new classifier margins and unresolved intervals; and
- a recommended maximum presentation interpolation step.

Constitution transport does **not** imply that a trait-space coupling is a
spatial deformation. Endpoint features are extracted independently with their
own margins. An Interactive fused Gromov--Wasserstein solve may propose candidate
pairs from manifest-declared attribute and pairwise-structure costs, but its
nonconvex result is never authoritative. Higher-order/hyperedge matching is used
only if a separate bounded loss is actually defined; it is not smuggled into a
pairwise FGW citation.

Canonical correspondence builds a bounded candidate graph, converts all costs
and network-representable birth/death/split/merge penalties to integers, and
solves exact min-cost flow on each connected component of at most 64 nodes and
256 edges **after** any explicit node/arc expansion for linear or separable
piecewise-convex event costs. A nonseparable fixed-charge or higher-order event hypothesis is
handled only by complete bounded active-set/ILP enumeration whose leaves reduce
to those flow problems; exceeding that bound returns
`Unresolved(CorrespondenceSearchExhausted)` with an optional continuation.
Every omitted endpoint pair carries a certified cost lower bound proving that it
cannot improve the current exact upper bound, or the component is not called
complete. If those pruning bounds cannot reduce it to the ceiling, the result
is `Unresolved(CorrespondenceComponentTooLarge)`; exact flow is exact only for
the complete graph plus its certified exclusions.
Equal exact optima use the declared integer id order and expose a zero margin.
Larger components are split only by a compiler-certified independent partition;
solved partitions return `Partial { value, continuation }` pages until the final
`Complete` page. A component that cannot be independently partitioned returns
no authoritative match page: it either produces the typed unresolved entity
event requested by the caller, or the query status above.
Thus repeatability is not misrepresented as a proof that a nonconvex heuristic
found a global match. The resulting matching is a transition description, not
the identity of either endpoint.

### 9.2 Topological risk

Elevation, wetness, lake level, habitat suitability, and selected relation
fields have persistence diagrams. The guarantee is restricted to canonical
piecewise-linear, tame scalar fields on the same declared finite complex,
refinement level, and filtration convention. Under those hypotheses, stability
gives the useful bound

$$
d_B(\operatorname{Dgm}(f),\operatorname{Dgm}(g))
\le \|f-g\|_\infty.
$$

The Model incrementally tracks persistent features along $\Gamma$. A Vineyards
update handles filtration-order changes on a fixed complex. A rewrite,
remeshing, or cell insertion/deletion requires declared inter-complex maps and a
bounded zigzag-persistence kernel; without them the event is `Unresolved`.
Physical event parameters are located separately by interval root finding over
the certified path and threshold function. Persistence itself does not supply
an exact split/merge time.

A small low-persistence pond may be assigned little modeling risk; an impending
merger of two high-persistence ocean basins or collapse of a habitat corridor
may be assigned more. $\chi_{\rm topo}$ combines a **manifest-defined**
persistence-weighted event cost, classifier margins, and glue/solver residual
growth. Diagram stability is a mathematical bound; interpreting persistence as
physical or gameplay importance remains a tested modeling choice.

This detects changes that a smooth derivative can miss and tells the
Visualization where they occur. It does not promise every emergent ecological
change is predictable.

### 9.3 Presentation history is not authority

At every instant the Traveler has one canonical point $q(s)$. The canonical
Realization is $W(q(s),x)$ everywhere. A Visualization may:

- preserve old meshes and simulation state in a near zone;
- advect features using the Transition Plan;
- realize newly encountered far regions from the newer packet;
- stage births/deaths outside the view or behind ecological succession; and
- cross-fade unresolved fine detail.

Those choices form a Visualization continuity cache. Eviction, revisiting, or a
different Visualization may change how the transition is concealed, but not
the current packet, World Space address, canonical observation, Reachability, or
the result of an Impression query.

There is no history-dependent effective-coordinate field in the Model. A saved
Visualization session may preserve its blend state for exact local replay, but
that state is explicitly noncanonical. If an Impression claims such replay, its
`VisualizationReplayBundle` includes the continuity/blend snapshot and every
referenced old presentation asset/state chunk.

## 10. Attractors and community evidence

### 10.1 A future external service boundary

The current repository has no server, networking, or accounts. This section
defines a future record interface, not work performed by the Model runtime.

The service stores immutable evidence records keyed by content id:

```text
GenerationId, used extensions, State address, and optional Reachability certificate
optional World Space destination
visit or published-Impression evidence kind
independent-visitor proof class
optional published Build reference
expedition/time bucket
creator/service authorization and schema version
```

Attractor summaries are derived indexes. Raw id-keyed evidence remains the
authority. Removal is an immutable, separately content-addressed remove-wins
tombstone naming the evidence id; mutable moderation state is not embedded in
the evidence sketch. A tombstone is accepted only with the creator's signature
for that published record, an explicitly scoped moderator capability, or the
service authority for a visit record. Unauthorized tombstones are invalid, not
CRDT competitors. The union of authorized evidence and tombstones is
commutative, associative, and idempotent, and recomputation excludes tombstoned
evidence. A scalar counter that cannot explain or retract its mass is not
sufficient.

Ordinary evidence need not decay. A time-bounded community expedition is a
query over declared expedition/epoch records, not destructive aging of the base
ledger.

Accepted visit evidence must name a self-decodable representable packet and a
valid source/policy route segment or checkpoint chain. The service replays that
claim under the referenced Model before it contributes weight; missing chunks,
bounded-search `Unresolved`, or a certificate for a different source is rejected.

### 10.2 Distributions, not one averaged coordinate

Evidence is clustered under the transport/rewrite distance. Incompatible or
multimodal strata remain a mixture rather than being averaged into a possibly
nonexistent world. Within a compatible mode, its representative is a constrained
Fréchet representative under the composite World Loom metric,

$$
\bar q=\arg\min_{q\in\mathcal F}
\sum_i w_iD(q,q_i)^2.
$$

Compatible measure blocks reduce to Wasserstein/HK barycenter subproblems;
coefficients, grammar rewrites, and feasibility constraints do not. The bounded
representable candidate set gives an existence rule for V1. If several exact
minimizers remain, the summary publishes deterministic separate modes and their
zero/low margin rather than inventing a unique center.

The summary includes a certified evidence-cluster enclosure, support radius, and
path-mode mixture. Weak evidence supplies only a broad transport direction.
Independent visit evidence, published Impressions, and Builds change public
weights under versioned rules. Personal subscriptions are local per-Traveler
preferences applied after the public summary is verified; they are not public
evidence and do not change another Traveler's Attractor.

For navigation, a mixture with centers $\bar q_m$, strengths $\omega_m$, and
enclosure radii $h_m$ supplies the bounded soft potential

$$
\mathcal A(q)=\epsilon_A\log\left(1+
\sum_{m\in C_R}\omega_m
\exp\left[-\frac{D(q,\bar q_m)^2}
{2\epsilon_A(h_m^2+h_{\min}^2)}\right]\right).
$$

$\epsilon_A>0$ and $\omega_m\ge0$ are dimensionless manifest data;
$h_{\min}>0$ and $h_m$ have Possibility-length units, making $\mathcal A$ a
dimensionless declared potential.

$C_R$ is a candidate-center set frozen for the entire normalized request and
then committed into every `SelectedEgressPlan`. For each summary mode, planning
must either include both certified directed support from the source and a
distance evaluator valid along the whole candidate path, prove the mode
directionally unreachable/disconnected, or bound its omitted potential
contribution below the requested interval tolerance. If bounded discovery cannot do one of those,
the Attractor term is `Unresolved(AttractorSupportUnknown)` rather than changing membership halfway along
$\Gamma$. Canonical normalization makes the set invariant under equivalent
program encodings.

Strengths have a manifest total bound, so the added baseline makes $\mathcal A$
finite and nonnegative. If $C_R$ is empty, the potential is exactly zero and
reports `NoAttractorSupport`; an unreachable mode is not rewarded for producing
a logarithm of zero. Its certified path-mode subgradients bias the same transport
planner as Yearnings. A broad
$h_m$ gives only a weak regional direction; the formula does not turn a diffuse
cluster into an exact State address.

An Attractor may expose an exact destination only when:

1. one evidence-cluster mode contains the required evidence threshold;
2. every State allowed by its certified enclosure normalizes to the same
   representable State Packet at every active scale;
3. the immutable State chunks and Model manifest are available; and
4. any advertised route certificate validates for its named source and policy.

Otherwise it remains a distribution, even if its mean happens to land on a
packet. Reachability is source-specific: a contributor's certificate never
proves that a new Traveler can reach the destination. Egress always plans and
certifies a fresh path from the Traveler's current packet.

### 10.3 Anonymous abuse resistance

Visit reporting is enabled by default but may be disabled by the Traveler, and
is designed not to publish a stable visitor identity. A future service could
use the RFC 9576--9578 Privacy Pass roles:
Client, Attester, Issuer, and Origin. The minimum abuse-resistance contract is:

1. the Attester privately authenticates one persistent abuse-prevention
   credential or equivalent costly proof; it is not a public social identity;
2. a versioned private quota ledger permits at most $N$ attestations per
   credential, expedition, epoch, and coarse destination bucket;
3. the Issuer blindly issues a token bound through the challenge to that public
   policy tuple only after an eligible attestation; and
4. the Origin validates redemption and keeps a spent-token set, rejecting replay,
   malformed buckets, and contributions beyond the issuance policy.

This composition lets the public evidence record contain a bucket and one-time
authorization without a stable public identity. Published Impressions remain
separate signed content under the publishing service's creator policy.

Privacy Pass supplies the issuance--redemption unlinkability component only
under declared non-collusion, anonymity-set, and network-side-channel
assumptions. It does not
by itself supply the credential, quota ledger, Sybil resistance, or destination
privacy. The composed service enforces the declared per-credential quota, but
its Sybil resistance is only as strong as credential issuance. Bucket metadata
partitions anonymity sets; IP, TLS, and timing remain linkable side channels.
Stronger reusable anonymous quota protocols are deferred until a reviewed
construction and threat model exist. The Model itself verifies only State
representation and route certificates.

### 10.4 Dual-space travel

An Attractor target is a distribution in Possibility and, optionally, a World
Space address conditional on an exact destination packet. The distances remain
separate:

$$
\ell_P^\rightarrow(q,C_a)=
\inf_{\Gamma:q\leadsto C_a}\mathscr L^\rightarrow[\Gamma],
\qquad
d_W=d_{\rm World\;3D}(x,x_a).
$$

$D$ remains useful for symmetric clustering/neighborhoods, but source-specific
arrival uses directed route length to the compatible target mode $C_a$. Bounded
planning may return upper/lower bounds or
`Unresolved(SearchHorizonExhausted)` rather than the infimum. A
controller estimates remaining directed length and physical travel, then adjusts
Explore speed, Egress length allocation, or the next path segment so normalized
arrival times approach one another. It never adds meters to transport length or
constructs a hybrid metric. Either axis may arrive first; diffuse Attractors
cannot promise exact arrival on the Possibility axis.

## 11. Builds

A Build is optional authored Visualization content attached to an Impression.
Its versioned payload contains a semantic construction graph, quantized authored
dimensions/transforms, attachment points, interaction/collision tags, and
content references. The Model supplies the canonical terrain tangent frame,
gravity direction, and attachment feature at the Impression address.

A Build appears only while its owning Impression is explicitly loaded by that
Traveler. Publication, discovery, proximity, or presence in an Attractor index
does not auto-load it. Unloaded, hidden, or removed means absent from the
Visualization.

Across compatible Visualizations, these properties reproduce exactly:

- part topology and authored hierarchy;
- quantized dimensions, transforms, joints, and named attachment points;
- semantic material/role tags;
- authored interaction and collision intent; and
- the Possibility, World Space, and model-time attachment address.

Meshes, tessellation, shaders, texture style, particles, audio, animation
quality, and accessibility presentation may vary. A Visualization declares the
Build schema and semantic tags it supports and must reject or visibly degrade an
unsupported Build.

A terrain-modification Build overlays Visualization geometry and may affect that
Visualization's local collision or simulation. It never changes canonical
terrain, water flow, ecology, Realization queries, Model State, Yearnings, or
Reachability. Removing or hiding its Impression removes the overlay. A future
different concept of Model-authored terraforming would require Egress to a new
constitution; it is not a Build.

Loaded Build collision may stop or redirect Exploration, so the shared Traveler
uses the resulting post-collision canonical World Space path. This may change
how quickly a particular play session consumes an already selected Egress path,
just as choosing a physical detour does, but it cannot change the Model's
directed paths, Resonance, conversion per unit arclength, or Reachable
Possibility. For the same collision-resolved World path, every compatible
Visualization supplies identical credit.
Collision resolution uses the versioned quantized semantic geometry in the
shared controller; rendered meshes and GPU readback are never locomotion or
Egress authority.

Build moderation and creator identity are service concerns. Publishing a Build
adds removable Attractor evidence through its Impression record, not through a
mutation of the world.

## 12. Determinism, identity, and versioning

### 12.1 Canonical numeric contract

Canonical execution specifies:

- normalized packet encoding and checked integer totals;
- a portable content digest with test vectors, while equality still checks
  canonical bytes;
- counter-based integer innovations;
- fixed-point state, ground costs, and solver coefficients;
- canonical CSR ordering and balanced reduction trees;
- fixed iteration counts or interval-certified termination with a hard maximum;
- versioned integer polynomial/table implementations of `exp`, `log`, and
  square root;
- integer active-set extraction and lexicographic tie rules; and
- exact dependency keys including every upstream chunk and algorithm revision.

Failure to enclose the requested tolerance returns `Unresolved`; `ModelError`
is reserved for the invalid/integrity cases in Section 6.1. It may not return a
platform-dependent best effort at Canonical grade.

State identity is the normalized packet, not the result of a floating solve.
Entity identity folds `GenerationId`, State root, integer spatial key, kind,
only the consumed capability/channel revision, innovation ancestry key where
applicable, and ordinal. It never folds an unused `PackageId` extension. Exact manifestation ids
change with the world; ancestry and Transition correspondence express continuity
honestly.

### 12.2 Schedule and cache independence

Every job consumes immutable inputs and publishes only under a complete
dependency key. A canceled job publishes nothing. Canonical cold initialization
is specified; a warm start may accelerate an Interactive result but cannot
change a Canonical one. Uniqueness plus a residual enclosure is not sufficient
for bit identity near a quantization boundary. Canonical either executes the
specified cold operation path or accepts a warm result only when its certified
enclosure lies wholly inside one canonical quantization cell and exact
normalization/tie rules therefore fix the same bytes. Otherwise it reruns cold.
A cached factorization is an optimization only when its dependency and
operation-order contract establishes that same result.

Worker count, job order, budget, cancellation, resource tier, cache ceiling, and
eviction affect latency and refinement availability, never settled results.
Deterministic farthest-first or equivalent eviction may be reused from the
current runtime.

The CPU reference is authoritative. SIMD kernels must be lane-wise equivalent
to scalar twins. GPU computation may refine derived presentation, but no GPU
readback creates identity, steering, persistence, Resonance, or gameplay input.

### 12.3 Version axes

The design needs distinct identities for:

- `GenerationId`, `PackageId`, and used-extension manifest closure;
- individual law/operator algorithm revisions;
- State Packet encoding;
- Impression/atlas record format;
- attribute/capability schemas;
- Visualization and simulation definitions; and
- Build semantic and asset formats.

Any canonical drift changes the appropriate identity and dependency closure.
An old Impression is either reproducible with its old package, explicitly
migratable into a new observation, or unsupported. It is never silently
reinterpreted.

## 13. Incremental execution and performance

### 13.1 Bounded runtime work

The first implementation should fix these ceilings:

| Operation | V1 ceiling | Acceptance target |
|---|---:|---:|
| State validation/normalization | 64 KiB, 4,096 patches | < 1 ms native, < 3 ms wasm |
| Egress probe | 8 modes, 2 rewrites, 4 active levels, <= 12 typed transport blocks, <= 256 atoms/block, <= 4,096 packet nonzeros, 24 scaling iterations | < 4 ms native, < 10 ms wasm at 10 Hz **when resolved** |
| Coarse planetary section | 1,280 faces, 48 reduced variables | < 3 ms incremental |
| Primitive 33x33 tile + halo | 8 coupled channels, 3 refinement levels | < 0.8 ms native |
| Derived terrain-to-soil tile | fixed 6-block job graph | < 2.0 ms native |
| Local ecology | 32-cell batch, <= 256 trait atoms/cell, <= 16 coupling edges/atom | < 1.0 ms native per batch |
| Transition candidates | <= 512 endpoint features, bounded Interactive FGW proposal | < 5 ms background |
| Canonical correspondence page | components <= 64 nodes/256 edges; larger work paginated | < 5 ms native per page |
| Persistence page | one fixed <= 8,192-cell complex, <= 512 tracked pairs/events | < 5 ms native background |
| Canonical Impression confirmation | requested subject + dependency closure | < 50 ms background |

These are proposed gates, not measured claims. A prototype must replace them
with native and wasm benchmark ledgers. Every dimension above is a hard request
cap. Overflow returns `Partial` with a canonical continuation or `Unresolved`;
it never allocates until the machine happens to run out of memory.

Transport benchmarks report both latency and the fraction of adversarial and
representative blocks whose primal/dual gap resolves within 24 iterations. A
fast solver that usually returns `Unresolved` does not meet the gate. Likewise,
the transition ledger reports unresolved/low-margin component rates, not only
candidate-generation time.

No local query cost grows with travel history or explored area. Fixed coarse
planet work is bounded by the declared canonical base complex. Fine work grows
with requested tiles and accuracy. Scope reads measure totals and tail bounds,
not a planet scan.

### 13.2 Structural sharing and caches

Nearby State Packets are expected to replace a few Merkle chunks, but global
Scope or topology changes can invalidate many. Structural sharing is therefore
a benchmark hypothesis, not a complexity theorem. The ledger records median,
tail, and worst-case changed-chunk fractions for local and pervasive requests.
Dependency subroots allow genuinely unchanged law blocks, restriction maps, and
reduced bases to be shared.
Recommended bounded caches are:

1. validated/compiled State Packets;
2. coarse planetary sections and Schur boundary summaries;
3. primitive and derived tiles keyed by full subroot closure;
4. transport plans and factorization blocks;
5. feature/persistence summaries; and
6. canonical subject confirmations.

Cache residency is never evidence that an input is current. Local integration
checks the complete dependency key and sheaf restrictions.

### 13.3 Neutral crate shape

One possible clean-slate organization is:

```text
loom-core       packets, fixed point, hashes, grammar types, measures
loom-transport  ground costs, scaling/proximal solvers, route certificates
loom-spatial    icosahedral complex, discrete forms, restrictions, SPDE blocks
loom-world      geology, hydrology, climate, soil, ecology capabilities
loom-api        snapshots, queries, errors, transitions, navigation
loom-compiler   offline manifest/code/certificate generator (tool crate)
```

The runtime crates are platform-neutral. They do not open files, create threads,
read clocks, call graphics APIs, or use sockets. Platform crates provide storage
and execution through traits. `loom-compiler` is a development tool and may use
native facilities without entering the wasm dependency graph.

### 13.4 Agent-maintained Model compilation

The compiler is the main deliberate use of machine maintainers. A package build
can:

1. type-check units, conservation, marginals, and dependency closures;
2. generate sparse primal, dual, adjoint, restriction, and prolongation kernels;
3. run high-fidelity reference solves over parameter cells;
4. greedily grow reduced bases wherever residual certificates fail;
5. synthesize fixed-point ranges and transcendental approximations;
6. use equality saturation to optimize exact expressions while checking
   reference equivalence;
7. fuzz active-set margins, seams, and quantization boundaries;
8. emit Rust scalar/SIMD kernels, manifests, documentation, and parity fixtures;
9. execute native/wasm differential suites and long-route adversarial probes;
   and
10. sign and content-address the resulting immutable package.

Agents may propose new causal motifs, automatically split parameter cells, or
repair a failed reduced basis. They do not tune a live world after release. A
changed artifact has a changed manifest identity and must pass review gates.

Compilation is also bounded. Each operator family declares a maximum parameter-
cell count, basis dimension, sparse fill, certificate size, and reference-solve
budget. If adaptive splitting or basis growth exceeds any cap, the compiler
marks that package region unsupported instead of hiding a curse-of-dimensionality
failure in an enormous artifact.

Human comprehensibility is required at the contract and diagnostic level, not
at the level of manually inspecting every generated matrix entry. Every failure
still names a law node, dependency, unit, residual, parameter cell, and fixture.

## 14. Relationship to the other designs and current implementation

### 14.1 Why this is genuinely a third option

| Concern | Options 1 and 2 | World Loom |
|---|---|---|
| Possibility ontology | fixed low-dimensional smooth latent manifold | glued inverse-limit space of typed causal programs and measures, with certified strata where available |
| Coordinate | 24--48 fixed-point scalars | canonical sparse State Packet with structural sharing |
| Validity | smooth bounded decoder / feasible chart | type, unit, conservation, marginal, and numeric certificates |
| Geometry | pullback Riemannian metric | multiscale balanced/unbalanced transport plus rewrite length |
| Egress | local gradient/trust-region direction | proximal minimum-directed-length path with bounded alternate modes |
| Scope | statistic or global attribute target | absolute mass-ratio objective on an applicability measure |
| Realization | parameterized procedural fields | frozen innovations plus certified variational global section |
| World change correspondence | derivatives, margins, or a scalar wake | transport couplings, feature matches, and topology events |
| Ecology | fixed lineage pool / parameterized roster | trait measures, trophic couplings, and persistent modes |
| Extensibility | add axes/decoder outputs | add typed motifs, strata, refinement laws, and adapters |
| Maintenance | hand/fitted finite manifest | agent-compiled certificate-carrying operator library |

The price is substantially more infrastructure, larger addresses, and harder
solver engineering. The benefit is that causal regimes, relational Yearnings,
birth/death, and topology change are native rather than awkward exceptions to a
smooth vector space.

### 14.2 What can be retained from the prototype

The current implementation has valuable engineering patterns:

- integer-derived permanent identity and explicit versioning;
- declared layer dependencies and dependency-hash-gated integration;
- immutable generated data and sparse persisted deviations;
- schedule, cancellation, worker-count, and cache independence;
- portable scalar/SIMD differential testing;
- abstract storage and execution boundaries;
- CPU-authoritative gameplay with GPU-only derived presentation; and
- CRDT union-by-content-id record exchange.

World Loom should reuse those patterns and harness philosophy, not reinterpret
the current eight-component regional state as a Loom constitution.

### 14.3 Incompatibilities and ADR work

This is a clean-slate Model major that may coexist with the prototype during
development. Current anchors, preserves, routes, region histories, possibility
signatures, and species ids are not exact Loom addresses.

A migration tool may turn an old capture into an attribute-by-value Impression,
construct a Yearning, and search for a similar Loom state. The result is a new
world and must say so.

Accepted ADRs remain immutable. Landing this option would require successor ADRs
for at least whole-world Possibility, canonical species/organism meaning,
transport navigation, State Packet content addressing, transition
correspondence, and the exact location of travel gating. Existing crate-boundary,
CPU-authority, dependency, persistence, and schedule-independence ADRs should
continue to govern.

## 15. Risks and honest failure modes

1. **The state is not tiny.** A 64 KiB packet is cheap for a world but expensive
   for every standalone Impression. Chunk deduplication and bundles are required.
2. **A Merkle root is not the world.** Missing chunks make an address
   unavailable; validation must report this instead of consulting an implicit
   service.
3. **Optimal transport is expensive and regularization is biased.** Small typed
   histograms, bounded modes, primal/dual bounds, and unresolved results are
   essential. A dense transition coupling may also fail the sparse endpoint
   packet cap; endpoint projection needs its own certified error or must fail.
4. **The grammar may be too restrictive.** New natural phenomena may require a
   new stratum or major package. The design makes this explicit but cannot make
   every future extension backward compatible.
5. **Convexity can look too orderly.** Frozen multiscale forcing and isolated
   finite topology events provide complexity, but an overly strong uniqueness
   policy could sterilize the worlds.
6. **Nonconvex active sets can explode.** Every such block needs a hard candidate
   ceiling and must fail closed if the compiler cannot certify it.
7. **Local solves can lie about global physics.** Schur summaries and sheaf
   residuals must prove equivalence or return wider bounds; locality cannot be
   assumed from wishful thinking.
8. **Reduced bases fail near bifurcations.** Residuals and topology margins must
   trigger a larger basis or canonical fallback.
9. **Feature matching is not identity.** Symmetric worlds can have several equal
   correspondences. Interactive FGW can miss the best candidates; exact bounded
   flow and unresolved pagination contain but do not abolish that risk.
10. **Maximum entropy is a modeling choice.** It resolves underdetermination; it
    is not evidence that nature optimizes entropy in the asserted way.
11. **Generated proof artifacts can be wrong.** Independent checkers, frozen
    compiler versions, adversarial fixtures, and reproducible package builds are
    required. Generator output must not be trusted because an agent wrote it.
12. **Stratified route search is locally bounded, not globally complete.** A
    two-rewrite horizon can miss a distant path. Long-range planning must expose
    uncertainty and iteratively deepen offline.
13. **Anonymous reporting remains a security system.** Privacy tokens reduce
    linkability but do not eliminate Sybil, issuer-collusion, or traffic-analysis
    risks.
14. **This may miss the fun.** Scientific elegance is not gameplay proof. A toy
   slice must demonstrate evocative, legible change before full investment.
15. **Endpoint feasibility is weaker than path feasibility.** Collocation and
   interval subdivision can fail to certify a valid path even when one exists;
   the only honest bounded-runtime result is `Unresolved`.
16. **Projectivity changes the modeled physics.** Hard restriction constraints
   make one coherent synthetic hierarchy, not a convergent discretization
   theorem for arbitrary continuum PDEs. Exact state-dependent Schur structure
   may be too dense or costly for the proposed caps.
17. **Global intent is globally expensive.** Pervasive Scope can legitimately
   invalidate most dependency chunks; structural sharing cannot be assumed to
   turn every Egress step into a local edit.
18. **Compiler dimensionality can explode.** Parameter-cell subdivision,
   active-set combinations, and reduced bases grow combinatorially. Hard build
   caps must reject unsupported regions rather than ship unverifiable tables.
19. **Local opportunity is only a rate heuristic.** Keeping it out of
   Reachability preserves the ontology, but may make the travel rate feel
   disconnected from a valid route; playtesting must judge that tradeoff.

## 16. Proposed implementation sequence

### Stage 0: mathematical kernel

Build the authoritative two-domain planar kernel in neutral `loom-core` and
`loom-transport`, with harnesses in `tools`: 64-atom material and trait
measures, one optional grammar rewrite, fixed-point unbalanced transport, State
Packet normalization, and one path-constrained minimizing-movement step.
Machine-check permutation, quantization, native/wasm, and certificate replay
properties before adding terrain.

Exit criterion: exhaustive small cases plus 10,000 randomized intent
permutations and schedules demonstrate one canonical endpoint and valid path-length
certificates. Randomized tests are evidence, not proofs of the unbounded case.

### Stage 1: projective planet and primitive section

Implement the nested icosahedral address, discrete incidence operators, lifting
restrictions, counter innovations, one coupled SPDE field, and sheaf overlap
checks. Render only elevation/material.

Exit criterion: all face seams and parent/child restrictions are canonical;
local refinements lie inside global reference error bounds.

### Stage 2: inventories and hydrology

Add atmosphere/ocean inventories, water-level solve, shared convex macro flow,
local runoff refinement, feature persistence, and Transition Plans for terrain,
coasts, rivers, and lakes.

Exit criterion: mass is exact, topology changes are preceded by margins/events,
every robust entity in the bounded workload is matched or explicitly labeled
birth/death/split/merge/unresolved, low correspondence margins are visible, and
walking transitions never substitute presentation history for canonical state.

### Stage 3: climate, soil, and ecology

Create the offline reduced-basis pipeline, canonical temporal forcing, monotone
soil/productivity solve, trait measures, trophic couplings, species modes, and
canonical organism manifestations.

Exit criterion: Scope changes measured prevalence within returned bounds; live
organism simulation remains replaceable and cannot affect navigation.

### Stage 4: full navigation and records

Add multimode route search, all Influence types, Resonance diagnostics,
Impressions, Builds, State chunk bundles, and Attractor summary import/export.
Keep external service and anonymous-token issuance mocked behind record types.

Exit criterion: every in-scope Model and record-algebra invariant has a
machine-checkable harness; the mocked quota/replay/privacy boundary passes its
threat-model tests, while any real service still requires protocol review and
deployment audit.

### Stage 5: production integration

Integrate neutral crates through `viewer-host`, `pov-host`, and `renderer`;
implement continuity policy in the shared host while native and web shells stay
environment adapters; establish memory ceilings, optimize scalar/SIMD paths,
and run prolonged fast-travel tests. GPU kernels remain presentation-only.

Exit criterion: CI parity, determinism, schedule, cache, cancellation, tier,
memory, performance, and browser sign-off gates pass without reinterpreting an
old Model address.

## 17. Conceptual-invariant conformance

| # | Invariant | World Loom construction |
|---:|---|---|
| 1 | One Possibility point is one complete world | one normalized State Packet denotes one global constitution; $W(q,\cdot)$ covers the planet |
| 2 | One canonical current point; local history only for continuity | the Traveler holds one packet; all retention/blending is Visualization state |
| 3 | Possibility and World Space are independent | transport/rewrite length and icosahedral geodesic distance are separate |
| 4 | Egress and Exploration are distinct | constitution path versus World Space motion |
| 5 | Gameplay couples them, but neither Model nor Visualization owns coupling | Traveler derives explicit Egress-length budget from physical displacement |
| 6 | Realization carries stable meaning | versioned typed capabilities and Canonical queries |
| 7 | Simulation belongs to Visualization | Model exposes forcing, distributions, response modes, and representative samples only |
| 8 | Identical inputs reproduce outputs | canonical packets, counters, operation order, solvers, and error policy are specified; Visualization replay names its complete simulator/backend/input tuple |
| 9 | Impressions remain meaningful across Visualizations | packet chunks + canonical address + subject/value schema reproduce Model meaning |
| 10 | Yearnings are weighted and order-independent | one canonical multiset objective with integer aggregation and simultaneous solve |
| 11 | Scope is prevalence, not spatial falloff | an exact mass ratio over a declared applicable measure |
| 12 | Visualization does not change Reachable Possibility | paths, feasibility, certificates, and Resonance use Model inputs only |
| 13 | Builds are optional Visualization content | Build overlays attach to Impressions and never enter the constitution or Realization |
| 14 | Attractor evidence is historical, abuse-resistant, and removable | future service contract: authorized evidence+tombstones, private persistent credentials, versioned issuance quotas, Origin replay rejection, source-specific route validation, and derived summaries |

## 18. Answers to the conceptual model's open questions

| Open question | Proposed answer |
|---|---|
| Model State and compact coordinate | a normalized typed causal program plus a projective tower of fixed-point measures/coefficients, encoded as a bounded sparse State Packet |
| Metric, neighborhood, topology | multiscale balanced/unbalanced transport inside certified cells; zero-mass grammar morphisms glue the candidate quotient; directed length defines admissible paths |
| Continuity risk and chaotic divergence | solver/glue residuals, active-set margins, persistent-homology events, and explicit feature/ancestry correspondence |
| Realization contract and capability negotiation | typed versioned channels with units, inputs, time semantics, error grades, refinement behavior, and required/optional status |
| Canonical versus Visualization time | integer astronomy/seasonal forcing and response modes are Model time; weather, behavior, and other transient evolution are Visualization simulation time |
| Canonical organism information | exact manifestation id + ancestry key + trait/niche/relationship record + representative sample address and margin; live pose/behavior excluded |
| Attribute and Scope representation across Models | semantic observable ids with explicit adapters; Scope is a mass ratio over a schema-declared applicability measure |
| Hold strength | finite weighted transport resistance against the activation snapshot; hard only for separately declared safety invariants; validity always wins |
| Resonance role | bounded-horizon fit, conditioned controllability, work, and topology diagnostics; a strictly rate-only local opportunity factor; no finite probe is called a global reachability proof |
| Coordinated dual-space arrival | controller equalizes estimated remaining travel times/directed length without combining metrics; either axis may arrive first |
| Attractor exactness | every State in one certified evidence-cluster enclosure normalizes to one packet and its chunks are available; any route proof names a source, and the current Traveler replans |
| Anonymous proof and rate limiting | private persistent credential + Attester quota ledger + Privacy Pass issuance/redemption unlinkability + Origin spent-token state; Sybil strength and metadata/network leakage remain explicit deployment assumptions |
| Build reproduction boundary | semantic graph, topology, authored transforms, attachments, and interactions are invariant; assets/style may vary; terrain modification is a Visualization overlay |

## 19. Acceptance criteria

An implementation is not conforming until it demonstrates all of the following:

1. canonical packet encoding is injective over normalized representable states,
   bounded, round-trippable, and bit-identical on native and wasm;
2. every accepted state passes types, units, inventory, marginal, refinement,
   solver, and manifest checks;
3. one packet queried at arbitrary World Space addresses behaves as one whole
   planet, with no authoritative regional Possibility state;
4. all icosahedral seams, overlaps, and parent/child restrictions satisfy their
   fixed-point and interval contracts;
5. canonical recomputation after eviction equals the original result;
6. transport distance, path, and certificate are independent of solver task
   schedule and cache warmness;
7. every permutation of the same Yearning multiset produces identical intent,
   path modes, Resonance, and committed State Packet;
8. duplicate multiplicity, Hold activation state, Disable, applicability, and
   uncertainty behave exactly as specified;
9. singular/common/pervasive requests use absolute destination prevalence over
   a nonzero applicability measure, alter it within certified bounds rather than
   only local density, and report zero/uncertain denominators without epsilon;
10. incompatible requests yield a weighted feasible compromise or explicit no-
    progress result; no invalid packet is emitted;
11. standing still commits zero Egress while supplying the same explicit path-length
    budget commits the same prefix regardless of frame cadence;
12. core Resonance is unchanged across World locations, Visualizations, resource
    tiers, worker counts, cache capacities, and organism simulation densities;
    the optional local factor changes only recommended rate, never Reachability;
13. topology changes have prior margin/event reporting at the declared query
    tolerance, or are explicitly classified as unpredictable/unresolved;
14. a Transition Plan maps every robust requested entity or records a
    birth/death/split/merge/unresolved event with a margin;
15. an Impression bundle reproduces its canonical subject without a service and
    across two different compatible Visualizations;
16. blended historical presentation can never be captured as a false current-
    world canonical id;
17. Model time forcing and Visualization simulation can be independently
    changed without conflating their identities, and replaying the identical
    complete Visualization tuple produces bit-identical declared output;
18. an unloaded Build is absent, loading its owning Impression makes it
    available, hiding/removing/unloading makes it absent again, and none of
    those operations changes any canonical query, State root, planned Egress
    path, or Resonance value; loaded collision may only change the actual
    post-collision Exploration arclength consumed;
19. Attractor evidence merges by id, removed evidence ceases to contribute,
    duplicate/replayed/over-quota visit contributions are rejected, redemption
    records expose no stable public credential under the stated non-collusion
    test model, and diffuse multimodal evidence is not exposed as an exact
    coordinate;
20. each route-certificate segment rejects a tampered, unrepresentable, or
    over-length destination within its declared bound; whole-chain cost is
    reported honestly unless a separately specified checkpoint proof is used;
21. cancellation, scheduler, worker count, budget, cache, resource tier, scalar/
    SIMD path, and long-travel history do not change settled canonical output;
22. memory use reaches a plateau under long fast travel;
23. every approximate result contains an error/residual bound that encloses the
    canonical reference in randomized tests; and
24. the benchmark ledger meets or consciously revises the V1 ceilings in
    Section 13 before a production claim is made;
25. installing an unused optional minor extension leaves every prior packet,
    counter, canonical query, entity id, and golden fixture bit-identical;
26. all bounded collection APIs either complete, return a deterministic
    continuation, or report `Unresolved`; no transition/entity result silently
    truncates;
27. local search exhaustion is never serialized as `ProvenUnreachable`, and
    adversarial fixtures exercise reachable paths beyond the initial horizon;
28. ocean inventory, coastline summaries, drainage flux, and global ecology
    summaries obey their declared parent/refinement contracts;
29. path feasibility is certified for every returned segment, not inferred from
    endpoint validity, and every committable checkpoint satisfies packet
    support/rank and constrained-rounding contracts; and
30. native and web continuity tests traverse packet, topology, and rewrite
    boundaries without a global blank/reload, while eviction and blend history
    leave the canonical packet and queries unchanged.

## 20. Research basis

The World Loom combination is novel to this project, but its ingredients have
substantial foundations:

- Benamou and Brenier's [dynamic formulation of optimal
  transport](https://doi.org/10.1007/s002110050002) motivates transport energy as a flow,
  not merely a distance between vectors.
- Liero, Mielke, and Savaré's [Hellinger--Kantorovich
  distance](https://doi.org/10.1007/s00222-017-0759-8) and Chizat et al.'s
  [dynamic unbalanced-transport
  formulation](https://doi.org/10.1016/j.jfa.2018.03.008) and
  [scaling algorithms for unbalanced
  transport](https://doi.org/10.1090/mcom/3303) support transport with controlled
  creation/destruction and motivate parallel iterative solvers. World Loom adds
  its own bounded-work primal/dual certificate contract.
- Jordan, Kinderlehrer, and Otto's [variational formulation of the
  Fokker--Planck equation](https://doi.org/10.1137/S0036141096303359) motivates
  one minimum-movement optimization per Egress step; Cuturi's
  [Sinkhorn-distance algorithm](https://proceedings.neurips.cc/paper_files/paper/2013/hash/af21d0c97db2e27e13572cbf59eb343d-Abstract.html)
  supplies the practical entropic scaling pattern.
- Sweldens' [lifting
  scheme](https://doi.org/10.1137/S0036141095289051) motivates exact
  coarse/detail refinement, while Desbrun, Hirani, Leok, and Marsden's
  [discrete exterior calculus](https://arxiv.org/abs/math/0508341) motivates
  topology-aware fields and conservation on cell complexes.
- Hansen and Ghrist's [spectral theory of cellular
  sheaves](https://doi.org/10.1007/s41468-019-00038-7) motivates explicit
  local-to-global restriction residuals.
- Lindgren, Rue, and Lindström's [SPDE link between Gaussian fields and sparse
  GMRFs](https://doi.org/10.1111/j.1467-9868.2011.00777.x) motivates correlated,
  nonstationary fields with sparse operators on a sphere; Hu et al.'s
  [systems of SPDEs](https://arxiv.org/abs/1307.1379) motivates the explicitly
  coupled extension. Neither source makes a fractional inverse solve local.
- Salmon et al.'s [counter-based parallel random
  generation](https://doi.org/10.1145/2063384.2063405) supports an addressable
  innovation thread independent of execution order.
- Cohen-Steiner, Edelsbrunner, and Harer's [stability of persistence
  diagrams](https://doi.org/10.1007/s00454-006-1276-5) and the
  [Vines and Vineyards](https://doi.org/10.1145/1137856.1137877)
  algorithm motivate topology-aware risk and event tracking on fixed
  complexes; Carlsson and de Silva's [zigzag
  persistence](https://doi.org/10.1007/s10208-010-9066-0) motivates the
  explicitly mapped changing-complex case.
- Vayer et al.'s [fused Gromov--Wasserstein distance for structured
  data](https://proceedings.mlr.press/v97/titouan19a.html) motivates matching
  both entity attributes and relation structure.
- Veroy, Rovas, and Patera's [certified reduced-basis
  method](https://www.numdam.org/articles/10.1051/cocv:2002041/) motivates expensive offline
  agent compilation with cheap online solves and rigorous output bounds.
- Agueh and Carlier's [Wasserstein
  barycenters](https://doi.org/10.1137/100805741) motivate Attractor centers in
  measure space rather than arithmetic means of addresses.
- The IETF [Privacy Pass architecture](https://www.rfc-editor.org/rfc/rfc9576),
  [challenge/redemption protocol](https://www.rfc-editor.org/rfc/rfc9577), and
  [issuance protocols](https://www.rfc-editor.org/rfc/rfc9578) provide a reviewed
  starting point for issuance--redemption unlinkability. They do not by
  themselves solve quotas, Sybil resistance, destination privacy, or network
  side channels.

These references justify useful mathematical and computational tools. They do
not prove that their proposed composition is correct, fun, performant, or
scientifically accurate. The staged acceptance harnesses must establish those
claims for this Model.

## Appendix A: illustrative packet and runtime types

```rust
/// Canonical bytes are the identity; the root accelerates sharing and lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatePacket {
    pub generation: GenerationId,
    pub used_extensions: Box<[ExtensionId]>,
    pub format_version: u16,
    pub program: ProgramNormalForm,
    pub patches: Box<[LawPatch]>,
    pub root: StateRoot,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LawPatch {
    MeasureAllocation {
        level: u8,
        law: LawId,
        atom: AtomId,
        /// Valid range 0..=2^24 denotes the fraction 0..=1.
        fraction_q24: u32,
    },
    Coefficient {
        level: u8,
        law: LawId,
        coefficient: CoefficientId,
        signed_value_q: i64,
    },
    SparseRelation {
        level: u8,
        law: LawId,
        support: Box<[RelationMass]>,
    },
}

#[derive(Clone, Debug)]
pub struct EgressPlan {
    pub source: StateRoot,
    pub intent: IntentDigest,
    pub modes: Box<[EgressMode]>,
}

#[derive(Clone, Debug)]
pub struct EgressMode {
    pub id: ModeId,
    pub path: CertifiedPath,
    pub resonance: ResonanceBreakdown,
    pub max_path_length_q32: u64,
}

#[derive(Clone, Debug)]
pub struct SelectedEgressPlan {
    pub id: SelectedPlanId,
    pub source: StateRoot,
    pub intent: IntentDigest,
    pub mode: ModeId,
    pub path: CertifiedPath,
    pub resonance: ResonanceBreakdown,
    pub rate_contract: RateContractDigest,
    pub max_path_length_q32: u64,
}

#[derive(Clone, Debug)]
pub struct TransitionPlan {
    pub from: StateRoot,
    pub to: StateRoot,
    pub law_couplings: Box<[SparseCoupling]>,
    pub feature_events: Box<[FeatureEvent]>,
    pub topology_events: Box<[TopologyEvent]>,
    pub channel_bounds: Box<[ChannelBound]>,
}
```

The actual implementation should use caller-owned arenas or capacity-bounded
containers on hot paths. The sketch shows semantic ownership, not final memory
layout.

## Appendix B: one navigation tick

Given current packet $q_n$, active Yearnings $Y$, Attractor summaries $A$, a
`NavigationAccumulator`, and a newly traversed canonical World Space path
segment after loaded-Build/other Exploration collision resolution:

1. validate and canonicalize the Yearning multiset;
2. convert captured attributes and Scope into measure/moment penalties;
3. enumerate the bounded applicable program path modes;
4. solve the proximal Egress candidates and the separate free/valid Resonance
   diagnostic probes with canonical reductions;
5. compute objective intervals, transport plans, topology risk, and Resonance;
6. return non-dominated modes, then explicitly select one mode to mint a
   `SelectedEgressPlan`;
7. let the Traveler integrate the segment under that selected plan with
   canonical cell-boundary quadrature, compute new credit
   $\beta\rho_{\rm plan}\int r_{\rm local}(x(s))ds$, and add it to the same
   `selected_plan_id`;
8. call `advance` with cumulative credited path length, commit the greatest
   certified representable checkpoint not beyond that prefix, and retain the
   remainder as fixed-point credit;
9. verify its feasibility and emit a reachability segment certificate;
10. at a packet/plan boundary, replan and deterministically apply remaining
    arclength; build a bounded/paginated Transition Plan for requested resident
    World Space; and
11. let the Visualization schedule refinement and continuity presentation.

Every Model operation in the tick is a pure function of explicit canonical
inputs. Physical travel supplies permission to advance; it does not become a
coordinate in Possibility.
