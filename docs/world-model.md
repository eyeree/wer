# The Infinite World Exploration World Model

This document describes the world model and its current native/browser viewer.
It starts with a non-technical account, then gives a formal model, and finally
walks through the algorithms and data structures component by component. The
final section records implementation concerns and promising improvements found
during review.

The distinction between **implemented** and **planned** matters. The repository
already contains a deterministic, streaming, top-down world prototype with
environmental layers, aggregate ecology, procedural species, steering,
persistence, routes, a derived 3D POV renderer, and a static browser runtime.
Native and browser expose aligned Map, POV, and side-by-side Split presentation
over one traveler and one world update. The project does not yet contain moving
or behaving organisms, networking, multiplayer, photography, or a community
service. The current viewer is still a world-model prototype, not a finished
game.

## 1. Non-technical overview

The project is an exploration game about traveling through landscapes and
through *possibilities* at the same time. It does not treat possible worlds as
separate levels selected by portals or loading screens. Instead, every place
has a natural tendency toward a particular kind of reality, and the player can
gradually bend that tendency while traveling.

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
scale-aware scheduling, and aligned native/browser presentation. The shared
viewer offers a false-colour Map, a derived terrain-and-organism POV, and Split
without making either presentation authoritative. The artistic and behavioral
fidelity envisioned by the project overview remains future work.

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
 climate + soils + biome + vegetation -> ecology -> canonical slot-0 organisms
                                                        |             |
                                                        |             +-> capture + resonance -> route cost
                                                        +-> tier-budgeted extra visual slots
```

There is no complete materialized world behind the authoritative streaming
window. Inside it, field capacity may park reproducible tiles while retaining
the small regional transformation history. Crossing the unload radius removes
that authority; an ordinary unpreserved region loaded again later starts at the
target implied by the then-current steering context. Sparse preserve
contributors and run-local session snapshots are the reconstruction exceptions.
Near-field organism vectors distinguish one fixed authoritative slot-0 sample
from optional higher-density presentation slots. Gameplay and shared-route cost
read only slot 0; Low/Mid/High may display different populations without
changing those inputs once the same L8 and roster prerequisites are ready.

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

Only masked dimensions participate. Before any `f32` reduction, every anchor
occurrence is sorted by one lexicographic raw-bit key: world-position bits,
mask, polarity, strength bits, falloff-radius bits, then masked target bits in
domain order. Source and unmasked target storage are excluded. Equal keys are
not deduplicated, so composition is a multiset operation and duplicates retain
their saturating influence. This canonical operation order makes every slice
permutation bitwise equal for identical IEEE inputs (ADR 0025).

The displayed polarity order is deliberate: Emphasize blends first and
Suppress blends last, although Suppress reflection remains relative to the
original unsteered base. Suppress therefore has final-blend priority; the model
does not use a simultaneous polarity solve.

The same canonical occurrences also define a direction-free active-influence
profile for each domain $d$:

$$
q_d(x)=1-\prod_{a:\,d\in M_a}(1-w_a(x)).
$$

Both polarities contribute equally to this relevance weight, and duplicates
remain multiplicative. The profile never chooses a desired state; steering and
the final projected target retain that responsibility (ADR 0026).

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

The convergence gate is built from canonical slot-0 near-field organisms. Let
$n\le 64$ be the number of nearest nodes actually selected after the fixed
distance/species/position total sort:

* density is $D=\min(n/8,1)$;
* species diversity $V$ is normalized Shannon entropy;
* distance quality $L$ is the mean of
  $\operatorname{clamp}(1-d_i/r_n,0,1)^2$;
* anchor compatibility $K$ is agreement between the covering region's
  authoritative current and final projected target over canonically active
  domains; and
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

Let $q_d(x_r)$ be the canonical active-influence profile from Section 2.4,
evaluated at the covering region center—the same point used to define $T_r$.
Compatibility is

$$
K=\operatorname{clamp}\left(
1-\frac{\sum_d q_d(x_r)|p_{r,d}-T_{r,d}|}
{\sum_d q_d(x_r)},0,1\right),
$$

folding domains in fixed possibility order. It is defined as one when the
denominator is zero, covering authority is missing, or a preserve effectively
owns the region. An ordinary stable near region is not a preserve: if its
pinned current differs from an active final target, that disagreement remains
meaningful. The target already contains bias, Emphasize-first/Suppress-final
composition, route normalization, and plausibility projection; resonance does
not reconstruct literal anchor desires. The five terms combine as

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

Terrain and Drainage have typed keys beyond that generic shape. Terrain folds
the 18 Planetary/Geology buckets in its absolute 3 by 3 possibility halo.
Drainage folds the stored possibility-field spacing plus the Terrain and
Drainage algorithm revisions before its macro coordinate. A field-recipe
change refreshes every authoritative target before integration, invalidates
fallback-sensitive Terrain closures, and retires old macro work.

The implementation checks both stored tiles and completed results against that
current expected key. A completed result must also match the key captured at
dispatch, while its current job id is the separate dispatch-identity gate. A
dirty bitset is only a scheduling hint and cancellation only saves work.
Dispatch readiness remains material: before deferring a consumer, the runtime
demand-repairs any missing or stale cached input through the declared graph.

The implemented directed acyclic graph is:

| Id | Layer | Direct domains | Declared layer inputs | Cached result | Cost |
|---:|---|---|---|---|---:|
| 0 | Terrain | 3 by 3 Geology/Planetary halo | none | elevation and slope `f32` | 4 |
| 1 | Geology | Geology | none | hardness `f32` | 2 |
| 2 | Drainage | none | Terrain revision | macro flow direction + accumulation | 31 |
| 3 | Climate | Climate, Hydrology, Planetary | Terrain | temperature, moisture | 1 |
| 4 | Hydrology | Hydrology, Planetary | Terrain, Drainage, Climate | river, wetness | 1 |
| 5 | Soils | none directly | Terrain, Geology, Climate, Hydrology | depth, fertility | 2 |
| 6 | Biome | none directly | Terrain, Climate, Hydrology, Soils | biome id `u8` | 1 |
| 7 | Vegetation | Ecology | Climate, Soils, Biome | density, canopy height | 1 |
| 8 | Ecology | Ecology | Climate, Soils, Biome, Vegetation | herbivore pressure, predator pressure, diversity, dominant roster index | 2 |

Layer ids are topological. Scanning dirty bits in ascending order therefore
visits dependencies before dependents. Drainage is special: it does not read a
terrain tile or live realized possibility state. Its declared Terrain edge
carries the Terrain algorithm revision, while its integer routing elevation
comes from the anchor-free base possibility field identified by the field
spacing in the macro key.

That special edge is intentionally coarser than the data dependency. A live
Geology or Planetary bucket change dirties Terrain and therefore propagates a
Drainage dirty hint, but the macro drainage hash does not contain those live
buckets. Its check normally clears this false positive without regenerating;
the invalidation ledger accounts for the exception explicitly.

### 2.7 Ecology as fields, shared archetypes, and instances

Ecology is a three-tier model:

1. environmental fields describe each cell;
2. a coarse habitat signature selects a shared species roster and food web;
3. the near window samples one canonical organism slot plus optional visual
   instances from local vegetation, the habitat, and roster biomass weights.

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
region leaves the near window. Slot membership is explicit: slot 0 is the only
capture/resonance/gameplay sample, while slots above zero are additive renderer
and diagnostic population.

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
bits for player state, anchors, active route-recorder state, active route-
tracker leg state, runtime metadata, and every authoritative resident region's
realized `current`, steered `target`, stability, and revision, parked residents
included. It is local to one run/platform, is never included in an atlas
bundle, and restores those regions parked before live admission re-derives
tiles. An exact save→load→settle comparison requires matching metadata,
canonical near state to be complete at save, and zero travel after load until
`authoritative_realization_complete`. Otherwise authoritative regional history
is still restored, but transient gameplay availability rebuilds one ready near
region per update because the session does not persist executor queues,
in-flight jobs, caches, rosters, organism vectors, or bypass the fixed
scheduler.

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
   presentation-grade across targets. Canonical anchor reduction and its
   signature have a narrower portable contract: identical input IEEE fields,
   base, bias, and evaluation position execute one raw-bit-sorted sequence on
   native and wasm.
3. **Settled schedule independence.** Pure jobs and dependency keys make a
   quiescent scripted endpoint independent of executor, worker count, budgets,
   cancellation, and retarget amortization. Mid-journey state is explicitly
   allowed to differ because job timing changes when prerequisite L8/roster
   inputs become ready. Once those inputs are equal, a fixed one-region-per-
   frame canonical publication schedule makes capture, resonance, and shared
   route bytes invariant to resource density and visual realization budgets.
   Field-cache capacity is narrower: with equal near-field
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
* `RECORD_FORMAT_VERSION = 2` changes the serialized record schema.

Each generation layer also has an `algorithm_revision`: Terrain and Drainage
are currently at 1 after A.8, and the other layers remain at 0. A local
algorithm revision invalidates only that layer and its dependents, whereas
changing the world version invalidates the entire generated contract.

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
derived fields. A restored session region recovers exact `current`, `target`,
stability, and revision as parked authority and follows that same live
admission path.

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

The steering-semantic fields are position, mask, polarity, strength, falloff
radius, and target values selected by the mask. `AnchorSource` is discovery and
legibility metadata, and target storage outside the mask is inert. The shared
canonical projection in [`anchor.rs`](crates/world-core/src/anchor.rs) sorts
those semantic fields and supplies both floating-point reduction order and the
multiset signature; callers do not maintain parallel field lists.

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
Organism capture searches the explicit canonical slot-0 view only; higher
resource-tier slots cannot become the nearest gameplay specimen.

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
5. retire stale organism currency and publish slot 0 for at most one nearest
   fresh, roster-complete near region;
6. construct resonance from the current canonical slot-0 view;
7. converge eligible realized states;
8. dispatch stale layers in topological order;
9. integrate again for the synchronous executor; and
10. expand already-canonical vectors to the tier's visual density under the
    presentation-realization budget.

Travel is supplied explicitly by the caller instead of being inferred inside
the map. Streaming, target calculation, and completion of already-dirty
field-active work continue while stationary; only `current -> target`
convergence is travel gated. Parked authority participates in target refresh
and convergence but never dispatches generation. The convergence formula is
first-order frame-slicing invariant only approximately: two half-distance
lerps do not have exactly the same transient result as one full-distance lerp.

Target-refresh invalidation folds raw bias bits and the core canonical anchor
multiset signature. Reordering the same occurrences, changing source, or
changing an unmasked target slot stays on the configured round-robin path.
Changing cardinality or any steering-semantic bit refreshes every authoritative
target immediately; the budget resumes on the next unchanged frame.

Possibility bucket flips use the declared domain-reader closure to mark dirty
layers. Revisions still record any material float-state movement, including
sub-bucket movement, but no longer determine tile staleness. Applying a newly
effective preserve winner follows the same separation: any exact realized
vector change advances revision once, while only quantized bucket flips dirty
tiles or cancel their in-flight work. Organism identity is keyed separately
from region revision: a same-bucket snap to canonical centers keeps tile
dependency hashes, jobs, entity ids, species, slots, cells, and jittered
positions intact, while an M/B/A expression-key change refreshes expressed
genome values.

### 3.6 Resonance

[`resonance.rs`](crates/world-runtime/src/resonance.rs) turns nearby realized
organisms into a one-frame graph. `RegionMap::resonance_at` gathers only
authoritative slot-0 organisms within the near radius, orders them by squared
distance with species and position tie-breaks, and truncates to the fixed
semantic ceiling `MAX_RESONANCE_NODES = 64`. The graph is not cached or
persisted, and neither `Budget` nor `ResourceTier` can change its contents.

Density dominates the formula, saturating at eight nodes. Entropy rewards a
mixture of species, distance rewards close nodes, compatibility rewards
agreement between authoritative current and the actual final projected target
over canonically active domains, and local canopy attenuates the result. The
active profile and target are both evaluated at the covering region center from
the same effective anchor multiset refreshed immediately before this read.
Missing authority, no active domain, and effective preserves are neutral;
ordinary pinned regions are not. With no nodes, resonance is exactly zero. The
next convergence pass uses only the scalar strength; node details remain
presentation/debug data.

The gate multiplies rather than adds to travel. This prevents a rich area from
transforming while the player stands still and prevents a barren crossing from
banking delayed change.

### 3.7 Routes through physical and possibility space

A [`RouteRecorder`](crates/world-runtime/src/route.rs) samples a journey at
every crossed 192-world-unit interval, up to 1024 nodes, carrying any remainder
into the next frame. The first node is immediate. Later nodes are interpolated
between the recorder's previous and current player positions in travel order;
if the due sample's covering region is not resident, the recorder keeps that
interval due and retries instead of moving later samples earlier. Each v2 node
stores:

* position rounded to integer world units;
* the covering region's quantized aspirational *target* vector;
* the covering region's quantized visible `current` vector, with `None` only
  for migrated v1 records that did not encode it;
* transition cost `floor(255 * (1 - resonance))`;
* stability in an 8-bit band;
* a canonical multiset signature of the effective anchors; and
* rounded segment distance since the previous node, with zero for the first
  node and migrated v1 nodes.

Transition cost uses the canonical `FrameStats::resonance_strength` from the
immediately preceding map update. If one frame emits several interval samples,
all of them use that frame-level resonance and effective-anchor set; recomputing
per-interpolated-position resonance is outside the current model. Higher visual
organism slots therefore do not change route nodes, content ids, or encoded
shared bytes.

The anchor summary folds cardinality and the complete raw-bit steering key for
every occurrence, retaining duplicates and excluding source/unmasked target
metadata. It covers the exact effective slice used by the immediately
preceding map update: the player's explicit anchors plus any selected route-
derived anchors when attraction is active. Route records also contain
discovery ids, a usage count, name, and journal. Difficulty is distance-
weighted by `distance_q` for v2 records; migrated v1 records without distance
metadata keep the old arithmetic mean of node costs divided by 255.

The stored target is not necessarily the world then visible to the player.
Near regions can remain pinned at `current` while their target retargets, so v2
records encode both the aspirational possibility signature and the visible
current signature. Recording while old-route attraction is active includes that
attraction in both the resulting target and the node's anchor summary.

When route attraction is enabled, nodes from every route in the open vault that
lie within 768 world units become derived Emphasize anchors. They affect
Climate, Hydrology, Ecology, Morphology, Behavior, and Aesthetics, but never
Planetary or Geology. A node's raw pre-normalization pull is

$$
w(u)=0.35\left(0.35+0.65\frac{u}{u+4}\right),
$$

where $u$ is route usage. Nearby candidates are sorted by squared distance,
route id, and node index, then capped by the frame budget (32 by default).
After truncation, all selected occurrences across every route share one global
peak budget. Let $s_i$ be their raw strengths and, for a common scale
$\lambda\in[0,1]$, define

$$
Q_d(\lambda)=1-\prod_{i:\,d\in M_i}(1-\lambda s_i).
$$

If the raw maximum over route domains is already at most 0.35, strength bits
are retained. Otherwise exactly 32 `f32` bisection trials retain the greatest
safe common scale with $\max_d Q_d(\lambda)\le0.35$. The exact ADR 0025
canonical product is tested; no transcendental inversion is used. Because
spatial falloff can only reduce each strength, the complete selected route
channel is capped at every evaluation position. Output stays nearest-first,
explicit anchors remain outside this budget, and the normalized route anchors
then pass through the same steering and plausibility projection as player
anchors (ADR 0026).

[`RouteTracker`](crates/world-runtime/src/route.rs) treats one continuous stay
inside a route corridor as a leg. On corridor exit, the leg increments usage if
it visited at least 60% of the route's distinct nodes. Firing on exit debounces
camping or lingering. Active leg state is part of the run-local session tier
when path tracking is enabled, but traversal remains unordered proximity, not
ordered segment progress.

`RouteGraph` is a rebuilt read-only view over all record nodes. A query scans
every node, computes Manhattan distance in the eight 12-bit possibility
buckets, sorts by distance/route/node, and returns the nearest $k$. The stored
signature seed determines build order but is not a metric index.

### 3.8 Field tiles, channel layout, and slope sampling

A [`FieldTile<T>`](crates/world-core/src/field.rs) owns a square row-major
`Vec<T>`, its resolution, and the dependency hash from which it was generated.
Tiles are immutable after integration and shared with workers through `Arc`.
`RegionTiles` is a structure-of-arrays container with 14 optional `f32` tiles,
one `u8` biome tile, and one `u16` dominant-species tile:

```text
elevation, hardness, temperature, moisture,
river, wetness, soil depth, fertility,
vegetation density, canopy height,
herbivore pressure, predator pressure, diversity, slope,
biome id, dominant roster index
```

At 32 by 32, a complete region has 60,416 bytes of sample payload, before map,
`Arc`, and allocation overhead. A content hash folds tile provenance and every
sample's exact bit pattern; it is a replay oracle, not a portable identity for
float tiles.

Terrain emits the slope used by Hydrology and Soils atomically with Elevation.
It is a centered finite-difference magnitude:

$$
s=\sqrt{\left(\frac{z(x_1,y)-z(x_0,y)}{(x_1-x_0)\Delta}\right)^2+
        \left(\frac{z(x,y_1)-z(x,y_0)}{(y_1-y_0)\Delta}\right)^2},
$$

where $\Delta=R/n=8$. A rolling one-cell elevation ghost ring supplies both
neighbors at every core cell, including all four tile edges and corners. The
ghost and a neighboring tile's core construct the same absolute cell-center
coordinate, possibility sample, and noise operations, so their shared-position
elevation bits agree exactly.

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

The denominator is $1.9375$. Elevation for a per-cell dequantized Geology and
Planetary sample is

$$
z(x,y)=600\,N(x,y)(0.5+G)-120(P-0.5).
$$

Sea level is zero. $G$ therefore scales relief from 0.5 to 1.5 times the base,
while increasing $P$ lowers the land; the end-to-end Planetary swing is 120
units. Gradient selection is integer identity; interpolation and possibility
scaling are float presentation math.

Terrain snapshots the absolute 3 by 3 region-center neighborhood. Resident
authority contributes realized `current` P/G buckets even when its fields are
parked; a missing coordinate contributes the projected/requantized anchor-free
field sample for that absolute coordinate. Every core and one-cell ghost
position bilinearly samples those centers in a fixed x-then-y order. Exact-
center axes fetch only the used center.

The Terrain dependency key folds all 18 ordered halo buckets. A P/G authority
change fans out the Terrain dependent closure to every field-active consumer
whose 3 by 3 halo contains the source. Loading, radius removal, preserve snaps,
session restoration, convergence, and field-recipe transitions share that
invalidation path; parking fields alone does not change authority.

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

Routing deliberately ignores each region's live realized state. Its isolated
evaluator contains no floating-point operation from control-point seeds through
the final centimeters. It extracts the field stream's 24-bit P/G components,
bilinearly combines them as exact integer rationals, floors them to 4096
buckets, and reconstructs exact Q30 bucket centers. Hashed gradients, octave
offsets, fade, interpolation, the 16:8:4:2:1 fBm spectrum, and Terrain's
conceptual P/G scaling all run in signed Q30 with `i128` products and sums.
Every division rounds to nearest with ties away from zero. The final conversion
is

$$
z_{cm}=\operatorname{round}_{\text{ties away}}(100z_{Q30}/2^{30}).
$$

This fixed evaluator is terrain-shaped but is not claimed bit-equal to the
float presentation surface. The macro dependency key includes the stored field
spacing and both Terrain and Drainage algorithm revisions. Terrain and Drainage
are revision 1; the world algorithm version remains 2.

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

Morphology, Behavior, and Aesthetics are not part of the aggregate Ecology
dependency hash. The four L8 outputs do not use them; near-field realization
tracks them with a separate expression key built from their quantized bucket
centers.

### 3.20 Near-field organism realization

[`realize.rs`](crates/world-runtime/src/realize.rs) uses a fresh, settled
Ecology-layer key, a stable habitat key, an explicit succession epoch, and a
separate M/B/A expression key to create transient `Organism` structs only
inside the near window. It reads the upstream vegetation, temperature,
moisture, fertility, and biome fields plus the roster/web; it does not sample
the L8 pressure, diversity, or dominant-index channels themselves. Each struct
contains feature id, species id, trophic role, explicit density slot, local
cell, world position, and expressed genome.

For resolution $n$, cell index $c=c_yn+c_x$, and resource-density slot $s$,
the feature index is

$$
i=c+sn^2.
$$

Slot 0 is the Phase 5 identity and canonical gameplay sample; higher slots are
additive presentation instances. The organism id folds world version, region,
layer 8, this feature index, and a compact revision derived from the typed
identity key. That key includes the fresh L8 provenance, field resolution,
sorted habitat-signature/roster content, and the succession epoch; it excludes
raw M/B/A floats and `RegionState::revision`. Separate SplitMix streams seeded
from the id decide:

1. create the organism if the first sample is below vegetation density;
2. classify the cell signature;
3. sample a surviving species proportionally to per-species biomass;
4. draw two fractions to jitter position uniformly inside the cell.

Expression is a pure post-selection transform: the selected species genome is
expressed under the M/B/A bucket centers and body size is capped at $s_{max}$.
Changing only Morphology, Behavior, or Aesthetics can update `expressed`, but
not presence, id, species, trophic role, density slot, local cell, or
world-space placement.

One density slot therefore has expected occupancy $d_v$; $k$ slots have
expected occupancy $kd_v$. A fixed pre-resonance pass publishes one nearest
whole slot-0 region per frame once its L8 key and complete roster set are
fresh. A separate post-integration pass may atomically recompute that vector at
the tier's one/two/four-slot visual density under
`max_realize_organisms`. Visual backpressure cannot delay or accelerate the
canonical publication schedule.

Runtime currency is split into canonical identity completion, canonical
expression completion, and visual presentation completion
`(identity, expression, slots)` over the same vector; empty barren vectors
retain these keys. Missing or changed stable identity provenance, missing
signatures or rosters, near exit, capacity parking, and session replacement
retire the vector before gameplay reads. Expression-only changes leave the old
canonical vector resident until the fixed pre-resonance publication pass can
atomically refresh it under the same identity and placement. There is no
movement, animation state, hunger, reproduction, age, interaction, or behavior
simulation.

### 3.21 Record schema and codec

[`record.rs`](crates/world-core/src/record.rs) is the boundary between live
float state and durable data. Every encoded body is preceded by

```text
Envelope { format_version, world_version, record_kind }
```

and both envelope and body use `serde` plus `postcard`. Readers reject a newer
format, a wrong kind, corrupt data, and trailing bytes. Version 1 route and
session bodies migrate explicitly into the v2 in-memory shape: legacy route
nodes have unknown visible-current signatures and zero segment distance, while
legacy session region targets are labelled as target-equals-current.

The record types are:

* `DiscoveryRecord`: source, habitat-signature seed, target signature, mask,
  polarity, quantized strength, integer radius and position, mutable name and
  journal;
* `RouteRecord`: ordered quantized nodes carrying target signature, optional
  visible-current signature, segment distance, discovery references, usage,
  mutable name and journal;
* `PreserveRecord`: coordinate-sorted region/signature pairs plus metadata;
* `SeenRecord`: a four-word bitmap for one 16 by 16 region chunk;
* `SessionSnapshot`: runtime metadata, exact player state, anchors,
  authoritative resident `current` and `target` region states, active route
  recorder state, and active route-tracker leg state, parked entries included;
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
the sorted coordinate/signature list. Migrated v1 routes whose nodes all have
unknown current signatures and zero distance preserve the v1 node fold and
therefore their legacy ids. New v2 route ids cover current-signature presence
and value plus segment distance. The merge rank for mutable text is the
lexicographic maximum of store-local sequence and a deterministic content hash.

Session metadata records the world/record versions through the envelope plus
the effective streaming config, frame budget, resource-tier label when known,
path-tracking toggle, route-attraction toggle, cache ceilings, and organism
density knob. Compatibility checks distinguish exact-compatible, compatible
but not exact, and incompatible loads. Executor queues, in-flight generation
jobs, disposable caches, rosters, realized organism vectors, renderer state,
and GPU resources are not persisted; exactness resumes after the documented
zero-travel settle under matching metadata, not by replaying worker queues.

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

Terrain adds one cross-region notification: when an authority's realized P/G
bucket pair can replace or be replaced by fallback, the runtime dirties the
Terrain dependent closure of every field-active consumer in its 3 by 3
neighborhood. The submitted owned halo is the same snapshot whose 18 buckets
formed the dispatch key; a late result from an older neighbor halo therefore
fails current-key integration. Parked sources remain authoritative halo input.

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

Shutdown is bounded by work that has already started. Dropping the executor
sets the shutdown flag and clears all queued lanes under the mutex, wakes every
worker, and then joins only the running closures. Workers check shutdown before
taking more queued work, and submissions racing after shutdown are dropped
without running. Native worker panics are caught and counted so the worker loop
continues and `parallelism()` remains the configured worker count. Runtime
generation closures also catch panics and send structured failed dispatch
results; the main-thread integrator retires only matching current job ids,
marks the failed layer/dependent closure dirty, and retries through normal
budgeted scheduling.

Per-frame work is bounded by counts and declared cost units, never elapsed
time. The low-tier 16.6 ms nominal budget is:

| Work | Limit per frame |
|---|---:|
| region loads | 48 |
| region convergence steps | 512 |
| generation cost | 96 |
| canonical slot-0 publication | 1 nearest whole region (fixed semantic work) |
| visual organism expansion | 400, with whole-region overshoot allowed |
| resonance nodes | fixed semantic ceiling of 64 |
| persistence operations | 8 |
| route-attraction nodes | 32 |
| target refreshes | unlimited |

Declared generation cost approximates about 25 microseconds per unit on the
reference machine. The measured halo Terrain job costs 4 and fixed-point macro
Drainage costs 31; other layers cost one or two. Budget exhaustion is
represented by deferred-work counters and is considered normal backpressure.
Finite budget scaling floors regeneration at the largest declared atomic layer
cost (currently Drainage at 31), so a quarter-budget schedule can defer work
without making a required job permanently inadmissible.

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
| organism slots/cell (displayed) | 1 | 2 | 4 |
| generation cost/frame | 96 | 192 | 384 |
| resonance nodes (gameplay) | 64 | 64 | 64 |
| target refreshes/frame | all | 160 | 240 |

An explicit environment override wins tier detection. Otherwise at most four
logical cores or a CPU-class graphics adapter selects Low; at least eight cores
and a discrete adapter selects High; other configurations select Mid.
`max_realize_organisms` controls only expansion beyond the already-published
canonical slot-0 vector. The one-region canonical admission and 64-node
resonance ceiling are fixed semantics, not tier capacity knobs.
Tier `max_retarget_regions` limits apply only while the canonical bias/anchor
fingerprint is unchanged; any cardinality or steering-semantic anchor change
refreshes all authoritative targets immediately.

The streaming data structures are:

* authoritative region map: coordinate -> `RegionState`, parked entries included;
* `RegionCache`: coordinate -> `RegionTiles`;
* `MacroCache`: level-4 coordinate -> shared `DrainageTile`;
* `RosterCache`: habitat signature -> shared `RosterEntry`;
* organism map: coordinate -> transient organism vector, with separate
  canonical identity, canonical expression, and visual
  `(identity, expression, slots)` currency maps;
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
scalar twins. Float Terrain presentation uses the four-wide fBm row kernel while
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

Identity-grade routing is deliberately separate: its Q30 evaluator is scalar
integer math and no longer calls the float SIMD row path. Terrain scales the
`n+2` relief row by per-cell halo samples and rolls three rows to emit Elevation
and centered Slope without a full ghost-patch allocation.

Target refresh is amortized on Mid and High. A steering-input hash folds bias
plus ADR 0025's canonical, cardinality-preserving anchor signature. Reordering
an unchanged multiset or editing source/unmasked target metadata does not force
work; count or steering-field changes force a full-authority target refresh.
Otherwise the runtime walks all authoritative coordinates, parked entries
included, round-robin under `max_retarget_regions`. Geometric stability is not
part of that budget: every authoritative region refreshes it every frame before
resonance and convergence.

### 3.27 CPU-authoritative presentation

The viewer and renderer are downstream of the model. The shared CPU
[`MapComposer`](../crates/viewer-host/src/map.rs) reads tiles and paints the
deterministic false-colour map used by both shells, screenshots, and headless
tests. It can display individual environmental channels, biomes, species,
ecology, influence, stability, discovered regions, routes, preserves, and
organism markers. The same crate owns atlas preparation, layout/focus,
CPU-authoritative inspection, and the semantic information-panel document;
native and web only render that document as bitmap pixels or DOM.

The normal native path uses a GPU field atlas. Each region slot packs:

```text
rgba32float: elevation, hardness, temperature, moisture
rgba32float: river, wetness, soil depth, fertility
rgba32float: vegetation, canopy, herbivore, predator
rgba32float: diversity, presence mask, unused, unused
rg16uint:    biome id, dominant roster index
```

The CPU-only Slope channel is explicitly skipped during atlas key/payload
packing. Elevation and Slope share the Terrain dependency hash, so Elevation's
existing upload provenance still observes every Terrain change without adding
a fifth plane or shader selector.

The shared
[`AtlasManager`](../crates/viewer-host/src/atlas.rs) assigns and recycles slots
and uploads a region only when its presence/dependency key changes. A storage
buffer maps visible window positions to slots. Sparse routes, rings, markers,
and other overlays remain CPU-drawn.
The POV chunk manager extends that tile key with the 18 buckets of the exact
owned Terrain halo snapshot passed to its mesh job. A neighbor P/G authority
flip therefore supersedes completed or in-flight old-halo meshes immediately,
before corrected Terrain and downstream tiles integrate.

The WGSL composition pass selects false-colour channels and can add up to three
hashed-gradient refinement octaves above the 32-by-32 authoritative sampling
rate. POV renders resident CPU terrain meshes and realized organism primitives.
Map, POV, and Split are panes of one logical viewer frame: the shared controller
computes travel and calls `RegionMap::update` once, then the renderer records
all visible panes in one surface acquire/submit/present. These details are
display-only. The renderer exposes no live readback method; no GPU result can
become a generation input, identity, steering value, resonance value, or
persistent record. ADR 0021's explicit file-bound headless capture is the only
readback exception.

The browser crate runs the streamed `RegionMap`, shared viewer/controller, CPU
Map path, WebGPU Map/POV/Split renderer where available, and visible
capability/loss fallbacks inside a static artifact. World jobs still use
`InlineExecutor`; Worker and IndexedDB code currently probes availability only,
and vault/session effects report that the browser service is unavailable. The
crate also exposes deterministic parity probes. CI executes every probe in Node
through pinned `wasm-bindgen-test` 0.3.76 and `wasm-pack` 0.13.1, while
`web-signoff` exercises the built app's sizing, input, focus, panel, Split, and
fallback behavior in a browser. Browser networking and accounts do not exist.

### 3.28 Verification surfaces

The repository uses several complementary checks:

* fixed golden hashes and record bytes in `world-core`, including additive
  canonical-anchor-signature, aggregate-route-attraction, and signed multi-
  macro drainage-topology fixtures;
* same-platform SIMD-versus-scalar differential tests;
* the continuity replay for pinned stability and bounded seams;
* `wer-ledger` for declared invalidation precision;
* focused `viewer-host` tests for the single binding/action authority,
  Map/POV/Split focus and layout, one-traveler/one-update controller traces,
  Map bytes and atlas deltas, CPU inspection, and stable panel documents;
* focused `pov-host` and renderer tests for resident-geometry ray picking,
  pane resource sizing, one-surface multi-view frame plans, and WGSL validity;
* focused `world-runtime` recovery tests for tight macro and roster ceilings,
  every-layer stale-result rejection, settled-cell roster inspection,
  fail-closed canonical invalidation/repair, fixed cross-budget publication,
  non-vacuous cross-density capture/resonance, and the exact sorted first 64
  canonical resonance nodes;
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
* native Map/POV/Split debug-capture tests for aligned layout, focus, pane,
  traveler/camera, hover, and information-panel reporting;
* the Phase 3 ecology harness (run as an integration test) for diversity and
  trophic bounds;
* `wer-anchor` for selective, coherent steering, resonance gating,
  Suppress-final compatibility, and exact canonical-multiset
  permutation/duplicate/polarity checks;
* `wer-vault` for persistence, merge laws, preserves, globally capped dense
  multi-route attraction, singleton usage, save/load, and an explicit 70-retry
  failure/ordering/delete/import-sequence scenario; and
* `wer-scale` for executor/budget/cancellation/amortization settled hashes,
  per-frame tight-versus-roomy regional-history equality under field pressure,
  field/pool plateaus, additive realization density, and exact Low/Mid/High
  canonical organism, anchored capture/resonance, actual route-record id/node,
  and encoded-byte equality; and
* wasm tests plus `web-signoff --assert-layout` for typed action decoding,
  controller traces, DPR/resize geometry, real input/focus routing, bounded
  panel cadence, Map/POV/Split behavior, and WebGPU loss fallback; the local
  `--profile-alignment` matrix adds Low/Mid/High structural/cache diagnostics
  and informational wall-clock telemetry.

Focused anchor tests exhaust all 720 permutations of an adversarial six-anchor
multiset at center, partial-falloff, and zero-influence positions, comparing
exact output bits. Runtime tests prove reorder and irrelevant metadata remain
amortized while duplicate/radius/masked-target edits refresh fully. A native
`World::update` regression reverses explicit anchors and route insertion while
requiring exact normalized strengths, target, compatibility, resonance cost,
signature, route id, and encoded bytes. The web parity surface adds fixed
duplicate-containing canonical-signature and quantized dense-route
normalization probes and executes those plus fixed routing elevations and three
complete macro topology folds in Node.

CI formats, lints, checks, and tests the native workspace, compile-checks
`world-core`, `world-runtime`, `viewer-host`, and `platform-web` for
`wasm32-unknown-unknown`, and then runs the shared parity suite in Node as
actual wasm.
