# The Infinite World Exploration World Model

This document describes the world model implemented in this repository through
Phase 6. It starts with a non-technical account, then gives a formal model, and
finally walks through the algorithms and data structures component by
component. The final section records implementation concerns and promising
improvements found during review.

The distinction between **implemented** and **planned** matters. The repository
already contains a deterministic, streaming, top-down world prototype with
environmental layers, aggregate ecology, procedural species, steering,
persistence, routes, and a debug renderer. It does not yet contain the planned
3D game renderer, moving or behaving organisms, a browser world runtime,
networking, multiplayer, photography, or a community service. The debug map is
the current playable shell, not merely a visualization of a finished 3D game.

## 1. Non-technical overview

The project is an exploration game about travelling through landscapes and
through *possibilities* at the same time. It does not treat possible worlds as
separate levels selected by portals or loading screens. Instead, every place
has a natural tendency toward a particular kind of reality, and the player can
gradually bend that tendency while travelling.

The landscape is reconstructed from its coordinates. Mountains, rock
provinces, river routes, climate, soil, vegetation, ecosystems, species, and
nearby organisms are generated in a repeatable chain. The engine therefore
does not need to save an infinite world. It saves the comparatively small set
of things the player has changed or named and regenerates the rest when needed.

The player influences the world with **anchors**. An anchor remembers selected
traits of a discovery: for example, the size and luminosity of an organism, the
wetness of a river, or the hardness of a rock formation. Emphasizing an anchor
pulls future regions toward the remembered traits; suppressing it pushes them
away. Several anchors can overlap, after which a set of plausibility rules
prevents combinations such as giant animals without enough primary
productivity or lush vegetation without enough water.

Change is spatially staged to preserve the illusion of one continuous world.
The ground close to the player is pinned and does not regenerate. A transition
band farther out changes gradually, and the far field is free to converge more
quickly. Merely waiting does not transform reality: movement supplies the
transition budget. Local life supplies a second gate called **resonance**, so
dense and varied ecosystems permit more transition than barren places.

The engine models ecology hierarchically rather than simulating every creature.
Each 256-unit region has coarse fields such as temperature, fertility,
vegetation density, herbivore pressure, and predator pressure. Similar habitat
cells share a small procedural species roster and food web. Only cells near the
player realize individual organism instances, using local habitat and
vegetation plus the shared roster's biomass weights. Leaving the area discards
the instances; returning reconstructs them from deterministic inputs.

Routes and preserves turn exploration into durable structure. A preserve pins
selected regions to their recorded possibility states. When preserves overlap,
the runtime retains every contributor and the lowest immutable content id owns
the effective state; deleting that record reveals the next contributor.
An expedition route stores samples of physical and possibility-space travel.
Following a route
creates weak anchors toward its remembered ecological character rather than
replaying the old world exactly. Named discoveries, routes, and preserves can
be exchanged in atlas bundles and merged without a central server.

The current result is best understood as a rigorous world-model prototype. It
machine-checks important cases of continuity, deterministic regeneration,
incremental invalidation, ecological hierarchy, steering, sparse persistence,
and scale-aware scheduling on a false-colour 2D map. The artistic and
behavioral fidelity envisioned by the project overview remains future work.

## 2. The abstract world model

### 2.1 State at a glance

At any moment, the implemented world is the combination of:

1. a coordinate-defined base field over an effectively unbounded region grid;
2. an eight-dimensional possibility state for each authoritative resident
   region, whether its derived fields are active or capacity-parked;
3. player bias, anchors, route attraction, and preserve contributor sets;
4. a deterministic nine-layer generation graph evaluated for quantized region
   states;
5. memoized habitat rosters and transient near-field organisms;
6. sparse persistent records; and
7. derived CPU/GPU presentation that never feeds back into authoritative state.

The main flow is:

```text
coordinates -> base possibility field -> bias + anchors -> plausibility projection
                                                    |
                                                    v
                 player distance -> regional target state
                                                    |
                         travel x resonance --------+
                                                    v
                                      regional realized state
                                                    |
                                             quantization
                                                    |
                                                    v
 terrain -----------------> climate -------------------------------+
 terrain + climate + macro drainage -> hydrology                   |
 terrain + geology + climate + hydrology -> soils                  |
 terrain + climate + hydrology + soils -> biome                    |
 climate + soils + biome -> vegetation ----------------------------+
 climate + soils + biome + vegetation -> ecology -> near organisms
```

There is no complete materialized world behind the authoritative streaming
window. Inside it, field capacity may park reproducible tiles while retaining
the small regional transformation history. Crossing the unload radius removes
that authority; an ordinary unpreserved region loaded again later starts at the
target implied by the then-current steering context. Sparse preserve
contributors and run-local session snapshots are the reconstruction exceptions.

### 2.2 Space and scale

Let a level-0 region be indexed by

$$
r=(r_x,r_y)\in\mathbb Z^2.
$$

One region edge is

$$
R=256
$$

world units. A continuous world position $x=(x_x,x_y)$ maps to

$$
r(x)=\left(\left\lfloor\frac{x_x}{R}\right\rfloor,
           \left\lfloor\frac{x_y}{R}\right\rfloor\right).
$$

`RegionCoord` also carries a hierarchy level $\ell$. A level-$\ell$ region
covers an edge of $R2^\ell$ world units and has the parent
$(r_x\mathbin{\gg}1,r_y\mathbin{\gg}1,\ell+1)$. In the current runtime,
ordinary field tiles are all level 0. Level 4 is used for macro drainage, so
one macro tile has a 16 by 16 region core.

At the production default, each ordinary field tile is a row-major
$n\times n$ array with

$$
n=32,
$$

making one authoritative sample cell 8 world units wide. The resolution is
configurable, and several harnesses use smaller values. At any resolution,
cell $(c_x,c_y)$ is sampled at its center:

$$
x_{r,c}=R(r_x,r_y)+\frac{R}{n}(c_x+\tfrac12,c_y+\tfrac12).
$$

The grid is conceptually infinite but technically bounded by signed 32-bit
region coordinates and floating-point spatial precision. It is enormous for a
game, but not mathematically unbounded.

### 2.3 Possibility space

Every region carries a vector

$$
p=(P,C,G,H,E,M,B,A)\in[0,1]^8,
$$

whose coordinates are:

| Symbol | Domain | Current meaning |
|---|---|---|
| $P$ | Planetary | ocean/land and broad planetary tendency |
| $C$ | Climate | warm versus cold baseline |
| $G$ | Geology | tectonic relief and rock expression |
| $H$ | Hydrology | water supply and wetness tendency |
| $E$ | Ecology | vegetation and population intensity |
| $M$ | Morphology | organism body-scale expression |
| $B$ | Behavior | activity and aggression expression |
| $A$ | Aesthetics | hue and luminance expression |

There is currently one scalar per domain. The richer sub-vectors discussed in
the design documents are not implemented.

#### The unsteered field

The base field uses a control-point lattice spaced every $s=8$ regions. A
control point at integer coordinate $(i,j)$ gets an integer seed from the world
algorithm version, lattice spacing, and coordinate. A portable SplitMix64
stream generates its eight values uniformly in $[0,1)$. For a region inside a
lattice cell, the four surrounding vectors are bilinearly interpolated.

With Euclidean division

$$
i=\left\lfloor r_x/s\right\rfloor,\qquad
j=\left\lfloor r_y/s\right\rfloor,
$$

and fractions

$$
u=(r_x\bmod s)/s,\qquad v=(r_y\bmod s)/s,
$$

the base vector is

$$
F(r)=(1-v)\big((1-u)C_{i,j}+uC_{i+1,j}\big)
     +v\big((1-u)C_{i,j+1}+uC_{i+1,j+1}\big).
$$

This produces smooth variation over roughly 2 km at the current scale. It is a
uniform sparse recipe, not the adaptive possibility quadtree proposed for a
later version.

#### Quantization

Environmental and Ecology tile generators never consume a live possibility
float directly. Each used domain is quantized into $Q=4096$ buckets:

$$
q(z)=\min\big(4095,\lfloor4096\,\operatorname{clamp}(z,0,1)\rfloor\big),
$$

and a generator reconstructs the bucket center

$$
\hat z(q)=\frac{q+\tfrac12}{4096}.
$$

Thus small movement inside a bucket changes the region's simulation state but
does not regenerate tiles. A bucket crossing is the discrete event that can
invalidate generation. The same grid is reused at the persistence boundary for
portable possibility signatures.

### 2.4 Steering and target states

For a region center $x_r$, direct player bias $b\in\mathbb R^8$ first modifies
the base field componentwise:

$$
b_r=\operatorname{clamp}(F(r)+b,0,1).
$$

An anchor $a$ has a position $x_a$, target $t_a$, domain mask, polarity,
strength $s_a$, and radius $R_a$. Its radial influence at $x$ is

$$
w_a(x)=
\begin{cases}
\operatorname{clamp}(s_a,0,1)
\left(1-\frac{\lVert x-x_a\rVert^2}{R_a^2}\right)^2,
& \lVert x-x_a\rVert<R_a,\\
0,&\text{otherwise.}
\end{cases}
$$

For each possibility dimension, emphasize anchors use an
influence-weighted target mean

$$
\mu_e=\frac{\sum_a w_at_a}{\sum_a w_a}
$$

and combined saturating weight

$$
W_e=1-\prod_a(1-w_a).
$$

A suppress anchor reflects its target away from the unsteered base,

$$
\rho_a=\operatorname{clamp}(2b_r-t_a,0,1),
$$

and forms $\mu_s,W_s$ in the same way. The implemented polarity order is

$$
v_1=b_r+W_e(\mu_e-b_r),
$$

$$
v_2=v_1+W_s(\mu_s-v_1).
$$

Only masked dimensions participate. The intention is that each mean and
product is a symmetric function of the anchor set; a numerical caveat is
discussed in the review section.

The result is projected through an ordered plausibility map $\Pi$. After all
dimensions are clamped, the implemented constraints are:

$$
H'=\min\big(H,\;0.5+0.6P,\;0.2+0.5C+0.4P,\;1\big),
$$

$$
U=\tfrac12H'+\tfrac12C,
$$

$$
E'=\min\big(E,\;0.2+0.8U,\;0.4+0.6(1-G),\;1\big),
$$

$$
M'=\min\big(M,\;0.3+0.7E',\;1\big).
$$

$P,C,G,B,A$ are otherwise only clamped. The topological rule order makes the
projection idempotent: $\Pi(\Pi(p))=\Pi(p)$, and the neutral vector is a fixed
point. The final target of a normal region is

$$
T_r=\Pi\big(\operatorname{steer}(b_r,\mathcal A,x_r)\big).
$$

A preserve instead sets both target and realized state to a persisted
quantized signature and forces full stability. Every covering preserve is
retained by content id, and the numerically lowest id supplies the effective
signature. Startup, session restore, and import install a complete contributor
batch before reconciling each touched resident once, so reversing records in
one synchronization batch produces the same revision, tiles, and organisms.
Separate UI calls remain separate material events: final ownership is still
set-derived, while revision and organism epochs retain sequential history.

### 2.5 Realized state, stability, travel, and resonance

An authoritative resident stores both its target $T_r$ and realized state
$p_r$, including while its derived field working set is parked. Field-active
generation samples the realized state. Let $d_r$ be the distance from the
player to the region center, and let $r_n,r_f$ be the near and far stability
radii. Stability is

$$
S(d)=
\begin{cases}
1,&d\le r_n,\\
0,&d\ge r_f,\\
1-\left(3t^2-2t^3\right),&\text{otherwise},
\end{cases}
\qquad
t=\frac{d-r_n}{r_f-r_n}.
$$

The default low tier uses $r_n=3R$ and $r_f=9R$. The runtime recomputes this
cheap geometric stability for every authoritative resident on every update,
before resonance and convergence. Regions inside the near radius are therefore
pinned in the same frame they cross it; regions outside the far radius are
fully free. Capacity-parked residents still refresh targets and converge in the
same ordered authoritative pass as field-active residents.

The convergence gate is built from near-field organisms. Let $n\le N$ be the
number of nearest nodes actually selected:

* density is $D=\min(n/8,1)$;
* species diversity $V$ is normalized Shannon entropy;
* distance quality $L$ is the mean of
  $\operatorname{clamp}(1-d_i/r_n,0,1)^2$;
* anchor compatibility $K$ is one minus the influence-weighted mean absolute
  difference between local realized state and masked anchor targets; and
* canopy occlusion is $O=\operatorname{clamp}(1-0.25e_v,0.7,1)$ for local
  vegetation density $e_v$.

If species $j$ occurs $m_j$ times among the $n$ nodes and $k_s$ species are
distinct, diversity is

$$
V=
\begin{cases}
0,&k_s<2,\\
-\displaystyle\frac{\sum_j(m_j/n)\ln(m_j/n)}{\ln k_s},&k_s\ge2.
\end{cases}
$$

Distance quality is

$$
L=\frac1n\sum_{i=1}^{n}
\operatorname{clamp}(1-d_i/r_n,0,1)^2.
$$

For every anchor that reaches the player, let $M_a$ be its nonempty set of
masked dimensions and

$$
\delta_a=\frac1{|M_a|}\sum_{j\in M_a}|p_{r,j}-t_{a,j}|.
$$

Compatibility is

$$
K=\operatorname{clamp}\left(
1-\frac{\sum_a w_a\delta_a}{\sum_a w_a},0,1\right),
$$

and is defined as one when the denominator is zero. The five terms combine as

$$
\rho=\operatorname{clamp}\left(
D(0.5+0.25V+0.25L)(0.6+0.4K)O,0,1\right).
$$

If player travel during the update is $\Delta x$, the global convergence rate
is

$$
\alpha=\min\big(\alpha_{\max},
k\max(\Delta x,0)\rho\tau\big),
$$

where the defaults are $k=0.01$, $\alpha_{\max}=0.2$, and
$\tau=0.35$ in transition mode or $1$ otherwise. Up to the per-frame
convergence budget, eligible regions are processed farthest-first and update by

$$
p_r^{\text{next}}=p_r+(1-S(d_r))\alpha(T_r-p_r).
$$

The update is a linear interpolation, not a physical simulation. Zero travel
or zero resonance gives exactly zero convergence. When the float vector
changes, the region revision increments; when one or more quantized buckets
change, the declared dependent layers become dirty.

### 2.6 Generated world as a dependency graph

For each region $r$ and layer $\ell$, generation has the abstract form

$$
Y_{r,\ell}=G_\ell\left(
r,\;\hat q_{D_\ell}(p_r),\;
\{Y_{r,d}:d\in\operatorname{deps}(\ell)\}
\right).
$$

$D_\ell$ is the set of possibility domains read directly by the layer. A
layer sees bucket centers for those domains and neutral 0.5 for every other
domain. Its dependency key is an ordered 64-bit fold of:

```text
world algorithm version
layer id and layer algorithm revision
region x, y, and hierarchy level
field resolution
directly-read possibility buckets in domain order
input-layer dependency hashes in declaration order
```

The current expected key is derived recursively from authoritative region
domains, effective revisions, resolution, and declared input keys even when an
input tile is absent from the cache. Drainage's expected macro key is likewise
computable without a resident macro tile. Input hashes therefore include
upstream provenance, so an upstream change changes downstream keys, absent a
64-bit collision.

The implementation checks both stored tiles and completed results against that
current expected key. A completed result must also match the key captured at
dispatch, while its current job id is the separate dispatch-identity gate. A
dirty bitset is only a scheduling hint and cancellation only saves work.
Dispatch readiness remains material: before deferring a consumer, the runtime
demand-repairs any missing or stale cached input through the declared graph.

The implemented directed acyclic graph is:

| Id | Layer | Direct domains | Declared layer inputs | Cached result | Cost |
|---:|---|---|---|---|---:|
| 0 | Terrain | Geology, Planetary | none | elevation `f32` | 2 |
| 1 | Geology | Geology | none | hardness `f32` | 2 |
| 2 | Drainage | none | Terrain revision | macro flow direction + accumulation | 17 |
| 3 | Climate | Climate, Hydrology, Planetary | Terrain | temperature, moisture | 1 |
| 4 | Hydrology | Hydrology, Planetary | Terrain, Drainage, Climate | river, wetness | 1 |
| 5 | Soils | none directly | Terrain, Geology, Climate, Hydrology | depth, fertility | 2 |
| 6 | Biome | none directly | Terrain, Climate, Hydrology, Soils | biome id `u8` | 1 |
| 7 | Vegetation | Ecology | Climate, Soils, Biome | density, canopy height | 1 |
| 8 | Ecology | Ecology, Morphology, Behavior, Aesthetics | Climate, Soils, Biome, Vegetation | herbivore pressure, predator pressure, diversity, dominant roster index | 2 |

Layer ids are topological. Scanning dirty bits in ascending order therefore
visits dependencies before dependents. Drainage is special: it does not read a
terrain tile or the live possibility state. Its declared Terrain edge carries
the Terrain algorithm revision, while its actual routing elevation comes from
the anchor-free base possibility field.

That special edge is intentionally coarser than the data dependency. A live
Geology or Planetary bucket change dirties Terrain and therefore propagates a
Drainage dirty hint, but the macro drainage hash does not contain those live
buckets. Its check normally clears this false positive without regenerating;
the invalidation ledger accounts for the exception explicitly.

### 2.7 Ecology as fields, shared archetypes, and instances

Ecology is a three-tier model:

1. environmental fields describe each cell;
2. a coarse habitat signature selects a shared species roster and food web;
3. the near window samples organism instances from local vegetation, the
   habitat, and roster biomass weights.

For a cell, the habitat signature is

$$
\sigma=(\text{biome id},\text{temperature band},
\text{moisture band},\text{fertility band}),
$$

with 12 biome ids, 6 temperature bands over $[-20,40]$ degrees C, 5 moisture
bands, and 4 fertility bands. At most $12\cdot6\cdot5\cdot4=1440$ signatures
exist. The signature deterministically creates at most 12 species, their
integer genomes, trophic roles, a predator-prey graph, and normalized biomass
shares.

The Ecology tile stores only compact aggregate values. A dominant-species
`u16` is an index into the signature's roster, not a global identity. Near the
player, each cell and resource-tier slot independently realizes an organism
with probability equal to vegetation density. A stable hash chooses whether
the organism exists, its species, and its jittered position; genome-plus-bias
arithmetic determines expression. These instances are discarded when their
region leaves the near window.

### 2.8 Persistence and sharing

Generated output is not persistent state. The vault stores only deviations and
player-authored meaning:

* quantized named discoveries;
* quantized expedition routes;
* preserves containing region coordinates and possibility signatures;
* a discovered-region bitmap; and
* one run-local session snapshot.

Shareable records contain integers and strings only. Their ids are 64-bit
content folds over immutable integer fields. Stores merge by union on id;
names and journals select the maximum `(sequence, content-hash)` rank, route
usage selects `max`, and seen bitmaps use bitwise union. These operations are
commutative, associative, and idempotent for non-colliding ids.

The session tier is deliberately different. It stores the exact IEEE float
bits for player state, anchors, and every authoritative resident region's
realized vector, stability, and revision, parked residents included. It is
local to one run/platform, is never included in an atlas bundle, and restores
those regions parked before live admission re-derives their targets and tiles.

### 2.9 Three grades of determinism

The code uses the word "deterministic" for three related but distinct
contracts:

1. **Portable integer identity.** Coordinate hashes, feature ids given the same
   complete integer key, control-point seeds, terrain-gradient seeds,
   lithology seeds, fixed-signature genomes, record ids, and record bytes are
   intended to match native and WebAssembly.
2. **Same-platform exact content.** Float field tiles, habitat classifications,
   live captures, resonance, and realized organisms reproduce bit-for-bit for
   the same inputs and schedule on one target, but are generally classified as
   presentation-grade across targets.
3. **Settled schedule independence.** Pure jobs and dependency keys make a
   quiescent scripted endpoint independent of executor, worker count, budgets,
   cancellation, and retarget amortization. Mid-journey state is explicitly
   allowed to differ because job timing changes when organisms become available
   to resonance. Field-cache capacity is narrower: with equal near-field
   prerequisites, authoritative regional history is compared and equal after
   every scripted frame even though derived field residency differs.

The distinction is essential. A permanent species id is portable *given the
same integer habitat signature*, but deriving that signature from float tiles
is presentation-grade. A shared discovery avoids the ambiguity by persisting
the already-derived integer identity and quantized target.

## 3. Detailed components

### 3.1 Coordinates, hashing, random sampling, and versioning

The platform-neutral foundation lives in
[`world-core`](crates/world-core/src/lib.rs). Spatial identity uses
`RegionCoord { x: i32, y: i32, level: u16 }`; local cell identity uses
`LocalPos { cx: u16, cy: u16 }`, flattened as $c_y n+c_x$. Ordered coordinates
and hierarchy levels are folded as integer bit patterns rather than float
positions.

The primitive integer mixer is SplitMix64. With unsigned 64-bit wrapping
arithmetic and

```text
gamma = 0x9e3779b97f4a7c15
c1    = 0xbf58476d1ce4e5b9
c2    = 0x94d049bb133111eb
```

it computes

$$
x'=x+\gamma,
$$

$$
z_1=(x'\oplus(x'\gg30))c_1,
$$

$$
z_2=(z_1\oplus(z_1\gg27))c_2,
$$

$$
\operatorname{SM}(x)=z_2\oplus(z_2\gg31).
$$

An ordered field fold is

$$
\operatorname{mix}(s,v)=\operatorname{SM}(s\oplus v\gamma).
$$

Different subsystems start from different fixed bases, then fold their fields
in a frozen order. Those bases are constants: the current interface exposes no
user-selectable world seed, so there is one canonical unmodified base world. A
general feature identity folds world version, region coordinate and level,
layer, feature index, and possibility revision. A SplitMix stream seeded from
such a hash supplies approximate random samples. `next_f32` uses the high 24
output bits divided by $2^{24}$. Stable feature ids are assigned only from
integer keys, not directly from this float draw. Some intended-stable topology
does nevertheless reach integer decisions through float-generated field
values; Section 4.1 examines that weaker indirect dependency.

There are two independent version axes:

* `WORLD_ALGORITHM_VERSION = 2` changes the identity of the generated base
  world; and
* `RECORD_FORMAT_VERSION = 1` changes the serialized record schema.

Each generation layer also has an `algorithm_revision`, currently zero for all
nine layers. A local algorithm revision invalidates only that layer and its
dependents, whereas changing the world version invalidates the entire generated
contract.

The central data-structure choice is ordered maps. Region state, tiles, macro
tiles, rosters, organisms, records, and in-flight jobs are keyed in `BTreeMap`s
or `BTreeSet`s. Stable iteration and explicit coordinate tie-breaks ensure that
count-based budgets choose the same work in the same order on repeated runs.

### 3.2 Possibility field and region-level state

[`PossibilityField`](crates/world-core/src/possibility_field.rs) stores only its
control-point spacing. It does not allocate the lattice: every control point is
recreated from its coordinate on demand. Eight successive RNG draws make a
control vector; bilinear interpolation makes neighboring region baselines
change gradually. The runtime subsequently applies plausibility projection,
so the actual neutral, anchor-free world is $\Pi(F(r))$, not the unconstrained
random field itself.

[`RegionState`](crates/world-runtime/src/region.rs) holds:

* stable coordinate;
* `current` and `target` possibility vectors;
* scalar stability;
* a wrapping revision counter;
* a nine-bit dirty-layer mask; and
* `Unloaded`, `Generating`, or `Ready` status.

The ordered region map is the sole authoritative set. `Unloaded` means that
authority exists with no admitted field working set; `Generating` and `Ready`
are field-active. A newly created ordinary authority computes its target and
sets `current = target`. Capacity reactivation instead retains `current` and
revision, recomputes target and geometry, marks every layer dirty, and rebuilds
derived fields. A restored session region recovers exact `current`, stability,
and revision as parked authority and follows that same live admission path.

### 3.3 Anchors and trait capture

[`Anchor`](crates/world-core/src/anchor.rs) is a compact influence record:

```text
world position, possibility target, domain mask,
Emphasize/Suppress, strength, falloff radius, source
```

Sources are an organism species id, landform, river, atmosphere, or manual
debug placement. Anchor falloff is compactly supported and continuously reaches
zero at its radius. Steering evaluates anchors at region centers; it is not a
per-cell influence field.

Trait capture is split between pure math in
[`capture.rs`](crates/world-core/src/capture.rs) and cache gathering in
[`RegionMap::capture_at`](crates/world-runtime/src/stream.rs). A capture target
is

$$
t_i=
\begin{cases}
\operatorname{clamp}(p_i+0.5\,\delta_i,0,1),&i\text{ is masked},\\
0.5,&\text{otherwise},
\end{cases}
$$

where $p$ is the covering region's realized possibility state and $\delta$ is
a bounded trait deviation.

For an organism, the implemented deviations are:

$$
\delta_M=\frac{\text{size class}}{7}-M,
$$

$$
\delta_A=\text{expressed luminance}-A,
$$

$$
\delta_B=\tfrac12(\text{activity}+\text{aggression})-B,
$$

$$
\delta_E=\frac{\text{trophic-tendency gene}}{255}-E.
$$

Environmental capture uses hardness for Geology, the maximum of river and
wetness for Hydrology, and normalized temperature
$\operatorname{clamp}((T+15)/50,0,1)$ for Climate. If no organism is available,
an Ecology capture can use vegetation density. The current Planetary capture
has no separate measured deviation; it records the region baseline. Captures
are presentation-grade because they read float tiles and realized organisms.

The eight fiction-facing `TraitCategory` variants currently map one-to-one onto
the eight scalar domains. Coloration maps to Aesthetics; morphology/scale to
Morphology; behavior to Behavior; ecological traits to Ecology; climate
affinity to Climate; landscape to Geology; waterways to Hydrology; and
atmosphere to Planetary.

### 3.4 Plausibility projection

[`project_plausible`](crates/world-core/src/anchor.rs) is a fixed analytical
projection, not an iterative ecosystem simulation or learned model. Its rules
only cap abundance-like dimensions. Steering can always reduce Hydrology,
Ecology, or Morphology, but cannot increase them beyond their supporting
conditions.

The rule interpretation is:

* Planetary supply and temperature cap liquid water;
* final water plus climate cap vegetation;
* high Geology is treated as active/exposed ground and caps canopy/ecology; and
* final Ecology caps body scale.

Because all dependencies point forward through the ordered formulas, one pass
is already a fixed point. Behavior and Aesthetics are unconstrained, and the
single Geology scalar stands in for both tectonics and the soil/wind proxy.

### 3.5 Region streaming and continuous transformation

[`RegionMap`](crates/world-runtime/src/stream.rs) owns the active sparse world.
The low-tier default radii are:

| Boundary | Radius | Meaning |
|---|---:|---|
| near | $3R=768$ | intended fully stable zone |
| far | $9R=2304$ | end of the smooth stability ramp |
| load | $12R=3072$ | create missing authority/admit eligible fields inside this circle |
| unload | $14R=3584$ | remove regional authority beyond this circle |

The load/unload gap provides hysteresis. Missing authority is created
nearest-first under the load budget, independently of field capacity. Field
admission is a separate nearest-first pass; near and contributor-covered
regions are exemptions, while ordinary field-active regions reserve their full
eventual payload. Regions converge farthest-first across the complete
authoritative set, because the far field is where change is intended to appear
first. Generation dispatch is nearest-first over field-active residents only.
Coordinate order breaks equal-distance ties.

One `update` performs these passes:

1. integrate completed jobs;
2. radius removal and capacity parking of derived fields;
3. create missing authority and admit eligible parked fields;
4. recompute all geometric stability, then budget target calculation;
5. construct resonance from the previous frame's organisms;
6. converge eligible realized states;
7. dispatch stale layers in topological order;
8. integrate again for the synchronous executor; and
9. when the L8 key is fresh, realize near-field organisms from settled habitat,
   vegetation, and roster inputs.

Travel is supplied explicitly by the caller instead of being inferred inside
the map. Streaming, target calculation, and completion of already-dirty
field-active work continue while stationary; only `current -> target`
convergence is travel gated. Parked authority participates in target refresh
and convergence but never dispatches generation. The convergence formula is
first-order frame-slicing invariant only approximately: two half-distance
lerps do not have exactly the same transient result as one full-distance lerp.

Possibility bucket flips use the declared domain-reader closure to mark dirty
layers. Revisions still record any material float-state movement, including
sub-bucket movement, but no longer determine tile staleness. Applying a newly
effective preserve winner follows the same separation: any exact realized
vector change advances revision once and retires old-revision organisms, while
only quantized bucket flips dirty tiles or cancel their in-flight work. Thus a
same-bucket snap to canonical centers changes the organism identity epoch but
keeps tile dependency hashes and jobs intact.

### 3.6 Resonance

[`resonance.rs`](crates/world-runtime/src/resonance.rs) turns nearby realized
organisms into a one-frame graph. `RegionMap::resonance_at` gathers all
organisms within the near radius, orders them by squared distance with species
and position tie-breaks, and truncates to the budget's node cap. The graph is
not cached or persisted.

Density dominates the formula, saturating at eight nodes. Entropy rewards a
mixture of species, distance rewards close nodes, compatibility rewards anchors
whose targets resemble the local realized state, and local canopy attenuates
the result. With no nodes, resonance is exactly zero. The next convergence pass
uses only the scalar strength; node details remain presentation/debug data.

The gate multiplies rather than adds to travel. This prevents a rich area from
transforming while the player stands still and prevents a barren crossing from
banking delayed change.

### 3.7 Routes through physical and possibility space

A [`RouteRecorder`](crates/world-runtime/src/route.rs) samples a journey after
each accumulated 192 world units, up to 1024 nodes. The first node is immediate.
Each node stores:

* position rounded to integer world units;
* the covering region's quantized *target* vector;
* transition cost `floor(255 * (1 - resonance))`;
* stability in an 8-bit band; and
* an order-independent summary of active anchors.

The anchor summary covers the player's explicit anchor slice. Route-derived
anchors are excluded even though their effects may already appear in the
recorded target and resonance cost. Route records also contain discovery ids,
a usage count, name, and journal. Difficulty is the arithmetic mean of node
costs divided by 255.

The stored target is not necessarily the world then visible to the player.
Near regions can remain pinned at `current` while their target retargets, so a
node may pair an aspirational possibility signature with a cost measured from
the currently realized ecology. Recording while old-route attraction is active
can also bake that attraction into the target while omitting it from the anchor
summary.

When route attraction is enabled, nodes from every route in the open vault that
lie within 768 world units become derived Emphasize anchors. They affect
Climate, Hydrology, Ecology, Morphology, Behavior, and Aesthetics, but never
Planetary or Geology. A node's peak pull is

$$
w(u)=0.35\left(0.35+0.65\frac{u}{u+4}\right),
$$

where $u$ is route usage. Nearby candidates are sorted by squared distance,
route id, and node index, then capped by the frame budget (32 by default).
They pass through exactly the same steering and plausibility projection as
player anchors.

[`RouteTracker`](crates/world-runtime/src/route.rs) treats one continuous stay
inside a route corridor as a leg. On corridor exit, the leg increments usage if
it visited at least 60% of the route's distinct nodes. Firing on exit debounces
camping or lingering.

`RouteGraph` is a rebuilt read-only view over all record nodes. A query scans
every node, computes Manhattan distance in the eight 12-bit possibility
buckets, sorts by distance/route/node, and returns the nearest $k$. The stored
signature seed determines build order but is not a metric index.

### 3.8 Field tiles, channel layout, and slope sampling

A [`FieldTile<T>`](crates/world-core/src/field.rs) owns a square row-major
`Vec<T>`, its resolution, and the dependency hash from which it was generated.
Tiles are immutable after integration and shared with workers through `Arc`.
`RegionTiles` is a structure-of-arrays container with 13 optional `f32` tiles,
one `u8` biome tile, and one `u16` dominant-species tile:

```text
elevation, hardness, temperature, moisture,
river, wetness, soil depth, fertility,
vegetation density, canopy height,
herbivore pressure, predator pressure, diversity,
biome id, dominant roster index
```

At 32 by 32, a complete region has 56,320 bytes of sample payload, before map,
`Arc`, and allocation overhead. A content hash folds tile provenance and every
sample's exact bit pattern; it is a replay oracle, not a portable identity for
float tiles.

Terrain slope used by Hydrology and Soils is a finite-difference magnitude:

$$
s=\sqrt{\left(\frac{z(x_1,y)-z(x_0,y)}{(x_1-x_0)\Delta}\right)^2+
        \left(\frac{z(x,y_1)-z(x,y_0)}{(y_1-y_0)\Delta}\right)^2},
$$

where $\Delta=R/n=8$. Interior cells use centered neighbors; tile-edge cells
use a one-sided difference from inside the same tile.

### 3.9 Terrain

[`terrain.rs`](crates/world-core/src/terrain.rs) implements five-octave
two-dimensional gradient noise. A hash of world version, octave, and integer
lattice coordinate selects one of eight gradients:

```text
(+/-1, 0), (0, +/-1),
(+/-1/sqrt(2), +/-1/sqrt(2))
```

For local lattice fractions $(u,v)$, each corner gradient is dotted with the
corner-to-sample displacement. Perlin's quintic fade

$$
f(t)=6t^5-15t^4+10t^3
$$

interpolates first in $x$, then in $y$, and the result is multiplied by
$\sqrt2$. Every octave also has a hash-derived offset in $[0,64)$ lattice
units on each axis so that octave zeros do not line up on a visible grid.

The fractal sum is

$$
N(x,y)=\frac{\sum_{o=0}^{4}2^{-o}
n_o\left(\frac{x}{4096/2^o}+\Delta x_o,
          \frac{y}{4096/2^o}+\Delta y_o\right)}
{\sum_{o=0}^{4}2^{-o}}.
$$

The denominator is $1.9375$. Elevation for a region's dequantized Geology and
Planetary buckets is

$$
z(x,y)=600\,N(x,y)(0.5+G)-120(P-0.5).
$$

Sea level is zero. $G$ therefore scales relief from 0.5 to 1.5 times the base,
while increasing $P$ lowers the land; the end-to-end Planetary swing is 120
units. Gradient selection is integer identity; interpolation and possibility
scaling are float presentation math. The possibility vector is constant across
an entire region's tile, not sampled per cell.

### 3.10 Geology

[`geology.rs`](crates/world-core/src/geology.rs) partitions the plane into
jittered Voronoi provinces. The lattice cell width is

$$
L_g=6R=1536.
$$

Each cell center is displaced by at most one quarter cell in both axes using
20-bit fractions from its integer seed. Because of that bound, searching the
surrounding 3 by 3 lattice cells is sufficient to find the nearest center.
Fixed loop order resolves the measure-zero equal-distance case.

The low three seed bits select one of eight lithology ids. Bits 8 through 23
give base hardness

$$
h_0=0.2+0.7\frac{b}{65536},
$$

and expressed hardness is

$$
h=\operatorname{clamp}\big(h_0(0.75+0.5G),0,1\big).
$$

Province boundaries and lithology ids ignore possibility. Only hardness
changes with Geology. The runtime caches hardness, while Soils recomputes the
possibility-independent lithology id directly from world coordinates.

### 3.11 Macro drainage

[`drainage.rs`](crates/world-core/src/drainage.rs) separates intended-stable
river *topology* from changing river *expression*. A level-4 macro tile has a
16 by 16 region core, a 16-region apron on every side, and therefore a 48 by 48
routing grid. Generation temporarily samples a 50 by 50 elevation grid so each
routing cell has a complete 3 by 3 neighborhood.

Routing deliberately ignores each region's live realized state. At the center
of every level-0 region, it samples

$$
p_b=\operatorname{requantize}(\Pi(F(r)))
$$

and evaluates Terrain under that anchor-free baseline. Elevation is then
rounded to integer centimeters:

$$
z_{cm}=\operatorname{round}(100z).
$$

For each of eight neighbors with strictly lower integer elevation, the descent
score is

$$
\operatorname{score}=
\begin{cases}
10(z_{here}-z_{there}),&\text{cardinal},\\
7(z_{here}-z_{there}),&\text{diagonal}.
\end{cases}
$$

The largest score wins; a coordinate hash chooses among equal candidates in
the fixed direction order East, Northeast, North, Northwest, West, Southwest,
South, Southeast. That order is part of deterministic tie resolution. A cell
with no lower neighbor receives `FLOW_NONE` and acts as a lake/wetland seed.
Strict descent guarantees an acyclic graph.

Accumulation begins at one for every routing cell. Cells are processed from
highest to lowest integer elevation and add their completed accumulation to
their downstream neighbor when that neighbor lies inside the 48 by 48 window.
Catchments entering from beyond the apron are therefore truncated. A
`DrainageTile` stores one `u8` direction and one `u32` accumulation per cell,
for 11,520 bytes of heap payload. Hydrology bilinearly interpolates the four
nearest accumulation cells at each fine sample.

The same world cell has a window-independent direction because direction uses
only its own 3 by 3 neighborhood. Accumulation is not window-independent:
overlapping macro tiles can see different truncated upstream areas. The
logarithmic river-width curve reduces, but does not remove, the resulting
macro-boundary discrepancy.

### 3.12 Climate

[`climate.rs`](crates/world-core/src/climate.rs) is a cheap per-cell expression
over elevation and the region buckets. Let

$$
z_+=\max(z,0).
$$

Sea-level temperature varies from -5 to 30 degrees C with the Climate scalar,
then uses a 0.0065 degree-per-unit lapse rate:

$$
T=-5+35C-0.0065z_+.
$$

Below sea level, moisture is one. On land,

$$
m=\operatorname{clamp}\big(0.15+0.55H+0.30P-0.0008z_+,0,1\big).
$$

There is no latitude, season, prevailing wind, rain shadow, or evolving
weather state. Climate is a static deterministic field for one realized
possibility state.

### 3.13 Hydrology expression

[`hydrology.rs`](crates/world-core/src/hydrology.rs) makes fixed drainage
topology visibly wetter or drier. It receives both Climate channels but the
current formula uses moisture, not temperature. With macro accumulation $a$,
channel intensity is

$$
r_0(a)=
\begin{cases}
0,&a\le5,\\
\sqrt{\operatorname{clamp}\left(
\frac{\ln(a/5)}{\ln(400/5)},0,1\right)},&a>5.
\end{cases}
$$

For land, expressed river strength is

$$
r=\operatorname{clamp}\big(r_0(a)(0.55+0.45m)(0.6+0.8H),0,1\big).
$$

Low-slope ponding is

$$
g=\operatorname{clamp}(1-s/0.05,0,1),
$$

and wetness is

$$
w=\operatorname{clamp}\big(
0.40m+0.30r+0.20g(0.3+0.7m)+0.15H+0.05P,
0,1\big).
$$

Open water instead has river strength zero and wetness one. Possibility can
therefore widen or weaken a river and change marsh extent without changing the
macro flow direction.

### 3.14 Soils

[`soils.rs`](crates/world-core/src/soils.rs) reads no possibility domain
directly; all sensitivity is inherited from Terrain, Geology, Climate, and
Hydrology. On land, flatness and softness are

$$
f=1-\operatorname{clamp}(s/0.4,0,1),
$$

$$
u=1-0.7h.
$$

Soil depth is

$$
d_s=\operatorname{clamp}(fu+0.25w,0,1).
$$

The temperature window around 15 degrees C is

$$
q_T=\max\left(1-\left(\frac{T-15}{25}\right)^2,0\right),
$$

and lithology id $l\in[0,7]$ supplies bias

$$
q_l=0.85+0.30l/7.
$$

Fertility is

$$
f_s=\operatorname{clamp}\big(
\sqrt{d_s}(0.3+0.7m)q_Tq_l,0,1\big).
$$

Underwater soil depth and fertility are both zero. This is a plausibility
formula, not erosion, deposition, chemistry, or temporal soil simulation.

### 3.15 Biomes

[`biome.rs`](crates/world-core/src/biome.rs) emits one of 12 `u8` classes using
ordered rules. Earlier rules win:

1. elevation below zero -> Ocean;
2. temperature below -10 -> Ice;
3. river strength at least 0.5 -> River;
4. wetness at least 0.78 -> Wetland;
5. temperature below -2 -> Tundra;
6. elevation above 850 -> Bare;
7. otherwise use the climate body below.

The climate body is:

| Condition, in order | Biome |
|---|---|
| moisture < 0.18 | Desert |
| temperature < 5 | Taiga |
| moisture > 0.75 and temperature > 18 | Rainforest |
| moisture > 0.45 | Temperate Forest |
| moisture > 0.28 | Shrubland |
| otherwise | Grassland |

Rainforest, Temperate Forest, or Taiga is demoted to Shrubland when soil depth
is below 0.2. The Biome layer receives the whole Soils result but does not read
fertility. Classification thresholds operate on float tile values and are
therefore presentation-grade at exact boundaries.

### 3.16 Vegetation

Each biome supplies base density $d_0$ and maximum canopy $c_0$:

| Biome | $d_0$ | $c_0$ |
|---|---:|---:|
| Ocean, Ice | 0 | 0 |
| River | 0.10 | 0 |
| Wetland | 0.55 | 6 |
| Desert | 0.05 | 1 |
| Grassland | 0.35 | 1.5 |
| Shrubland | 0.30 | 3 |
| Temperate Forest | 0.75 | 25 |
| Rainforest | 0.95 | 35 |
| Taiga | 0.60 | 15 |
| Tundra | 0.15 | 0.5 |
| Bare | 0.02 | 0.3 |

[`vegetation.rs`](crates/world-core/src/vegetation.rs) computes

$$
d_v=\operatorname{clamp}\left(
\min\big(d_0(0.4+0.6f_s)(0.5+E),m+0.1\big),0,1\right).
$$

The moisture cap prevents abundant plants in a dry cell. Canopy warmth and
rooting terms are

$$
q_c=\max\left(1-\left(\frac{T-16}{28}\right)^2,0\right),
$$

$$
q_r=\operatorname{clamp}(d_s/0.5,0,1),
$$

giving

$$
c_h=c_0q_rq_c(0.5+0.5d_v).
$$

Density and canopy are aggregates only. No individual plants, succession age,
competition, or growth state are stored.

### 3.17 Habitat signatures and roster caching

[`HabitatSignature`](crates/world-core/src/habitat.rs) coarsens a cell so many
cells can share one ecosystem archetype. The generic band function is

$$
\operatorname{band}(x;l,h,k)=\min\left(k-1,
\left\lfloor k\operatorname{clamp}\left(\frac{x-l}{h-l},0,1\right)\right\rfloor
\right).
$$

The signature contains:

```text
biome id
temperature band = band(T; -20, 40, 6)
moisture band    = band(m;   0,  1, 5)
fertility band   = band(f;   0,  1, 4)
```

Its integer seed folds the world version and four fields. The classification
itself is presentation-grade because it reads floats. Given an already-fixed
signature, species ids and integer genomes are portable; selected fixed-input
food-web fingerprints are parity surfaces, while entropy, expression, and
realization still use presentation floats.

`RosterCache` is a `BTreeMap<HabitatSignature, Arc<RosterEntry>>`. A roster
entry contains a species roster, food web, and a hoisted population table.
Generation collects the distinct signatures in a region before dispatching
Ecology, ensures each entry exists, and gives the worker an immutable
signature-keyed snapshot.

The union of those `region_signatures` across field-active regions is the
indispensable roster working set. Parked authority has no Ecology field to
consume and does not pin roster entries. Cache maintenance walks the active set
in deterministic signature order and calls `ensure` to repair any missing pure
entry. Capacity eviction then removes only disposable entries, retaining its
reverse-signature victim order. The configured roster bytes are a target with
this required-set floor: if active entries alone exceed it, all are retained
and the reported cache bytes expose the overage.

The representative productivity used to build a signature's food web places
fertility and moisture at their band centers:

$$
\bar f=(f_b+0.5)/4,\qquad \bar m=(m_b+0.5)/5,
$$

$$
\bar p=\operatorname{clamp}\left(
\min\big(d_0(0.4+0.6\bar f),\bar m+0.1\big),0,1\right).
$$

This web is shared across the signature. Per-cell vegetation changes pressures
later but does not rebuild the graph.

### 3.18 Species rosters and genomes

[`species.rs`](crates/world-core/src/species.rs) gives each biome a base
richness:

| Biome | Base species count |
|---|---:|
| Ocean | 2 |
| Ice, Bare | 1 |
| Desert, Tundra | 3 |
| River | 4 |
| Shrubland | 5 |
| Grassland | 6 |
| Wetland, Taiga | 8 |
| Temperate Forest | 9 |
| Rainforest | 12 |

Fertility adjusts this by $f_b-1$, and the result is clamped to 1 through 12.
If temperature band is below 1, moisture plus fertility band is below 2, or
the roster has fewer than three species, every species is assigned Producer.
Otherwise, for roster size $n$:

$$
n_p=\max(\lfloor2n/5\rfloor,1),
$$

$$
n_h=\min\big(\max(\lfloor3(n-n_p)/5\rfloor,1),n_p\big),
$$

$$
n_c=\min\big(\lfloor(n-n_p-n_h)/2\rfloor,n_h\big).
$$

After carnivores are reserved, wet signatures with moisture band at least 3
split half the remainder into omnivores; decomposers take the rest. Rosters are
stored in Producer, Herbivore, Omnivore, Carnivore, Decomposer order. Species
$i$ receives an id by hashing `(signature seed, i)`.

[`Genome`](crates/world-core/src/genome.rs) hashes a species id under separate
salts into three domain-separated words:

* appearance: 8-bit hue, 8-bit luminance, 3-bit size class, 4-bit form;
* behavior: 8-bit activity, aggression, and sociality; and
* niche: 8-bit trophic tendency, diet breadth, temperature tolerance, and
  moisture tolerance.

Base body size is an exponential ladder:

$$
s_0=0.1\,2^{\min(\text{size class},7)},
$$

from 0.1 to 12.8 world units. Under region bias $(M,B,A)$, the expressed
fields are

$$
\text{hue}=\left(\frac{g_h}{255}+0.35(A-0.5)\right)\bmod1,
$$

$$
\text{luminance}=\operatorname{clamp}\left(
\frac{g_l}{255}(0.5+A),0,1\right),
$$

$$
\text{size}=s_0(0.5+M),
$$

$$
\text{activity}=\operatorname{clamp}\left(
\frac{g_a}{255}(0.5+B),0,1\right),
$$

$$
\text{aggression}=\operatorname{clamp}\left(
\frac{g_g}{255}(0.5+B),0,1\right).
$$

At a neutral bias, expression reproduces the base genes. Genome identity never
changes; expression is float presentation state.

### 3.19 Food webs and aggregate populations

[`foodweb.rs`](crates/world-core/src/foodweb.rs) constructs one graph per
roster/signature. The maximum sustainable predator size at representative
productivity $p$ is

$$
s_{max}=0.5+12.3\operatorname{clamp}(p,0,1).
$$

Oversized omnivores and carnivores are pruned. For each consumer:

* Herbivores may eat producers.
* Omnivores may eat producers and animals.
* Carnivores may eat animals.
* Producers and decomposers have no outgoing edges.

Diet breadth permits

$$
n_{prey}=1+\left\lfloor\frac{g_{diet}}{64}\right\rfloor
$$

prey entries, between one and four. Candidates are taken in stable roster
order. Animal prey must have base size no more than 1.2 times predator size.
A carnivore with no admissible animal prey is pruned.

Pruning marks indices but does not remove species from the roster. A pruned
entry has zero biomass and cannot be realized. The edge list is not filtered a
second time if a prey entry is later marked pruned, a mismatch discussed in
Section 4.2.

The four biomass tiers are producers, herbivores plus omnivores, carnivores,
and decomposers. If a tier has a surviving species, raw masses are

$$
b_0=\max(p,0.001),\qquad
b_1=0.1b_0,\qquad
b_2=0.1b_1,\qquad
b_3=0.15b_0,
$$

with absent tiers set to zero. They are divided by their total to form shares
$B_0,\ldots,B_3$. Each surviving species receives its tier share divided
equally among surviving species in that tier.

The memoized population table stores:

* dominant roster index, the lowest index with maximal per-species biomass;
* $B_1$ as herbivore base;
* $B_2$ as predator base; and
* normalized Shannon diversity.

For local vegetation density $d_v$ and Ecology bucket $E$, Ecology-layer output
is

$$
q=d_v\operatorname{clamp}(E,0,1),
$$

$$
\text{herbivore pressure}=\operatorname{clamp}(B_1q,0,1),
$$

$$
\text{predator pressure}=\operatorname{clamp}(B_2q,0,1).
$$

If positive per-species biomass values are $b_i$ and
$p_i=b_i/\sum_jb_j$, diversity is

$$
D=-\frac{\sum_i p_i\ln p_i}{\ln n},
$$

with zero for zero or one surviving species. Diversity and dominant index are
signature/web properties; they do not vary with the local Ecology scalar or
vegetation density.

Morphology, Behavior, and Aesthetics are folded into the Ecology dependency
hash, but the four aggregate outputs do not use them. Their role is to force a
new L8 key so near-field organism expression is rebuilt.

### 3.20 Near-field organism realization

[`realize.rs`](crates/world-runtime/src/realize.rs) uses a fresh, settled
Ecology-layer key to create transient `Organism` structs only inside the near
window. It reads the upstream vegetation, temperature, moisture, fertility,
and biome fields plus the roster/web; it does not sample the L8 pressure,
diversity, or dominant-index channels themselves. Each struct contains feature
id, species id, trophic role, local cell, world position, and expressed genome.

For resolution $n$, cell index $c=c_yn+c_x$, and resource-density slot $s$,
the feature index is

$$
i=c+sn^2.
$$

Slot 0 is the Phase 5 identity; higher slots are additive. The organism id
folds world version, region, layer 8, this feature index, and the region's
current revision. A SplitMix stream seeded by that id is consumed in a fixed
order:

1. create the organism if the first sample is below vegetation density;
2. classify the cell signature;
3. sample a surviving species proportionally to per-species biomass;
4. express its genome under $(M,B,A)$ and cap size at $s_{max}$; and
5. draw two fractions to jitter position uniformly inside the cell.

One density slot therefore has expected occupancy $d_v$; $k$ slots have
expected occupancy $kd_v$. Before clearing or publishing an organism vector,
the coordinator verifies that every roster signature tracked for the resident
region is present. An incomplete roster defers realization, preserves the
previous vector, and does not advance the region's L8 organism key; roster
maintenance repairs the pure inputs for a later retry.

Organism vectors are normally keyed by the region's L8 dependency hash. They
are reused while that key is unchanged, rebuilt as a whole when it changes,
and recycled when the region leaves the near window. A material preserve-winner
snap is an explicit additional invalidation: it retires the vector and key so
realization uses the new region revision even when center normalization stayed
inside the same L8 buckets. There is no movement, animation state, hunger,
reproduction, age, interaction, or behavior simulation.

### 3.21 Record schema and codec

[`record.rs`](crates/world-core/src/record.rs) is the boundary between live
float state and durable data. Every encoded body is preceded by

```text
Envelope { format_version, world_version, record_kind }
```

and both envelope and body use `serde` plus `postcard`. Readers reject a newer
format, a wrong kind, corrupt data, and trailing bytes. The schema has a
migration hook but no older version yet exists.

The record types are:

* `DiscoveryRecord`: source, habitat-signature seed, target signature, mask,
  polarity, quantized strength, integer radius and position, mutable name and
  journal;
* `RouteRecord`: ordered quantized nodes, discovery references, usage, mutable
  name and journal;
* `PreserveRecord`: coordinate-sorted region/signature pairs plus metadata;
* `SeenRecord`: a four-word bitmap for one 16 by 16 region chunk;
* `SessionSnapshot`: exact player state, anchors, and authoritative resident
  region states, parked entries included;
  and
* `AtlasBundle`: discoveries, routes, and preserves sorted by id.

A discovery reconstructed as an anchor uses integer position, bucket-center
target and strength, and integer falloff. A preserve applies those same bucket
centers directly to region `current` and `target` and forces stability one.
Runtime ownership is not encoded as another record field: the ordered set of
covering record ids is retained per coordinate, and its lowest id is effective.
Removing that id selects the next contributor; removing the last one releases
the resident without snapping it back.

Content ids exclude mutable names, journals, sequence counters, and route
usage. Discovery ids include every quantized steering field and position;
route ids include ordered nodes and discovery references; preserve ids include
the sorted coordinate/signature list. The merge rank for mutable text is the
lexicographic maximum of store-local sequence and a deterministic content hash.

### 3.22 Vault and storage

[`Vault<S>`](crates/world-runtime/src/vault.rs) is generic over a synchronous
byte-keyed `Storage` trait. Its in-memory state is a set of ordered maps for
discoveries, routes, preserves, and seen chunks, plus the current session,
store sequence, ordered dirty-key set, and issue log.

The namespace is:

```text
meta/store
session/current
disc/<16-hex-digit id>
route/<16-hex-digit id>
pres/<16-hex-digit id>
seen/<8-hex-digit x><8-hex-digit y>
```

Opening reads metadata, session, and all four record namespaces. Bad individual
records are skipped and reported; a bad store header aborts opening. The store
sequence heals upward to the maximum loaded record sequence.

Mutations update the in-memory maps and insert a `DirtyKey` in $O(\log n)$.
A budgeted `flush` returns structured progress or a typed storage-operation/key
failure after at most `max_persist_ops` successful writes; exhausting that
budget with dirty work remaining is ordinary backpressure. `flush_all` returns
success only when the dirty set is empty. A dirty key is retired only after its
backend operation succeeds, and deterministic data-before-metadata order means
a failed record stays retryable without metadata overtaking it. Preserve and
route deletion likewise commits in memory only after durable backend removal.
Every valid added, merged, or unchanged import result contributes its resulting
sequence to the live local maximum; when that maximum is higher, import raises
the counter and dirties metadata immediately. Every valid sequence through
`u64::MAX` remains accepted and shareable. The next local edit is strictly
newer when representable; at exhaustion, discovery, preserve, route, and
session authoring return a typed error before changing records, session state,
metadata, or dirtiness. Issue telemetry is deduplicated under stable identities,
capped at 64 retained entries, and accompanied by a saturating
suppressed-report counter.

Native storage durably creates missing ancestors, creates a collision-safe
hidden temp sibling, writes and `sync_all`s the complete temp file, atomically
renames it, and `sync_all`s the containing directory before reporting success.
Removal, including a not-found retry after an unlink/sync failure, also crosses
the containing or nearest-existing directory barrier. Tests use an immediate
`BTreeMap`-backed memory store and a staged native file-operations seam. The
trait deliberately remains synchronous; an asynchronous/lazy browser backend
and its equivalent transaction durability boundary remain future work.

The vault itself never owns generated tiles, rosters, organisms, or renderer
data. Its logical size is
$O(\text{authored records}+\text{visited level-0 regions}+\text{bounded session
window})$: named records grow with authoring, while the discovered bitmap adds
one bit for every visited region.

### 3.23 Incremental generation and job integration

Generation jobs are pure functions over owned metadata and immutable `Arc`
snapshots. The main thread is the only cache writer. `RegionMap` maintains an
`in_flight` ordered map keyed by `(coordinate, layer)`; each entry contains a
monotonic job id, the expected dependency key captured at dispatch, and an
atomic cancellation token.

Dispatch works to a fixed point within the frame's cost budget:

1. order field-active regions nearest-first;
2. close each dirty consumer's work set over missing or stale declared inputs,
   restoring lower-layer and macro requests before readiness is tested;
3. scan each dirty mask in topological layer-id order;
4. clear a dirty false positive when stored and expected hashes already match;
5. defer a layer until all declared inputs are present and fresh;
6. submit it with Critical, Normal, or Background priority according to
   distance; and
7. integrate completed results and repeat while new work became ready.

The dirty bit is cleared at dispatch. If possibility state or an upstream tile
changes while a job runs, the layer is dirtied again and its cancellation token
is flipped. On arrival, job id establishes that the result belongs to the
current dispatch. Before any channel changes, its dependency hash must equal
both that dispatch's captured key and the key recursively expected from current
authoritative state. Accepted output atomically replaces all channels of that
layer and, when its key replaces a missing or different key, dirties every
transitive dependent.

Failed provenance retires the completed dispatch, reclaims owned tile buffers,
leaves the previous cached layer untouched, marks the layer and its dependent
closure dirty, and cancels obsolete dependent jobs before normal budgeted
dispatch retries them. A rejected macro applies the Drainage closure to every
covered authority, but a parked region remains `Unloaded` and dispatches
nothing. Parking retires level-0 dispatch identities, so late cancellation-off
results are reclaimed without recreating fields. Dirty bits and cancellation
are never substitutes for the identity and provenance checks.

Drainage jobs are keyed by level-4 macro coordinate. Their priority is inherited
from the nearest dependent region that requests the tile. Ecology jobs first
snapshot all required roster entries in the same way Hydrology snapshots its
macro drainage tile.

### 3.24 Executors and temporal budgets

The neutral [`TaskExecutor`](crates/world-runtime/src/task.rs) accepts a boxed
`FnOnce + Send` and a three-level priority. `InlineExecutor` runs it immediately
and is the reference for tests and headless tools.

The native [`LaneExecutor`](crates/tools/src/executor.rs) owns three FIFO
`VecDeque`s behind a mutex and condition variable. Worker threads always pop
Critical before Normal before Background. Automatic sizing reserves the caller
thread and creates `available_parallelism - 1` workers, with a minimum of one.
Cancellation is implemented inside job closures, not by the executor; a worker
checks its token once after dequeue and before running the kernel.

Per-frame work is bounded by counts and declared cost units, never elapsed
time. The low-tier 16.6 ms nominal budget is:

| Work | Limit per frame |
|---|---:|
| region loads | 48 |
| region convergence steps | 512 |
| generation cost | 96 |
| realized organisms | 400, with whole-region overshoot allowed |
| resonance nodes | 64 |
| persistence operations | 8 |
| route-attraction nodes | 32 |
| target refreshes | unlimited |

Declared generation cost approximates about 25 microseconds per unit on the
reference machine. A macro Drainage job costs 17; other layers cost one or two.
Budget exhaustion is represented by deferred-work counters and is considered
normal backpressure.

### 3.25 Resource tiers, caches, and pools

Resource tiers choose capacity and pacing presets:

| Setting | Low | Mid | High |
|---|---:|---:|---:|
| near radius | 3 regions | 3 | 3 |
| far radius | 9 | 11 | 13 |
| load radius | 12 | 14 | 17 |
| unload radius | 14 | 16 | 19 |
| field cache | 48 MiB | 96 MiB | 160 MiB |
| macro cache | 12 MiB | 16 MiB | 24 MiB |
| roster cache | 8 MiB | 8 MiB | 8 MiB |
| organism slots/cell | 1 | 2 | 4 |
| generation cost/frame | 96 | 192 | 384 |
| resonance nodes | 64 | 96 | 128 |
| target refreshes/frame | all | 160 | 240 |

An explicit environment override wins tier detection. Otherwise at most four
logical cores or a CPU-class graphics adapter selects Low; at least eight cores
and a discrete adapter selects High; other configurations select Mid.

The streaming data structures are:

* authoritative region map: coordinate -> `RegionState`, parked entries included;
* `RegionCache`: coordinate -> `RegionTiles`;
* `MacroCache`: level-4 coordinate -> shared `DrainageTile`;
* `RosterCache`: habitat signature -> shared `RosterEntry`;
* organism map: coordinate -> transient organism vector;
* region-signature sets that identify roster dependencies; and
* preserve contributors: coordinate -> ordered `(content id, signature)` map,
  whose first entry is the effective owner.

Normal radius removal drops regional authority, tiles, organisms, signature
bookkeeping, and in-flight jobs outside the unload radius, but not sparse
preserve contributors. A later load initializes from the then-effective
lowest-id signature, including when the old winner was deleted while the
coordinate was absent.

Field capacity is different: it parks farthest field-active, non-preserved
regions outside the near radius while retaining their `current`, `target`,
stability, and revision. Every disposable `Generating` or `Ready` region
reserves the full eventual payload, so partial generation cannot over-admit the
target. Parking removes tiles, organisms, signatures, and level-0 jobs;
reactivation recomputes target and geometry, dirties every layer, and rebuilds
without creating a new history epoch. Near and contributor-covered fields are
explicit exemptions above the target.

Macro capacity removes farthest macro tiles. A dirty active consumer later
demand-rebuilds a missing or stale macro through declared dependency repair;
a freshly integrated demanded macro is retained transiently until Hydrology
snapshots it, so asynchronous work also makes progress below a one-tile target.
Macro/roster dependency protection follows field-active consumers rather than
parked authority. Roster capacity first ensures that protected signature union, then
removes only disposable entries in reverse signature order. Its logical byte
target may be exceeded when the active working-set floor is larger.

`TilePool` keeps bounded stacks of reusable `Vec<f32>`, `Vec<u8>`, and
`Vec<u16>` allocations. Buffers travel from main-thread pool to a job, into a
tile, and back when an old tile has no other `Arc` owner. Organism vectors have
a smaller count-bounded pool. Allocation reuse has no effect on generated
content.

### 3.26 Same-math performance paths

[`simd.rs`](crates/world-core/src/simd.rs) provides row kernels with permanent
scalar twins. Terrain interpolation uses four-wide `f32` vectors while
retaining scalar `f64` lattice coordinates; Climate, Hydrology, Soils, and
Vegetation use eight-wide lanes. Tail elements run the scalar functions.

The contract is same operation order per lane: no fused multiply-add, fast
math, reassociation, or cross-lane float reduction. The logarithmic river curve
runs the scalar function separately per lane. Differential tests include
random inputs and branch boundaries and require identical `f32` bit patterns on
the platform under test.

The other major optimization is the Ecology hoist. Dominant index, biomass
shares, and diversity depend only on `(roster, food web)`, so `RosterEntry`
computes them once instead of scanning the roster for every cell. The per-cell
loop becomes table lookup plus pressure scaling, preserving operation order and
output bits.

Target refresh is amortized on Mid and High. A steering-input hash over bias and
anchor fields forces a full-authority target refresh when controls change.
Otherwise the runtime walks all authoritative coordinates, parked entries
included, round-robin under `max_retarget_regions`. Geometric stability is not
part of that budget: every authoritative region refreshes it every frame before
resonance and convergence.

### 3.27 CPU-authoritative presentation

The renderer is downstream of the model. The CPU
[`MapComposer`](crates/platform-native/src/viz.rs) reads tiles and paints the
deterministic false-colour map used by screenshots, headless tests, and the
reference view. It can display individual environmental channels, biomes,
species, ecology, influence, stability, discovered regions, routes, preserves,
and organism markers.

The normal native path uses a GPU field atlas. Each region slot packs:

```text
rgba32float: elevation, hardness, temperature, moisture
rgba32float: river, wetness, soil depth, fertility
rgba32float: vegetation, canopy, herbivore, predator
rgba32float: diversity, presence mask, unused, unused
rg16uint:    biome id, dominant roster index
```

An `AtlasManager` assigns and recycles slots and uploads a region only when its
presence/dependency key changes. A storage buffer maps visible window positions
to slots. Sparse routes, rings, markers, and other overlays remain CPU-drawn.

The WGSL composition pass selects false-colour channels and can add up to three
hashed-gradient refinement octaves above the 32-by-32 authoritative sampling
rate. These details are display-only. The renderer exposes no readback method;
no GPU result can become a generation input, identity, steering value,
resonance value, or persistent record.

The current web crate compiles the neutral code for `wasm32` and exposes parity
probe functions. It does not run `RegionMap`, a worker executor, browser
storage, or the renderer.

### 3.28 Verification surfaces

The repository uses several complementary checks:

* fixed golden hashes and record bytes in `world-core`;
* same-platform SIMD-versus-scalar differential tests;
* the continuity replay for pinned stability and bounded seams;
* `wer-ledger` for declared invalidation precision;
* focused `world-runtime` recovery tests for tight macro and roster ceilings,
  every-layer stale-result rejection, settled-cell roster inspection, and
  deferred then retried near-field realization;
* focused preserve regressions for forward/reverse overlap application,
  resident atomic-batch reconciliation, winner and non-winner deletion,
  same-signature owner changes, revision-only bucket-center normalization,
  exact in-flight cancellation, session restore, tile/job preservation,
  organism re-realization, and successor recovery after resident or evicted
  deletion;
* focused neutral vault failure injection for structured progress/error
  outcomes, data-before-metadata retry order, commit-after-remove deletion,
  bounded issue state, and immediate valid-result import sequence advancement;
* native file-protocol tests for durable ancestor creation, temp-file write and
  synchronization, atomic rename, directory synchronization, not-found delete
  barriers, and every staged failure/retry boundary;
* the Phase 3 ecology harness (run as an integration test) for diversity and
  trophic bounds;
* `wer-anchor` for selective, coherent steering and resonance gating;
* `wer-vault` for persistence, merge laws, preserves, routes, save/load, and an
  explicit 70-retry failure/ordering/delete/import-sequence scenario; and
* `wer-scale` for executor/budget/cancellation/amortization settled hashes,
  per-frame tight-versus-roomy regional-history equality under field pressure,
  field/pool plateaus, tiers, and additive realization density.

CI formats, lints, checks, and tests the native workspace and compile-checks
the three neutral/web crates for `wasm32-unknown-unknown`. The parity exports
are golden-tested in a native build and compiled for wasm, but CI does not
currently execute those exports in a wasm engine or browser.

## 4. Questionable aspects and opportunities for improvement

This section separates likely correctness or contract violations from
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

5. **Restore resource-tier invariance for gameplay and shared records**
   (finding 6). Compute resonance, capture, and persisted route cost from one
   fixed authoritative sample; treat higher organism slots as visual additions
   only. Gate this with cross-tier capture and byte-identical route tests.

6. **Canonicalize all anchor reductions and signatures** (findings 1 and 21).
   Sort anchors by a complete bitwise key before floating-point reduction, make
   the polarity rule explicit, and sign the count plus every
   steering-relevant field. The steering calculation and its signature must
   describe exactly the same multiset.

7. **Enforce the intended semantics of route and suppress influences**
   (findings 7 and 17). Cap total route attraction after combining nodes, and
   score Suppress compatibility against the reflected or final desired state
   rather than against the literal suppressed target.

8. **Make stable topology and ordinary region boundaries satisfy their stated
   guarantees** (findings 9 and 19). Move routing elevation to fixed-point math
   or narrow the portability contract, execute parity probes in wasm, and give
   Terrain and slope either smoothly sampled possibility inputs or cross-region
   halos. Any output-changing fix must follow the versioning and golden-fixture
   process.

9. **Separate stable organism identity, placement, succession, and expression**
   (finding 12). Do not let M/B/A-only changes re-roll species or position; key
   expression from explicitly quantized or exact inputs and use a distinct,
   content-derived succession epoch when re-identification is intended.

10. **Harden content equality and canonical set encoding** (findings 24 and
    25). Compare immutable bodies when ids match, reject or deterministically
    handle duplicates, and move public/untrusted exchange to a wider
    cryptographic digest. Define tombstone and multi-replica counter semantics
    before calling deletion and usage fully CRDT-compatible.

11. **Make executor failure and shutdown bounded** (finding 30). Stop or clear
    queued work during shutdown, convert worker panics into structured failed
    jobs, repair in-flight bookkeeping, and ensure lost workers are replaced or
    their work is deterministically requeued.

12. **State and encode the truth of snapshots and route samples** (findings 22
    and 29). Persist the configuration required for exact session restoration,
    distinguish a route's aspirational target from visible `current` state, and
    sample every crossed route interval without discarding travel remainder.

13. **Expand the verification surface alongside these fixes** (finding 33).
    Add frame-slicing, all-cache-ceiling, cross-tier persistence, full settled
    state, ordinary border, multi-node route, all-biome SIMD, and executed-wasm
    cases. This is a cross-cutting exit criterion for the preceding correctness
    work, not a substitute for it.

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

#### 1. Anchor combination is not bitwise order-independent

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

The related runtime steering signature folds anchors in slice order, so a mere
reorder forces an unnecessary whole-window retarget even when steering output
is unchanged.

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

#### 6. Resource tiers feed gameplay and persistent identity

High tier adds organism slots and increases the resonance-node cap. Resonance
reads realized organism count and species entropy, so hardware tier changes the
convergence rate during travel. Extra slots can also change which organism is
nearest during capture. Route nodes persist `1 - resonance` as part of their
content id, so the same physical expedition can produce different shared route
bytes on Low and High hardware.

The effect is not even monotone after density saturates at eight nodes: adding
farther or less evenly distributed organisms can lower the mean-distance or
diversity term while density stays one.

This contradicts the description of organism density as presentation-only and
the claim that shared surfaces are tier-invariant. Authoritative resonance and
capture should use a fixed virtual sample—such as slot 0 or aggregate tiles—
while extra organisms remain visual. Add cross-tier capture and route-record
byte tests.

#### 7. A per-node route cap does not make route attraction weak in aggregate

Each route node is capped below 0.35, but overlapping nodes combine through

$$
W=1-\prod_i(1-w_i).
$$

At the default 32-node cap, 32 fresh nodes with $w=0.1225$ already produce
$W\approx0.9847$; at usage four, $W\approx0.9998$. A dense or overlapping
route can therefore almost force its weighted target and overwhelm a player
anchor, contrary to the stated soft-attraction contract.

Aggregate nearby route nodes into a single normalized field with a cap on
*total* route pull, or divide contribution by local route-node density before
combination.

#### 8. Transition mode currently reverses the high-level movement fantasy

The project overview says ordinary movement is fast exploration and only slow,
deliberate transition movement should significantly change reality. Native
movement speed is identical in both modes, while transition mode multiplies
convergence by 0.35 and free movement uses 1.0. Ordinary travel therefore
changes reality about 2.86 times more per unit than explicit transition mode.

Either make free travel nearly neutral and transition mode enable convergence
while reducing physical speed, or revise the product description and controls
to match the implemented mechanic.

#### 9. "Integer drainage topology" still depends on float thresholds

Drainage decisions are integer *after* elevation is rounded to centimeters,
but that integer is produced by float possibility interpolation and
quantization, Perlin interpolation, fBm summation, relief scaling, and
`f32::round`. (The plausibility projection is currently inert here because it
does not change Terrain's Planetary or Geology inputs.) A small target/compiler
difference near a half-centimeter boundary can change a descent edge and then
an entire accumulation tree. The present pipeline therefore does not
structurally satisfy the stronger claim that floating point never decides
permanent topology.

A fixed-point routing-elevation evaluator would make the guarantee real. A less
disruptive near-term step is to describe the guarantee more narrowly and run
the parity exports inside an actual wasm engine in CI over many cells. Current
CI only compiles the wasm exports and executes their goldens natively.

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

#### 12. Appearance-only changes can re-roll identity and placement

Morphology, Behavior, and Aesthetics make the L8 dependency hash change even
though aggregate L8 values do not use them. The rebuilt realization uses the
incremented region revision in feature ids, so an Aesthetics-only bucket flip
can re-roll presence, species choice, and positions rather than merely changing
expression. This weakens the intended M/B/A expression-only behavior, causes
unnecessary aggregate regeneration, and can reselect species and placement on
the next realization.

Separate the aggregate Ecology key, stable entity identity/placement key, and
expression key. M/B/A changes should update expression while retaining entity
id and species; true succession can use an explicit content-derived epoch.

There is a related cache mismatch: realization reads raw M/B/A floats, but the
L8 key contains only their buckets. Sub-bucket drift does not refresh current
organisms, yet leaving and re-entering the near window can rebuild them from
new raw floats and a new revision under the same L8 bucket key. Use bucket
centers or include exact expression inputs in the realization key.

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

#### 17. Suppress anchors are scored backward by resonance compatibility

Anchor compatibility rewards the local world for being close to every
anchor's literal target, irrespective of polarity. For Suppress, the desired
direction is away from that target, so the current formula makes a world rich
in the suppressed trait resonate more strongly.

Compare against the reflected suppress desire or, more simply, against the
final combined steered target.

#### 18. Capture is weakly localized and Planetary capture is only a baseline

The nearest-organism search accepts any organism in the 256-unit covering
region; there is no maximum capture distance. M/B/A capture can also return a
baseline anchor when no organism was found, despite the documentation's
"nothing capturable" language. Planetary has no atmospheric observable at all.

Add a capture radius and an explicit result describing which feature supplied
each trait. Weather, cloud, ocean, or atmospheric fields are needed before a
Planetary capture can be distinctive.

#### 19. Terrain and downstream fields are not border-identical

The base possibility field varies smoothly *between region samples*, but every
tile uses one constant region possibility vector. Neighboring terrain tiles can
therefore apply different relief scale and sea shift to the same continuous
noise near a border. A loose base-field bound permits a substantial elevation
step, and steering/convergence history can make it larger.

Slope then uses only the local tile and one-sided edge differences, adding
derivative seams to Hydrology and Soils. Sample slow possibility per cell or
blend it across borders, and give slope a cross-region halo or analytical
terrain derivative. These changes alter generated output and require the
normal versioning process.

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

#### 21. Anchor-set signatures do not describe actual steering sets

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

#### 22. Route recording and traversal depend on frame sampling

A frame that crosses several 192-unit intervals emits at most one node, resets
accumulated travel to zero, and discards overshoot. Record exact previous
position, carry the remainder, and interpolate every crossed sample up to the
node cap.

The node's possibility signature is the covering region's target, while its
difficulty is measured from the currently realized resonance. In a pinned near
region these can describe an unseen aspiration and the visible world
respectively. That is defensible as a route through possibility space, but the
schema and user-facing language should state the distinction explicitly.

Traversal requires 60% of nodes in one broad corridor leg but not ordered
progress, direction, or continuous path coverage; clustered nodes can be
credited together. Track route-segment progress if usage is meant to represent
following an expedition. Route difficulty should then be distance-weighted
rather than an unweighted mean of frame-dependent nodes.

#### 23. Route queries and per-frame scans do not scale with an atlas

Route attraction and traversal scan every node of every route before
truncation. Resonance similarly scans and sorts every near organism.
`RouteGraph` scans and sorts all nodes for every nearest-possibility query; its
signature-seed ordering does not accelerate the L1 metric.

A bounded top-$k$ heap is an immediate improvement. Larger stores need a
physical spatial index for corridors and a metric tree or quantized spatial
index for eight-dimensional possibility signatures.

#### 24. A 64-bit content fold is not proof of immutable equality

Merge logic assumes equal ids imply equal immutable fields "by construction."
A 64-bit hash collision is unlikely but possible, and the mixer is not intended
to authenticate untrusted internet bundles. On an id collision, merge does not
compare immutable bodies.

Before public sharing, use a wider cryptographic digest, compare immutable
content on equal ids, and optionally sign authored records. Also note that
deletion is not a CRDT operation without tombstones, while `usage = max` loses
independent traversal increments that a per-replica grow-only counter could
retain.

#### 25. Canonical "sets" preserve duplicate multiplicity

Atlas canonicalization sorts but does not deduplicate record ids, and preserve
construction sorts but does not deduplicate repeated region coordinates. Byte
encoding and preserve identity can therefore depend on duplicates despite set
language. Validate uniqueness and define an explicit duplicate policy.

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
revision once and retires old-revision organisms, while only domain bucket
flips dirty the ADR 0007 reader closure. Same-bucket center normalization
therefore rebuilds organism identities without changing tile hashes or
in-flight tile work. Runtime unit tests, including resident forward/reverse
atomic batches and session restore, the native effective-owner deletion seam,
end-to-end overlap and evicted-deletion tests, and the `wer-vault` sign-off
scenario exercise these contracts. Separate UI calls remain distinct material
history; only canonical synchronization batches reconcile once. Duplicate-
coordinate canonicalization (finding 25) remains open; durable local delete
failure handling is resolved by Improvement A.3 and finding 28.

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
contracts. This does not add CRDT tombstones or resolve findings 24/25, and the
synchronous eager interface/browser backend limitation in finding 27 remains
open.

#### 29. Session exactness has narrower preconditions than its headline

The snapshot omits resource tier/configuration, target vectors, unfinished
route recording, route-tracker leg state, executor queue state, and caches.
Exact restoration is demonstrated for the same algorithm, field,
configuration, platform, anchors, and scripted follow-up—not arbitrary builds
or hardware modes. Encode those preconditions in metadata or narrow the stated
contract.

### 4.4 Scheduling, portability, and verification gaps

#### 30. Native executor shutdown drains work it says it discards

`LaneExecutor::Drop` sets `shutdown`, but workers check for queued jobs before
checking that flag. They therefore drain the entire backlog and can stall
shutdown doing work whose result receiver is gone. Check shutdown first or
clear queues explicitly.

Strict Critical-over-Normal-over-Background priority also has no fairness or
aging, so continuous nearby work can starve the far field indefinitely. The
queue is unbounded, and cancelled no-op closures remain queued. Bounded queues,
weighted aging, and cancellation-aware removal would improve backpressure.

A worker panic is not converted into a failed job, does not clear in-flight
bookkeeping, and does not replace the worker. Generation errors should return
structured results or trigger deterministic requeue/recovery.

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

**Status:** Open. Improvements A.1 and A.4 close the focused cache-recovery,
dependency-provenance, and field-capacity authority gaps described below.

The scale harness checks executor counts, budget scale, cancellation, retarget
amortization, and per-frame tight-versus-roomy regional history, but has no
alternate frame-slicing case despite the ADR claim. Its memory scenario
pressures only the field cache rather than all caches together. Its tier
identity scenario does not record routes or captures. The full hash now begins
with coordinate/current/target/stability/revision authority, but deliberately
omits derived field-admission status and still omits overrides, full organism
position/expression, and executor queues, so equality is not equality of every
stated component.

Focused runtime tests now pressure field/macro/roster ceilings, reserve full
field payload before queued work integrates, reject stale results for every
layer shape, exercise cell inspection and realization recovery, and assert
stored versus expected keys in those scenarios. This does not turn `wer-scale`
into a simultaneous all-cache test or make its hash cover external queues and
every derived presentation detail.

Add the remaining frame-slicing, all-cache-ceiling, cross-tier persistence, and
full settled-state scenarios. Run wasm parity exports—not just compilation—in
CI. Add ordinary region-border tests for elevation, slope, soil, and biome, plus
multi-node route softness. The SIMD differential generator should also sample
all 12 biome ids; it currently calls `next_below(11)`, so Ice (id 11) is never
exercised there.

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
