# New World Model Option 1 — a navigable manifold of procedural planets

## Status and purpose

This document proposes one complete Model for the concepts in
[`conceptual-model.md`](conceptual-model.md). It is a design, not a description of
landed behavior. The current prototype is documented in
[`world-model.md`](world-model.md); references to it below are comparisons and
implementation guidance, not compatibility requirements.

The design has four goals:

1. one compact point in Possibility denotes one complete world;
2. nearby points usually denote perceptually and ecologically related worlds;
3. Yearnings can be resolved into deterministic, order-independent motion; and
4. a bounded neighborhood can be queried quickly enough for real-time travel.

The Model is a deterministic, lazy function. It does not materialize a planet,
run weather, animate organisms, retain transition history, or render anything.
Those are Visualization responsibilities. It supplies canonical fields,
entities, time-independent ecological structure, temporal forcing, derivatives,
and continuity-risk estimates from which a Visualization can do those jobs.

The name **V1** identifies this proposed contract, not the current value of
`WORLD_ALGORITHM_VERSION`.

## 1. Summary of the construction

A world is addressed by a coordinate

$$
z=(z_0,\ldots,z_{31})\in\mathcal P\subset[-1,1]^{32}.
$$

The valid set $\mathcal P$ is the image of a fixed, smooth plausibility map
$\Phi$ applied to the full cube. Therefore every representable coordinate is
valid. The 32 coordinates do not directly mean “temperature” or “number of
predators.” They are independent latent controls. A fixed, versioned decoder
turns them into correlated planetary constants and coefficients of continuous
spatial fields.

World Space is an oblate spheroid. A location is addressed by a face of a
quadrilateral cube map plus two fixed-point coordinates; height is measured
along the ellipsoid normal. All canonical spatial fields are sums of:

- low-rank global basis functions whose coefficients vary smoothly with $z$;
- coordinate-hashed residual functions, shared by all worlds but smoothly
  warped and weighted by $z$; and
- algebraic or bounded-stencil derived fields enforcing broad physical and
  ecological relationships.

This gives the Model two useful execution modes:

- **continuous mode**, using `f32`/`f64` arithmetic and analytic derivatives for
  navigation and presentation; and
- **canonical mode**, using quantized inputs, fixed-point reductions, and
  integer identities for portable Impressions and shareable addresses.

The same equations define both modes. Canonical mode specifies the portable
rounding points.

## 2. Formal definition

### 2.1 Versions and seeds

A Model identity is

$$M=(\text{family},\text{major},\text{minor},K),$$

where $K\in\{0,1\}^{128}$ is a public world-family seed. `major` changes the
meaning of addresses; `minor` may add optional capabilities without changing
existing canonical results. Every hash domain includes $M$ and a typed domain
tag. Integer hashing is a specified portable permutation such as the current
SplitMix64 fold or a future explicitly versioned replacement.

No permanent identity is derived from a floating-point bit pattern. Canonical
identities fold quantized Possibility, integer spatial addresses, entity type,
and ordinal.

### 2.2 Possibility coordinates

The external coordinate is a signed fixed-point vector

$$q\in Q^{32},\qquad Q=\{-2^{23},\ldots,2^{23}\}/2^{23}.$$

Its continuous interpretation is $u=\operatorname{clamp}(q,-1,1)$. A
coordinate is decoded in three steps:

$$h_0=u,$$

$$h_{j+1}=\tanh(A_jh_j+b_j),\qquad j=0,1,$$

$$\theta=\Phi(u)=\theta_c+S h_2.$$

$A_j,b_j,S,$ and $\theta_c$ are small, frozen, rational matrices whose binary
representations are part of the Model version. $\theta$ contains the physical
and procedural parameters in Appendix A. Bounds are imposed by smooth scalar
maps:

$$
\operatorname{range}_{a,b}(x)=a+(b-a)\frac{1+\tanh x}{2},
\qquad
\operatorname{positive}_a(x)=a+\log(1+e^x).
$$

Coupled quantities use constructions that are valid by definition. Fractions
are a softmax, covariance matrices are $LL^T+\epsilon I$, ordered thresholds
are cumulative positive increments, and resource allocations are normalized
nonnegative vectors. Thus $\Phi$ is total: it never returns an invalid planet.

The **Model State** is exactly $(M,q)$. Decoded values and caches are derived,
not state. Two different $q$ values intentionally denote different Model
States even if finite-resolution observations happen to coincide.

Theoretical Possibility is the real cube $[-1,1]^{32}$. Representable
Possibility is $Q^{32}$. Reachable Possibility is defined in Section 5.

### 2.3 World Space

The canonical surface address is

$$x=(f,a,b),\quad f\in\{0,\ldots,5\},\quad a,b\in I_{48},$$

where $I_{48}$ is signed Q2.46 on $[-1,1]$. A specified cube-map tie rule gives
one unique face on edges and corners. Mapping the cube direction $d(x)$ to an
oblate spheroid with equatorial radius $R_e$ and polar radius $R_p$ gives the
reference surface $s(x;z)$. A full three-dimensional address adds signed
centimetre altitude $y$ along the ellipsoid normal and, where required, a
canonical time coordinate.

The compact address avoids longitude singularities, supports exact quadtree
cells, and makes neighboring sample discovery bounded. Great-circle distance
is the Model's World Space metric. Face seams are merely chart seams: every
field is evaluated from the normalized three-dimensional direction, so values
and first derivatives agree across them.

### 2.4 Canonical time

The Model exposes forcing at integer time $t$ in SI seconds from a versioned
epoch. It defines orbital phase, axial illumination, lunar tide phase, and
expected climatic envelopes. It does not define instantaneous storms,
organism locations, or behavior.

For example, top-of-atmosphere forcing is

$$
I(x,t;z)=L(z)D(t;z)^{-2}\max(0,n(x;z)\cdot s_\star(t;z)).
$$

Queries may request a time or a time interval. Interval queries return bounds
and moments suitable for Visualization simulation. An Impression includes $t$
only when its subject depends on canonical forcing.

## 3. Generating a complete world lazily

### 3.1 A common field family

Every primitive scalar field uses a shared evaluator. For unit direction
$d\in S^2$, latent state $z$, channel $c$, and octave count $O_c$,

$$
F_c(d,z)=\mu_c(z)+
\sum_{k=0}^{K_c-1}\alpha_{c,k}(z)B_k(W_c(d,z))+
\sum_{o=0}^{O_c-1}\beta_{c,o}(z)N_{c,o}(W_c(d,z)).
$$

$B_k$ are low-degree real spherical harmonics. $N_{c,o}$ is a compact-support
gradient lattice on the cube-map hierarchy. Its gradients are hashes of
integer vertices and do not change with $z$. $W_c$ is a small smooth,
orientation-preserving warp:

$$W_c(d,z)=\operatorname{normalize}(d+\gamma_c(z)V_c(d)).$$

The coefficients $\mu,\alpha,\beta,\gamma$ are affine functions of decoded
$\theta$ followed by bounded maps. Octave amplitudes obey
$|\beta_{c,o}|\le C_c2^{-H_co}$ with $H_c>1$. Consequently the infinite ideal
field converges, and a finite query has a known truncation error:

$$
\left|F_c-F_c^{(O)}\right|\le
\frac{C_c2^{-H_cO}}{1-2^{-H_c}}.
$$

The evaluator returns value, spatial gradient, and latent directional
derivative in one pass. A query requests only the derivative columns it needs.

This family is continuous in both spaces. Hashed residuals provide unbounded
detail without making nearby worlds unrelated: worlds share the residual
recipe and vary its amplitude and warp continuously.

### 3.2 Planet and geology

Decoded planetary parameters include radii, gravity, atmosphere mass and
composition, ocean inventory, stellar flux, axial tilt, rotation period, and
orbital elements. Bounds restrict V1 to broadly Earth-like worlds that the
required Visualization capability can present.

Crustal potential is a vector field $C(d,z)\in\mathbb R^3$. Plate cells are a
weighted spherical Voronoi diagram of at most 32 sites. Site directions are
continuous rotations of fixed hashed directions; weights and velocities are
smooth in $z$. A soft minimum

$$
D_i(d,z)=\frac{\exp(-\kappa\delta_i(d,z))}
{\sum_j\exp(-\kappa\delta_j(d,z))}
$$

replaces discontinuous nearest-site membership when computing canonical
uplift. Boundaries may still be classified by `argmax` for names and entity
identity, with the risk reported explicitly.

Raw elevation combines continental potential, boundary uplift, volcanic
hotspots, impact basins, and multiscale residual relief. Sea level $\lambda$
is not a free threshold. It is the unique solution of

$$
\int_{S^2}\max(0,\lambda-E_0(d,z))\,dA=V_o(z).
$$

V1 approximates this global integral over a fixed icosahedral quadrature of
16,384 samples and solves with 16 deterministic bisection steps. The resulting
$\lambda(z)$ is cached once per active Possibility coordinate. Elevation is
$E=E_0-\lambda$.

Lithology is a normalized mixture of at most eight canonical rock families.
Age, permeability, hardness, albedo, nutrient supply, and erosion resistance
are continuous fields derived from plate provenance and mixture weights.

### 3.3 Drainage and standing water

Exact global river topology is inherently discontinuous under small elevation
changes. V1 separates a continuous water-potential field from canonical
topology.

At level $L_d$ (nominally 2 km cells), quantized elevation is evaluated on a
one-cell halo. Depressions are resolved by a deterministic priority flood, and
each cell routes to its lowest neighbor using a total integer tie order.
Accumulation is reduced in height order. This is close to the proven integer
macro-routing approach in the current implementation, generalized to a
planetary hierarchy.

The canonical drainage graph for a cell is a pure function of $(M,q,L_d,$ cell
address$)$. A local query computes a watershed patch and follows boundary
outlets into a memoized coarser graph; it never computes the whole planet.
Coarser levels supply conservative inflow summaries. Refinement continues
until requested error or display scale is met.

River discharge is

$$Q_r=A_r\,P_{\rm eff}\,(1-\eta_{\rm soil})$$

with seasonal bounds. Lake level is the lowest spill saddle of the filled
basin, limited by water balance. The Model returns both the continuous wetness
potential and discrete river/lake entities. It also returns distance to the
nearest routing tie so the Visualization can anticipate a topology change.

### 3.4 Climate

V1 uses an equilibrium energy-moisture model, not fluid dynamics. A fixed
six-layer multigrid over the sphere solves monthly mean temperature and column
moisture. At hierarchy level $l$ the state per cell is
$c=(T,M,U,V)$, and one Jacobi step is

$$c_i^{n+1}=A_i(z)c_i^n+\sum_{j\in N(i)}B_{ij}(z)c_j^n+f_i(E,I,z).$$

Matrices are constructed to be contractive:
$\|A_i\|+\sum_j\|B_{ij}\|\le1-\epsilon$. Six V-cycles with a fixed traversal
order yield a deterministic result and a certified residual. The coarse grid
has 1,536 cells; fine corrections are evaluated only around requested tiles.

The output includes monthly mean/range of temperature, precipitation,
humidity, wind, snow cover, and cloud potential. Orographic fine correction
uses local elevation gradient. A Visualization may synthesize weather whose
statistics remain inside these envelopes.

### 3.5 Soil and biome

Soil state is an algebraic steady-state tuple

$$S=(\text{depth},\text{water capacity},\text{organic carbon},
\text{nutrients},\text{pH},\text{disturbance}).$$

It is a bounded function of lithology, slope, drainage, climate moments, and
canonical biological productivity. To avoid a circular dependency, soil and
productivity solve a four-iteration contraction initialized from abiotic soil:

$$S_{n+1}=f_S(G,C,P_n),\qquad P_{n+1}=f_P(C,H,S_{n+1}),$$

with fitted coefficients constrained so the combined Jacobian norm is below
0.7. Four iterations have a declared maximum residual; a high-precision query
may run to tolerance.

A biome is not authoritative state. It is a label obtained from continuous
environmental attributes by maximum membership among a versioned fuzzy set.
Queries return all memberships, so a Visualization can blend boundaries.

### 3.6 Species pool and ecological assembly

Each world owns a canonical global species pool of $S=4096$ lineages. Species
$i$ has an integer genome

$$g_i=H(M,q,i),$$

where $q$ is folded directly; genome decoding maps its bits and selected smooth
world parameters to:

- morphology and life-history traits;
- environmental preference vector $m_i$ and tolerance covariance $\Sigma_i$;
- resource production/consumption vector $r_i$;
- behavior and reproductive strategy; and
- canonical observable attributes and relationship roles.

Using the entire quantized world coordinate means exact species identities can
change at a Possibility quantum. Continuous lineage correspondence is provided
separately: lineage slot $i$ persists throughout Possibility and its phenotype
is the smooth function $\psi_i(z)$. The integer `species_id` identifies the
exact manifestation at $q$; `(model, lineage_slot)` identifies ancestry across
nearby states.

At environmental vector $e(x,z)$, suitability is

$$
a_i=\exp\left[-\tfrac12(e-m_i)^T\Sigma_i^{-1}(e-m_i)\right].
$$

Only a deterministic candidate set of 64 lineages is evaluated: 48 are found
by locality-sensitive hash buckets of environmental preference and 16 are
coordinate-hashed rare candidates. Candidate lookup is bounded and independent
of cache contents.

Local equilibrium biomass $b\in\mathbb R_{≥0}^{64}$ is the fixed point

$$
b_i^{n+1}=\operatorname{softplus}
\left(\ell_i+\log(a_i+\epsilon)+\sum_jJ_{ij}\frac{b_j^n}{1+b_j^n}\right),
$$

normalized to the local energy budget after each iteration. $J$ is decoded
from resource traits and constrained to spectral norm below 0.8. Eight fixed
iterations give stable, order-independent assembly. The result is an expected
population distribution, food web, diversity, and carrying capacity—not
individual simulated organisms.

Canonical organism manifestations are addressable samples

$$o=(\text{cell},\text{lineage slot},\text{sample ordinal},t_{\rm epoch}).$$

Their canonical age class, sex/reproductive mode where applicable, size,
health envelope, and observable traits are hashed samples from the equilibrium
distribution. Their instantaneous position and behavior belong to the
Visualization.

### 3.7 Canonical entities

The Model exposes entities only when they have durable meaning: plate,
watershed, river reach, lake, biome membership maximum, lineage, and canonical
organism manifestation. An id is

$$
\operatorname{id}=H(M,q,\text{kind},\text{canonical spatial key},
\text{ordinal}).
$$

Every entity includes its defining attributes and a **margin**, the minimum
change in its classifier before identity changes. Attributes without a robust
margin should be captured by value rather than by entity id.

## 4. Realization contract

### 4.1 Query levels

The Model is queried through immutable snapshots:

```rust
pub struct ModelAddress { pub model: ModelId, pub q: [i32; 32] }
pub struct WorldPoint { pub face: u8, pub u_q46: i64, pub v_q46: i64, pub cm: i32 }
pub struct CanonicalTime(pub i64);

pub trait Model {
    fn open(&self, at: ModelAddress) -> Result<Snapshot, ModelError>;
}

pub trait Realization {
    fn planet(&self) -> Planet;
    fn sample(&self, request: &SampleRequest, out: &mut SampleBatch);
    fn tile(&self, request: &TileRequest, out: &mut Tile);
    fn entities(&self, request: &EntityRequest, out: &mut EntityBatch);
    fn ecology(&self, request: &EcologyRequest, out: &mut EcologyBatch);
    fn sensitivity(&self, request: &SensitivityRequest) -> Sensitivity;
}
```

`SampleRequest` contains sorted positions, time or time interval, channel mask,
accuracy level, and optional latent direction. Results retain input order.
`TileRequest` names a cube-map quadtree cell, resolution, halo, and channel
mask. All methods are pure: output depends only on the request and snapshot.

Capabilities are named and versioned. V1 requires geometry, climate moments,
hydrology, soil, ecology, canonical attributes, sensitivity, and time forcing.
Species meshes, audio, animations, and live weather are never capabilities of
the Model.

Builds are also outside this contract. The Model can verify the Model State and
World Space address to which a Build's Impression refers, and can provide
terrain and semantic attachment frames there. Loading, presenting, simulating,
or hiding the Build cannot alter $q$, any canonical field, or Reachability.

### 4.2 Accuracy and error

Each query selects `Preview`, `Interactive`, or `Canonical` accuracy.

- Preview uses fewer field octaves and climate corrections.
- Interactive bounds terrain error below one projected pixel or a supplied
  world tolerance.
- Canonical uses fixed octave counts, fixed solver iterations, canonical
  rounding, and returns the reference result for Impressions.

Every approximate result carries componentwise error bounds and a dependency
key. Refinement with the same snapshot narrows those bounds; it must not change
canonical identity unless a prior result explicitly marked that identity
unresolved.

### 4.3 Impression payload

A portable Impression stores:

```text
model family + major version + public seed
32 signed Q1.23 Possibility coordinates
cube face + two Q2.46 World coordinates + centimetre altitude
optional canonical time
subject kind + canonical id or canonical attribute record
attribute schema version + canonical query accuracy
```

The Model can validate the address without a Visualization. The canonical
attribute record is preferred for unstable classifiers and is sufficient for a
Yearning even if a later compatible Visualization cannot depict the subject's
original form.

## 5. Geometry and motion in Possibility

### 5.1 Perceptual metric

Euclidean latent distance is cheap but not meaningful enough. At state $z$,
define a fixed probe set of 256 planetary locations and a normalized observable
summary

$$R(z)\in\mathbb R^{192}.$$

It contains global physical parameters and deterministic moments of terrain,
climate, hydrology, biome memberships, productivity, lineage traits, and food
web structure. Let $J_R(z)$ be its Jacobian. The local metric is

$$
G(z)=\lambda I+J_R(z)^TWJ_R(z),\qquad \lambda>0,
$$

where diagonal $W$ is versioned and dimensionless. For a small displacement
$\delta z$,

$$d_P(z,z+\delta z)^2\approx\delta z^TG(z)\delta z.$$

The floor $\lambda I$ ensures distinct coordinates remain separated even in a
locally insensitive direction. Distances over long paths are lengths under
this Riemannian metric. Runtime navigation only needs $G$, a 32 by 32 symmetric
matrix, refreshed when $z$ moves by a configured radius.

### 5.2 Reachability

A continuously differentiable path $z(s)$ is reachable when it remains in the
cube and obeys

$$
\dot z^TG(z)\dot z\le v_P^2,
\qquad
\chi(z,\dot z)\le\chi_{\max}.
$$

$\chi$ is continuity risk (Section 7), not validity. Because $\Phi$ is total,
there are no invalid holes. All interior representable states are connected,
but high-risk regions require smaller steps or a path around them. Reachable
Possibility over a finite Egress budget is the set attainable under these
bounds. Hardware and Visualization choices do not occur in this definition.

Quantized movement accumulates a continuous private navigator coordinate and
rounds to $Q^{32}$ only when committing a new canonical state. Error-feedback
rounding prevents small requested movement from being lost.

### 5.3 Attribute map for Yearnings

The Model publishes a schema of normalized canonical attributes. An attribute
$a$ defines:

- an observation function $A_a(z,x,t)$;
- a population measure $\mu_a(z)$ over the whole planet or an applicable
  population;
- a monotone membership function $m_a$;
- derivatives or deterministic finite differences; and
- applicability and uncertainty.

Scope $s\in[0,1]$ maps to a target quantile

$$\tau(s)=0.001^{1-s}0.8^s.$$

Thus singular is approximately the top 0.1%, common lies between, and
pervasive approaches 80% of applicable cases. The prevalence statistic is a
smooth soft quantile over the fixed planetary probes and species lineage
slots. It describes the destination world, never radial falloff around the
source Impression.

### 5.4 Order-independent Yearning objective

Each enabled Impression attribute becomes a term $(f_i,r_i,w_i)$:

- Accentuate: $f_i$ is negative target prevalence or magnitude;
- Repress: $f_i$ is positive target prevalence or magnitude;
- Hold: $f_i=(A_i(z)-A_i(z_0))^2$;
- Disable: no term.

$r_i$ contains the requested scope and captured canonical value. Inputs are
canonicalized by content id; equal semantic terms are combined by exact integer
weight addition. The objective is

$$
L(z)=\sum_i\bar w_i f_i(z;r_i)
+\rho\,d_P(z,z_0)^2
+\eta\,C(z),
$$

where $C$ penalizes high continuity risk and $\bar w_i=w_i/\sum_jw_j$. Hold is
a weighted resistance, not an absolute constraint; Model validity and the step
bound always take precedence.

One Egress intent step solves the trust-region problem

$$
v^*=\arg\min_v
\left[\nabla L^Tv+\tfrac12v^T(H_L+\gamma G)v\right]
$$

subject to $v^TGv\le\Delta^2$ and cube bounds. V1 uses eight deterministic
preconditioned conjugate-gradient iterations, then metric-normalizes and clips
the result. The diagonal plus low-rank Hessian approximation is positive
definite. Summations use canonical term order and compensated reduction.

Conflicting Yearnings therefore compromise simultaneously. They cannot acquire
priority from insertion order. If the gradient nearly vanishes, the Model
returns a deterministic set of up to three eigen-directions representing
meaningfully different compromises rather than inventing an arbitrary one.

### 5.5 Attractors and exact destinations

An exact Impression is simply a target $z_a$. A diffuse Attractor is a mean
$\bar z$, covariance $\Sigma_a$, strength $w_a$, and Model identity. Its term is

$$f_a(z)=\tfrac12(z-\bar z)^T(\Sigma_a+\epsilon I)^{-1}(z-\bar z).$$

It becomes exact-address capable only when its 95% metric-radius is below one
canonical Possibility quantum in every resolved direction. Community evidence
is external to the Model; the Model only validates and navigates the supplied
distribution.

## 6. Resonance

Resonance measures how well the local geometry of Possibility supports the
requested intent. It never depends on rendered organisms, cache readiness,
frame timing, or hardware.

Let $g=-\nabla L$, $v^*$ be the constrained solution, and let $J_Y$ contain
the gradients of active Yearning statistics. Define

$$
r_{\rm align}=\frac{\max(0,g^Tv^*)}
{\sqrt{g^TG^{-1}g}\sqrt{{v^*}^TGv^*}+\epsilon},
$$

$$
r_{\rm support}=\exp\left(-\frac{\|J_Yv^*-d_Y\|_W^2}
{\|d_Y\|_W^2+\epsilon}\right),
$$

$$
r_{\rm safe}=\exp(-\chi(z,v^*)/\chi_0).
$$

The reported Resonance is

$$\mathcal R=(r_{\rm align}r_{\rm support}r_{\rm safe})^{1/3}.$$

It is a confidence and rate signal. The Traveler may require a gameplay
threshold, but the Model's recommended canonical Egress rate is
$v_P\mathcal R$. Low Resonance means the intent conflicts, has weak local
sensitivity, or points through a discontinuity. This resolves the conceptual
question without tying reachable worlds to local realized creature counts.

## 7. Continuity and sensitivity

The Model reports rather than hides unavoidable discontinuities. For a proposed
direction $v$, it computes

$$
\chi(z,v)=
w_f\|J_Fv\|_{\infty}
+w_c\sum_k\frac{|\nabla m_k\cdot v|}{m_k+\epsilon}
+w_t\sum_e\frac{|\nabla \delta_e\cdot v|}{\delta_e+\epsilon}.
$$

The terms respectively measure rapid continuous-field change, proximity to
classification margins, and proximity to topology tie margins. The query also
returns per-channel predicted change and the spatial locations of the largest
risks.

For two committed states $z_0,z_1$, a `TransitionDescriptor` contains:

- the metric displacement and recommended maximum interpolation step;
- matched lineage slots and entities whose ids remain robust;
- changed river/lake/classification cells in requested World Space bounds;
- conservative bounds on every requested continuous channel; and
- a deterministic blend coordinate $s\in[0,1]$.

The descriptor does not make regional history part of Model State. A
Visualization may retain the old realization near the Traveler, blend far
fields, or delay entity replacement. The Traveler still has one canonical
state $z(s)$, and an Impression always records that state rather than a local
mixture.

## 8. Real-time execution design

### 8.1 Work decomposition

Opening a snapshot performs only latent decoding, sea-level lookup, coarse
climate lookup, and metric reuse. Spatial queries are organized as immutable
jobs:

```text
decode q -> global constants / coefficients
         -> coarse climate and sea level
tile address -> primitive fields -> drainage -> local climate correction
             -> soil/productivity -> ecology -> entity extraction
```

Each result key folds Model id, $q$, canonical time bucket where relevant,
quadtree address, accuracy, channel, and algorithm revision. Like the current
dependency-hash design, cache residency is never authority and stale results
cannot integrate into another snapshot.

The neutral implementation exposes a caller-supplied scratch arena and a job
graph. Native and browser hosts choose threads or inline execution. No neutral
crate opens files, creates threads, invokes graphics APIs, or reads a clock.

### 8.2 Bounded costs

Recommended Interactive constants are:

| Operation | Bound | Target desktop CPU time |
|---|---:|---:|
| Snapshot decode | two 32-wide MLP layers | < 0.05 ms |
| Primitive field batch | 8 octaves, 16 channels, 1,024 points | < 0.4 ms |
| 33x33 terrain tile + halo | fixed stencil | < 0.3 ms |
| Drainage patch | 35x35 priority flood | < 0.5 ms |
| Local climate correction | 4 stencil iterations | < 0.3 ms |
| Soil/productivity | 4 algebraic iterations | < 0.2 ms |
| Ecology per cell | 64 candidates, 8 sparse iterations | < 0.02 ms |
| Metric and Yearning step | 256 probes, batched derivatives | < 2.0 ms |

These are acceptance budgets, not claimed measurements. They must be replaced
by benchmark results on native and wasm before implementation sign-off. The
important structural bound is that none grows with explored area or planet
surface area.

A 60 Hz Visualization should use the Model asynchronously: complete near-field
collision data first, then visible terrain, environmental shading, ecology,
and entity detail. Model commits occur at a slower fixed navigation cadence
(for example 10 Hz); interpolation is presentation-only. At fast travel speed,
the requested spatial tolerance selects fewer octaves, so work follows screen
error rather than meters traveled.

### 8.3 Data layout and Rust implementation

Hot field batches use structure-of-arrays storage, 32-bit aligned values, and
preallocated scratch buffers. Fixed-size latent matrices use stack arrays.
Sparse ecological interactions store at most eight edges per candidate.
Quadtree keys are packed integers. Public result types contain no references to
cache storage.

Portable scalar kernels are the reference. Native SIMD and optional wasm SIMD
evaluate the same lane-wise operations and are differential-tested, following
the successful pattern in the current `world-core/src/simd.rs`. Canonical
reductions remain scalar fixed-order unless an exactly equivalent SIMD
reduction is proven.

Suggested crate boundaries are:

```text
world-model-v1-core   fixed point, hashes, coordinates, latent decoder, metric
world-model-v1-fields primitive/derived fields and canonical entities
world-model-v1-nav    Yearning objective, trust-region solve, sensitivity
world-model-v1-api    capability and Realization contract types
```

All four are platform-neutral. Existing `world-runtime` patterns for abstract
execution, byte ceilings, deterministic scheduling, and stale-result rejection
can host them without making the mathematical design depend on current region
state or layer declarations.

### 8.4 Caches

Four bounded caches are sufficient:

1. snapshot globals keyed by $(M,q)$;
2. coarse sea-level/climate solutions keyed by $(M,q,$ time bucket$)$;
3. primitive and derived tiles keyed by complete dependency key; and
4. species preference buckets keyed by $(M,q,$ environment band$)$.

Eviction changes latency only. Results are immutable. Recomputing after
eviction is exact at the selected accuracy. A Visualization may maintain its
own continuity cache, but that is not Model authority.

## 9. Determinism contract

V1 defines three explicit grades:

1. **Address identity:** integer hashes, fixed-point coordinates, entity ids,
   dependency keys, and encoded Impression attributes are bit-identical on all
   conforming targets.
2. **Canonical realization:** canonical queries specify every rounding point,
   iteration count, traversal order, approximation, and transcendental
   implementation. They are bit-identical on native and wasm. Portable
   polynomial approximations replace platform `tanh`, `exp`, and `log` here.
3. **Interactive realization:** results satisfy declared error bounds but may
   differ in low bits through SIMD or hardware arithmetic. They may never
   create a portable id. Canonical resolution confirms an observation before
   an Impression is committed.

All jobs are pure and integration is keyed, so settled results are independent
of worker count, scheduling, cancellation, and cache capacity. A Model major
version change is required for any canonical drift. A channel-local revision
may be used only when its dependency closure and Impression compatibility are
machine-readable.

Required verification includes golden native/wasm samples; cube-face seam
tests; analytic-versus-finite-difference Jacobians; metric positive-definiteness;
solver contraction bounds; Yearning permutation tests; fixed-point topology
tests; cache/schedule independence; error-bound containment; and performance
plateaus under long travel.

## 10. Relationship to the current implementation

The current implementation already demonstrates several mechanisms worth
retaining at the engineering level:

- deterministic integer hashing and versioned identities;
- lazy coordinate-derived content rather than stored generated worlds;
- declared dependencies and dependency-hash-gated integration;
- integer macro drainage topology;
- bounded caches, pools, deterministic scheduling, and cancellation;
- canonical order-independent steering reductions;
- aggregate ecology with near-field realization outside the field model; and
- strict neutral/platform crate boundaries with native/wasm verification.

V1 deliberately changes the underlying semantics:

| Concern | Current model | Proposed V1 |
|---|---|---|
| Point in Possibility | eight scalars plus authoritative per-region current state | one global 32-D coordinate denotes the entire planet |
| World Space | effectively planar streamed regions | finite oblate planet with exact cube-map addresses |
| Plausibility | ordered clamp/projection rules | total smooth decoder with validity by construction |
| Steering | spatial anchors over region targets | global attribute/prevalence objective from Yearnings |
| Scope | not represented as global prevalence | smooth planetary/species quantile target |
| Distance | component differences | observable pullback Riemannian metric |
| Resonance | realized near-organism and travel gate | model-only intent support and continuity confidence |
| Continuity | region current/target history in runtime | sensitivity descriptor; history remains Visualization state |
| Ecology | habitat-signature roster of at most 12 species | global lineage pool plus bounded local assembly |
| Species sharing | identity becomes portable at persistence boundary | canonical model identity and lineage correspondence |
| Time | runtime/presentation concern | canonical orbital and environmental forcing only |
| Determinism | mixed portable and same-platform grades | portable canonical query required for Impressions |

Compatibility is neither required nor implied. In particular, current anchors,
preserves, routes, eight-component possibility signatures, and generated
regions cannot be reinterpreted as V1 addresses. A migration tool could embed a
current observation as a Yearning and search for a similar V1 state, but the
result would be a new Impression, not the same world.

## 11. Acceptance criteria for an implementation

The design is implementable when a prototype can demonstrate all of the
following without special cases:

1. any 32-component coordinate opens a valid deterministic planet;
2. a canonical address reproduces selected fields and entities on native and
   wasm;
3. a nearby Possibility step predicts observed change within returned bounds;
4. cube-face and tile seams are value- and gradient-continuous;
5. arbitrary Yearning input permutations produce identical steps and
   Resonance;
6. singular/common/pervasive requests measurably affect planetary prevalence;
7. conflicting Accentuate, Repress, and Hold terms yield a bounded compromise;
8. a local Realization query has cost independent of travel history and planet
   size;
9. a long high-speed traversal stays within fixed memory ceilings;
10. canonical topology and entity changes are preceded by a shrinking reported
    margin;
11. schedules, cancellation, worker count, and cache capacity do not change
    settled results; and
12. measured Interactive queries meet a 60 Hz host's background work budget on
    the supported native and browser reference machines.

## Appendix A: decoded parameter blocks

The fixed decoder produces these bounded blocks. They are derived values, not
additional Model State.

| Block | Examples | Construction constraint |
|---|---|---|
| Astronomy | stellar luminosity, orbit, moons | stable bounded orbits |
| Planet | equatorial/polar radius, mass, rotation, tilt | positive mass/radii, bounded flattening |
| Atmosphere | pressure, greenhouse strength, gas fractions | positive pressure, fractions sum to one |
| Hydrosphere | water volume, ice response, salinity | nonnegative inventories |
| Crust | plate sites, velocities, rock mixture priors | normalized mixtures, bounded velocities |
| Field spectrum | means, harmonic coefficients, octave amplitudes, warps | convergent amplitudes, valid warps |
| Climate | transport, lapse, moisture, seasonal response | contractive solver matrices |
| Soil | weathering, retention, nutrient response | bounded positive rates |
| Ecology | energy efficiency, trait covariance, interaction strength | positive covariance, contractive interactions |
| Navigation | observable weights, risk weights, trust radius | positive metric floor and bounds |

The decoder matrices should initially be hand-designed and fitted from a corpus
of accepted procedural worlds. Training or fitting is a development tool only:
the shipped matrices and approximations are frozen data, and runtime evaluation
is fully deterministic.

An implementation is not conforming until a **parameter manifest** supplies
every matrix entry, range endpoint, basis normalization, hash tag, solver
coefficient, polynomial coefficient, probe coordinate, channel weight,
iteration count, and rounding rule named by this document. The manifest is
hashed into the Model major identity and is the numeric instantiation of these
equations. This document specifies the system and its constraints; the first
implementation supplies the finite constants, which then become immutable test
fixtures rather than tunable runtime configuration.

## Appendix B: one navigation tick

Given current canonical state $q_0$, active Yearnings $Y$, and maximum metric
step $\Delta$:

1. decode $q_0$ and retrieve or compute snapshot globals;
2. canonicalize and combine semantic Yearning terms;
3. batch-evaluate the fixed observable probes and requested derivatives;
4. form $G$, $L$, the constrained direction $v^*$, risk $\chi$, and Resonance;
5. reduce $\Delta$ until the risk bound holds;
6. integrate one midpoint step in continuous latent coordinates;
7. clamp to the cube and error-feedback-round to Q1.23;
8. open the new immutable snapshot and emit a transition descriptor; and
9. let the Visualization decide when and where to refine and blend the new
   Realization.

The tick is a pure function of its explicit inputs. The gameplay layer may
multiply $\Delta$ by physical distance traveled, coordinate dual-space arrival,
or refuse Egress while stationary. None of those policies changes the Model's
definition of Possibility or Reachability.
