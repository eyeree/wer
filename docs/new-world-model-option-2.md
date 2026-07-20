# New World Model Option 2 — Attribute-native worlds on a spherical planet

## Status and purpose

This document proposes a clean-slate Model for the concepts in
[`conceptual-model.md`](conceptual-model.md). It is a design, not a description
of landed behavior or measured performance. The current implementation remains
documented in [`world-model.md`](world-model.md); references to it below are
comparisons and implementation guidance, not compatibility promises.

This revision makes five contracts explicit:

1. one exact integer **Canonical Attribute State** denotes one complete world;
2. World Space is a finite round planet, not an infinite plane;
3. every core YearningSteerable attribute is audited against the realized planet;
4. Canonical Realization and Egress are portable across native and wasm; and
5. continuity history is a bounded Visualization concern, never hidden Model
   state.

The proposal is intentionally narrower than a general world-program system. Its
distinctive bet is:

> **Option 2 trades an opaque latent decoder and an open-ended law/program
> ontology for a smaller promise: every intrinsic axis has public attribute
> semantics, every named core outcome is audited against the realized sphere,
> and Egress takes the locally least-cost feasible intrinsic change—under the
> pullback attribute metric—that reduces the current weighted intent.**

The resulting Model is fixed-vocabulary. It cannot acquire a new independent
physical degree of freedom or a new causal regime without a Model-major change.
That is a deliberate cost of keeping steering direct, inspectable, and small.

Making World Space planetary does not turn this into another proposal. Option 1
uses an opaque latent cube, an oblate cubed sphere, and realized probe
summaries. Option 3 makes the coordinate a natural parameter of a statistical
world-law and navigates with Fisher geometry; its continuity mechanism
transports realized distributions. The World Loom uses a typed causal program,
a nested icosahedron, projective solvers, and transport/rewrite navigation.
Option 2 instead uses a public attribute state, direct attribute-conditioned
feature synthesis, bounded attribute locks, a compact natural-gradient solve,
and a two-endpoint presentation wake. Option 2 and Option 3 may share a
12-patch equal-area addressing primitive without sharing a state ontology,
metric, generator, or continuity authority.

Notation: \(\|v\|\) is the Euclidean norm,
\(\|v\|_g=\sqrt{v^\top g\,v}\), and
\(\operatorname{clamp}(z,a,b)\) clamps to \([a,b]\). A hat denotes a
fixed-point or integer quantity. “Canonical” means that formats, operation
order, rounding, bounds, and failure behavior are part of the Model version.

---

## 1. System overview

The static Model is

\[
\mathcal M =
  \big(M,\ \mathcal U,\ F,\ \mathcal D,\ \mathcal O,\ g_u,\ W,\ \mathcal Q\big),
\]

with:

| Symbol | Name | Responsibility |
|---|---|---|
| \(M\) | Model identity and manifest | versions, public family seed, coefficients, numeric formats, grids, and revisions |
| \(\mathcal U\) | intrinsic state space | the \(k\) independent public coordinates of complete valid worlds |
| \(F\) | named attribute map | decodes intrinsic coordinates into the \(A\) audited outcome attributes |
| \(\mathcal D\) | attribute-first decoder | derives planetary constants, field coefficients, and bounded calibration parameters |
| \(\mathcal O\) | Realization audit | measures the named attributes on the realized sphere |
| \(g_u\) | pullback attribute metric | defines perceptual distance and Egress in intrinsic coordinate space |
| \(W\) | spherical Realization | immutable planetary fields and canonical entities at a state, place, and Model time |
| \(\mathcal Q\) | query and numeric contract | accuracy grades, error bounds, portable arithmetic, and deterministic status results |

`ModelRoot(M)` is the content-derived digest of the complete canonical manifest:
family and major identity, public family seed, attribute/grid schemas, layer and
numeric revisions, coefficients, tables, and capability declarations. A bare
version number is never sufficient provenance.

There are five different kinds of state. They must not be conflated:

- \(q_\star\in\widehat{\mathcal U}\) is the one canonical Model State and
  therefore the address of one complete world.
- \(n_\star\) is a Traveler-owned fixed-point **Navigation Accumulator**. It
  retains sub-quantum Egress progress but is not another world coordinate.
- \(x_\star\in S^2\) plus radial altitude is the Traveler's World Space
  location.
- A Traveler-owned **Transition Cooldown** records the odometer span after
  every q-changing commit during which another Egress step cannot accrue.
- A bounded **Transition Wake** may retain one old/new endpoint pair for
  presentation. It is Visualization session state and never enters \(W\),
  Resonance, an Impression, or a permanent identity.

The canonical pipeline for one navigation epoch is:

~~~text
active Yearnings
    -> exact id-keyed reduction of one-sided requests and activation-time Holds
    -> constrained least-squares objective in measured attribute space
    -> canonical metric/KKT solve and request-specific Resonance
    -> fixed-point Navigation Accumulator advanced by an integer travel quantum
    -> unambiguous lattice candidate, feasibility check, and attribute-lock audit
    -> q0 -> q1 canonical commit, or a typed no-commit result
    -> W(q1, spherical address, Model time)
    -> optional Visualization blend of W(q0) and W(q1) from a TransitionRecipe
~~~

Rendered frames may interpolate or predict this work, but rendered frame
subdivision never chooses a Model State.

---

## 2. Possibility and its exact coordinate

### 2.1 Attribute-native topology

The theoretical intrinsic state space is a feasible subset

\[
\mathcal U\subset
\mathbb T^{k_c}\times\prod_{j=1}^{k_b}[0,1],
\qquad k=k_c+k_b,
\]

with intrinsic coordinate \(u\in\mathcal U\). A typed public map

\[
F:\mathcal U\longrightarrow
\mathcal A\subset
\mathbb T^{A_c}\times\mathbb R^{A_b},
\qquad a=F(u),
\]

produces the named audited outcome vector. Usually \(A\ge k\): for example,
\(m-1\) intrinsic conditional-allocation coordinates decode to \(m\) named
simplex shares. \(F\) is total and periodic in cyclic inputs, but it need not be
surjective onto arbitrary ambient attribute vectors or expose an inverse
outside its feasible image. It is, however, **state-separating on the feasible
space**: \(F(u_0)=F(u_1)\) implies
that \(u_0\) and \(u_1\) are the same point under the declared cyclic and
constrained-face equivalences. Conditional coordinates made irrelevant on a
simplex boundary are fixed to their canonical zero value by validation. Thus
two valid \(q\) values cannot produce identical exact content merely while
changing permanent ids. The named-output Q formats and \(h_j\) lattice must
preserve that separation on \(\widehat{\mathcal U}\); a manifest with a
fixed-point collision is invalid.

Differential formulas use the manifest's continuous, piecewise-smooth
theoretical extension of \(F\). Canonical evaluation at a lattice point uses
the corresponding fixed-point formulas and returns an enclosure of that
extension; at a knot it uses the declared one-sided derivative. Finite
cell/lineage allocations are a later AttributeLock operation, not silently
differentiated integer counts.

A first implementation is expected to use \(24\)–\(48\) intrinsic axes. Unlike
an opaque latent vector, every intrinsic axis has a versioned public meaning
and an explicit contribution through \(F\) to named audited outcomes:

- a **cyclic** axis must denote a genuinely circular audited quantity, such as
  dominant terrain orientation, hue phase, longitude of periapsis, or dominant
  ecological pattern orientation; and
- a **bounded** axis denotes a normalized scalar or prevalence, such as radius,
  sea fraction, warmth, relief, productivity, or trait prevalence.

Because active constraints create faces and corners, \(\mathcal U\) is more
precisely a stratified feasible space (a manifold with boundary on each smooth
stratum), not an everywhere-smooth manifold. Grouped simplex axes have public
conditional-allocation semantics and decode to final named shares by the
manifest's exact dependent-remainder rule; later finite census allocations use
the separate largest-remainder rule in Section 3.2.

Ordinary interval-valued magnitudes never use cyclic axes. Every cyclic value
enters the decoder through a periodic embedding

\[
e_j(\alpha_j)=
  \big(\cos(2\pi\alpha_j),\ \sin(2\pi\alpha_j)\big),
\]

implemented canonically by a frozen fixed-point table and interpolation rule.
Decoder output, the metric, and field coefficients therefore agree at the
\(0/1\) seam. There is no claimed inverse from an arbitrary ambient Euclidean
observation to a torus; a torus cannot have one. State separation on the typed
feasible space is not such a claim. Impressions store \(q\) directly, and
Egress uses local tangent information, so no inverse API is needed.

The feasible subset \(\mathcal U\) is the preimage under \(F\) of the coupled
validity rules on named outcomes. Examples include nonnegative inventories,
normalized composition simplexes, a water/relief/sea-fraction relation, energy
and moisture bounds, productivity limits, and trophic energy inequalities.
Section 3 defines how those rules are enforced without pretending that a
forward clamp is a metric-nearest projection.

### 2.2 Canonical lattice

The runtime Model State is the integer lattice point

\[
q\in\widehat{\mathcal U}\subset
(\mathbb Z/2^{32}\mathbb Z)^{k_c}
\times\{0,\ldots,2^{32}-1\}^{k_b}.
\]

It is stored as exactly \(k\) unsigned words. Interpretation is:

\[
\alpha_j(q)=
\begin{cases}
q_j/2^{32}, & j\text{ cyclic},\\[2mm]
q_j/(2^{32}-1), & j\text{ bounded}.
\end{cases}
\]

The vector \(u(q)=(\alpha_1(q),\ldots,\alpha_k(q))\) is the typed intrinsic
coordinate consumed by \(F\), \(g_u\), and Egress. Thus cyclic maximum wraps
to zero on the next unit, while both endpoints of a
bounded axis are representable. Serialization is little-endian `u32` in
manifest axis order. There is no signed Q32 overflow and no disagreement
between “floor” and “round”: the only real-to-lattice operation is
round-to-nearest, ties-to-even, followed by modular reduction for cyclic axes or
saturation for bounded axes.

Each axis also declares a reachable commit quantum \(h_j\) in integer word
units. For a cyclic axis, \(h_j\) is a power of two dividing \(2^{32}\). The
validated lattice contains those multiples; a bounded axis additionally admits
\(2^{32}-1\) as its exact upper endpoint. Normal Egress therefore does not treat
every stored low bit as a useful world transition. The manifest chooses
\(h_j\) from the audited sensitivity and error contract: one step must be
observable within the declared canonical resolution, while the vector trust
radius in Section 8 bounds simultaneous multi-axis displacement. Changing
\(h_j\) changes representable addresses and therefore changes Model identity.

Not every \(k\)-word bit pattern is feasible. A public constructor validates
the \(h_j\) lattice, fixed-point constraints, and constructional AttributeLock
certificate, and returns either a
`CanonicalAttributeState` or `InvalidState`. Representable Possibility is the
set of validated lattice points, not the set of all byte strings.

The same \(q\) always denotes the same Model State. In particular, there is no
“sub-bucket canonical world”: \(W\), attributes, entities, dependency keys, and
Resonance all consume the exact lattice point \(q\).

### 2.3 Navigation Accumulator

Smooth low-speed input is retained outside the Model State in

\[
n_j\in\mathbb Z,
\]

with 32 fractional bits below one \(q_j\) unit. Cyclic accumulators reduce
modulo \(2^{64}\); bounded accumulators saturate to
\([0,(2^{32}-1)2^{32}]\). The candidate lattice word first uses
\(\operatorname{round}_{\mathrm{even}}(n_j/2^{32})\), then rounds to the
nearest admissible \(h_j\) point with the same tie rule.

The accumulator:

- belongs to the Traveler/session, not to \(\mathcal M\);
- is serialized in a run-local session snapshot for exact replay;
- cannot affect \(W\) before a validated \(q\) commit; and
- is reset to \(q_j2^{32}\) when an exact Impression address is opened.

It also stores the digest of the normalized active intent plan: `ModelRoot(M)`,
attribute/metric schema identity, Yearning ids, weights, activation snapshots,
selected Attractor mode, and the exact Attractor evidence-snapshot root. If any
of those inputs or that digest changes, every sub-quantum component resets to
\(q_j2^{32}\) before the new plan can accrue travel credit. Progress earned
under one intent cannot steer a later unrelated request.

Egress consumes fixed integer World Space arclength quanta. Splitting the same
physical path among different render frames therefore leaves \(n_\star\) and
all committed \(q_\star\) values unchanged.

### 2.4 Theoretical, representable, and reachable

- **Theoretical Possibility** is the continuous feasible intrinsic space
  \(\mathcal U\), with named outcome image \(F(\mathcal U)=\mathcal A\).
- **Representable Possibility** is its validated lattice
  \(\widehat{\mathcal U}\).
- **Reachable Possibility** from \(q_0\) is the set of lattice points connected
  by the bounded canonical Egress steps in Section 8, with every intermediate
  candidate feasible and with nonzero request support.

Reachability may have narrow or disconnected lattice components at a particular
precision. The continuous feasible space may itself have multiple components
unless the manifest certifies otherwise. The Model reports those conditions; it
does not silently jump a gap.

---

## 3. Feasibility and the realized attribute contract

### 3.1 One fixed core vocabulary

The core schema is the versioned vector
\(a(q)=F(u(q))\) with units, range, cyclicity, applicability, and semantics
fixed by \(M\). The \(q\) words are intrinsic independent coordinates: exact
equalities such as a composition simplex are parameterized by \(k-1\) public
conditional-allocation words, and \(a(q)\) decodes the final \(m\) named
shares. Dependent equality quantities are never stored as separately rounded
words.

Core state attributes have two explicit capability classes:

- **YearningSteerable** attributes are fixed prevalence, CDF-knot, predicate,
  or allocation outcomes. They support Accentuate, Repress, Hold, and Disable
  with the Scope semantics in Section 7.
- **HoldableContext** attributes are audited global scalar or cyclic context.
  They enter Realization, the metric, coupling, Attractors, and Hold, but do not
  pretend that Accentuate/Repress Scope is a scalar magnitude control.

A **DerivedQuery** observable is not part of the core state at all.
Representative groups are:

| Group | Representative core attributes | Capability class |
|---|---|---|
| Planet/orbit | radius, gravity, day length, axial tilt, stellar forcing | HoldableContext |
| Surface/geology | sea fraction, relief CDF knots, uplift prevalence, dominant orientation | YearningSteerable basis plus HoldableContext orientation |
| Climate/hydrology | warmth/aridity CDF knots, drainage and standing-water prevalence, seasonality | YearningSteerable basis plus HoldableContext seasonality |
| Ecology | productivity CDF knots, biome mixture, lineage-diversity basis | YearningSteerable |
| Morphology | fixed body-scale CDF knots and locomotion-predicate prevalences | YearningSteerable |
| Behavior | fixed activity, aggression, and sociality CDF/predicate knots | YearningSteerable |
| Aesthetics | hue/luminance/chroma CDF knots and pattern-orientation mixture | YearningSteerable basis plus HoldableContext phases |

The decoder

\[
\mathcal D:a\longmapsto m
\]

derives nuisance generator parameters from these public attributes. It may add
correlated coefficients, field spectra, ecological rates, and calibration
thresholds, but those derived values are not extra independent dimensions.
Realization may use raw \(q\) only for full-state provenance and
coordinate-specific manifestation identity after semantic content has been
selected; generation cannot smuggle an unaudited residual channel around
\(F\).

### 3.2 Feasible attribute space

Validity is expressed in attribute space with fixed-point inequalities and
equalities. A simplified subset is:

\[
\begin{aligned}
V_{\mathrm{water}} &\ge 0,\\
\sum_i c_{\mathrm{atmos},i}&=1,\qquad c_{\mathrm{atmos},i}\ge0,\\
a_{\Pr[X>t_i]}&\ge a_{\Pr[X>t_{i+1}]}
  \quad\text{for every ordered CDF knot pair},\\
a_{\mathrm{vegetation}}
  &\le f_{\mathrm{veg}}(a_{\mathrm{water}},a_{\mathrm{warmth}},
                        a_{\mathrm{soil}}),\\
a_{\mathrm{consumer}}
  &\le \eta_{\mathrm{trophic}}a_{\mathrm{npp}},\\
\|r_{\mathrm{water}}\|_\infty
  &\le \varepsilon_{\mathrm{water}},\qquad
\|r_{\mathrm{energy}}\|_\infty
  \le \varepsilon_{\mathrm{energy}}.
\end{aligned}
\]

Simplexes use intrinsic stick/conditional-allocation coordinates. The
theoretical \(F\) yields continuous final shares; its canonical fixed-point
evaluation assigns the final named-Q remainder by attribute id so the shares
sum exactly to one. When those shares allocate a finite audit census, the
AttributeLock uses exact largest-remainder allocation with attribute-id
tie-breaking. Positive quantities use bounded monotone maps. Covariance-like
coefficients are constructed from \(LL^\top+\epsilon I\). Thus exact equalities
hold by parameterization and independent word rounding cannot violate them.
Fixed CDF bases use monotone-increment coordinates, so decoded knot prevalences describe
a valid distribution and the AttributeLock preserves that ordering. Coupled
water, energy, and ecological closure uses a fixed count of damped integer
iterations; it returns its residual bounds rather than claiming that a one-way
triangular ceiling captures all feedback.

Normal Egress remains feasible through an active-set tangent solve and a
canonical backtracking line search. A small
\(\operatorname{CanonicalRetraction}\) exists for internal initialization: it
wraps cyclic axes, saturates bounded intrinsic axes, and repeats the
manifest's ordered feasibility relaxations to a fixed point. If it does not
reach one under the declared cap, initialization fails. Every returned value is
therefore a fixed point and the returned-domain operation is idempotent. It is
explicitly **not** claimed to be the nearest point under \(g_u\).

External invalid records are rejected or handled by an explicit versioned
migration. They are never silently “projected” into a different world.

### 3.3 Canonical population measures

Every steerable prevalence is a measured statistic on the finite planet, not a
distribution parameter merely consumed by generation. For an attribute \(a\),

\[
\phi_a(q)=
\frac{
  \sum_{i\in P_a(q)}
    \widehat w_i\,\widehat A_a(i,q)\,\widehat m_a(i,q)}
{\sum_{i\in P_a(q)}
    \widehat w_i\,\widehat A_a(i,q)},
\]

where:

- \(P_a\) is a fixed equal-area cell set, canonical lineage roster, or their
  declared product;
- \(\widehat A_a\) is a fixed-point applicability predicate;
- \(\widehat m_a\) is a fixed-point membership function;
- \(\widehat w_i\) is an integer area, population, or applicability weight; and
- numerator and denominator use checked widened integer sums.

The attribute manifest specifies the census level, fixed predicate/CDF knots,
denominator, denominator-zero behavior, and error bound. Environmental
prevalence is area/frequency over applicable spherical cells. Organism
prevalence is over applicable canonical species or lineage mass, never over the
incidental organisms currently visible.

Sea fraction is the equal-area fraction of canonical audit cells at or below
sea datum. A body-scale basis entry is the weighted fraction of applicable
lineages above one fixed manifest knot. A captured threshold may select a knot
or use a declared monotone interpolation of adjacent knots with a certified
approximation interval; it never changes the meaning of a core axis. Requests
outside the fixed basis are query-only or `UnsupportedCapability`. There is
therefore a reproducible population and fixed schema behind every Scope value.

Where a canonical attribute summarizes time-varying forcing, \(P_a\) also
contains the manifest's fixed Model-time phase samples and weights. Core
attributes are time-independent summaries of that measure; an observation at a
specific \(\tau\) is a separate time-indexed derived observable.

### 3.4 AttributeLock and RealizationAudit

For each core YearningSteerable axis, the generator contains a bounded monotone
**AttributeLock** so that the canonical audit satisfies

\[
\operatorname{err}_a\big(\mathcal O_a(W(q)),a_a(q)\big)
\le\varepsilon_a.
\]

\(\operatorname{err}_a\) is absolute error for scalar/prevalence axes,
simplex distance for allocations, and shortest wrapped distance for cyclic
axes.

Typical locks are:

- sea datum selected by a stable integer order statistic subject to the decoded
  water inventory and basin constraints;
- a zero-mean field basis plus a bounded forcing/bias solve for global warmth or
  aridity;
- a constrained biome-suitability multiplier, followed only at the end by
  largest-remainder quantization of an already feasible allocation;
- suitability-constrained trait-expression thresholds over fixed lineage slots;
  and
- normalized integer allocations for atmosphere, water, and trophic budgets.

Locks solve nuisance inputs or multipliers subject to the same water, energy,
habitat, and trophic constraints as the downstream layers; they do not repaint
audited labels or traits after physics merely to satisfy a quota. The audit
repeats the measurement from the generated output and does not trust the
requested decoder input.

An axis may be advertised as **YearningSteerable** only if a monotone
construction or bounded calibration certifies its tolerance over **every** state in
\(\widehat{\mathcal U}\). This constructional certificate is part of state
validation; the shipped and held-out corpora are regression evidence, not the
domain of the guarantee. A word vector for which the base Canonical audit
cannot complete within its contract is not a valid
`CanonicalAttributeState` and cannot appear in an Impression. Core
HoldableContext attributes have the same all-state audit obligation but need
not provide a monotone distributional Accentuate/Repress construction.
Attributes without the guarantee appropriate to either class are derived
query-only observables.

The named core vector \(a=F(u)\) is thus “attribute-native” in a testable
sense: within its declared quantization/error interval, each named value is the
value measured on the world. The intrinsic \(u\) words are its public,
constraint-safe parameterization, not a second set of alleged observations.
The UI can also read the coupling Jacobian and residual bounds to show likely
collateral changes.

### 3.5 Honest extensibility boundary

- A query-only derived observable may be added compatibly if it does not enter
  state, steering, the metric, identity, or an existing dependency key.
- A directly steerable attribute changes the feasible manifold, locks,
  Jacobian, metric, reduction schema, and usually the coordinate format. It is
  a Model-major change.
- A new independent physical/ecological degree of freedom changes the address
  schema and is a Model-major change.
- Discrete causal regimes and relationship-topology rewrites are intentionally
  a poor fit. This option does not claim the World Loom's structural
  extensibility.

---

## 4. Spherical World Space and canonical time

### 4.1 A finite round planet

World Space is the surface and radial neighborhood of a round planet:

\[
S^2_{R(q)}=\{R(q)d:d\in\mathbb R^3,\ \|d\|=1\},
\]

plus signed integer-centimetre altitude \(h\in[h_{\min},h_{\max}]\) along
\(d\). The manifest requires \(R_{\min}+h_{\min}>0\). Radius is a core bounded
attribute. The angular address of a place remains the same as radius changes
through Egress; validation rejects an altitude outside the manifest interval.

Surface distance is great-circle distance

\[
d_{S^2}(x,y;q)=R(q)\arccos(d_x\cdot d_y),
\]

with a frozen fixed-point dot/angle kernel in Canonical mode. Full
three-dimensional World Space uses the declared product metric

\[
d_W(x,y;q)^2=d_{S^2}(x,y;q)^2+
\eta_h^2(h_x-h_y)^2,
\]

where \(\eta_h>0\) is a manifest unit scale. Exploration and Egress credit
therefore include vertical flight rather than treating it as zero travel.
Ground neighborhoods and Transition Wake radii use \(d_{S^2}\); dual-space
arrival uses full \(d_W\). None uses planar Euclidean distance.

### 4.2 EqualAreaPlanetGrid

The selected `EqualAreaPlanetGrid v1` uses the equal-area coordinates

\[
z=\sin\varphi\in[-1,1],\qquad \lambda\in[0,2\pi),
\]

for which unit-sphere area is \(dA=d\lambda\,dz\). A base patch is
\((b,r)\in\{0,1,2\}\times\{0,1,2,3\}\) with local
\((\xi,\eta)\in[0,1)^2\):

\[
z=-1+\frac{2}{3}(b+\eta),\qquad
\lambda=\frac{\pi}{2}(r+\xi),
\]

\[
d(\xi,\eta)=
\big(\sqrt{1-z^2}\cos\lambda,\
     \sqrt{1-z^2}\sin\lambda,\
     z\big).
\]

Each patch has normalized area \(1/12\), because its unit-sphere area is
\((2/3)(\pi/2)=\pi/3\). Bisecting both \(\xi\) and \(\eta\), with child digit
\(2\eta_{\mathrm{high}}+\xi_{\mathrm{high}}\), creates four equal-area children.
Thus level \(L\) contains

\[
N_L=12\cdot4^L
\]

cells of equal canonical weight. This simple \(z\)-longitude construction is
not a cube map, an icosahedron, or an unspecified library profile.

At \(\lambda=2\pi\), ownership wraps to \(\lambda=0\). Cell interiors are
half-open. The two exact poles have explicit `NorthPole` and `SouthPole` point
variants and no longitude or chart coordinate; a cell-form point address is
valid only for \(-1<z<1\). Cells still cover the closed sphere for integration:
the lowest global incident cell owns each measure-zero pole. Polar cell edges
collapse to zero-measure vertices and carry no direct flux; vertex halos join
incident sectors through the one canonical polar vertex. All other shared
edges are half-open with the lower global cell id owning an exact boundary
point. These rules make the longitude seam and poles address-normalization
cases, not generator branches.

The remaining numeric manifest freezes:

- base-patch order and orientation;
- nested child-digit/Morton order;
- fixed-point \(\pi\), square-root, sine/cosine, center, corner, and
  unit-direction approximations with monotonic error intervals;
- half-open edge ownership and lowest-global-cell-id corner tie-breaking;
- parent, child, reciprocal neighbor, and one-ring halo rules; and
- canonical levels used by each physical layer and audit.

The exact equations above, their portable transcription, and known-answer
fixtures at every base edge, corner, longitude seam, and polar neighborhood are
the contract. Instantiating the Q formats/tables and validating their error
intervals remains an early implementation gate; a floating reference library
alone is insufficient.

A canonical **point** has one fixed precision \(L_p\) and is the tagged sum:

\[
x=\begin{cases}
(\text{grid version},\operatorname{Cell}(\text{pixel id at }L_p,
   \hat\xi,\hat\eta),\text{altitude}_{\mathrm{cm}}),&-1<z<1,\\
(\text{grid version},\operatorname{NorthPole},
   \text{altitude}_{\mathrm{cm}}),&z=1,\\
(\text{grid version},\operatorname{SouthPole},
   \text{altitude}_{\mathrm{cm}}),&z=-1.
\end{cases}
\]

Cell subcoordinates use Q0.32 exactly: a word \(s\) denotes
\(s/2^{32}\in[0,1)\). Real inputs round to nearest, ties-to-even; an exact upper
edge normalizes to subcoordinate zero in the owner neighbor before encoding, so
the value one is never stored. No cell variant may encode either pole. The
point level satisfies \(L_p\le30\), and every query/canonical level is also at
most 30, so \(12\cdot4^L\) base-plus-Morton ids fit `u64`. The level is not part
of a point address, so the same geometric point cannot acquire aliases at
multiple LODs. Variable-level tile and physics queries instead use a separate
\(\operatorname{EqualAreaCellId}=(L,\text{nested pixel id})\); its parent cell
is derived from the point's fixed address. Hashes use normalized global
point/cell keys, never a face-local floating-point UV. Validation rejects a
point whose grid version differs from \(M\), a non-owner seam encoding, a pole
encoded as a cell, or an out-of-range fixed subcell value.

For an exact pole variant, a containing-cell query at level \(L\) returns the
lowest-global-id incident cell, while flux/topology queries use the separate
polar control-volume key below. Thus a pole has one deterministic cell
ownership answer without acquiring a fictitious longitude.

The 12 patches are addressing charts only. Scalar fields are evaluated from a
versioned fixed-point three-dimensional unit direction or a global cell key.
Vector fluxes use oriented global edges or three-dimensional tangent vectors.
Cross-patch stencils use the same reciprocal neighbor table and halo as
interior stencils. There is no longitude identity at a pole, and no generator
may branch on an arbitrary local azimuth there.

The chart becomes increasingly anisotropic in the last latitude row under
unbounded refinement, so it is **not** used as an unqualified graph PDE grid.
The manifest fixes a maximum canonical physics level \(L_{\mathrm{phys,max}}\).
At every physics level, all cells incident to one pole are conservatively
restricted to one north or south polar control volume for climate, drainage,
hydrology, and other flux/topology solves. Widened sums in global cell-id order
compute its sources and conserved inventories. Its canonical scalar result is
assigned to each incident polar cell; the supernode itself has zero audit mass,
so the original equal-area cells are each counted exactly once. Boundary flux
is prolonged by fixed integer edge weights plus largest-remainder edge-id
ties, preserving the exact supernode total.

Macro Drainage treats incident polar cells as directed into the polar
supernode. The supernode is either a declared basin sink or has one outgoing
noncollapsed boundary edge selected by filled routing elevation and then global
edge id. That supernode/boundary pair owns any polar river manifestation; no
collapsed edge or arbitrary polar longitude owns topology. The grid
profile must certify a maximum nonpolar cell aspect ratio, metric condition
number, and polar-supernode stencil condition at every allowed physics level.
If any bound fails, that level is invalid. Finer nested cells may support
pointwise feature sampling and presentation, but cannot silently refine a
canonical graph solve or topology. This preserves equal-area audits while
making polar numerical stability a finite, testable contract.

The shared polar vertex and each polar control volume have direct global keys.
A local point/halo query reads those keys and never walks the
\(4\cdot2^L\) incident-cell fan. The finite fan is reduced only as part of the
manifest-capped whole-level flux pass, with an independently stated operation
and byte bound. Thus increasing presentation LOD cannot turn one polar sample
into unbounded local work.

This hierarchy is deliberately distinct from a six-face cube map and a
20-triangle icosahedral hierarchy. Equal cell area gives this proposal the
actual canonical measure used by audits at each declared level:

\[
\mu_L(C)=\frac{\#C}{12\cdot4^L}.
\]

The manifest's equal-area map and exact four-child refinement must separately
prove refinement consistency and convergence to normalized spherical area if a
continuum limit is claimed. The finite-level counting measure above does not
depend on that unproved limit.

### 4.3 Canonical Model time

Model time is

\[
\tau\in\mathbb Z
\]

seconds from a versioned epoch. Decoded planetary constants include stellar
luminosity, orbit period/eccentricity, rotation period, axial tilt, longitude
of periapsis, and optional moon/tidal parameters. Canonical integer phase
accumulators plus frozen periodic tables determine diurnal, seasonal, orbital,
and tidal forcing. For example,

\[
I(d,\tau;q)=
L(q)D(\tau;q)^{-2}
\max\big(0,d\cdot s_\star(\tau;q)\big).
\]

The Model exposes forcing, deterministic envelopes, and time-indexed canonical
fields. Instantaneous storms, organism behavior, animation, and other transient
simulation remain Visualization responsibilities. Each time-dependent layer
defines an exact \(\operatorname{time\_key}_\ell(\tau)\) and evaluates only the
representative/interpolation rule named by that key. A cache may use that key;
it may not coarsen arbitrary \(\tau\) behind the API. Climate normally caches
the fixed 12-month array for \(q\), while a portable phase interpolator derives
the current envelope.

The manifest declares a finite integer forcing cycle \(P_\tau\) for all
periodic Model channels and reduces phases modulo it before table lookup;
nonperiodic channels must instead provide time-independent global bounds.
Checked time arithmetic and overflow status are part of \(\mathcal Q\). This is
the cycle over which Section 9 certifies a reusable TransitionRecipe.

---

## 5. Realization on the sphere

### 5.1 Fixed spherical feature bank

Nearby states should share continuous primitive feature sites rather than
reseed every detail. Primitive channel \(c\) uses a state-independent bank of
compact spherical kernels:

\[
\Phi_c(a,d,\tau)=
b_c(a,\tau)+
\sum_{\ell=0}^{L_c}
\sum_{j\in J_{c,\ell}(d)}
w_{c,\ell,j}(a,\tau)
\psi_\ell(d\cdot p_{c,\ell,j}).
\]

The sites \(p_{c,\ell,j}\), kernel shapes \(\psi_\ell\), and feature-bank ranks
are domain-separated integer hashes of \(M\), channel, level, and global cell
id. They do **not** hash \(q\). Bounded coefficients and orientations are
periodic/smooth functions of the attribute state. Consequently, adjacent states
share the sites of continuous primitive channels while changing their
expression. Thresholded drainage, classification, and ecological topology can
still change discontinuously; margins and changed cells expose those events
rather than calling them a smooth transform.

Kernels are sampled from the three-dimensional direction, so patch boundaries
do not create field seams. Compact support bounds local query work. Tail bounds
or exact finite support determine the reported field error at each accuracy.

### 5.2 Declared planetary dependency graph

Above the nine spatial layers, a bounded
\(\operatorname{PlanetaryClosure}(q)\) jointly solves the audit-level sea datum,
ocean/ice/atmospheric/land water allocation, annual energy/moisture envelope,
and productivity capacity. It makes sea fraction and total water inventory
compatible before either becomes a layer input. The closure emits immutable
budget and forcing roots; downstream layers refine within those allocations and
cannot feed an undeclared value back into the closure.

The first implementation uses the same conceptual nine-layer order as the
current declared graph, with a spherical algorithm behind every node:

| Layer | Canonical outputs and bounded construction |
|---|---|
| Terrain | elevation and slope from the feature bank, fixed audit-level zero-mean corrections, relief lock, and sea datum |
| Geology | lithology, hardness, uplift, and erosion coefficients from terrain plus conserved crustal allocations |
| Macro Drainage | fixed-level directed edges, integer priority flood, stable cell-id ties, accumulation, basins, and cross-patch outlets |
| Climate | monthly energy/moisture envelopes from stellar forcing, altitude, ocean mask, and a fixed number of conservative graph-flux sweeps |
| Hydrology | runoff, standing water, snow/ice, and water balance from drainage plus climate with a reported inventory residual |
| Soils | depth, texture, nutrients, and moisture capacity from geology, climate, and hydrology using bounded contractive updates |
| Biome | canonical class memberships and margins from climate, soils, and water; soft memberships remain available across class boundaries |
| Vegetation | cover, structure, and productivity subject to water, energy, nutrient, and biome limits |
| Ecology | fixed lineage roster, habitat scores, sparse food web, trophic biomass, trait prevalence, and organism-candidate manifestations |

This table is a set of required, still-uninstantiated algorithm obligations and
research hypotheses, not shorthand that all layers are “analogous noise” and
not evidence by itself that the closures converge or produce varied worlds.
Each layer manifest must fix its input channels, canonical level, halo,
traversal order, iteration count, coefficients, conserved quantities, residual
bounds, entity keys, and downstream revision closure before implementation
sign-off.

Water closure conserves the decoded inventory among ocean, ice, standing water,
soil moisture, and atmospheric capacity within
\(\varepsilon_{\mathrm{water}}\). Climate sweeps report energy and moisture
residuals. Ecological assembly enforces nonnegative biomass and a bounded
trophic-energy budget. Concretely, the Ecology manifest fixes a roster cap,
genome/trait score formats, at most \(E_{\max}\) candidate food-web edges per
lineage, integer edge-score ties, a fixed number of sparse biomass-relaxation
iterations, and a per-cell organism-candidate cap. Habitat and trophic failure
remove expression from a slot; they do not allocate a post-hoc organism merely
to hit a prevalence quota.

If a Canonical query cannot meet a declared residual under its work cap, it
returns a continuation or `Unresolved`; it never emits a platform-dependent
“best effort” as canonical truth.

### 5.3 Stable canonical entities

Entity identity distinguishes a cross-world **candidate correspondence key**
from one exact manifestation:

- a **lineage slot** hashes \(M\), kind, global feature-bank key, and slot, but
  not \(q\); it proposes a bounded correspondence candidate across nearby
  worlds, not an authoritative identity;
- a **lineage manifestation** hashes the slot plus \(q\) and canonical roster
  revision;
- a river edge hashes \(M,q,L_d,\text{from cell},\text{to cell}\);
- a basin or terrain feature hashes its canonical classifier cell/slot and
  \(q\); and
- an organism candidate hashes
  \(M,q,\text{cell},\text{lineage slot},\text{candidate index}\), plus a Model
  layer time key only if the subject is time-dependent.

Every classified entity carries a margin or interval showing how close its
identity is to a tie or topology change. A TransitionRecipe may use equal slot
keys as direct candidate matches and reports changed topology cells, but it
does not claim a general lineage/river topology correspondence solve. Exact
subjects in Impressions use the coordinate-specific manifestation id.

### 5.4 Query grades and failure results

The Model exposes three grades:

1. **Preview** may use hardware floats and reduced work for UI prediction. It
   cannot select \(q\), create a portable id, or back an Impression.
2. **Interactive** returns declared error bounds at a requested spatial
   tolerance for continuous/nonsemantic channels. Its return types exclude
   canonical topology and entity identity; those require Canonical queries.
3. **Canonical** fixes every arithmetic and algorithmic choice and is
   bit-identical on native and wasm. Only complete Canonical results define
   topology, entities, audited attributes, Egress commits, and Impression
   subjects.

A Preview/Interactive request whose channel mask includes canonical
topology/entity semantics returns `UnsupportedCapability` for those channels;
it cannot smuggle an approximate id through a general sample API.

Every bounded general Realization query takes an explicit accuracy grade,
`WorkCap`, and optional typed continuation. State validation is inherently
Canonical and takes the latter two through `WorkRequest`. A continuation binds
`ModelRoot(M)`, query
kind, complete original-input digest, accuracy/numeric profile, and prior
frontier; it has a manifest maximum encoded size and contains no cache pointer
or executor handle. Supplying it to a different query is `InvalidInput`. The pure
semantic result for one such invocation is one of:

- `Complete { value, bounds, provenance }`;
- `Continue { deterministic_continuation, bounds }`;
- `Partial { value, bounds, continuation }` for APIs that permit bounded
  partial data;
- `UnsupportedCapability`;
- `InvalidInput`, `InvalidState`, or `ArithmeticOverflow`; or
- `Unresolved { reason, bounds }` when a required result cannot be certified
  under the declared cap.

A host service may separately report `Queued`, `Running`, or
`Ready(SemanticResult)`. Those availability states depend on scheduling and
cache residency and are not Model results. Given the same semantic input,
declared cap, and continuation, a **Canonical** pure result is bit-identical.
Preview and Interactive implementations may differ within their declared
enclosures; neither can decide identity, topology, an Impression, or Egress.

Refinement is nested through exact four-child ancestry. Canonical layer levels
are fixed by the manifest; a rendering LOD may resample or restrict them but
cannot change a river, class, lineage, or permanent id.

### 5.5 Immutable queries and cache keys

The authoritative Realization is always

\[
W(M,q,x,\tau).
\]

Layer results are immutable pure functions keyed by:

\[
(\operatorname{ModelRoot}(M),\text{grid version},\sigma_\ell(q),
\text{canonical cell or edge},\operatorname{time\_key}_\ell(\tau),
\text{layer},\text{revision},\text{query grade/tolerance}).
\]

\(\sigma_\ell(q)\) is the ordered projection of exactly the intrinsic \(q\)
words certified to determine every decoded attribute consumed by layer
\(\ell\), combined with complete upstream provenance. If \(F\) or a coupling
makes all axes relevant, the projection is the full \(q\); a named-output
subset is never mistaken for independent stored words.
State-independent feature-bank tiles may therefore be reused across Egress.
Exact world and manifestation identities still fold full \(q\); projected
signatures are dependency/cache provenance, not alternate Model identities.
Query grade and tolerance likewise belong to result keys, never to a permanent
semantic entity id.

That key describes a completed immutable layer artifact. A cached `Partial`,
`Continue`, or `Unresolved` invocation additionally keys the exact
`WorkRequest` digest—cap plus continuation frontier—and the complete query
input. Changing a work cap can change where an invocation pauses, never the
bytes of a completed Canonical artifact.

Cache residency is never authority. Eviction changes latency only, stale jobs
cannot integrate into a newer snapshot, and recomputation returns the same
Canonical result. Resource tiers may change scheduling, resident bytes, and
presentation detail, never canonical fields, topology, entities, attributes,
or navigation.

---

## 6. Distance and neighborhoods in Possibility

### 6.1 Periodic perceptual embedding

Let \(e(a)\) contain the sine/cosine pair for every cyclic attribute and a
versioned perceptual transform for every bounded attribute. With

\[
B(u)=\frac{\partial F}{\partial u}\in\mathbb R^{A\times k},
\qquad
J_u(u)=\frac{\partial(e\circ F)}{\partial u}
      =J_e(F(u))B(u),
\]

the metric used by navigation is the pullback to the \(k\)-dimensional
intrinsic space:

\[
g_u(u)=J_u(u)^\top S(F(u))J_u(u)+C_u(u)+\varepsilon I_k.
\]

\(S\) weights directly audited attribute changes. \(C_u\) is a positive
semidefinite coupling penalty in intrinsic coordinates, derived from a smooth
manifest navigation surrogate for feasibility and the AttributeLocks: it makes directions expensive
when a small named change causes large necessary collateral movement. The UI
can inspect \(B\), \(S\), and \(C_u\) as the explanation of that coupling.
The pullback is essential: named outcomes may have \(A>k\), but Egress always
produces exactly \(k\) intrinsic components for the \(k\)-word lattice.

Navigation does not differentiate the hard finite census. Core request losses
are functions of the public coordinate \(a\) itself; an adapter over fixed CDF
knots has a manifest piecewise-smooth interpolation and certified audit
interval. Integer order statistics, largest-remainder allocation, class ties,
and topology are commit checks, not fictitiously smooth observables. On a
surrogate knot or active-set boundary, the Canonical kernel uses the declared
one-sided derivative/subgradient and reported margin; if the audit interval
cannot certify the proposed one-sided improvement, no commit occurs.

The theoretical geodesic distance is

\[
d_{\mathcal U}(u_0,u_1)=
\inf_\gamma\int_0^1
\sqrt{\dot\gamma(t)^\top g_u(\gamma(t))\dot\gamma(t)}\,dt,
\]

over feasible paths in \(\mathcal U\). Cyclic residuals always take the
shortest wrapped difference, with the manifest's deterministic tie direction
at exactly half a turn.

The manifest certifies in fixed point a global lower eigenvalue bound
\(g_u(u)\succeq\underline\lambda I_k\), with
\(\underline\lambda>0\), over all feasible cells. Let

\[
r_{\mathrm{wrap}}(u_0,u_1)^2=
\sum_{j\in\mathrm{cyclic}}
\min(|\Delta u_j|,1-|\Delta u_j|)^2+
\sum_{j\in\mathrm{bounded}}|\Delta u_j|^2.
\]

Then
\(\sqrt{\underline\lambda}\,r_{\mathrm{wrap}}\) is a valid lower
bound even when feasibility obstacles force a detour. An explicitly sampled
feasible path, with certified quadrature and metric-enclosure error, supplies
an upper bound. The fixed-step Canonical oracle refines these bounds using
manifest cellwise eigenvalue/Lipschitz enclosures; if it cannot produce a
feasible upper path or certify a requested comparison, it continues or returns
`Unresolved`. A result may prune an Attractor search or certify a destination
only when the whole interval gives the same decision. The lower-bound
certificate, not a sampled path alone, is therefore part of Model validation.

This remains Option 2's compact geometry: distance is computed from the public
attribute state and its explicit couplings, not from a bank of opaque latent
probes, a Fisher law, or causal-program transport.

### 6.2 Canonical numeric kernel

Any operation that can select the next permanent state uses the Canonical
kernel:

- attributes, Jacobians, metric entries, weights, and utilities use declared
  signed Q formats;
- reductions use checked `i128` accumulators in manifest axis/id order;
- the symmetric system uses a fixed-pivot fixed-point \(LDL^\top\)
  factorization with declared diagonal regularization;
- normalization uses a frozen integer square-root/reciprocal routine;
- periodic and kernel functions use versioned tables or polynomials;
- every multiply/shift rounds ties-to-even at a named point;
- FMA contraction, subnormal behavior, host transcendentals, and
  platform-dependent reductions are forbidden in Canonical decisions; and
- overflow, loss of positive definiteness, or an ambiguous lattice enclosure
  produces no commit and a typed result.

A float implementation may preview the same direction. It may not be rounded
into a canonical coordinate without Canonical confirmation.

---

## 7. Yearnings and exact reconciliation

### 7.1 Attribute requests

An active Yearning expands each selected Impression attribute into a
content-id-keyed request. The Impression selects a fixed manifest predicate/CDF
knot or a declared bounded interpolation adapter; it cannot invent a new core
axis. Scope supplies an absolute destination prevalence \(s\in[0,1]\). Let
\(p_r(a)\) be that direct core value or certified adapter value.

The manifest maps each named Scope level (including singular, common, and
pervasive) to one exact fixed-point \(s\), with strictly monotone values and a
declared endpoint policy. A UI may expose finer values only on that same
fixed-point scale; Scope is never inferred from screen radius or local falloff.

For a prevalence request:

\[
\begin{aligned}
L_{\mathrm{accentuate}}(a;s)
  &= [s-p_r(a)]_+^2,\\
L_{\mathrm{repress}}(a;s)
  &= [p_r(a)-(1-s)]_+^2,\\
L_{\mathrm{hold}}(a;h_r)
  &= \operatorname{dist}_r(p_r(a),h_r)^2.
\end{aligned}
\]

Thus stronger Repress Scope means a smaller allowed captured-trait prevalence
and a larger required complement prevalence.
Accentuate or Repress contributes zero once its one-sided inequality is already
satisfied; the Canonical derivative at equality is zero, so it does not ratchet
beyond the request. \(\operatorname{dist}_r\) is signed scalar difference,
simplex distance, or shortest wrapped circular distance as declared.

**Hold captures \(h_a\) once, when the Yearning is activated.** The quantized
activation value is stored in the active Yearning and does not retarget to the
moving current state on later reconciliations. Disable emits no request.

Scope remains prevalence. For an environmental trait it is area/frequency over
applicable spherical cells; for an organism trait it is prevalence over the
canonical applicable roster. A truly global scalar such as axial tilt does not
pretend that Scope is a “magnitude band”: it is ineligible for
Accentuate/Repress unless the schema exposes a distributional observable, such
as the prevalence of seasonal forcing above a captured threshold. Hold may
still snapshot the scalar itself. For such a scalar, Accentuate and Repress
return `UnsupportedCapability`, Hold uses the activation-time scalar distance,
and Disable emits nothing; there is no omitted scalar Scope formula.

### 7.2 Exact order-independent reduction

Weights are unsigned fixed point. Scalar targets/differences use signed Q
formats, cyclic values use unsigned phases plus wrapped signed residuals, and
simplex values use their intrinsic allocation format. Requests are keyed by

\[
(\text{Yearning id},\text{Impression id},\text{attribute id},
\text{Influence}),
\]

and an identical id is deduplicated before evaluation. Distinct requests may
carry identical values and correctly contribute their separate weights.
Checked `i128` coefficients, loss values, and gradient contributions are
summed exactly. A division required by a specific operation—for example weight
normalization—occurs once at that operation's named rounding point with
ties-to-even. Therefore the same request multiset has the same objective
regardless of input order or batching. Mathematical associativity of
floating-point addition is not assumed.

The reconciled objective is composed back into intrinsic space:

\[
L_{\mathcal U}(u)=
\sum_r \widehat w_r L_r(F(u))
-\widehat\gamma\,\mathcal K(u),
\]

where \(\mathcal K\) is the multimodal Attractor potential from Section 10.
Weights express compromise, never processing priority. Feasibility remains a
hard constraint.

---

## 8. Canonical Egress and Resonance

### 8.1 Constrained natural-gradient direction

At the current state, the unconstrained natural-gradient direction is typed in
intrinsic coordinates:

\[
d_{u,0}=-g_u(u)^{-1}\nabla_u L_{\mathcal U}(u).
\]

The actual direction is the solution of the small active-set tangent problem

\[
\begin{aligned}
\min_{d_u}\quad&
\tfrac12d_u^\top g_u(u)d_u+
\nabla_u L_{\mathcal U}(u)^\top d_u,\\
\text{subject to}\quad&
\nabla_u \bar c_i(u)^\top d_u\le0,
\qquad \bar c_i=c_i\circ F,
\quad\text{for active feasibility constraints},\\
&(d_u)_j\text{ does not point out of an active intrinsic bounded face}.
\end{aligned}
\]

Exact equality constraints are already eliminated by the intrinsic coordinate
parameterization in Section 3.1. They are not independently rounded after this
solve.

The active set is ordered by constraint id. The Canonical solver factors the
SPD metric \(g_u\), forms the active-constraint Schur complement
\(A g_u^{-1}A^\top\), ranks it with fixed id/tolerance rules, and drops
constraints with invalid multipliers by the declared tie rule. It does not
apply an SPD factorization directly to an indefinite KKT saddle matrix. A fixed
active-set cap and fixed-point \(LDL^\top\) factorizations make the result
portable. Section 8.2 performs the joint integer line search; the
`CanonicalRetraction` is not used as a substitute for that solve.

Option 2 intentionally returns one compromise direction. It does not claim to
enumerate structurally different causal modes.

### 8.2 Movement epochs and atomic commits

Rendered frames do not submit arbitrary distance deltas. A canonical movement
reducer samples normalized World Space input at fixed gameplay ticks, produces
an ordered fixed-point polyline, and updates one monotonic integer
`MovementOdometer`. It retains high-precision distance remainder before
centimetre publication; render-frame subdivision cannot insert movement
segments. Each logical segment uses the full metric from Section 4.1 and the
radius of the \(q\) active at that segment's start.

Navigation opens an immutable epoch only when the odometer crosses the manifest
quantum \(s_q\). Let

\[
\ell=\min(\beta\rho s_q,\Delta_{\max})
\]

be the metric trust-radius-capped intrinsic step. If \(d_u=0\), \(\rho=0\), or
\(\ell=0\), the defined result is a no-op and no normalization occurs.
Otherwise, with \(Q_j=2^{32}\) for a cyclic axis and
\(Q_j=2^{32}-1\) for a bounded axis, the tentative accumulator delta is

\[
\Delta n_j=
\operatorname{round}_{\mathrm{even}}
\left(
2^{32}Q_j\,\ell\,\frac{(d_u)_j}{\|d_u\|_{g_u}}
\right).
\]

This converts normalized intrinsic displacement, measured by the pullback
attribute metric, into the accumulator's word-subunit scale. The transaction
then:

1. freezes \(q,n\), the intent digest, movement epoch, requests, and all
   dependency roots;
2. forms \(n_b=n+\operatorname{round}_{\mathrm{even}}
   (\Delta n/2^b)\) for \(b=0,1,\ldots,b_{\max}\);
3. jointly rounds \(n_b\) to the nearest \(h_j\)-lattice candidate using the
   manifest cyclic/bounded tie rules;
4. filters candidates by the intrinsic equality
   parameterization, all integer inequalities, the vector trust radius, and
   the complete required AttributeLock/audit intervals;
5. accepts the first q-changing filtered candidate \(q_b\) only when the
   outward-rounded objective enclosures certify
   \[
   \overline L_{\mathcal U}(u(q_b))
   \le \underline L_{\mathcal U}(u(q))
      -\eta_E\|u(q_b)-u(q)\|_{g_u(u(q))}^2,
   \]
   where \(\eta_E>0\) is fixed by the manifest; a same-q filtered candidate can
   update only the session residual and bypasses no future q-change test; and
6. atomically publishes the new \(q\) and the bounded error-feedback residual
   \(r=n_b-q\,2^{32}\), represented operationally as the absolute accumulator
   \(n'=q\,2^{32}+r=n_b\). Cyclic residual subtraction uses the shortest
   modular representative.

The sufficient-decrease displacement uses the same shortest wrapped cyclic
representative and half-turn tie rule as the metric.

The whole delta is shortened together; axes are never repaired independently.
The retained residual is bounded to the rounding cell around \(q\), so it
cannot bank an unbounded future leap. If no lattice word changes, the bounded
residual alone may commit to the session. If feasibility, overflow, or a final
audit fails, \(q\) and the subcell residual do not change. The session's Egress
odometer cursor still advances atomically to mark that movement epoch consumed.
An `Unresolved` epoch therefore retains no possibility credit and cannot be
resubmitted accidentally.

The finite candidate set for this epoch is exactly the jointly rounded
backtracking ray above. If its q-changing candidates are certifiably valid but
none meets sufficient decrease, the typed no-op is `NoAcceptedRay`; this is
explicitly not a proof that no off-ray lattice point improves the objective.
If any feasibility, audit, Attractor, or objective interval could change the
ray conclusion, the result is `Unresolved`, not a guessed commit. A same-q residual update is
`SubquantumProgress` only when the accumulated value has not yet reached any
distinct candidate. Once a distinct candidate is examined and rejected, a
no-accepted/unresolved result leaves the prior residual unchanged rather than
banking rejected credit.

Canonical Egress and its audit summary are a synchronous, fixed-work semantic
state machine at the movement boundary. Unlike general spatial queries, an
Egress transaction cannot return `Continue`: it returns a complete
commit/no-op or `Unresolved` within the manifest cap before the next canonical
movement epoch is accepted. Executor speed may delay the host from accepting
the next epoch but cannot queue a different future path. If the lattice
enclosure straddles a boundary, the same transaction retries at the manifest's
wider precision; persistent ambiguity is a no-commit result.

That Egress work cap is owned by and versioned with \(M\). A caller may budget
host time and thereby delay when the transaction runs, but cannot submit a
smaller semantic cap that changes commit/no-commit behavior or the reachable
path. Caller-selected `WorkCap`s apply only to resumable general queries.

While the Traveler-owned Transition Cooldown is active, movement advances its
odometer phase but does not advance \(n\). On completion, the Egress odometer
baseline moves to the current odometer; there is no accumulated navigation
backlog. Every **q-changing** \(q_0\to q_1\) commit creates this cooldown
whether or not any Visualization starts or retains a wake. A residual-only
session update with unchanged \(q\) does not. This pacing is a canonical
Traveler/gameplay rule, not a decision made from renderer state. Disabling the
visual blend therefore does not change the reachable path or Egress rate.

### 8.3 Request-specific Resonance

Resonance is

\[
\rho=\rho_{\mathrm{align}}\,
\frac{\sum_r\widehat w_r\rho_{\mathrm{support},r}}
     {\sum_r\widehat w_r}.
\]

An Attractor-only route contributes a versioned request/support term to the same
reduction. If there are neither active Yearning requests nor a selected
Attractor contribution, the denominator is zero by definition and
\(\rho=0\): travel alone does not invent an Egress direction.

Alignment reports how much of the requested descent survives the feasibility
tangent solve:

\[
\rho_{\mathrm{align}}=
\operatorname{clamp}
\left(
\frac{\|d_u\|_{g_u}}{\|d_{u,0}\|_{g_u}+\delta},0,1
\right).
\]

Support is request-specific:

- ecological and morphological requests use applicable productivity, habitat,
  or lineage support;
- hydrological requests use water/terrain applicability;
- terrain, planetary, and aesthetic requests use their own declared physical
  support and are not automatically throttled by local organism density; and
- a request that can introduce a currently absent trait has a versioned
  emergence floor. A true zero is reserved for a certified local
  impossibility.

Support is an exact reduction over a canonical equal-area spherical cap around
\(x_\star\), selected by a fixed-point three-dimensional dot threshold. Empty
denominators and applicability rules are part of the attribute manifest.
Resonance samples \(W(q_\star,\cdot,\tau)\), never resident presentation tiles
or Transition Wake history.

### 8.4 Reachability and failure

Integrating accepted epochs yields a path through validated lattice states.
Preview errors, scheduler delays, cache capacity, rendering LOD, and
Visualization choice cannot change that path. A direction may be slow,
unsupported, or unresolved; the Model reports which condition occurred rather
than fabricating a destination.

---

## 9. Transition Wake — bounded presentation history

### 9.1 Canonical endpoint invariant

There is no per-tile effective coordinate \(\Xi\). Canonical content before and
after a commit is exactly

\[
W_0(x)=W(q_0,x,\tau),
\qquad
W_1(x)=W(q_1,x,\tau).
\]

Both endpoints are reevaluated at the current Model time as \(\tau\) advances;
the recipe does not freeze weather/forcing at commit time. Its two
`EndpointStateRoot`s identify only the time-independent
\((\operatorname{ModelRoot}(M),q)\) dependency
closures. Every current endpoint sample has a separate ordinary dependency key
containing the exact layer time key. Any optional time-dependent event page
carries the exact time key it describes.

Every datum in the base recipe is either time-invariant or certified over the
entire finite forcing cycle declared by \(M\): continuous/contact mismatch
bounds, classifier/topology margins, and fixed-slot candidate rules included.
A narrower time-local value is allowed only in a separately keyed optional page
and cannot weaken or replace the base enclosure. If any required base datum
lacks a cycle-wide certificate, it moves to an optional exact-time page; if the
remaining base cannot support a safe blend, the wake is unavailable and the
defined presentation is immediate \(W_1\). Standing still therefore cannot
advance \(\tau\) beyond a recipe's validity.

The commit changes the Traveler's canonical state immediately to \(q_1\).
Inspection, capture, entities, Resonance, Impressions, and all Model queries
read \(W_1\). A Visualization may derive a temporary conservative displayed
surface from the two endpoint meshes, but canonical
Traveler collision, ground contact, path length, and movement always use
\(W_1\). A compatible Visualization may offset the camera, feet, and temporary
mesh so that the old-looking surface meets that canonical contact; it may not
move the Traveler or feed a different arclength to Egress. The recipe bounds
the required height/normal correction. If that bound exceeds the manifest
visual tolerance, the wake is not started and \(W_1\) is shown immediately.
Thus renderer choice cannot change a later world path, Model observation, or
identity.

Normalized movement input is interpreted in the canonical \(W_1\) Traveler
orientation and tangent/control basis. A display-only camera or foot offset is
downstream of that reducer and can never rotate, scale, or otherwise reinterpret
movement input.

### 9.2 Far-first spherical transition

On \(q_0\to q_1\), a fixed-work base `TransitionRecipe` contains:

- both time-independent endpoint-state roots;
- the commit World Space point \(x_i\) and integer travel odometer \(s_i\);
- per-channel continuous-change bounds;
- cycle-certified fixed-slot candidate-match rules and classifier/topology
  margin bounds that remain in the base; and
- manifest near/far radii, maximum delay, duration, and blend curve.

Changed classification/topology cells and exceptional candidate matches are
separate deterministic `TransitionEventPage` queries. The manifest caps each
page by count and encoded bytes and orders it by
\((\text{layer},\text{cell},\text{event kind},\text{id})\); larger horizons use
a continuation. A commit never waits for an optional page. If the base recipe
is not `Complete` at commit, the defined presentation is immediate \(W_1\)
with `TransitionMetadataUnavailable`.

For a sample \(x\), let \(d=d_{S^2}(x,x_i;q_1)\). The delay is largest near the
commit point and zero in the far field:

\[
\delta(d)=
\delta_{\max}
\left[
1-\operatorname{smootherstep}
\left(\frac{d-r_n}{r_f-r_n}\right)
\right].
\]

With \(\Delta s\) the integer arclength traveled since the commit,

\[
\lambda(x,\Delta s)=
\operatorname{smootherstep}
\left(
\frac{\Delta s-\delta(d)}{D}
\right),
\]

where \(\operatorname{smootherstep}\) clamps its argument to \([0,1]\).
Far content therefore begins moving to the new endpoint first; content near the
commit point retains the old presentation longer. Standing still freezes
\(\Delta s\), and each fixed point's \(\lambda\) is monotone. The scalar is
evaluated at shared canonical vertices/edges, so neighboring tiles cannot
choose incompatible transition coordinates.

Continuous presentation channels may interpolate endpoint values within the
recipe's bounds. Discrete rivers, coast classifications, species, and food-web
edges are never treated as fractional canonical physics: the Visualization
stages old/new endpoint representations using slot candidates, available event
pages, and margins, or falls back to \(W_1\) where those data are unavailable.
Canonical picking still reports \(q_1\). This is fixed-slot candidate staging,
not a general lineage or topology correspondence solver.

### 9.3 Bounded composition, eviction, and revisit

After a q-changing commit, the canonical Traveler's Transition Cooldown
remains active until

\[
\Delta s\ge\delta_{\max}+D.
\]

Travel during this interval advances the cooldown and, when present, the
recipe, but, as Section 8.2 specifies, does not accumulate another Egress step.
The cooldown exists even when recipe metadata is unavailable or a Visualization
chooses immediate \(W_1\). Thus one possible transition interval completes
before the next canonical commit, with no rate/backlog or
Visualization-dependence assumption. At most one recipe and two endpoint worlds
are active. There is no per-tile coordinate ODE, multidimensional bucket
history, or explored-area growth.

An exact destination load or Model migration may interrupt a wake. Its defined
rebase is to discard the old presentation recipe, install the requested
canonical \(q\), reset the Navigation Accumulator to that lattice point, clear
the Transition Cooldown, and report `TransitionInterrupted`; it never guesses
a mixed canonical state.

The active recipe is stored in the bounded Visualization session snapshot.
Endpoint tiles are pure cache entries and can be regenerated after eviction.
If presentation history is unavailable, the deterministic fallback is to show
\(W(q_1)\) immediately and report `HistoryUnavailable`. Revisit after a
completed transition likewise shows the current canonical world. When
\(\lambda\) completes, \(q_0\) and its event pages are discarded even if some
\(q_1\) presentation tiles missed their budget; a status/placeholder may be
shown, but old state is never retained into a third endpoint. These choices may
change presentation history, but never Model meaning or a shared address.

This preserves the proposal's distinctive “wake” experience while removing the
old path-dependent Realization and cache-authority contradiction.

---

## 10. Impressions, Attractors, and dual-space travel

### 10.1 Impressions and Builds

A canonical Impression contains:

\[
I=\big(
M,\ q,\ x,\ \tau,\
\text{canonical subject id},\
\{(\text{attribute id},\widehat\phi,\widehat\varepsilon)\}
\big).
\]

Here \(x\) is the equal-area spherical point plus centimetre altitude. Model
time is always stored, even when the selected subject and attributes are
time-independent, so opening the record has one exact \(W(M,q,x,\tau)\). A UI
may mark time irrelevant for a static observation, but serialization never
replaces it with “current time.” The subject id is a coordinate-specific
manifestation; a lineage-slot candidate key may accompany it as correspondence
metadata.

An Impression never contains a Transition Wake or Navigation Accumulator.
Opening it reproduces \(W(M,q,x,\tau)\) immediately and initializes sub-quantum
navigation at exactly \(q\). A compatible Visualization may then choose its own
entry presentation.

A Build is Visualization data anchored to the same
\((M,q,x,\tau)\) address and a versioned build-local frame whose tangent basis
is derived from the canonical three-dimensional direction by choosing the
least-aligned Cartesian reference axis, with axis-id ties broken by the
manifest, then applying fixed-point Gram–Schmidt. This remains defined at
polar points. Loading or removing the Impression controls presentation of the
Build. It does not alter \(q\), \(W\), or any Model entity.

### 10.2 Multimodal KDE Attractors

Published Impression evidence forms separate clusters on
\(\widehat{\mathcal U}\). The potential is

\[
\mathcal K(u)=
\sum_{c\in\mathcal C_{\mathrm{near}}}
\widehat w_c\,
k\!\left(
\frac{d_{\mathcal U}(u,m_c)}
     {H_c}
\right),
\]

with a compact-support versioned polynomial kernel \(k\). The representative
\(m_c\) is the evidence coordinate minimizing the certified sum of within-
cluster distance intervals; unresolved ties continue at higher precision, then
break by evidence id only when the intervals certify equal cost. It is a
feasible torus-aware medoid, not an undefined arithmetic mean. Clusters are not
summed into one destination vector, so distinct destinations remain multimodal.

Canonical Egress does not assume that geodesic distance has one analytic
gradient at a cut locus, competing shortest paths, or a feasibility boundary.
In an interior stratum, each intrinsic axis has a fixed probe quantum
\(\delta_j\) and outward-rounded intervals for
\(\mathcal K(u+\delta_j e_j)\) and
\(\mathcal K(u-\delta_j e_j)\); cyclic axes wrap. At active coupled constraints,
the same fixed-pivot active-set factorization constructs an id-ordered tangent
basis, probes certified feasible rays in that basis, and transforms the
resulting covector back; a bounded face uses its feasible one-sided stencil.
These exact id-ordered finite differences define the Model's Attractor steering
derivative. If a required probe is not certifiably feasible, its interval does
not quantize to one canonical component, or alternative components could
change the KKT active set or lattice decision, Egress is `Unresolved`.
Candidate acceptance still tests the true potential interval through Section
8.2's sufficient-decrease rule. Thus no arbitrary geodesic witness or
cache-selected subgradient can choose a world.

An evidence snapshot stores content-id-keyed add records and removal
tombstones as two grow-only sets. Merge unions both sets; an id is active only
when added and not tombstoned, so removal wins commutatively, associatively,
and idempotently. Active records are independently rate-limited and reduced in
canonical id order. Tombstones are paged authority, not resident cache state.

Admission is exact-model only. An active evidence record must carry the current
`ModelRoot(M)` and the exact attribute, lattice, metric, and distance-oracle
schema identities before it can enter \(\widehat{\mathcal U}\) or a cluster.
A foreign record is `IncompatibleEvidence`, not reinterpreted coordinate bytes.
A versioned migration may emit a new audited Impression and new evidence id;
the unmigrated record remains outside the index.

Cluster membership is itself canonical rather than insertion-order heuristic.
For the immutable evidence set, form an id-ordered graph whose pair has an edge
exactly when \(d_{\mathcal U}\le H_{\mathrm{join}}\), then take connected
components ordered by their lowest evidence id. A certified upper bound at or below
the threshold proves an edge; a lower bound above it proves a non-edge. A pair
whose distance interval straddles the threshold remains an explicit unresolved
membership, and any canonical potential/gradient affected by it returns a
residual interval until deterministic refinement decides it. Removal recomputes
the component relation from the remaining immutable ids. Batching, arrival
order, cache state, and a guessed midpoint can never choose a cluster.

The certified bandwidth is the larger of the cluster's distance enclosure and
\(H_0/\operatorname{isqrt}(1+N_{\mathrm{independent}})\), evaluated in the
manifest's fixed-point units. More independent evidence can narrow uncertainty
but cannot hide observed dispersion or collapse distinct modes.

A deterministic metric cover tree stores the Section 6.1 certified
\(\sqrt{\underline\lambda}\,r_{\mathrm{wrap}}\) lower bounds for nearest
cluster lookup; raw coordinate buckets are not assumed to be geodesic-nearest
under nonuniform \(g_u\). A bounded search returns either the exact contributing
cluster set or a residual potential/gradient interval. Ambiguous residuals
cannot select a canonical Egress step.

The external evidence library may grow, but its canonical index is paged by a
content-derived root. Each query fixes maximum visited nodes, returned clusters,
encoded bytes, and a deterministic continuation; the runtime resident index has
a byte ceiling. Cache eviction cannot remove evidence from authority, and an
incomplete page contributes an interval rather than silently omitting a
cluster.

Increasing independent evidence may shrink a cluster's certified enclosure.
An exact destination is exposed only when the entire enclosure contains one
representable lattice point. “Bandwidth below one scalar quantum” alone is not
sufficient.

### 10.3 Dual-space arrival

Possibility distance \(d_{\mathcal U}\) and World Space distance \(d_W\) remain
independent. Coordinated arrival adjusts Traveler rates so that estimated
remaining times agree:

\[
\frac{d_{\mathcal U}(u(q),u(q_t))}{v_{\mathrm{Egress}}}
\approx
\frac{d_W(x,x_t;q)}{v_{\mathrm{Explore}}}.
\]

This uses the spherical-plus-altitude World Space metric and never merges the
two metrics. A diffuse Attractor biases a route; only a certified
single-lattice-point cluster supplies an exact Possibility destination.

---

## 11. Determinism, identity, and versioning

### 11.1 Portable contract

Canonical conformance has three parts:

1. **Address and identity.** Model states, spherical addresses, Model time,
   hashes, dependency keys, entity ids, and encoded observations are integer
   and bit-identical on every conforming target.
2. **Canonical Realization and navigation.** Attributes, Egress commits,
   fields, topology, entities, audits, and pure semantic query results specify
   all formats, operation order, traversal, iterations, tables, rounding, work
   caps, and overflow behavior. Native and wasm results are bit-identical.
3. **Bounded noncanonical work.** Preview and derived presentation may use
   hardware arithmetic within declared error/presentation rules. They cannot
   create permanent identity or choose a canonical state.

Permanent hashes are domain-separated ordered integer folds over
`ModelRoot(M)`, the grid version, full \(q\) words, canonical spatial/entity
key, semantic entity kind,
its manifest-fixed identity level/revision, and relevant exact layer time key.
Requested query grade/tolerance belongs only to a result/dependency key and
cannot create a second permanent identity. A permanent identity never depends
on a float bit pattern, task order, face-local UV, rendering LOD, or cache
residence.

### 11.2 Schedule and cache independence

Jobs are immutable and integrate only when their full expected dependency key
matches the active snapshot. Worker count, lane scheduling, budget scale,
cancellation, cache ceiling, resource tier, SIMD width, and rendered-frame
subdivision may affect host availability and latency only. They cannot change a
Canonical pure semantic result for the same input, work cap, and continuation.
Noncanonical grades must remain inside their bounds. Same-operation Canonical
SIMD must be differential-tested against the scalar Canonical kernel.

The neutral Model creates no threads, files, sockets, clocks, DOM objects, or
GPU resources. Hosts provide the `TaskExecutor`; the native implementation may
use the current three-lane `LaneExecutor`, while the browser may execute
inline. Canonical generation is CPU-side. GPU work is allowed only for derived
presentation unless a future version defines and proves a portable canonical
GPU contract.

### 11.3 Versions

\(M\) contains:

- Model family and major version;
- attribute schema and numeric-kernel version;
- spherical grid/address version;
- public world-family seed;
- layer revisions and dependency closure;
- canonical coefficient/table manifest; and
- Realization capability versions.

Any change that alters an existing canonical result for the same inputs needs a
distinguishable Model identity. A layer-local revision is valid only when its
dependency closure and Impression compatibility are machine-readable. Record
format versioning is separate from Model identity.

The shareable record contains only canonical integer data. The run-local session
additionally records the Navigation Accumulator, movement odometer, Transition
Cooldown, and optional active TransitionRecipe.

---

## 12. Execution bounds and evidence required

### 12.1 No unmeasured real-time claim

Compact attribute navigation is structurally small, but it is not the whole
cold path. A state commit can require global attribute locks, sea datum,
coarse climate, audit cells, endpoint tiles, and transition metadata. This
proposal therefore makes no microsecond claim and no thin-annulus regeneration
theorem.

The bounded work that must appear in one ledger is:

| Work | Structural bound |
|---|---|
| request reduction | number of active id-keyed requests times referenced attributes |
| metric/KKT solve | at most \(k\le48\), fixed active-set cap, dense fixed-point solve |
| Attractor search/derivative | paged certified cluster search plus at most \(2k\) fixed finite-difference probes |
| attribute audit/locks | manifest audit level \(N_A=12\cdot4^{L_A}\), fixed calibration iterations |
| sea and global closure | fixed equal-area level, fixed selection/sweep counts |
| coarse climate | 12 Model months, fixed graph level and flux iterations |
| local realization | requested cells, fixed halos, layer work caps and accuracy |
| transition | at most two endpoint dependency closures and one scalar recipe |
| Impression confirmation | complete Canonical audit, subject, topology, and attribute queries |

These are bounds on operation counts, not proof that the product budget is met.
Cold snapshots and sustained Egress must be measured on native and wasm. The
ledger reports latency together with `Complete`, `Continue`, and `Unresolved`
rates for a fixed representative corpus and each declared cap; a low latency
obtained by frequently failing to certify a result is not a passing outcome.
Selection thresholds are fixed before the implementation trial rather than
inferred from its results.

### 12.2 Fixed memory and queues

All caches have explicit byte ceilings. The host uses count-budgeted priority
lanes for current-near Canonical work, current-visible work, and speculative/far
work. A state commit may invalidate broad composed output; the implementation
must report and measure that cost rather than claim only a thin annulus changes.

At most two endpoint snapshots participate in a normal wake. Per-layer
generation queues and stale-result tombstones have fixed count/byte caps.
State-independent feature bases may be shared, but state-dependent composed
tiles remain separately keyed. When work exceeds a presentation budget, the
Visualization may lag or show the canonical endpoint fallback with status.
Canonical content never changes to accommodate a budget.

Long straight travel, rapid travel, loops, turns, tangential travel around a
cap, revisits, cache eviction, and unfinished transition interruption must all
plateau in memory.

### 12.3 Required decision gates

An implementation of this proposal is not eligible for selection until it
demonstrates:

1. bit-identical Canonical addresses, Egress commits, audits, fields, topology,
   and entities on native and wasm, including adversarial rounding boundaries;
2. one cold/warm performance ledger covering navigation, planetary global
   work, visible endpoint generation, transition work, and Canonical Impression
   confirmation;
3. fixed memory under long, fast, turning, looping, and revisiting travel with
   simultaneous Yearnings and pervasive Scope;
4. schedule, cancellation, cache-capacity, resource-tier, SIMD, and
   frame-subdivision independence;
5. blind novice/intermediate/expert tests of Accentuate, complement Repress,
   activation-time Hold, Disable, conflict, absent traits, barren contexts, and
   Scope monotonicity;
6. a held-out quality corpus measuring diversity, repeated motifs,
   conservation/plausibility failures, ecological structure, and nearby-world
   correspondence; and
7. visible continuity tests across state quanta, patch seams, poles, coast and
   river topology, species appearance/disappearance, fixed-slot candidate
   changes, unresolved topology staging, cache eviction, exact destination
   loads, and interrupted wakes.

Additional sphere gates require reciprocal neighbors across every noncollapsed
patch edge, bounded direct access to the shared polar vertices and polar
control volumes without an unbounded incident-cell halo, exact polar
restriction/prolongation and drainage ownership, zero supernode audit mass,
unique edge/corner ownership, finite normalized cell masses summing exactly to one, bounded
geometric-area error/convergence, seam-equal field samples/gradients,
cross-patch drainage continuity, stable polar forcing, and exact parent/child refinement. Coordinate
gates cover zero, bounded maximum, cyclic wrap, half-turn ties, every \(h_j\)
boundary, state separation and constrained-face canonicalization, serialization
round trips, and accumulator overflow/saturation.

---

## 13. Rust-facing contract sketch

The neutral core uses integer public types:

```rust
/// Content identity of the complete canonical Model manifest.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ModelRoot(pub [u8; 32]);

/// Validated, exact address of one complete world.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct CanonicalAttributeState {
    pub words: [u32; K],
}

/// Traveler/session state. It cannot affect Model queries before a commit.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NavigationAccumulator {
    pub subcell: [i128; K],
    pub intent_digest: IntentDigest,
    pub egress_odometer_base_cm: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TransitionCooldown {
    pub started_odometer_cm: u64,
    pub completes_after_cm: u32,
}

/// Unique angular address. Exact poles have no longitude or chart alias.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AngularAddress {
    Cell {
        pixel_at_point_level: u64,
        sub_xi: u32,
        sub_eta: u32,
    },
    NorthPole,
    SouthPole,
}

/// Unique fixed-precision World Space point; query LOD is not encoded here.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct WorldPoint {
    pub grid_version: u16,
    pub angular: AngularAddress,
    pub altitude_cm: i32,
}

/// Variable-level cell address for fields/tiles, never an Impression point.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct EqualAreaCellId {
    pub level: u8,
    pub nested_pixel: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ModelTime(pub i64);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Accuracy {
    Preview,
    Interactive { tolerance_q: u32 },
    Canonical,
}

/// Work for one invocation and an optional exact resume frontier.
#[derive(Clone, Debug)]
pub struct WorkRequest {
    pub cap: WorkCap,
    pub continuation: Option<Continuation>,
}

/// Every non-Egress query carries both its grade and resumable work request.
#[derive(Clone, Debug)]
pub struct QueryRequest {
    pub accuracy: Accuracy,
    pub work: WorkRequest,
}

#[derive(Clone, Debug)]
pub enum SemanticResult<T> {
    Complete { value: T, bounds: Bounds, provenance: DepKey },
    Partial { value: T, bounds: Bounds, continuation: Continuation },
    Continue { bounds: Bounds, continuation: Continuation },
    UnsupportedCapability,
    InvalidInput,
    InvalidState { violations: ViolationSummary },
    ArithmeticOverflow,
    Unresolved { reason: UnresolvedReason, bounds: Bounds },
}

#[derive(Clone, Debug)]
pub struct Page<T> {
    // Length and bytes are capped; SemanticResult carries any continuation.
    pub items: Vec<T>,
}

/// A paged query still carries an explicit resumable work request.
#[derive(Clone, Debug)]
pub struct PageRequest {
    pub query: QueryRequest,
    pub max_items: u32,
    pub max_encoded_bytes: u32,
}

/// Authoritative Traveler/session result, including cooldown expiry.
#[derive(Clone, Debug)]
pub struct EgressSessionUpdate {
    pub next_nav: NavigationAccumulator,
    pub next_cooldown: Option<TransitionCooldown>,
}

/// Atomic q-changing result. Presentation recipe creation is separate.
#[derive(Clone, Debug)]
pub struct EgressTransaction {
    pub next_q: CanonicalAttributeState,
    pub next_nav: NavigationAccumulator,
    pub next_cooldown: TransitionCooldown,
    pub audit: AuditSummary,
    pub provenance: DepKey,
}

#[derive(Clone, Debug)]
pub enum EgressResult {
    Commit(EgressTransaction),
    NoOp { session: EgressSessionUpdate, reason: NoOpReason },
    Unresolved {
        session: EgressSessionUpdate,
        reason: UnresolvedReason,
        bounds: Bounds,
    },
}

pub trait Model {
    fn root(&self) -> ModelRoot;

    fn validate_state(
        &self,
        words: [u32; K],
        work: WorkRequest,
    ) -> SemanticResult<CanonicalAttributeState>;

    fn planet(
        &self,
        q: CanonicalAttributeState,
        request: QueryRequest,
    ) -> SemanticResult<PlanetDescriptor>;

    fn attributes(
        &self,
        q: CanonicalAttributeState,
        request: QueryRequest,
    ) -> SemanticResult<AuditedAttributes>;

    fn sample(
        &self,
        q: CanonicalAttributeState,
        point: WorldPoint,
        time: ModelTime,
        channels: ChannelMask,
        request: QueryRequest,
    ) -> SemanticResult<Sample>;

    fn cell(
        &self,
        q: CanonicalAttributeState,
        cell: EqualAreaCellId,
        time: ModelTime,
        channels: ChannelMask,
        request: QueryRequest,
    ) -> SemanticResult<Cell>;

    fn canonical_entities(
        &self,
        q: CanonicalAttributeState,
        bounds: SphericalBounds,
        time: ModelTime,
        kind: EntityKind,
        page: PageRequest,
    ) -> SemanticResult<Page<CanonicalEntity>>;

    fn transition_recipe(
        &self,
        from: CanonicalAttributeState,
        to: CanonicalAttributeState,
        origin: WorldPoint,
        commit_odometer_cm: u64,
        work: WorkRequest,
    ) -> SemanticResult<TransitionRecipe>;

    fn transition_events(
        &self,
        root: TransitionEventRoot,
        horizon: SphericalBounds,
        time: ModelTime,
        page: PageRequest,
    ) -> SemanticResult<Page<TopologyEvent>>;
}

/// Transactional: a Commit publishes q, navigation, and a newly created
/// cooldown together; NoOp/Unresolved publish the returned session update.
/// The fixed Egress cap comes from the Model manifest, never the caller.
pub fn egress_epoch(
    model: &impl Model,
    current: CanonicalAttributeState,
    nav: &NavigationAccumulator,
    active: &ActiveYearnings,
    attractors: &AttractorIndex,
    point: WorldPoint,
    time: ModelTime,
    movement: MovementEpoch,
    cooldown: Option<TransitionCooldown>,
) -> EgressResult;

/// Time-independent root of one (ModelRoot(M), q) closure; no time key.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct EndpointStateRoot(pub [u8; 32]);

/// Root of the optional paged correspondence/topology event set.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TransitionEventRoot(pub [u8; 32]);

/// Visualization input only; never an argument to Model::sample/cell/entities.
#[derive(Clone, Debug)]
pub struct TransitionRecipe {
    pub id: TransitionRecipeId,
    pub from: CanonicalAttributeState,
    pub to: CanonicalAttributeState,
    pub origin: WorldPoint,
    pub commit_odometer_cm: u64,
    pub endpoint_state_roots: [EndpointStateRoot; 2],
    pub event_root: Option<TransitionEventRoot>,
    pub bounds: TransitionBounds,
}
```

No resident tile stores a possibility coordinate. Model caches key immutable
endpoint results; the Visualization may cache endpoint presentation and the one
bounded `TransitionRecipe`.

---

## 14. Conceptual invariant conformance

| # | Invariant | Option 2 contract |
|---|---|---|
| 1 | One point in Possibility is one complete world | one validated \(q\) determines the whole spherical \(W(q,\cdot,\tau)\) |
| 2 | One canonical point; nearby content keeps history | \(q_\star\) is singular; the bounded two-endpoint wake is explicit Visualization history |
| 3 | Possibility and World Space have independent metrics | \(d_{\mathcal U}\) is the pullback attribute geodesic; \(d_W\) is spherical |
| 4 | Egress and Exploration are distinct | \(q_\star\) and \(x_\star\) are separate states and flows |
| 5 | Travel couples but does not redefine Egress | the Traveler supplies fixed arclength quanta to the Model-owned direction |
| 6 | Realization has stable meaning | audited attributes, immutable queries, versions, bounds, and canonical entities |
| 7 | Simulation belongs to Visualization | Model time supplies forcing; weather/behavior/presentation simulation is external |
| 8 | Identical Model inputs reproduce Realization | Canonical \(W(M,q,x,\tau)\) and statuses are native/wasm bit-identical |
| 9 | Impressions remain meaningful | integer Model state, sphere point, time, subject, and audited values require no wake |
| 10 | Yearnings are weighted and order-independent | id-keyed fixed-point requests use checked exact reduction and one constrained objective |
| 11 | Scope is destination prevalence | every Scope value has an equal-area/applicable-population denominator |
| 12 | Visualization does not change reachable Possibility | Egress and Resonance read canonical \(W\), never wake or GPU state |
| 13 | Builds are optional presentation | spherical Build anchors attach to Impressions and never enter \(q\) or \(W\) |
| 14 | Attractor evidence is historical/removable | id-union evidence, rate limits, removable clusters, certified enclosures |

---

## 15. Relationship to the current implementation and other options

### 15.1 Engineering ideas retained

This clean-slate Model can reuse proven engineering patterns without reusing the
current per-region state:

- domain-separated integer hashing and versioned identities;
- declared layer dependencies and complete dependency-hash provenance;
- integer macro-drainage topology with stable tie-breaking;
- immutable jobs, stale-result rejection, cancellation, and byte-bounded
  caches;
- exact id-keyed reductions for portable steering;
- fixed candidate lineage slots plus near-field organism manifestations;
- same-operation scalar/SIMD differential tests;
- the neutral `TaskExecutor` boundary, native `LaneExecutor`, and browser
  inline execution; and
- CPU-authoritative canonical generation with GPU-derived presentation only.

The layer order cited here is Terrain, Geology, Macro Drainage, Climate,
Hydrology, Soils, Biome, Vegetation, and Ecology. This proposal does not repeat
the obsolete climate-before-geology shorthand or describe the current executor
as work-stealing.

### 15.2 Deliberate replacements

- per-region possibility vectors become one global Canonical Attribute State;
- planar integer regions become a finite equal-area spherical hierarchy;
- an allegedly invertible torus chart becomes a periodic public attribute
  manifold with no global inverse claim;
- a purported metric-nearest triangular clamp becomes tangent-constrained
  Egress plus a non-nearest canonical retraction;
- nominal prevalence parameters become audited spherical/population
  statistics;
- floating summation and navigation become fixed-point canonical decisions;
- per-tile \(\Xi\) and its ODE become a two-endpoint Visualization recipe; and
- speculative microsecond and thin-annulus claims become explicit benchmark,
  memory, and correctness gates.

### 15.3 What remains unique

The proposal retains five connected ideas:

1. a torus-times-box, fixed public intrinsic parameterization of named
   attributes rather than an opaque latent vector;
2. AttributeLocks that make the named core outcomes directly meaningful as
   realized planetary measurements;
3. one compact constrained least-squares/natural-gradient Egress direction;
4. historical multimodal KDE Attractors in that same attribute geometry; and
5. a prescribed far-first spherical presentation wake with exactly two
   canonical endpoints.

Its spherical domain is shared product scope, not shared ontology. It adopts
neither Option 1's cubed oblate latent planet, Option 3's statistical
world-law/Fisher/WFR construction, nor Option 4's typed programs, transport
navigation, structural rewrites, and certificate compiler.

### 15.4 Known limitations

- The core schema is fixed; new causal regimes and independent controls require
  a Model-major redesign.
- Direct locks can reduce variety or expose correlations when several requested
  outcomes compete for the same physical budget.
- A state-independent feature bank may produce a recognizable house style.
- Bounded coupled closure and audit tolerances establish internal synthetic
  coherence, not scientific truth or fun.
- A canonical commit may cause broad endpoint generation despite basis reuse.
- Fixed-point KKT, spherical global closure, and sustained endpoint turnover
  remain performance risks until measured.
- One compromise direction can average desires that players perceive as
  qualitatively different modes.
- `NoAcceptedRay` may stall even when an unsearched off-ray lattice move would
  improve the objective; this option deliberately does not add multimode route
  search.

These are the intended research questions for Option 2. They are not papered
over by the coordinate, wake, or real-time claims.
