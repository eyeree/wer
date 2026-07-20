# New World Model Option 3: Possibility as a statistical manifold of world-laws

## Status and purpose

This document proposes the third complete Model for the concepts in
[`conceptual-model.md`](conceptual-model.md), a sibling to
[`new-world-model-option-1.md`](new-world-model-option-1.md) (a latent cube
decoded into a procedural planet) and
[`new-world-model-option-2.md`](new-world-model-option-2.md) (a plane with a
travel-gated relaxation "wake"), and distinct from
[`new-world-model-option-4.md`](new-world-model-option-4.md) (the World Loom's
typed causal constitution). It is a design, not a description of landed
behavior; the current prototype is documented in
[`world-model.md`](world-model.md), and references to it are comparisons, not
compatibility requirements.

The name **V3** identifies this proposed contract, not the current value of
`WORLD_ALGORITHM_VERSION`.

Options 1 and 2 converge on a common shape: a compact latent coordinate, a
frozen decoder into generator parameters, an analytic *pullback* metric built by
weighting a Jacobian, and a travel-gated gradient Egress. V3 retains a compact
fixed-dimensional coordinate and travel-gated motion, but it is not another
latent decoder: the coordinate is the natural parameter of the world-law and
the geometry follows from that law.

The World Loom is V3's closest conceptual neighbor because both use statistical
measures and optimal transport. They nevertheless put those ideas on opposite
sides of the Model boundary. The Loom stores a variable-sized typed causal
program and uses multiscale transport, rewrites, directed control paths, and
bounded mode search to choose and explain a destination. V3 stores an
approximately 40-dimensional fixed-point natural-parameter vector and uses the
Fisher Hessian of one frozen statistical family to navigate. In V3, optimal
transport is confined to transient Realization continuity (§10); it never
chooses a canonical destination. V3 therefore keeps its fixed-vocabulary ceiling
in exchange for compact addresses, a unique convex target within a selected
statistical mode, and a small identity-checkable navigation kernel. Section 17
states the trade precisely without claiming that either proposal can gain new
canonical physics without versioning.

**The V3 thesis, in one paragraph.** A world is not a bundle of parameters; it is
a *probability law over canonical whole-planet summaries of what a Traveler can
observe*. Points of Possibility are
therefore members of an **exponential family of world-laws**, and one convex
**free-energy function** $A(\theta)$ generates the navigation subsystem: its
Hessian *is* the Fisher information metric on Possibility (the unique metric
invariant under sufficient statistics, up to scale, by Chentsov's theorem, once
the observables are fixed — computed as an exact algebraic identity, never a
differentiated probe summary); its gradient *is* the expectation-coordinate
vector, including the prevalence coordinates that Scope acts on; and it turns
Yearning reconciliation into a convex maximum-entropy
program with a unique minimizer. That law is exposed as one finite **spherical
planet**: World Space is $S^2$ with a nested twelve-patch equal-area address,
and every law transforms the same coordinate-independent innovation field rather
than reseeding the ground sample. Continuity is handled at a *second* geometric
level by **unbalanced optimal transport** (the Wasserstein–Fisher–Rao metric):
the world's *living and biome content are spatial mass distributions*, so when the
law changes they genuinely *transport and grow* across World Space — forests
spread, species ranges migrate, blooms and extinctions happen in place — rather
than only lagging a coordinate. These two geometries, information geometry for
*navigation* and optimal transport for *continuity*, are kept strictly separate;
they are the two ideas that make V3 its own proposal.

The design has five goals of its own, in addition to the four it shares with
Option 1 — restated here so this document stands alone: (i) deterministic,
portable, bit-reproducible world identity across native and wasm; (ii) lazy
realization at bounded per-sample and per-frame cost, independent of explored
area; (iii) a non-Euclidean Possibility metric under which a numerically small but
consequential move is metrically *far*; and (iv) weighted, validity-respecting
Yearning reconciliation that does not depend on input order. The five that are
distinctive to V3:

1. every representable coordinate is a *valid, well-posed probability law* by
   construction (validity is membership in a convex moment set, not a clamp);
2. Scope/prevalence is a *first-class coordinate* — the thing a Yearning asks for
   is literally a mean coordinate of the world-law, so it cannot be a spatial
   falloff even in principle;
3. Yearning reconciliation is a *strictly convex program with a unique minimizer*,
   within each fixed statistical regime; separated causal regimes remain explicit
   candidates rather than being silently averaged (§7.3);
4. the navigation algebra, *given the frozen parameter bank*, reduces to small
   algebraically specified operations that an AI maintainer can check against identities
   (metric $=\nabla^2A$, KL $=$ Bregman, idempotent projection) rather than
   against a screenshot; and
5. continuity is a physical *transport* of realized living content with fixed
   resident-memory and per-frame work caps.

Goal 4 is deliberately scoped: the algebra is identity-checkable, but the frozen
parameter bank that instantiates $A$ is a *fitted* object validated by
world-quality tests, not by an identity (§3.4, §13).

---

## 1. Overview of the construction

The Model is the tuple

$$
\mathfrak M=\big(M,\ \Omega,\ \boldsymbol\nu,\ A,\ \varphi,\ g,\ \mathcal U,\ \mathcal D,\ \mathcal W,\ \Pi_\mu\big),
$$

| Symbol | Name | Role | § |
|---|---|---|---|
| $M$ | Compatibility descriptor/root | family seed plus separable law, sphere, innovation, and layer identities | 2.2, 12 |
| $\Omega$ | World Space | a finite spherical planet plus signed altitude | 2.3 |
| $\boldsymbol\nu$ | Canonical observation measures | fix the area/time/population denominator of every statistic | 2.4, 4 |
| $A$ | Free energy (log-partition) | the single generating function of the navigation subsystem | 3 |
| $\varphi$ | Observables / mean map | $\varphi(\theta)=\nabla A(\theta)$: scalar means and prevalences Yearnings act on | 4 |
| $g$ | Information metric | $g(\theta)=\nabla^2A(\theta)$: the (Chentsov-unique) metric on Possibility | 5 |
| $\mathcal U$ | Common innovation | coordinate-independent spherical modes, candidates, and lineage slots | 6.2 |
| $\mathcal D$ | Realization decoder | maps law means/responsibilities to physical and field coefficients | 6 |
| $\mathcal W$ | Realization | the spherical fields, topology, and canonical entities exposed by the law | 6 |
| $\Pi_\mu$ | Typed feasibility projection | Bregman projection of **means** onto a safe interior moment set | 3.3 |

The running coordinate $\theta$ ranges over the manifold $\Theta$; it is the
**Model State** (§2.2), carried over the static structure $\mathfrak M$, not a
component of it.

Around this the **Traveler** carries a committed coordinate $\hat\theta_\star$
moved by **Egress** (§7–8), a spherical World Space position $x_\star$ moved by
**Exploration**, Canonical Model time when requested, and a derived presentation
transition that morphs the realized world (§10) — never an authoritative
coordinate.

The pipeline for one navigation tick, with the Model/Traveler boundary marked:

```text
Yearnings (Impressions + Influence + Scope + weights)  +  community Attractors
        │
        ▼  MODEL: canonical one-sided request groups + separate clusters/regimes
   ≤ K_nav combined candidates; one unique convex mean target μ⁺ each (§7)
        │  Traveler selects a mode or receives AmbiguousModes
        │
   MODEL: Canonical ρ = spherical ecological support × law alignment  (§9)
        │
        ▼  TRAVELER POLICY: Δs = β̂ · ρ̂ · accumulated spherical arclength
   MODEL: follow μ(α)=(1-α)μ⋆+αμ⁺; enclose dual and one Q24 cell       (§8)
        │  Complete commit or fail-closed Pending/Unresolved
        │
        ▼  VISUALIZATION rebases; MODEL supplies endpoint/correspondence data
   spherical unbalanced transport in the bounded streaming annulus   (§10)
     ecology/biome measures: TRANSPORT across World Space + GROW/FADE (WFR)
     abiotic fields: exact commuting/matrix Bures blocks + bounded spectral blend
        │
        ▼  Canonical, lazy, cached by committed address/cell/time/layer
   𝒲( M, θ̂, x, t ) → geology, terrain, water/drainage, climate, soils, ecology
```

The reconciliation and Resonance are Model math (information geometry, no
Visualization dependence, satisfying invariant 12). Mode selection and the
conversion $\Delta s=\hat\beta\hat\rho\Delta\ell_W$ are a versioned **Traveler
policy** (invariant 5), not Model generation identity. The transport is a
transient *presentation of the path already travelled*, so it never changes
$\theta_\star$ (invariant 2), and the Model supplies only its endpoints and
geodesic parameters — the Visualization owns the morph state (invariant 7).

Notation. $\langle\cdot,\cdot\rangle$ Euclidean inner product;
$\lVert v\rVert_g=\sqrt{v^\top g\,v}$; $\nabla A,\nabla^2A$ gradient and Hessian;
$\mathrm{KL}(p\Vert q)$ relative entropy; $B_\psi(a,b)=\psi(a)-\psi(b)-
\langle\nabla\psi(b),a-b\rangle$ Bregman divergence of a convex $\psi$;
$\cos_+\angle(a,b)=\max(\langle a,b\rangle/(\lVert a\rVert\lVert b\rVert),0)$;
integer/fixed-point quantities carry a hat, $\hat\theta$. Vectors are columns.

---

## 2. Possibility as a statistical manifold

### 2.1 A world is a law over whole-planet observation records

V3 does not put a terrain patch, lineage, and organism into one fictitious common
sampling measure. Instead, each attribute $a$ first defines a typed encounter
stratum $\mathcal O_a$, a fixed canonical measure/applicability rule $\nu_a$, and
a local membership/value $t_a$. Applying all of them to a complete accepted
planet produces one **whole-planet observation record**

$$
r(\mathcal W)=\big(R_1,\ldots,R_k\big),\qquad
R_a=\mathbb E_{o\sim\nu_a(\mathcal W)}[t_a(o)],
$$

Every coordinate admitted to $T$ has a manifest-proved positive lower bound on
its applicable denominator throughout $\mathcal P_{\mathrm{safe}}$, so every
component of $r(\mathcal W)$ is numeric. A useful query whose denominator can be
empty remains a **derived observable** and may return `NotApplicable`; it is not a
sufficient-statistic coordinate and is never encoded as numeric zero. A future
law may instead promote its numerator and applicability mass as two numeric
statistics, but that is a new schema rather than an implicit ratio convention.
Examples include mean relief, area-conditioned aridity, drainage density, canopy
fraction, lineage-weighted body scale, and candidate-weighted branching or hue.
The **mean-record space** is
$\overline{\mathcal R}=\mathbb R^{k_s}\times
\operatorname{conv}\{c_1,\ldots,c_R\}$; a realized representative record is
serialized in the schema's fixed-point encoding with its quantization bound.

The law itself uses a distinct idealized atom space
$\mathcal Z=\mathbb R^{k_s}\times\{1,\ldots,R\}$. An atom $z=(x,j)$ carries the
whole-planet summary $T(z)=(x,c_j)$. Thus the categorical index is part of the
statistical envelope, not a claim that the coupled representative's prevalence
record must equal one archetype vertex. The sufficient statistics are identity
or frozen transforms of these summary components, not functions pretending that
heterogeneous local subjects share one denominator.

A **world-law** is a probability law over $\mathcal Z$: the statistical family
of idealized whole-planet summary atoms that the coordinate regards as similar.
The deterministic planet exposed at one coordinate is coupled to the same
archetypes and moment-closed so that its record in
$\overline{\mathcal R}$ matches $\mathbb E_\theta[T]$ within the finite bounds of
§6.3. It is deliberately a mean-matched representative, not a draw from
$p_\theta$. A player walking a short route likewise sees local subjects from
$\mathcal O_a$, not an unbiased draw from $p_\theta$.

V3 restricts world-laws to a **minimal exponential family**

$$
p_\theta(dz)=\exp\!\big(\langle\theta,T(z)\rangle-A(\theta)\big)h(dz),
\qquad
A(\theta)=\log\!\int_{\mathcal Z}e^{\langle\theta,T(z)\rangle}h(dz),
$$

with $\theta\in\Theta\subseteq\mathbb R^{k}$ the **natural (canonical)
parameter** — the compact Possibility Coordinate — $h$ a fixed base probability measure, and
$A$ the **log-partition** (equivalently **free energy**), convex, and strictly
convex because the family is minimal (§3). Because the family is minimal, the
number of natural parameters equals the number of sufficient statistics: **$\dim
\theta=\dim T=k$**, and the mean map below is a map $\mathbb R^k\to\mathbb R^k$.
Richer *presented* attributes a Visualization might show are deterministic
functions of existing canonical channels or means, not additional sufficient
statistics.

The base measure is not left implicit. For the first coupled family, $j$ is a
joint archetype, $c_j$ its prevalence record, and $x$ its normalized
scalar-summary atom. The frozen measure is

$$
h(dx,dj)=\pi_j\,\mathcal N(dx;q+d_j,Q_0),
\qquad T(x,j)=(x,c_j).
$$

Here $\pi_j>0$ and $\sum_j\pi_j=1$.

Its moment-generating function is exactly the $A$ in §3.1. The Gaussian scalar
envelope is an explicit statistical idealization: not every atom in its support
must be the record of a physically realizable finite planet. Representable means
are restricted to the bounded, physically certified safe set, and the generator's
one realized record must meet those means (§3.3, §6.3). A held-out ensemble of
whole generated planets tests the mean bridge and whether this idealized law is
useful; neither those representatives nor a within-planet histogram are treated
as draws from $p_\theta$.

This is the Jaynes maximum-entropy construction relative to $h$: among record
distributions with prescribed sufficient-statistic averages, it adds no further
commitment, and $\theta$ are the Lagrange multipliers. Maximum-entropy ecology and
species-distribution models motivate the use of prevalence statistics, but V3's
law is specifically over **whole-planet records**; it does not borrow a theorem
that makes its separate spherical Realization automatically correct.

Crucially, the *law* is the shareable, navigable meaning of the world; the
*specific terrain a Traveler walks on* is one deterministic **coupled
representative** of that law. Its innovation ids do not hash $\theta$: nearby coordinates filter,
tilt, and threshold the same spherical modes, candidate sites, and lineage slots
(§6.2). The law is what is close between nearby worlds; the shared innovation is
what makes their samples correspond; nonlinear topology may still differ sharply
through emergence. This split —
law for meaning, representative for the ground underfoot — is what lets V3 satisfy both
halves of the conceptual model's continuity clause at once: *"nearby points in
Possibility produce related Realizations"* (nearby laws are $\mathrm{KL}$-close)
while *"emergent or chaotic behaviour may still create sharp local differences"*
(two coupled representatives of close laws can still differ locally).

### 2.2 The coordinate and the Model State

The **Model State** is exactly $(M,\hat\theta)$; $\theta$ denotes its mathematical
decode and everything else is derived. $M$ is a compatibility descriptor/root:
it names the law family, public $128$-bit world-family seed, canonical epoch, and
the separate law, spatial-profile, innovation, and per-layer-closure identities.
Records store the complete root so compatibility can be checked, but a canonical
hash consumes only the smallest identity closure on which its output depends.
In particular, common innovation consumes the family seed, spatial-profile id,
and innovation revision — never an unrelated layer revision or the root manifest
hash — while a layer dependency key consumes that layer and its declared upstream
revision closure. Updating one downstream layer therefore cannot reseed untouched
fields or entity slots. A capability
minor that adds only a derived query is negotiated separately and does not
silently change generated identity (§12). A
practical first instance uses $k\approx 40$ natural parameters (the navigation
algebra of §5, §8, and §13 is budgeted for $k\le48$), split into a
**prevalence block** $\theta_{p}$ (bounded traits — §3.2) and a **scalar block**
$\theta_{s}$ (unbounded magnitudes), grouped to mirror the prototype's eight
possibility domains but giving each domain a small sub-vector rather than one
scalar (Appendix A).

For determinism the coordinate is stored in fixed point,

$$
\hat\theta_j=\big\lfloor 2^{B}\,\theta_j+\tfrac12\big\rfloor\in\mathbb Z,
\qquad B=24,
$$

(round-to-nearest, exact ties toward $+\infty$, so cell $n$ is the half-open
interval $[n-\tfrac12,n+\tfrac12)$ in scaled units). $\hat\theta$ is the portable identity of
a world. This mirrors the prototype's $Q=4096$ possibility quantisation but
promotes it from a per-region device to *the* single global world address and
widens it so the metric geometry of §5 has room to express fine navigation.
The shipped manifest also gives finite safe natural-parameter bounds; an `i32`
bit pattern is never interpreted as an arbitrary divergent parameter (§3.3).

### 2.3 Spherical World Space

The first V3 spatial profile is one finite sphere of manifest radius $R_P$,

$$
\Omega=S_{R_P}^2\times[y_{\min},y_{\max}],
$$

where signed centimetre altitude $y$ follows the outward normal. A full point has
the canonical body-fixed address

$$
x=(p,u,v,y),\qquad p\in\{0,\ldots,11\},\quad
u,v\in\mathrm{Q0.48}[0,1),\quad y\in\mathbb Z\ \text{cm}.
$$

The profile requires $R_P+y_{\min}>0$.

The first profile is specifically the twelve-base-pixel **HEALPix NESTED**
equal-area map, not an unspecified twelve-patch projection. The profile freezes
the piecewise maps $E_p:[0,1)^2\to S^2$, their inverse branch equations, base-face
orientation table, bit-interleaving order, and every neighbor/edge remap. Those
normative integer/fixed-point equations and conformance fixtures are part of the
spatial-profile identity. If a direction has
multiple patch preimages, the lowest patch id owns it and the edge/corner table
rewrites $(u,v)$ into that owner's half-open domain; no excluded `1.0` coordinate
is serialized. A level-$L$ cell is $(p,m)$ with a $2L$-bit Morton path,
$L\le L_{\max}=32$, and reference-sphere area
$4\pi R_P^2/(12\cdot4^L)$. Q0.48 point coordinates may locate subcell positions
beyond the cell hierarchy. Census and entity levels are separately frozen below
$L_{\max}$ (§6). Equal area always means equal reference-sphere area (equivalently
equal solid angle), not sloped
terrain area or water-column volume.

The HEALPix mapping equations guarantee equal reference solid angle; fixtures
verify implementations at every supported level, along with round-trip
address ownership, reciprocal neighbors, and continuity at seams, poles, and
antipodes. Field kernels consume the decoded body-frame unit direction, not patch-local
coordinates, so patch boundaries are address seams rather than physical seams.
Seam transforms, neighbor order, antipode handling, and the fixed-point
direction map are fixtures. Decoded rotation changes inertial illumination and
tides; it never rotates terrain, Builds, or Impression addresses.

The World Space line element is

$$
ds_W^2=(R_P+y)^2ds_{\mathrm{unit}}^2+dy^2.
$$

Walking distance integrates this metric along the fixed-tick terrain path
$y=Z(n)$; flight and vertical travel use their sampled altitude. Reference-sphere
great-circle queries set $y=0$. Egress credit uses the fixed-tick path integral,
not endpoint chord distance or render-frame polylines.
The twelve-patch hierarchy is only V3's address, equal-area census, and lazy
field substrate. It is not Option 1's six-face cube-map address or latent-decoder
geometry, and it is not the World Loom's icosahedral typed-state/constitutive
solver or its transport navigation substrate.

### 2.4 Canonical observation measure and time

Canonical Model time is signed integer SI seconds $t$ from the epoch in $M$.
The manifest fixes the orbital/rotation parameterization, admissible ranges,
epoch convention, and portable fixed-point phase/trigonometric tables; the joint
law deterministically decodes rotation period, orbital period, obliquity,
eccentricity, stellar forcing, and tidal coefficients. Static geology and base
identities do not depend on $t$; illumination, tides, seasonal climate envelopes,
phenology, and time-conditioned attributes do. Weather, behavior, growth, and
other live simulation remain Visualization state.

Because $A$ has no free time argument, a steerable sufficient statistic cannot be
"whatever the value is now." It declares a time-independent value, a cycle
average, a **fixed named phase-bin** average, or a coefficient in a frozen
temporal basis. Its spatial/population denominator is likewise explicit:
equal-solid-angle surface cells, applicable cells, canonical lineages, bounded
organism candidates, or post-inhibition living-measure mass. Together these declarations form
$\nu_a$, the canonical observation measure for statistic $a$. An arbitrary-$t$
field value is a derived Canonical query reconstructed from the forcing/basis
contract; it is not another coordinate of $\nabla A$. An Impression stores exact
$t$ or a named phase interval when its observed subject depends on it, while a
Yearning refers to the corresponding fixed phase-bin/basis statistic.

Time reduction uses checked modular arithmetic with a frozen negative-time and
overflow rule. Static layers omit time from dependency keys; phase-independent
and phase-binned layers consume only the minimal declared time key.

### 2.5 Dual coordinates: natural $\leftrightarrow$ mean

Because $A$ is convex, it has a Legendre–Fenchel dual
$A^*(\mu)=\sup_\theta[\langle\theta,\mu\rangle-A(\theta)]$, and the two are
conjugate: $\mu=\nabla A(\theta)$, $\theta=\nabla A^*(\mu)$, $A^{**}=A$. With
$h$ the normalized base measure, the exact identity is
$A^*(\mu(\theta))=\mathrm{KL}(p_\theta\Vert h)$. Calling this ordinary negative
Shannon/differential entropy would be wrong for a nonuniform $h$.
The two coordinate systems have direct meaning:

- **Natural coordinates $\theta$** are the *knobs* — what Egress moves.
- **Mean (expectation) coordinates $\mu=\nabla A(\theta)=\mathbb E_\theta[T]$**
  contain scalar means and prevalence means. Scope acts on their schema-normalized
  values directly (§4, §7).

This duality is the engine of the whole design: it reappears in the metric (§5),
in reconciliation (§7), in Egress (§8), and in Attractors (§11), each a different
reading of the same $A$/$A^*$ pair.

### 2.6 Theoretical, Representable, Reachable

- **Theoretical Possibility** is the natural-parameter domain
  $\Theta=\{\theta:A(\theta)<\infty\}$, convex.
- **Representable Possibility** is the finite Q24 lattice whose decoded mean lies
  in the manifest's safe interior set $\mathcal P_{\mathrm{safe}}$ (§3.3).
- **Reachable Possibility** from $\theta_0$ is the set connected to $\theta_0$ by
  certified selected-mode Egress paths (§7–8) that stay in the safe set and have
  positive Resonance (§9). It is generally a proper subset; separate unresolved
  statistical modes need not form one convex region.

A fourth, physically meaningful set is specific to the law view: the **marginal
convex support**

$$
\mathcal P=\{\mu:\mu=\mathbb E_p[T]\text{ for some }p\}=\operatorname{conv}\{T(z):z\in\mathcal Z\},
$$

the set of achievable expectation vectors. Its prevalence projection is the
polytope $\operatorname{conv}\{c_r\}$, while its scalar directions are unbounded.
$\nabla A$ is a bijection from $\Theta$ onto the interior of $\mathcal P$ for the
minimal regular family. On a *prevalence* boundary — a trait pushed to
a degenerate extreme (perfectly pervasive, or perfectly absent) — the natural
parameter diverges and the face-normal prevalence statistic becomes
deterministic, so $g=\operatorname{Cov}[T]$ *degenerates*
($\lambda_{\min}\!\to\!0$) in that direction. Scalar variation and variation
tangent to the face may remain. This is a theoretical
limit, not a storable address: the safe-set erosion and Resonance response gate
make "everything exactly the same" unreachable without projecting a finite
coordinate onto an infinite-parameter boundary.

---

## 3. The free energy and the feasible manifold

### 3.1 One function generates the navigation subsystem

From $A$ alone the Model gets its mean map, metric, and reconciliation geometry.
V3 therefore specifies $A$ as a fixed, versioned, convex closed form. The first
draft used independent scalar and prevalence blocks; that would make their Fisher
cross-covariance identically zero and could not carry the claimed physical
correlations. V3 instead uses one **coupled categorical-Gaussian family**. Each
joint archetype has an unbounded scalar context $d_r$ and bounded prevalence
configuration $c_r$:

$$
A(\theta)=\tfrac12\,\theta_s^\top Q_0\theta_s+\langle q,\theta_s\rangle
+\log\!\sum_{r=1}^{R}\pi_r\exp\!\big(
\langle\theta_s,d_r\rangle+\langle\theta_p,c_r\rangle\big).
$$

- $Q_0\succ0$ gives scalar magnitudes such as relief energy and mean warmth an
  unbounded theoretical range and a permanent conditioning floor.
- $c_r$ lies in the product of the declared bounded prevalence ranges, while
  $d_r$ records the scalar context in which that prevalence configuration was
  accepted. The same responsibility $s_r$ therefore couples, for example,
  productivity and large-body prevalence.
- The $c_r$ affinely span the prevalence subspace and the complete family is
  minimal. There is no quadratic floor on a prevalence direction, so the Fisher
  response can still vanish at the hull boundary.

Writing $a_r=(d_r,c_r)$ and
$s_r(\theta)=\pi_r e^{\langle\theta,a_r\rangle}/
\sum_{r'}\pi_{r'}e^{\langle\theta,a_{r'}\rangle}$,

$$
\nabla A=
\left(Q_0\theta_s+q+\sum_r s_r d_r,\ \sum_r s_r c_r\right),
\qquad
\nabla^2A=
\begin{pmatrix}Q_0&0\\0&0\end{pmatrix}+\operatorname{Cov}_{s}[a].
$$

The off-diagonal block $\operatorname{Cov}_s[d,c]$ is the missing
scalar/prevalence coupling. The metric remains algebraically specified and
matrix-free: apply the block floor plus
$U(\operatorname{diag}s-ss^\top)U^\top v$, where $U=[a_1\cdots a_R]$. As the
responsibilities concentrate at a prevalence-hull extreme, response in the
corresponding prevalence direction vanishes; scalar directions retain $Q_0$.
No autodiff, realized probe Jacobian, or transport solve defines this metric.

### 3.2 Validity is intrinsic

There is no plausibility clamp cascade. Every $\theta\in\Theta$ yields a valid
law, and every reachable prevalence $\mu_p=\sum_r s_r c_r$ lies in the interior of
$\operatorname{conv}\{c_r\}$ by construction. Physical relationships between
attributes are split by what convex means can honestly guarantee. Archetype
inequalities constrain the categorical contribution and its covariance, but the
$Q_0\theta_s$ term means they do **not** by themselves constrain the full scalar
mean. Frozen linear cross-block halfspaces (for example, large-body prevalence
bounded by full productivity), scalar ranges, and prevalence-hull erosion are
therefore explicit facets of $\mathcal P_{\mathrm{safe}}$. Known nonconvex
alternatives live in separately certified convex regime/corridor cells (§7.3). Inventory,
topology, and nonlinear physical validity are guaranteed by the bounded spherical
closures and their safe-set certificate (§6.1, §6.3), not by calling correlation
causation. Thus statistical validity is intrinsic and the realized-planet contract
is separately checkable. This is a different mechanism from Option 2's triangular
parent-gated chart, not a claim that every scientific constraint is convex.

### 3.3 Typed interior projection

The closed prevalence hull contains limiting laws whose natural parameter is
infinite, so it is not a valid solve domain for a finite `Coord`. The manifest
therefore defines a compact convex **safe mean set**
$\mathcal P_{\mathrm{safe}}\subset\operatorname{int}\mathcal P$ by eroding every
prevalence-hull facet by a fixed rational margin, bounding the scalar means, and
intersecting frozen cross-block halfspaces and interval-certified nonlinear
closure cells. Every regime and transition corridor used by navigation is a
nonempty certified convex subset of this set. Scope targets,
reconciliation, and external same-schema mean data use this set. The margin is
small enough to express the intended saturation but large enough to bound
$\nabla A^*$ and the canonical condition number.

Projection is explicitly typed in mean space and accepts only same-family means
already in $\operatorname{int}\mathcal P$; data outside the convex support is
rejected before this operation:

$$
\Pi_\mu(\mu)=\arg\min_{\mu'\in\mathcal P_{\mathrm{safe}}}
B_{A^*}(\mu',\mu),
\qquad
\Pi_\theta(\theta)=\nabla A^*\!\left(\Pi_\mu(\nabla A(\theta))\right).
$$

Because the set is convex and $A^*$ is strictly convex, $\Pi_\mu$ is unique and
idempotent. Code cannot pass a natural `Coord` to it. Ordinary Egress interpolates
inside $\mathcal P_{\mathrm{safe}}$ and needs no projection; $\Pi_\theta$ is only
for same-schema hand edits and quantization recovery. Its real-valued result is
not yet a `Coord`: Canonical code rounds it under the half-open Q24 contract and
rechecks an enclosure of the decoded mean against $\mathcal P_{\mathrm{safe}}$.
A different law/schema
version is rejected or explicitly migrated, never projected into a new meaning.

### 3.4 The archetype bank is a fitted fixture, not an identity

The bank $\{d_r,c_r,\pi_r\}$, the scalar form $(Q_0,q)$, and the observable schema $T$
together determine $A$, hence the mean map, metric, feasible mean set, and
(via §6.3) the realized statistics. Choosing $T$ and the joint bank *is* choosing the
geometry — Chentsov's uniqueness (§5) applies only *after* $T$ and $A$ are fixed,
so this is as much a design surface as Options 1/2's weight matrices, merely
relocated into a bank of frozen data. It is fitted once, offline, from a
content-addressed corpus of accepted spherical worlds, with immutable training,
validation, and held-out splits. Release gates require affine rank and condition
bounds, facet coverage beyond every Scope target, named cross-block constraints,
schema-wide realized-moment bounds (§6.3), held-out diversity and repeated-motif
limits, and provenance for every accepted world. A bank that misses any gate is
rejected rather than reweighted at runtime. **World quality remains empirical,
not an algebraic identity.** Once accepted, the complete bank and split manifest
are hashed into the law identity (Appendix A).

---

## 4. Observables — the Realization/attribute contract

The Model exposes the fixed **sufficient-statistic schema**
$T=(T_1,\dots,T_k)$ and the mean map
$\varphi(\theta)=\nabla A(\theta)=\mathbb E_\theta[T]$. Each entry fixes:

- units, range, membership function, and whether it is scalar or prevalence;
- its subject population and applicability predicate;
- its area, lineage, candidate, living-mass, and/or canonical-time denominator;
- for a steerable coordinate, a certified positive applicability floor; for a
  derived observable only, zero-denominator behavior (`NotApplicable`, never an
  invented zero prevalence);
- the final Realization channels from which it is measured; and
- a Canonical estimator $R_a(\hat\theta)$ and maximum admitted error
  $\varepsilon_a$ against the law mean.

This is one language for what Yearnings push on, what Impressions capture, and
what the finite spherical census verifies. The player-facing meaning is the
statistic over its declared applicable population, not an unqualified average of
unlike terrain patches and organisms.

Two attribute *kinds* matter for Yearnings, distinguished as in the conceptual
model:

- **Scalar attributes** name a world-wide magnitude (relief energy, mean warmth) —
  a scalar-block mean.
- **Prevalence attributes** name *how widespread* a trait is across the world's
  species or regions. This is where V3 is structurally clean: a prevalence *is*,
  by definition, a prevalence-block mean $\mu_a=\mathbb E_\theta[T_a]\in[0,1]$ —
  the fraction of the declared applicable population expressing the trait. Scope therefore
  cannot be a spatial falloff even in principle (invariant 11); it targets a
  coordinate of $\theta$'s image. "Make branching plants pervasive" sets a target
  for the mean of the branching-indicator statistic.

**Query and accuracy contract.** Immutable snapshots support **Preview** (cheap,
uncertified), **Interactive** (a bounded approximation), and **Canonical** (the
portable reference kernel of §12). Results are `Complete`, `Pending` with a
deterministic continuation, `Partial` with certified componentwise bounds when
the consumer permits it, or `Unresolved`; a Canonical best effort is never
silently returned. Every result carries dependency keys, population count or
mass, applicability, temporal measure, and error bounds. Refinement narrows the
bounds without changing identity.

An entry is steerable only when its held-out and runtime bounds demonstrate
$|R_a(\hat\theta)-\nabla A(\theta)_a|\le\varepsilon_a$ across the admitted
coordinate set and twice that error plus coordinate/quota quantization error is
strictly narrower than every adjacent Scope-band gap.
Otherwise it remains a diagnostic derived observable or reports `Unresolved`.
A Visualization declares the groups and accuracy it consumes. Adding an optional
observable that is purely derived from existing canonical channels can be a
compatible capability addition; adding a sufficient statistic changes $k$, $A$,
the bank, metric, address, and moment closure, and therefore creates a new major
law family (§12).

---

## 5. Distance and neighbourhoods — the information metric

Euclidean distance in $\theta$ is meaningless: the same numeric step means very
different things depending on where you are. V3 uses the metric a manifold of
probability laws admits.

**The metric on Possibility is the Fisher information metric**

$$
g(\theta)=\nabla^2A(\theta)=\operatorname{Cov}_\theta[T]\ \succeq0 ,
$$

positive definite on the interior of $\Theta$. By **Chentsov's theorem** (Čencov;
extended to continuous sample spaces by Ay–Jost–Lê–Schwachhöfer), the Fisher
metric is the *unique* Riemannian metric, up to a single positive scale, invariant
under sufficient statistics — the unique metric that does not depend on how we
coordinatise observations, *once the observable schema $T$, family $A$, and fitted
bank are chosen*. V3 has no additional independent metric-weight matrix after
those choices (§3.1); the metric is then determined and its only free constant is
an overall scale (a game-feel dial for how
"far" a given amount of world-change feels).

Local distance is the metric length $d_g(\theta,\theta+\delta)^2\approx\delta^\top
g(\theta)\delta$, and the phenomenon the conceptual model asks for falls out: a
small numeric move where $\operatorname{Cov}[T]$ is large (the law responds
strongly) is metrically *far*; a large numeric move in a flat direction of the
covariance is metrically *near*. High-sensitivity / continuity-risk directions are
the large-eigenvalue directions of $g$, derived from that same metric when the
bounded eigensolve is worth its cost.

For a *closed-form* comparison of two world-laws — needed far more often than a
geodesic length — V3 uses the **KL / Bregman divergence**, which for an
exponential family is exactly the Bregman divergence of $A$:

$$
\mathrm{KL}\big(p_{\theta_1}\Vert p_{\theta_2}\big)
=B_A(\theta_2,\theta_1)
=B_{A^*}(\mu_1,\mu_2).
$$

Read the argument order carefully in each form: in natural coordinates the first
$\mathrm{KL}$ argument $\theta_1$ is the *second* slot of $B_A$; in mean
coordinates the first $\mathrm{KL}$ argument $\mu_1$ is the *first* slot of
$B_{A^*}$. Getting this backwards silently inverts every projection — it is the
kind of trap an agent maintainer must check against the identity, so both forms
are written out. (The Fisher metric is the second-order form of this divergence,
so the two are consistent; the geodesic Fisher–Rao *distance* has no closed form
for a general family, and V3 never needs it — metric *steps* for Egress, Bregman
*divergences* for comparison.)

Cost: $g$ is $k\times k$ with $k\le48$ in the first manifest. Its block floor
plus joint-archetype covariance is algebraically specified and applied
matrix-free in $O(Rk)$. Egress still needs bounded Newton/CG solves; the identity
removes numerical differentiation, not numerical inversion (§8, §13).

---

## 6. Realization: one spherical statistical planet

### 6.1 The layer graph and planetary closures

For a committed coordinate, canonical surface address, and Model time,

$$
\mathcal W(M,\hat\theta,x,t)=
(P,G,Z,W,D,C,H,S,B,E,\Lambda),
$$

is a declared dependency graph:

```text
planet/orbit P
  ├─ geology G ─ terrain/slope Z ─┬─ water level W ─ macro drainage D
  │                               └──────────────────────────┐
  ├─ finite water inventory ────────────────> W              │
  └─ atmosphere/orbit + time forcing ───────> climate C <────┘
                elevation Z + ocean W ──────> C
                      D + C ─ hydrology H ─ soils S ─ biome B
                                                   └─ ecology E ─ entities Λ
```

The profile fixes geometric radius and the address chart. The joint law decodes
gravity, rotation, atmosphere and finite water inventory, geological spectral
energy, climate response, soil rates, and ecological coefficients. On census
radial columns with solid angle $\Delta\Omega_i$, water volume determines one
global sea level from the monotone finite-sphere equation

$$
V(h)=\sum_i\frac{\Delta\Omega_i}{3}
\left[(R_P+h)^3-(R_P+z_i)^3\right]_+.
$$

The canonical solve chooses the least representable $h$ whose enclosure contains
the inventory target, then apportions its subquantum boundary-column residual in
cell-id order so the water ledger closes exactly. Zero inventory gives a dry
planet; the safe set excludes inventories above $V(y_{\max})$; shoreline equality
uses the lower/dry owner and then cell id. Macro drainage uses
integer centimetre elevations, canonical spherical neighbors, hierarchical
priority flooding, and feature-id tie breaks; every non-endoreic route terminates
at the ocean when one exists, and flux is conserved at cell junctions. Integer-time insolation,
elevation, ocean distance, and transport envelopes drive a fixed-order coarse
energy/moisture climate closure with residual enclosures; hydrology,
soils, biome, and ecology consume the settled upstream products. Each nonlinear
closure has a residual/error result and a fixed work cap rather than assuming one
converged float answer.

Global sea-level, coarse drainage, cycle forcing, and moment-ledger work is
bounded by one frozen census, not by explored area, but it is real cold work and
is included in §13's ledger. A refined child set inherits its parent's outlet and
inventory, and every added residual has zero parent-weighted sum; restriction
therefore reproduces the parent value exactly. A refinement that cannot satisfy
those conservative residual and outlet conditions is unavailable rather than
silently replacing a settled parent closure.

### 6.2 Spherical fields over common innovation

Smooth primitive channels use a spherical Matérn-like law. On $S_{R_P}^2$, the
Laplace--Beltrami eigenvalue for harmonic degree $\ell$ is
$\ell(\ell+1)/R_P^2$, so the isotropic reference spectrum is

$$
S_\ell(\theta)\propto
\left(\kappa(\theta)^2+\frac{\ell(\ell+1)}{R_P^2}\right)^{-(\nu_M(\theta)+1)}.
$$

Here $\kappa$ is inverse range and $\nu_M$ is Matérn smoothness, distinct from the
measure family $\boldsymbol\nu$. The lazy sample combines a frozen number of low
spherical-harmonic modes, localized needlet bands with explicit truncation error,
and compactly supported geodesic residual kernels on the equal-area hierarchy.
Low-degree joint cross-channel filters, a body-frame anisotropic covariance field
with positive-eigenvalue bounds, and a shared plate/regime skeleton supply
nonstationary tectonic and climatic structure; locally Matérn residuals supply
detail. The anisotropy changes covariance/filter response, not spatial coordinates,
so it cannot fold the sphere or introduce a chart seam. The fast path is not a
globally stationary isotropic texture, though its local covariance family remains
bounded and inspectable.

Most importantly, every coefficient, plate seed, candidate site, and lineage slot
comes from a **common innovation id**

$$
u=H(\text{family seed},\text{spatial-profile id},\text{innovation revision},
\text{channel},\text{band},\text{cell/mode},\text{slot}),
$$

which excludes $\hat\theta$. The coordinate changes gains, cross-channel filters,
thresholds, and marks over the same $u$. Nearby laws therefore reshape the
same ridges, regime patches, candidate sites, and lineage possibilities until a
named topology threshold is crossed. Cache dependency keys and coordinate-specific
manifestation ids include $\hat\theta$; innovation and lineage-slot ids do not.
This common-random-number coupling is a fixed field bank, not the World Loom's
typed innovation program or transport navigation.

Canonical field evaluation freezes basis order, coefficient quantization,
portable tables, seam transforms, refinement reductions, and topology tie rules.
Preview/Interactive may use fewer bands or hardware floats; they cannot create an
Impression or permanent entity.

### 6.3 Mean-preserving spatial structure

The Realization consumes the law's joint-archetype responsibilities $s_r$ through
the single bounded canonical quantization $\hat s_r$ below; they are not two
unrelated decoders connected only by an ecological fit. Let
$\mathcal C_L$ be the $N=12\cdot4^L$-cell equal-solid-angle canonical census.
The manifest contains archetype-specific common spherical residuals $G_r(i)$ and
bounded amplitudes $\gamma_r$. Canonical balanced apportionment first turns the
real responsibilities $s_r$ into integer global column quotas $n_r$ using the
manifest's fixed per-cell responsibility denominator $Q_R$, with
$\sum_r n_r=NQ_R$, defining $\hat s_r=n_r/(NQ_R)$ and an explicit
$|\hat s_r-s_r|$ bound. Begin with the positive spatial prior

$$
\widetilde q_{ir}=\hat s_r(\theta)\exp(\gamma_rG_r(i)).
$$

Local responsibilities are the unique KL/I-projection

$$
q=\arg\min_{q\ge0}\sum_{i,r}q_{ir}\log\frac{q_{ir}}{\widetilde q_{ir}}
\quad\text{s.t.}\quad
\sum_rq_{ir}=1,\qquad \frac1N\sum_iq_{ir}=\hat s_r\ \ \forall r.
$$

Canonical matrix scaling with fixed row/column order, residual enclosures, and a
cold certified interval/high-precision fallback approximates the unique
I-projection while preserving the integer row/column quotas exactly. Separate $\gamma_r$ values let rare
archetypes vary in relative spatial concentration instead of one tiny $s_r$
forcing all spatial variation to zero. The safe parameter bounds and responsibility
precision ensure no shipped column falls below the representable mass quantum.
Thus the planet has correlated spatial regimes while their whole-sphere joint
mean is $\hat s$, within the declared discretization bound of the $s$ that
generates $A$. Each scaling sweep is $O(NR)$; the sweep cap and slower certified
fallback are reported and cached explicitly in §13's moment-ledger row. Scalar primitive fields use the
analogous centered form
$F_a(i)=\mu_a+\widetilde F_a(i)-N^{-1}\sum_j\widetilde F_a(j)$.

These identities close linear census moments. They do not magically preserve a
mean through sea-level selection, drainage, ecological competition, or another
nonlinear layer, so the final layer graph owns a **moment ledger**. Each steerable
statistic names a monotone correction parameter or a balanced discrete
apportionment, a bracket, and an error budget. Fixed-order bracketed solves set
scalar moments; common-innovation ranks assign exact integer quotas for bounded
memberships over applicable cells, lineages, or candidates. Coupled corrections
are an $A^*$-Bregman/I-projection in the same fixed statistic space, followed by
the affected dependency closure. Corrections run in one frozen dependency order;
every sweep reruns all affected downstream closures. The manifest admits a safe
parameter cell only after analytic bounds or outward interval evaluation over the
**whole cell** proves the correction map contractive, the brackets valid, and
conservation compatible—sampling the enormous Q24 lattice is not called
certification. After the bounded sweep, one simultaneous interval check covers
every conservation residual and steerable moment. Rerunning a settled closure is
bit-idempotent; otherwise the result is unresolved and the coordinate or
capability is not admitted.

The Canonical result reports the finite-planet estimator and a bound. A bounded
prevalence with $N_a$ applicable subjects has unavoidable discretization at most
$1/N_a$; steerable prevalence coordinates have the schema-proved positive
applicability floor from §2.1. A derived query outside that contract may return
`NotApplicable`. If a correction or bound cannot finish under its cap, the query
is `Pending` or `Unresolved` and no Impression or Egress decision consumes it.
This schema-wide bridge covers abiotic and biotic statistics, replacing the
earlier asymptotic ecology-only promise.

The single realized planet supplies one record $r(\mathcal W)$, not a histogram
of draws from the whole-record law. The ledger therefore checks each component
against $\mu=\nabla A(\theta)$ and may separately report bounded, schema-specific
local histograms without identifying them with $p_\theta$. Offline release tests
use an independent ensemble of generated planets to measure the conditional
residual $r(\mathcal W_\theta)-\nabla A(\theta)$, physical correlations, and
visible higher-order quality. The separate archetype corpus fits the idealized
atom law. Neither test equates a distribution of mean-matched representatives
with $p_\theta$. Failure rejects the bank or demotes a capability; this is an
empirical family-quality gate, not canonical identity for one planet.

### 6.4 Bounded hash-thinned ecology and entities

Living content is a finite marked candidate population with a real dominating
bound. A finite-band Gaussian suitability field $Z_\theta(x,t)$ feeds a
**bounded logistic-normal activation density**

$$
\lambda_\theta(x,t)=\lambda_{\mathrm{cap}}
\operatorname{sigmoid}(Z_\theta(x,t)),\qquad
0\le\lambda_\theta\le\lambda_{\mathrm{cap}},
$$

where every equal-area entity cell at frozen $L_e\le L_{\max}$ has exactly $C_e$
common candidate sites of reference area $A_e$, and
$\lambda_{\mathrm{cap}}=C_e/A_e$. Thus
$\lambda/\lambda_{\mathrm{cap}}$ is exactly the pre-inhibition activation
probability and $\lambda$ its expected pre-inhibition density. These are manifest
constants. This Bernoulli/hash thinning of a finite population is deliberately
not called a Cox or Poisson process, whose cell count would be unbounded.
Candidate ids come from common innovation. A candidate
is active iff its canonical hash fraction is below
$\lambda/\lambda_{\mathrm{cap}}$; deterministic neighbor priority supplies
inhibited spacing. Canonical trait marks use balanced rank apportionment over the
active applicable candidates, so their census prevalence meets the moment-ledger
quota rather than relying on an infinite-domain ergodic argument. The canonical
hierarchical quota summary stores prefix counts and rank intervals over all
$12\cdot4^{L_e}$ cells; cold construction is finite and ledgered, while a local
tile expands only its bounded interval.

A separately capped set of lineage slots derives immutable genomes and traits
from the same innovation root. Integer compatibility and energy-budget predicates
form trophic edges; stable lineage/edge ids, canonical tie order, and productivity
and biomass quotas bound the local food-web assembly. Slot identity
persists across nearby laws; a manifestation id additionally includes the
committed coordinate. Higher resource tiers may add decorative individuals but
cannot change the canonical candidate population, Scope, or capture.

After inhibition and quota marks, the canonical finite atomic measure
$\varrho=\sum_i w_i\delta_{x_i}$ (or its fixed presentation kernel) and its
trait-specific submeasures are the living mass transported for presentation in
§10; the suitability or pre-inhibition $\lambda$ is not substituted for that
population.

### 6.5 Determinism and caching by committed address

$\mathcal W$ consumes only committed $\hat\theta$, a canonical spherical cell or
point, and canonical time where relevant. A tile is a pure function of

$$
(M_{\mathrm{law}},M_{\mathrm{sphere}},M_{\mathrm{closure(layer)}},
\hat\theta,\text{spherical cell},\text{canonical time key},\text{layer}),
$$

where $M_{\mathrm{closure(layer)}}$ contains only that layer's algorithm revision
and declared upstream revision identities, not the compatibility root. The tile key
uses exact $t$ unless the channel declares a forcing bucket with invariant output
or a returned interval bound; it never aliases two changing Canonical values.
It includes $\hat\theta$ even though its innovation phases do not. Global census,
sea-level, climate, and routing summaries have separate immutable keys and byte
ceilings. Farthest-first eviction, cancellation, or a smaller cache may repeat
work but cannot change a settled result. There is no authoritative sub-bucket
world state: unconsumed travel and numeric remainder belong to the Traveler
commit policy, and the world changes only when §8 certifies a new address.

---

## 7. Yearnings → a reconciled target law

Reconciliation produces one unique **target law after a statistical regime and,
if used, an Attractor cluster have been selected**. It does not average separated
community destinations or incompatible causal regimes into one pseudo-count.
Within a selected mode the program is strictly convex and all inputs are reduced
canonically.

### 7.1 Per-attribute requests

Each active Yearning $y$ has weight $w_y>0$, source Impressions, and for each
usable attribute $a$ an Influence and Scope. An observed subject selects a
**fixed schema prevalence predicate**: for a binary trait this is its membership;
for a scalar observation the UI snaps to a versioned predicate/bin such as
“applicable cells at least this warm,” whose whole-planet prevalence is already a
sufficient-statistic coordinate. It never turns Scope into a scalar-magnitude
dial. The Impression stores that predicate id and the source planet's applicable
global prevalence $p_{0,y,a}$; the subject's own membership is evidence for which
predicate was selected, not the prevalence baseline.

Multiple source Impressions create separate predicate terms. Terms with the same
schema/predicate id are grouped by checked weight and breakpoint reductions;
different predicates are never averaged into a new undeclared statistic. Frozen
nonnegative source/attribute shares $\omega_{y,a}$ sum to one over the active
terms, and $w_{y,a}=w_y\omega_{y,a}$, so adding another source cannot silently
multiply the Yearning's total weight.

Accentuate and Repress therefore apply only to prevalence coordinates
$p_a(\mu)=\mu_a$. Hold may apply to either a prevalence or scalar coordinate; the
schema supplies an affine scalar normalization $z_a(\mu_a)\in[0,1]$, with
$z_a=p_a$ for prevalence, and stores $h_{y,a}$ **once when Hold becomes active**.
The exact convex request penalty is

| Influence | threshold/reference | penalty $q_{y,a}(\mu_a)$ |
|---|---|---|
| **Accentuate** | $\ell_{y,a}=\max(p_{0,y,a},\tau(s_y))$ | $\tfrac12w_{y,a}[\ell_{y,a}-p_a(\mu)]_+^2$ |
| **Repress** | $u_{y,a}=\min(p_{0,y,a},1-\tau(s_y))$ | $\tfrac12w_{y,a}[p_a(\mu)-u_{y,a}]_+^2$ |
| **Hold** | $h_{y,a}=z_a(\mu_a(\hat\theta_{\mathrm{activation}}))$ | $\tfrac12w_{y,a}\eta_{\mathrm{hold}}[z_a(\mu_a)-h_{y,a}]^2$ |
| **Disable** | — | $0$ |

Here $[x]_+=\max(x,0)$. Accentuate is an absolute one-sided lower request;
Repress is the complement-based absolute upper request. An already-satisfied
request exerts no force in the wrong direction. A scalar magnitude can be Held,
but it can be Accentuated or Repressed with Scope only through one of the fixed
prevalence predicates above. Hold ignores Scope, and its
fixed-point activation snapshot remains unchanged until the request is disabled,
reactivated, or explicitly reconfigured; it is part of canonical replay input.
This prevents a moving-current Hold from ratcheting after the state it was meant
to protect.

Scope maps to a target through a fixed monotone table
$\text{singular}\to\text{common}\to\text{pervasive}$, kept strictly interior —
$\tau(s)=\tau_{\min}^{1-s}\tau_{\max}^{s}$ with $0<\tau_{\min}<\tau_{\max}<1$ (so
"pervasive" asks for, say, $80\%$, never $100\%$; the last approach to totality is
where Resonance vanishes, §9). For Repress, pervasive Scope means the complement
should be pervasive and therefore asks for at most $1-\tau_{\max}$ of the selected
trait. Hold is finite and can be outweighed by stronger aggregate intent or
feasibility; Disable contributes nothing.

### 7.2 A convex maximum-entropy program (order-independent)

The squared hinges cannot in general be collapsed into one average target without
changing their one-sided meaning. V3 groups equal fixed-point breakpoints and
accumulates their weights with checked integers, then evaluates groups in
attribute-id/breakpoint order. For an optional selected Attractor cluster define
the well-typed term

$$
Q_i(\mu)=
\begin{cases}
\kappa_i B_{A^*}(\mu,\mu_i),&\text{cluster }i\text{ selected},\\
0,&\text{no cluster selected}.
\end{cases}
$$

For selected regime $m$, let $\mathcal C_m(\mu_\star)$ be the certified
transition corridor of §7.3. The target is the minimizer in **mean coordinates**:

$$
\mu_{m,i}^{+}=\arg\min_{\mu\in\mathcal C_m(\mu_\star)}\ \Big[
\underbrace{B_{A^*}\!\big(\mu,\mu(\theta_\star)\big)}_{\text{KL to the current law}}
+\underbrace{\sum_{y,a}q_{y,a}(\mu_a)}_{\text{one-sided intent and activation Hold}}
+\underbrace{Q_i(\mu)}_{\text{zero or one selected community destination}}\Big],
\qquad \theta_{m,i}^{+}=\nabla A^*(\mu_{m,i}^{+}),
$$

This objective is **strictly convex**:
$B_{A^*}(\cdot,\mu_\star)$ has Hessian $g^{-1}\succ0$, hinges and the Attractor
term are convex, and the nonempty feasible set is compact and convex. The minimizer exists,
is finite, and is unique for contradictory requests. Order-independence is then a
theorem — a unique minimizer of a strictly convex function does not depend on the
input order — and is machine-checkable after the checked canonical reduction.

Posing the fit in mean coordinates matters. The earlier natural-coordinate form
$\tfrac12\sum_a\pi_a(\nabla A(\theta)_a-\bar\mu_a)^2$ is a nonlinear least-squares
penalty whose exact Hessian carries an indefinite third-derivative term; it is
*not* convex for large residuals. The mean-coordinate form above is convex by
construction. What is genuinely V3 is (i) the **proximal term is exact KL** to
the current law; (ii) the feasible set is the **safe moment set**, so
"Model validity takes precedence over literal satisfaction" is automatic — an impossible
combination is simply not in the selected chart and the flow settles at the
$A^*$-Bregman-closest feasible law; and (iii) the whole thing is one **convex
program with a unique minimizer inside the chosen fixed mode**. Canonicalization
still covers reduction, numeric evaluation, and quantization enclosure (§12); the
mathematical uniqueness alone does not make IEEE reductions portable.

### 7.3 Fixed statistical modes, not averaged causal stories

A convex family can place its mean between archetypes that correspond to visibly
different causal stories. V3 does not pretend that mathematical uniqueness makes
such an average intuitive. The immutable bank therefore labels each archetype
with one of a small, versioned set of **statistical regime charts** (for example,
canopy-supported versus cliff-supported gliding). Each chart contributes a convex
safe destination core $\mathcal R_m\subset\mathcal P_{\mathrm{safe}}$; it adds no
runtime grammar or new state dimension. Empty or uncertified cores are discarded.
Because the current mean need not already be in that core, the actual solve and
path use the explicitly certified convex corridor

$$
\mathcal C_m(\mu_\star)=
\operatorname{conv}\big(\{\mu_\star\}\cup\mathcal R_m\big)
\subseteq\mathcal P_{\mathrm{safe}}.
$$

Thus the current point is feasible and every path point is valid. A weak or
conflicted request may minimize before entering $\mathcal R_m$; in that case the
Model reports progress toward the mode, not that the destination regime has
already been reached.

The Model evaluates at most $K_m$ manifest-bounded candidate charts and no more
than $K_{\mathrm{nav}}$ combined chart/Attractor candidates. It returns their unique targets, objective
bounds, predicted moment changes, and stable mode ids. A Traveler may select one.
An optional deterministic policy may select the lowest certified objective with
mode-id tie breaks and hysteresis, but if separated candidates remain within the
ambiguity tolerance it returns `AmbiguousModes` rather than silently averaging
them. Once selected, §8 follows one Fisher-geometric target. This bounded atlas
preserves V3's fixed vocabulary and convex kernel; it is not the Loom's open-ended
typed rewrite and path search.

A chart/Attractor pair is admitted only when the enclosed cluster center belongs
to that chart's destination core; otherwise it is reported incompatible rather
than pulling a corridor toward a center it cannot contain.

---

## 8. Egress dynamics

Egress is a single Resonance- and travel-gated step from the current coordinate
$\theta_\star$ toward one selected reconciled target $\theta^{+}$ of §7. The
Model proposes and certifies the information-geometric step; the Traveler owns
the travel credit and mode-selection policy.

### 8.1 The step direction

Natural-coordinate straight lines do not give a simple proof that the convex
mean-space objective decreases. V3 therefore follows the **mixture geodesic**,
the straight segment in expectation coordinates,

$$
\mu(\alpha)=(1-\alpha)\mu_\star+\alpha\mu^+,
\qquad
\theta(\alpha)=\nabla A^*(\mu(\alpha)),\qquad 0\le\alpha\le1.
$$

The selected transition corridor is convex and contains both endpoints, so the
entire curve is feasible and has a finite natural parameter. Its Fisher arclength is

$$
L(\alpha)=\int_0^\alpha
\sqrt{(\mu^+-\mu_\star)^\top
g(\theta(a))^{-1}(\mu^+-\mu_\star)}\,da.
$$

A fixed quadrature and bracket choose the largest $\alpha$ whose enclosed length
does not exceed the allowed step. Because the §7 objective is convex in $\mu$ and
$\mu^+$ is its minimizer, its value along this segment is non-increasing before
quantization. This replaces the earlier natural-coordinate step and fixes the
monotonicity contract rather than merely changing its sign.

### 8.2 Computing $\nabla A^*$ and the solve

Inverting $\mu=\nabla A(\theta)$ and evaluating arclength have no closed form for
the joint softmax family. Damped Newton iterations use
$\theta\leftarrow\theta-g(\theta)^{-1}(\nabla A(\theta)-\mu)$ at a fixed iteration
budget for Interactive evaluation. The Canonical path additionally computes
residual/roundoff enclosures; a fixed iteration count alone is not a determinism
or accuracy proof. The benefit of the dual route is feasibility and the monotone
mixture curve, not zero cost.

With $U=[a_1\cdots a_R]$, each metric--vector product is

$$
\begin{pmatrix}Q_0&0\\0&0\end{pmatrix}v+
U(\operatorname{diag}s-ss^\top)U^\top v,
$$

so fixed-order preconditioned CG is $O(Rk)$ per metvec and never forms a dense
Hessian. Near the safe-set boundary, interval residuals may force the slower
canonical factorization path named in the manifest.

### 8.3 Resonance- and travel-gated step (Traveler policy)

The Traveler accumulates **credited spherical path arclength** $\Delta\ell_W$ in
fixed-point millimetres from the actual body-frame path sampled at the fixed
navigation tick, including altitude, and consumes it at that cadence. Endpoint
chord distance and render frames are not used. The accumulator is tagged by an
**intent digest** of the canonically reduced requests, Hold snapshots, selected
regime revision/mode, normalized Attractor snapshot root/center/precision, and
Traveler policy. Retained credit is direction-specific:
changing that digest deterministically discards it and cancels its continuation;
newly walked distance starts a new accumulator rather than steering a stale
request.
The allowed Fisher length is

$$
\Delta s=\hat\beta\,\hat\rho\,\Delta\ell_W,
$$

where $\hat\rho$ is Canonical Resonance (§9) and $\hat\beta$ belongs to a versioned
Traveler policy, not Model identity. The policy id, rate, cadence, carried travel
remainder, selected mode, and Hold activation snapshots are replay inputs. Fixed
cadence plus the carried integer remainder makes the result independent of frame
subdivision. Zero credited travel or zero Resonance gives exactly zero Egress.

Every permanent commit runs the frozen Canonical solve even if Interactive math
already previewed a direction. It produces an interval enclosure for every
component of $\theta(\alpha)$. A new $\hat\theta$ is committed only if every
enclosure lies wholly inside one Q24 rounding cell and its decoded mean remains
inside both the safe set and selected corridor. Q24 rounding cells are half-open
under the frozen ties-toward-$+\infty$ rule of §2.2; an exact rational equality certificate at a
half-quantum selects that rule's owner, so exact boundary values do not refine
forever. If an enclosure otherwise straddles a cell boundary, the Model follows
the deterministic cold refinement schedule. Exhausting the cap returns
`Pending` with a continuation or `UnresolvedQuantization`; the current address
and unconsumed travel credit remain unchanged.

A same-cell completion consumes no credit, so subquantum walking accumulates
until a distinct address can be certified. A shortened monotone step consumes
only the exact fixed-point credit used by that step and carries the conversion
remainder. A `Pending` continuation freezes its original intent digest, input
snapshot, and offered credit. Its extended snapshot digest also includes $M$,
$\hat\theta_\star$, canonical position/time, the support dependency key, and a
monotone navigation-sequence number. Distance walked later accumulates separately and cannot alter
the continuation's result or completion timing. `Pending` retains its frozen
credit while the digest is unchanged. `UnresolvedQuantization` has no hidden
continuation: it releases the offered credit back into the same-digest active
accumulator, and a fresh solve is attempted only after new credited distance is
added (or the digest changes, which discards it). This avoids both consuming
subquantum motion and indefinitely blocking later credit behind an unresolvable
snapshot. Only a distinct successful commit consumes credit; any later-credit
bucket is then solved afresh from the new coordinate. There is no
platform-dependent "nearest" commit. Continuation results are consumed in
navigation-sequence order, so later movement cannot race a stale support snapshot.

### 8.4 Reachability and settling

Integrating §8.3 from $\theta_0$ produces an Egress path that stays in the selected
safe transition corridor and moves only where $\rho>0$; its image over all admissible Yearning schedules is
Reachable Possibility (§2.6). Reachability depends only on Model quantities
($A$, $g$, certified regime candidates, and Canonical $\rho$) plus explicit
Traveler policy — never on Visualization readiness (invariant 12). The
pre-quantization §7 objective is non-increasing along the mixture segment. A
commit is accepted only when interval evaluation proves the quantized endpoint
does not increase it at all; otherwise the step is shortened or remains
unresolved. This is a per-tick statement: the KL
proximal term is recentered at the next committed state, so V3 does not claim one
unchanging global Lyapunov objective across an entire expedition.

---

## 9. Resonance

Resonance must be "a property of the Traveler's interaction with a Model and its
current Realization … not a property of a particular Visualization." V3 defines it
from Model fields only, as two factors,
$\rho=\rho_{\text{support}}\cdot\rho_{\text{align}}\in[0,1]$.

**Support** — is there enough living, connectable world around the Traveler? A
spherical-cap average of the Model's canonical ecological connectivity intensity
$\kappa_{\!e}$ (not rendered organism instances) is

$$
\rho_{\text{support}}=\operatorname{clamp}\!\left(
\frac{1}{A_{\mathrm{cap}}(r_n)}
\int_{B_{S^2}(x_\star,r_n)}\kappa_{\!e}(\hat\theta_\star,x,t)\,dA,0,1\right),
\qquad
A_{\mathrm{cap}}(r)=2\pi R_P^2\big(1-\cos(r/R_P)\big),
$$

with $0<r_n<\pi R_P$. It is a spatial ecology average, not a susceptibility. Every
use that can change a commit, including the smooth rate multiplier, uses the same
portable fixed-cell quadrature and canonical time policy; resident tiles and
resource tier may supply only a UI preview. The result includes quadrature and
field error bounds, and an unresolved support interval prevents a commit.

**Alignment** — this is the genuinely new, exponential-family part. It reports
whether the *net requested move* is one the law can actually make, and whether the
Yearnings agree:

$$
\rho_{\text{align}}
=\underbrace{\frac{\chi}{\chi+\varepsilon_\rho}}_{\text{can the law respond?}}\cdot
\underbrace{\exp\!\Big(-D_Y/\sigma_0^2\Big)}_{\text{do the active request forces agree?}}.
$$

The mixture path's actual initial mean tangent is
$d_\mu=\mu^+-\mu_\star$, not the endpoint chord. Let
$\bar\mu=z(\mu)$ apply the frozen schema affine normalizers, and let
$\bar g^{-1}=J_z^{-\top}g(\theta_\star)^{-1}J_z^{-1}$ be the Fisher metric in
those normalized mean coordinates. For
$u=(\bar\mu^+-\bar\mu_\star)/
\lVert\bar\mu^+-\bar\mu_\star\rVert$, define

$$
\chi=\big[u^\top\bar g^{-1}u\big]^{-1}.
$$

This is the reciprocal Fisher cost along the **actual mixture tangent**. It is
continuous as a prevalence component appears, includes every scalar/prevalence
cross block, and never mixes natural- and mean-coordinate vectors. A zero tangent
defines $\chi=0$; a prevalence-only Schur value may be reported as a diagnostic,
but it does not replace this rate factor.

Here $\widetilde r_y=-w_y^{-1}\nabla_{\bar\mu}\sum_a
q_{y,a}(\mu_{\star,a})=-w_y^{-1}J_z^{-\top}\nabla_\mu\sum_a
q_{y,a}(\mu_{\star,a})$ is one request's **unweighted**, schema-normalized desired
direction, and $\mathcal Y_+=\{y:\lVert\widetilde r_y\rVert>0\}$. Over that set,
$\bar r=(\sum_{y\in\mathcal Y_+}w_y\widetilde r_y)/
(\sum_{y\in\mathcal Y_+}w_y)$ and
$D_Y=(\sum_{y\in\mathcal Y_+}w_y\lVert\widetilde r_y-\bar r\rVert^2)/
(\sum_{y\in\mathcal Y_+}w_y)$. These sums use the same
checked canonical grouping as §7. Satisfied one-sided requests are excluded
rather than misread as zero-direction opposition;
opposed active requests increase disagreement without inventing an averaged
target value. With $\mathcal Y_+=\varnothing$, $D_Y=0$; with
$\theta^+=\theta_\star$, the response factor is defined as zero because there is
no requested Egress direction.

The **response** factor uses this effective susceptibility in the requested
direction. As a prevalence is driven toward its degenerate
boundary, the face-normal prevalence statistic becomes deterministic and its
effective susceptibility collapses, so the factor
$\to0$: the law can no longer respond, and Resonance vanishes — the principled
reason "make everything the same" stalls. (This is the correct boundary physics:
susceptibility *vanishes* at a deterministic limit; it does not diverge. The
mean-coordinate cost of further prevalence gain diverges correspondingly — the
dual reading of the same fact. Scalar and tangent-to-face variation may remain.) The representable safe set
never reaches that singular limit. Its finite prevalence-response cutoff is a
frozen, empirically calibrated design gate, tested over the safe boundary cells;
it is not claimed to follow automatically from positive definiteness or scalar
cross-coupling. The **agreement** factor falls when
active request forces disagree — "low Resonance indicates ambiguity,
incompatibility, or insufficient local support."

Resonance is thus simultaneously a **rate scale** (it multiplies $\Delta s$), a
**confidence signal** (its factors report *why* movement is slow — barren
surroundings, a saturated request, or conflicting desires), and a **threshold**
under the trivial policy $\rho<\rho_{\min}\Rightarrow\Delta s=0$. It never touches
rendered geometry, frame timing, or hardware. Ecological support is a shared
design pattern; the Fisher susceptibility and convex-request disagreement are
specific to V3's law geometry.

---

## 10. Continuity as unbalanced optimal transport (the "drift")

This is the second, distinct geometry of V3. Egress changes the world-law; the
realized world must transition without a global reload. V3 does not merely *report*
the coming change (Option 1's sensitivity descriptor) and does not only *lag* the
coordinate (Option 2's crossfade). For the layers where content is a genuine
spatial mass distribution — the **living and biome measures** — it **moves the
content**: the transition follows a geodesic of **unbalanced optimal transport**,
so forests spread and ranges migrate across World Space and blooms and extinctions
happen in place.

### 10.1 Why unbalanced

Balanced optimal transport (the Wasserstein metric) conserves total mass: it can
*migrate* a forest belt across the map but cannot make a trait more prevalent —
moving mass in must drain it from elsewhere. Scope needs mass to be *created and
destroyed* (a species blooming into pervasiveness, a biome going extinct). The
metric that does both is the **Wasserstein–Fisher–Rao (WFR)** metric, also called
Hellinger–Kantorovich (Chizat–Peyré–Schmitzer–Vialard; Liero–Mielke–Savaré;
Kondratyev–Monsaingeon–Vorotnikov). Its dynamic form adds a *source term* to the
continuity equation:

$$
\mathrm{WFR}_{\delta_W}^2(\varrho_0,\varrho_1)
=\inf_{v,\alpha}\ \tfrac12\!\int_0^1\!\!\int_{S_{R_P}^2}
\big(\lVert v\rVert_{S^2}^2+\delta_W^2\alpha^2\big)\varrho\,dA\,d\tau
\quad\text{s.t.}\quad
\partial_\tau\varrho+\operatorname{div}_{S^2}(\varrho v)=\varrho\alpha,
$$

where $v$ is tangent to the sphere and the ground cost is great-circle distance.
The transport field $v$ is horizontal — **content migrates across World Space**.
The reaction field $\alpha$ is vertical — **content blooms and fades in place**.
The profile freezes one WFR/HK normalization and its derived
$d_{\mathrm{cut}}(\delta_W)$ table. The length-scale $\delta_W$ sets the
crossover: below that normalization-specific cutoff transport is favored; beyond
it, destruction and recreation are favored over implausibly long motion. No
normalization-independent $\pi\delta_W$ formula is assumed. This is exactly the Continuity requirement — near-field content
slides smoothly, distant content converges in place — and $\delta_W$ is a physical
game-feel dial. (A clarification the theory forces: the "Fisher–Rao" *inside* WFR
is the Hellinger metric on square-roots of spatial mass — a different object from
the information metric $g=\nabla^2A$ of §5, which lives on the compact coordinate.
V3 keeps them explicitly separate: information geometry navigates Possibility; WFR
transports the Realization. The free energy generates the former, never the
latter.)

### 10.2 What is closed-form, and what genuinely transports

- **Post-inhibition living measures $\varrho$ and biome-mixture measures are
  spatial measures**, so unbalanced WFR applies literally: the transport field $v$
  migrates forest belts and species ranges across the sphere, and the reaction field
  $\alpha$ grows and fades them where prevalence changes. The coupled WFR solve
  transports the **realized atomic measure (or its fixed kernel)**, not suitability
  or the pre-inhibition activation density $\lambda$. For a common slot weight or
  fixed-kernel density $w$, the square-root curve
  $w_\tau=((1-\tau)\sqrt{w_0}+\tau\sqrt{w_1})^2$ is only the exact reaction-only
  fallback at one location; it is not presented as a
  separable component of a general migrating WFR geodesic. Where a non-Gaussian biome
  boundary must reshape, an **unbalanced Sinkhorn / scaling iteration** at a
  fixed work cap supplies a bounded presentation approximation. This is
  the layer where V3's "content physically moves" claim is real, and it is the
  living heart of the game.
- **Abiotic primitive fields** use exact Bures--Wasserstein displacement on
  shared commuting spectral blocks and explicitly bounded finite SPD matrix
  blocks. A shared diagonal isotropic harmonic block uses
  $S_{\ell,\tau}=((1-\tau)\sqrt{S_{\ell,0}}+
  \tau\sqrt{S_{\ell,1}})^2$; a fixed finite cross-channel block uses the standard
  matrix Bures map. Noncommuting needlet, anisotropic, or cross-filter pieces use
  a bounded phase-locked spectral interpolation whose endpoint error is reported
  and which is explicitly a presentation heuristic, not “exact Bures.” Common
  innovations keep phase and feature slots coupled. Stated honestly: this is an
  **in-place spectral reshaping** — relief changes energy,
  roughness, anisotropy, and level — *not* rigid horizontal migration of a ridge.
  Genuine horizontal migration of an abiotic feature would need optimal transport
  of elevation as a spatial mass (not closed-form); V3 does not claim it on the
  fast path and does not need it, because the "large changes appear in the
  distance and resolve on approach" experience comes from the streaming annulus
  (§10.3), where the far field is realized directly at the new law.

### 10.3 The streaming annulus and the Model/Visualization split

The transition is computed only in the geodesic annulus
$r_n\le d_{R_P}(x,x_\star)\le r_f<\pi R_P$ between the pinned near zone and resolved
far zone. The manifest caps the annulus level, active cells, living measures,
iterations, and two endpoint buffers, so the actual bound is $O(B_{\max})$ with
fixed memory, not merely an unspecified $O(\text{band})$. Near the Traveler the
transport rate is zero; the far field realizes directly at the newest canonical
$\hat\theta_\star$; the band is mid-transport. Large changes therefore appear
first in the distance and resolve on approach.

Both endpoints are evaluated at the same canonical Model time/key $t_c$; ordinary
temporal evolution is a separate update and is never hidden inside the morph.
The **Model supplies continuity information** — canonical endpoint measures, the WFR
length-scale $\delta_W$, the commuting/matrix Bures descriptors and heuristic
spectral parameters for abiotic fields, and the
grow/fade rates — while the **Visualization owns the transient morph state and
performs the blend** (invariant 7; the conceptual model assigns boundary blending
to the Visualization). The morph is presentation-grade and discarded once the new
$\hat\theta$ tiles are resident; history lives entirely here, and the canonical
coordinate is always the single $\theta_\star$.

**Interrupted transitions are explicitly rebased.** Canonical commits carry a
monotone sequence number. If commit $q_2$ arrives while the presentation is still
morphing $q_0\to q_1$, the Visualization samples its currently displayed annulus
at the next fixed presentation tick, uses that measure and those band amplitudes
as the new transient source, targets $q_2$, and discards the superseded endpoint.
Living mass/source terms and shared-slot correspondence are rebased together;
abiotic spectra start from their current interpolated amplitudes. Repeated commits
coalesce in sequence order and never allocate a third endpoint or extend
canonical Egress. If the bounded transport path is unavailable, Visualization may
fall back to reaction-only grow/fade and phase-locked crossfade; it cannot delay or
change the Model State.

An Impression captured
mid-transition therefore always samples the *canonical* realization
$\mathcal W(M,\hat\theta_\star,\hat x,t)$, never the transient transport buffer. A
displayed organism maps to the canonical entity with the same common slot id; if
that slot is absent at the canonical endpoint, capture is refused rather than
silently changing the subject.

---

## 11. Impressions, Attractors, Builds, and dual-space travel

### 11.1 Impressions

An Impression is a compact Canonical record containing:

- generation identity $M$ and committed $\hat\theta$;
- twelve-patch Q0.48 surface address, signed centimetre altitude, and canonical
  tangent-frame revision;
- canonical time or phase interval when the subject depends on forcing;
- subject kind plus common slot/canonical entity id when one exists;
- for each captured attribute, the quantized observed membership/value, the
  applicable law mean, estimator bound, and schema id; and
- an optional versioned Build content id (§11.3).

Capture waits for `Complete` Canonical fields, topology, entity, applicability,
and moment-ledger results. A thin classifier stores the measured Canonical value
and margin rather than relying on a reclassified low-precision sample. A
compatible Model re-derives the law and canonical subject from the exact sphere,
time, and slot address; a Visualization may depict it differently.

### 11.2 Attractors as normalized, separate clusters

Raw Attractor history may be large, but it is not passed unbounded into a
navigation tick. A versioned normalization pass applies content-id deduplication,
publisher/expedition caps, removals, and stable ranking, then emits at most $K_A$
bounded evidence summaries with a snapshot root. The Canonical Model accepts only
that normalized shortlist; clustering, barycenters, and rank ties use frozen
fixed-point reductions and interval enclosures, and the combined regime/Attractor
candidate count is capped by $K_{\mathrm{nav}}$. The deterministic pass produces separate

$$
\mathcal A_i=(\text{id}_i,\mu_i,\Sigma_i,\kappa_i,\text{World bounds},
\text{evidence ids}),
$$

where the orientation and domain of the center are explicit,

$$
\mu_i=\arg\min_{\mu\in\mathcal P_{\mathrm{safe}}}
\sum_{j\in i}\omega_j B_{A^*}(\mu,\mu_j).
$$

Thus $\mu_i$ is a constrained $A^*$-Bregman barycenter, $\Sigma_i$ is its
dispersion/radius, and $\kappa_i$ is precision. Repeated evidence at one nonzero
coordinate leaves $\mu_i$ at that coordinate and increases $\kappa_i$; it does
not replace the destination by $n\theta_i$. Separated clusters remain separated
and are ranked or selected before §7's convex solve. A selected cluster contributes
$\kappa_iB_{A^*}(\mu,\mu_i)$, whose minimizer stays at $\mu_i$ as evidence grows.

The source evidence set is union-by-content-id and idempotent. Each publisher/expedition
has a manifest/service-profile contribution cap; a visit proof contributes once,
a Build contributes once per content id, and personal subscriptions change only
the local ranking. Creator removal or moderation tombstones remove the matching
contribution on deterministic recomputation. Networking, identity proof, and
moderation remain external services; the neutral Model accepts only normalized
records and never opens a socket. These rules make strength historical,
rate-limited, attributable, and removable rather than an irreversible summed
counter.

A cluster becomes an exact destination only when its center's Canonical inversion
is enclosed in one coordinate cell and its metric radius plus numeric error is
below that cell's inscribed Fisher radius. Until then it supplies a diffuse mode,
not an invented exact Impression.

### 11.3 Build attachment and reproduction

A Build is a separate, versioned content-addressed payload attached to an
Impression. Its canonical portion contains the authored construction graph,
quantized dimensions and transforms, attachment points, collision/interaction
tags, semantic material roles, terrain-overlay masks, and referenced content ids.
The Impression supplies $M$, $\hat\theta$, spherical address, relevant Model time,
and a canonical tangent frame. The frame derives `up` from the sphere normal and
`east/north` from a versioned reference meridian, with an explicit pole and seam
tie rule.

Compatible Visualizations must preserve graph connectivity, quantized scale,
attachments, interaction semantics, and semantic material roles. If collision
can alter the shared Traveler path or Egress credit, its quantized collision
geometry and transform are exact inputs to the shared Traveler controller;
Build-format tolerances apply only to rendered placement. Mesh tessellation, shaders, textures,
audio, particles, and permitted animation style may differ. A terrain-modifying
Build is an overlay in Visualization space: loading or removing it never changes
$\mathcal W$, the moment ledger, Scope, Resonance, or Reachability. A Build appears
only as an authoritative interactive object at its associated
$(M,\hat\theta,x,t)$, or under an explicit migration that creates a new anchor.
At another Model State it may be shown only as a noninteractive ghost/preview: it
cannot affect collision, path credit, or loaded-placement Attractor evidence. Its removable evidence record
may increase the precision of the associated Attractor cluster once, as §11.2
specifies.

### 11.4 Dual-space travel

An Attractor may specify both a Possibility region and a World Space location; the
two distances are independent (invariant 3): remaining mixture-curve Fisher
length $L(1)$ in Possibility and great-circle/altitude path length $d_W$ on the
sphere. Coordinated arrival is a rate controller that matches estimated times,

$$
\frac{L(1)}{\text{egress rate}}
\ \approx\
\frac{d_W(x_\star,x_{\text{target}})}{\text{explore speed}},
$$

adjusting rates without identifying the spaces. A World Space destination is
interpreted only with its associated target Model State and chart revision.
Exactness depends on the Attractor's certified precision.

---

## 12. Determinism, identity, and versioning

V3 separates four contracts rather than calling every repeatable hash
"deterministic."

1. **Portable address and identity.** Coordinates, twelve-patch addresses, time,
   record ids, common innovation/slot ids, manifestation ids, and dependency keys
   have normalized integer encodings. Innovation and slot ids exclude
   $\hat\theta$; coordinate-specific manifestations and dependency keys include
   it. Each hash folds only its declared component identity/revision closure, not
   the compatibility root wholesale. Hash fold order and every tie rule are frozen.
2. **Canonical Model operations.** The law/dual/metric solve, normalized Attractor
   clustering, regime ranking, reconciliation, moment ledger, spherical fields, sea level, drainage topology,
   climate forcing, canonical candidates/entities, support quadrature, Resonance,
   and every Egress commit are bit-identical on native and wasm. The operation
   manifest freezes reduction trees, integer widths and overflow behavior, FMA
   contraction, subnormal policy, conversions, intermediate rounding, portable
   `exp`/`log`/`sqrt`/trigonometric tables, CG/preconditioner order, interval
   outward rounding, and the cold refinement path. Fixed iteration count alone is
   insufficient. A permanent result is emitted only when its error/quantization
   enclosure proves the declared result.
3. **Interactive and Visualization approximation.** Preview fields, fewer bands,
   resident-tile Resonance display, decorative organisms, Bures/WFR/Sinkhorn
   morphs, shaders, and live simulation may differ by platform or tier. They never
   choose a committed address, canonical topology/entity, Impression value, Scope
   statistic, or Reachability result.
4. **Settled schedule independence.** A completed snapshot is independent of
   scheduler, worker count, budget scale, cancellation, cache capacity, resource
   tier, and frame subdivision. Different execution choices may change completion
   latency, but never a continuation's frozen inputs, credit, or eventual result.

Bounded operations return `Complete`, `Pending { continuation }`, `Partial {
bounds }` only where the API declares partial semantics useful, or `Unresolved {
reason }`. A Canonical Egress commit, entity identity, or Impression confirmation
is all-or-nothing: `Partial` never crosses those boundaries, and `Unresolved`
retains the prior state. Presentation continuity may degrade without changing the
result.

Versioning is split by meaning:

- the **law identity** covers coordinate layout, $T$, $A$, joint bank, safe set,
  regime atlas, and moment contract; adding a sufficient statistic requires a new
  identity and address schema;
- the **spatial profile** covers the sphere chart and canonical time decoder, the
  **innovation identity** covers only the common random root, and each field,
  topology, or entity kernel has a separate `algorithm_revision` plus its minimal
  upstream revision closure;
- optional derived **capability schemas** may advance without changing old
  results when they consume only existing canonical channels;
- Traveler/navigation policy, record format, Build format, and Visualization each
  have separate versions.

An Impression stores every consumed identity. A changed identity is rejected or
handled by an explicit migration that creates a new Impression; it is never
silently projected or reinterpreted.

---

## 13. Performance hypotheses and kill gates

No timing in this proposal is evidence. Let $k\le48$, $R\le256$, at most
$K_{\mathrm{nav}}$ combined regime/Attractor candidates,
$N_c=12\cdot4^{L_c}$ canonical census cells, and
$B_{\max}$ resident transition cells. Matrix-free law evaluation is plausibly
$O(Rk)$ per metvec, but a navigation commit includes several Newton/CG solves,
mixture-curve quadrature, support quadrature, interval propagation, and sometimes
the cold enclosure path. Spherical realization also adds fixed $O(N_c)$ global
closures. Those costs must be measured together.

One committed native/wasm ledger reports cold and warm latency, allocation, peak
scratch, completion rate, and error/calibration rate for:

| Ledger row | Required contents |
|---|---|
| Package/startup | bank, regime atlas, sphere tables, portable math tables, decode, peak duplicate memory |
| Navigation | every candidate-mode reconciliation, dual inversion, CG/residual history, mixture arclength, enclosure refinements, commit/no-commit result |
| Canonical support and Scope | spherical-cap quadrature, applicability, moment-ledger queries, bounds, zero-population cases |
| Cold spherical state | sea level, coarse drainage, cycle climate, $O(N_cR)$ per responsibility-scaling sweep plus certified fallback, global entity/quota summaries, simultaneous nonlinear moment closure |
| Visible realization | invalidated cells, all dependency layers, topology/entities, native and wasm throughput |
| Continuity | WFR/Bures buffers, optional coarse Sinkhorn, rapid rebase/coalescing, degraded fallback |
| Impression | cold Canonical confirmation of fields, subject, time, statistics, and optional Build anchor |
| Sustained travel | long/fast/turning/revisiting paths, multiple Yearnings, cache plateau, two endpoint buffers, cancellation and tier variants |

Before benchmark results or held-out/playtest labels are opened, a release
candidate freezes a gate manifest naming native/browser machines, corpora,
latency percentiles, completion rates, byte ceilings, and quality thresholds.
They cannot be relaxed after observing results. The first profile starts with:

1. the math spike ($k\le48,R\le256$) produces bit-identical native/wasm commits,
   including adversarial quantization boundaries, and at least 99% of the frozen
   ordinary-intent corpus completes within the predeclared 10 Hz navigation budget;
2. the spherical spike passes seams, poles, antipodes, common-innovation
   correspondence, finite water/drainage, bounded ecology, schema-wide moment
   bounds, entities, and interrupted-transition tests without a global
   blank/reload, with at least 99% **end-to-end Canonical completion** on the
   frozen ordinary spherical corpus (not merely completion of the math solve);
3. every steerable statistic satisfies
   $2\varepsilon_a+\varepsilon_{\mathrm{quant}}<$ its adjacent Scope-band gap,
   and blinded players identify Accentuate, complement Repress, activation Hold,
   conflict, and mode choice at the predeclared rate; and
4. cold/warm sustained travel reaches fixed byte ceilings on native and browser,
   with no latency hidden by frequent `Pending`/`Unresolved` or by incorrect
   visible prevalence.

All solver iterations, census levels, quadrature points, candidate modes,
resident cells, endpoint buffers, and continuation bytes are hard caps in the
manifest. Work does not grow with *explored history*, but cold work does grow with
the chosen fixed census and visible refinement. Until these gates pass, V3 is a
research proposal with a credible complexity shape, not a demonstrated real-time
engine.

---

## 14. Rust realization sketch

The neutral core (no threads, filesystem, or GPU — the crate-boundary rule of
`AGENTS.md`) exposes the free energy and its derivatives; everything else is built
on them.

```rust
/// Stable identity of the law/schema that gives numeric arrays their meaning.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct LawId([u8; 32]);

/// Compact natural parameters of one world-law, encoded in Q24.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Coord { law: LawId, q24: [i32; K] }

/// Only a validated interior mean may enter the Bregman projection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InteriorMeansQ { law: LawId, values: [i64; K] }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SafeMeansQ { law: LawId, values: [i64; K] }

// Checked constructors validate domain and bind every value to one law identity;
// only Canonical Model operations construct InteriorMeansQ and SafeMeansQ.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SphereCell { patch: u8, level: u8, morton: u64 }

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SpherePoint {
    patch: u8,
    u_q48: u64,
    v_q48: u64,
    altitude_cm: i64,
}

// Constructors validate patch < 12, level <= 32, unused Morton bits == 0,
// Q0.48 components < 2^48, altitude bounds, and canonical seam ownership.

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Canonical<T> {
    Complete(T),
    Pending(Continuation),
    Partial { value: T, bounds: ErrorBounds },
    Unresolved(UnresolvedReason),
}

/// Atomic identity/state boundaries cannot carry a Partial payload.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AtomicCanonical<T> {
    Complete(T),
    Pending(Continuation),
    Unresolved(UnresolvedReason),
}

pub trait Model {
    fn law(&self, theta: Coord) -> Canonical<LawEval>; // A, grad A, metvec + bounds
    fn project_mean(&self, mu: InteriorMeansQ) -> Canonical<SafeMeansQ>;
    /// Includes Q24 quantization and a second safe-set enclosure.
    fn dual(&self, mu: SafeMeansQ) -> Canonical<CoordEnclosure>;
    fn snapshot(&self, request: SnapshotRequest) -> Canonical<Snapshot>;
    fn realize(&self, theta: Coord, cell: SphereCell, time: ModelTime,
               layer: Layer) -> Canonical<Tile>;
    fn dep_key(&self, theta: Coord, cell: SphereCell, time: ModelTime,
               layer: Layer) -> u64;
    fn resonance(&self, request: ResonanceRequest) -> Canonical<Resonance>;
    /// Endpoint/correspondence data only; Visualization owns and rebases morph state.
    fn continuity(&self, old: Coord, new: Coord, time: ModelTime,
                  bounds: SphereBounds) -> Canonical<ContinuityDescriptor>;
}

/// One unique target per bounded regime/Attractor mode; separated modes stay separate.
pub fn reconcile_modes(m: &impl Model, request: ReconcileRequest)
    -> Canonical<Vec<ModeTarget>>;

/// Commit only after mixture-path and Q24 enclosures select one portable cell.
pub fn egress_commit(m: &impl Model, request: CommitRequest)
    -> AtomicCanonical<Commit>;
```

Suggested neutral crate split:

```text
world-model-v3-core   fixed point, intervals, hashes, sphere addresses, A/grad/Hessian-metvec
world-model-v3-fields spherical modes/residuals, bounded hash-thinned ecology, physical closures, entities
world-model-v3-ledger equal-area census, applicability, moment closure, Canonical estimators
world-model-v3-nav    regime candidates, convex reconciliation, mixture Egress, clusters, Resonance
world-model-v3-flow   WFR/Bures endpoint/correspondence data — no morph state
world-model-v3-api    capability and Realization contract types
```

The Hessian identity is algebraic, not autodiff. The normal path is matrix-free;
the manifest may name a bounded dense cold fallback for certification. Platform
adapters own executors, files, networking, storage, and rendering.

---

## 15. Conceptual invariants — conformance

| # | Invariant | How V3 satisfies it |
|---|---|---|
| 1 | One point in Possibility = one complete world | $\theta$ is one global world-law; $\mathcal W(M,\hat\theta,S_{R_P}^2,t)$ is one finite complete planet (§2, §6). |
| 2 | One canonical point; nearby content keeps history | one committed $\hat\theta_\star$; common innovations couple nearby ground samples and bounded WFR history is transient Visualization state (§6.2, §10.3). |
| 3 | Possibility and World Space independent metrics | Fisher geometry on $\theta$ (§5, §8) vs. spherical arclength and geodesic transport (§2.3, §10, §11.4). |
| 4 | Egress = Possibility, Exploration = World Space | distinct flows $\theta_\star$, $x_\star$ (§1, §8). |
| 5 | Egress coupled to Exploration but owned by neither | a versioned Traveler policy converts accumulated fixed-point spherical path arclength to $\Delta s=\hat\beta\hat\rho\Delta\ell_W$ (§8.3). |
| 6 | Realization carries stable meaning | integer $\hat\theta$ + versioned observable schema $T$/$\mathcal W$ (§4, §12). |
| 7 | Simulation belongs to Visualization | $\mathcal W$ yields fields/laws only; the Model supplies transport *information*, the Visualization owns the morph (§10.3). |
| 8 | Identical Model inputs reproduce Realization | Canonical $\mathcal W(M,\hat\theta,x,t)$, topology, ledger, and entities are pure portable results or explicit unresolved statuses (§12). |
| 9 | Impression addresses meaningful across Visualizations | the generation/law identity, spherical address/time, common slot/entity, schema values, and bounds are Canonical (§11.1). |
| 10 | Yearnings weighted, order-independent | one-sided request groups reduce canonically; every selected regime/Attractor mode has one strictly convex mean-space minimizer (§7). |
| 11 | Scope = prevalence, not spatial falloff | prevalence is a mean under a declared whole-sphere applicable measure and is checked by the finite moment ledger (§4, §6.3). |
| 12 | Visualization does not change Reachable Possibility | Reachability uses Canonical regime ranking, law geometry, support quadrature, and Traveler policy; morph readiness never enters (§7–10). |
| 13 | Builds are optional Visualization content | a versioned semantic payload attaches in the canonical spherical frame but never enters $\theta$, $A$, $\mathcal W$, Scope, or Reachability (§11.3). |
| 14 | Attractor evidence historical, abuse-resistant, removable | capped id-keyed records form normalized separate clusters; tombstones remove contributions and repeated evidence cannot overshoot (§11.2). |

---

## 16. Open questions from the conceptual model — answered or narrowed

- **What is a Model State / Possibility Coordinate?** The natural parameters
  $\theta$ of an exponential-family world-law, decoded to generator parameters.
  (§2–3)
- **What metric/topology for Possibility?** The Fisher information metric
  $g=\nabla^2A$ — the unique metric invariant under sufficient statistics, up to
  scale (Chentsov), *once the observables are chosen*; KL/Bregman for closed-form
  comparison. (§5)
- **How is continuity risk / sensitivity exposed?** As the spectrum of $g$: large
  eigenvalues are high-susceptibility directions; the prevalence-hull boundary is
  the degeneration locus where $\lambda_{\min}(g)\to0$. (§5, §9)
- **Yearning attributes and Scope across Models?** A versioned observable schema
  $T$ with scalar and prevalence kinds; Scope is a monotone map into interior
  prevalence targets $\mu_a$. (§4, §7)
- **How strongly may Hold resist?** A stiff but finite penalty against the
  activation-time snapshot; overridden only by greater aggregate intent or
  feasibility, never retargeted to the moving current state. (§7.1)
- **Is Resonance threshold / rate / precision?** All three, from
  $\rho=\rho_{\text{support}}\rho_{\text{align}}$: an ecological-support term
  (shared with Option 2) and a law-susceptibility/agreement term (new). (§9)
- **Dual-space coordinated arrival?** A rate controller compares remaining
  mixture-curve Fisher length with great-circle/altitude route length without
  equating the spaces. (§11.4)
- **When does an Attractor become exact?** When a normalized cluster center
  encloses to one coordinate cell and dispersion plus numeric error fits inside
  its Fisher radius; evidence increases precision without moving past the center.
  (§11.2)
- **Continuity risk / chaotic divergence between nearby states?** The
  law/coupled-representative split and common innovation localise it: nearby laws are $\mathrm{KL}$-close,
  the same modes/slots provide correspondence, named topology thresholds may
  still diverge sharply, and $g$ reports susceptibility. (§2.1, §5, §6.2)
- **Which time belongs to the Model?** Integer epoch-relative orbital,
  illumination, tide, seasonal envelope, and phenology forcing; weather,
  behavior, growth history, and replay remain Visualization simulation. (§2.4)
- **What must a Build reproduce?** Semantic graph, quantized placement/scale,
  attachments, interactions/collision, and material roles; presentation styling
  may vary and terrain edits remain overlays. (§11.3)
- **How are distinct causal modes handled?** A bounded versioned regime atlas
  returns separate convex candidates and an ambiguity result; it does not grow an
  open-ended causal grammar. (§7.3)

---

## 17. Relationship to the current implementation and to the sibling proposals

V3 keeps the engineering mechanisms the prototype has proven — integer SplitMix64
hashing and versioned identities; lazy coordinate-derived content; declared
dependencies and dependency-hash-gated integration; bucketed quantisation as the
tile-invalidation event; bounded caches, pools, deterministic scheduling, and
cancellation; explicit determinism grades; the CRDT/atlas sharing laws; and the
neutral/platform crate boundaries with native/wasm verification. It changes the
*semantics*:

| Concern | Current prototype | Proposed V3 |
|---|---|---|
| Point in Possibility | per-region 8-vector + authoritative current state | one global exponential-family world-law $\theta$ |
| World Space | streamed planar regions | one finite twelve-patch equal-area spherical planet + altitude/time |
| Validity | ordered `project_plausible` clamp cascade | intrinsic law validity in a safe convex moment set + certified physical/moment closures |
| Distance | component differences of scalars | Fisher information metric $g=\nabla^2A$ (Chentsov-unique given $T$) |
| Steering | Emphasize-first/Suppress-last blend + raw-bit sort | one-sided intent; separate bounded modes; unique convex target per selected mode |
| Scope | not represented as global prevalence | a mean parameter verified against a finite applicable whole-sphere census |
| Resonance | near-organism density/diversity gate | ecological support × law-susceptibility/agreement |
| Continuity | per-region current/target lerp history | unbalanced-OT transport of living/biome mass; Bures spectral reshaping for abiotic |
| Ecology | habitat-signature roster, ≤12 species | bounded logistic-normal activation over finite candidate/lineage slots + quota-checked marks |
| Attractors | route/anchor weak steering | normalized separate clusters; evidence raises precision without overshooting centers |
| Time / Builds | mostly presentation/runtime concerns | integer canonical forcing; versioned semantic Build attachment remains Visualization overlay |

**Why the sphere does not turn V3 into Option 1.** A finite planet, water
inventory, time forcing, and drainage are baseline Realization obligations, not a
Possibility ontology. Option 1 still addresses a latent cube, decodes it through a
procedural-planet map, and measures a pullback metric at realized probes. V3's
address is a natural parameter, its metric is the Hessian identity
$g=\nabla^2A=\operatorname{Cov}[T]$, its whole-sphere census consumes one bounded
canonical quantization of that law's archetype responsibilities, and its twelve-patch equal-area hierarchy
is neither Option 1's cube map nor its latent decoder. Option 2 remains distinct in
its direct attribute chart and path-dependent spatial wake.

The load-bearing V3 combination is therefore: a coupled exponential family,
natural/mean duality, Fisher/KL navigation, finite moment closure against the same
law means, common-random-number spherical fields, bounded hash-thinned ecology,
and WFR/Bures only for presentation continuity. Spherical geography strengthens
the sampling measure; it does not replace those commitments.

**Contrast with the World Loom.** The World Loom
([`new-world-model-option-4.md`](new-world-model-option-4.md)) is the proposal V3
is most easily confused with — both abandon a generic latent decoder, both invoke
optimal transport, both are honest research architectures — so the difference is
worth stating exactly.

| Concern | World Loom | V3 |
|---|---|---|
| Coordinate | a typed causal-constitution *program packet* (variable size, Merkle-rooted, extensible) | a compact fixed-point *vector* $\hat\theta$ (~40 dims, one global address) |
| Validity | by typed *compilation* + numeric feasibility certificates | membership in the safe convex support of one free energy $A$ |
| Navigation metric | multiscale balanced/unbalanced transport + rewrite lengths + directed Finsler/control length | algebraic Fisher identity $g=\nabla^2A=\operatorname{Cov}[T]$; no transport navigation solve |
| Egress | JKO-inspired proximal probes, bounded active-set/mode and rewrite path search | bounded fixed regime candidates; one convex target and numerically certified mixture geodesic per selected mode |
| Role of optimal transport | **the navigation mechanism** (decides which world is reached) | **continuity presentation only** (transient, discarded, never authoritative) |
| What it optimizes for | extensible typed causal/relational structure with explicit structural paths | compact, identity-checkable navigation of a *fixed* statistical vocabulary |

Neither dominates. V3 cannot add a sufficient statistic or independent physics
without a new law identity and refit. The Loom can add some optional structures
with existing opcodes, but new canonical operations require kernel/application
versioning and some phenomena require a new stratum or major package. In exchange
for its lower ceiling, V3's navigation surface is a fixed bank plus small convex
and linear-algebra kernels rather than transport/rewrite route search. The actual
latency advantage remains a §13 measurement gate, not a conclusion.

Compatibility is neither required nor implied. Current anchors, preserves, routes,
eight-component signatures, and generated regions cannot be reinterpreted as V3
addresses. A migration tool could embed a current observation as a Yearning and
search for a similar V3 law, but the result would be a new Impression, not the same
world.

---

## 18. Risks and honest failure modes

The honesty is distributed through the document; consolidated here so a
decision-maker sees the whole liability in one place. Ordered roughly by how much
of the design each threatens.

1. **The joint archetype bank remains the systemic quality dependency.** $A$, the
   means, Fisher geometry, reachable combinations, regime atlas, and much of the
   moment closure flow from $\{d_r,c_r,\pi_r\}$, $(Q_0,q)$, and $T$. Rank, corpus,
   calibration, and held-out gates can reject a bad bank; no identity proves that
   an accepted bank is plausible, diverse, or fun. Bit-exactly wrong worlds remain
   possible.
2. **The fixed vocabulary is a deliberate ceiling.** A property outside $T$ may be
   derived but cannot become an independent steering direction. Adding a
   sufficient statistic, physical degree of freedom, or regime outside the frozen
   atlas changes the coordinate, free energy, metric, bank, and moment closure. It
   requires a new major law family rather than a compatible append.
3. **Finite moment closure is stronger than asymptotic calibration, not a continuum
   identity.** The law is over idealized archetypal summary atoms; the one planet
   is a coupled mean-matched representative, not a sample with the law's full
   higher-order distribution. Linear census quotas are exact at their declared
   fixed-point target and bounded against the law mean, but nonlinear physical corrections and their bounds
   are fitted/certified machinery. A coarse census can miss rare or localized
   visible structure; aggressive recentering can sterilize terrain or introduce a
   recognizable house style, and an incorrectly certified coupled correction can
   cycle or repair one moment by breaking conservation elsewhere. The final
   simultaneous/idempotence gate must demote a statistic that cannot keep its
   promised bound.
4. **Coupled fields are still synthetic physics.** Cross-covariance,
   nonstationary stress deformation, water conservation, and a dependency graph
   improve causal connection, but they do not prove geophysical fidelity. Shared
   low modes and plate seeds may become recognizable across worlds; topology
   thresholds can still change coasts, rivers, and ranges abruptly.
5. **The bounded regime atlas can miss or misclassify causal alternatives.** It
   prevents known separated modes from being averaged, but it cannot express an
   unforeseen regime. Frequent `AmbiguousModes`, unstable mode hysteresis, or
   semantically bland within-mode compromises would fail the steering gate.
6. **Portable commits require a substantial numeric kernel.** Softmax/dual
   inversion, CG, arclength quadrature, interval rounding, spherical tables,
   moment closure, and adversarial Q24 boundaries all need native/wasm parity.
   Fail-closed `Pending`/`Unresolved` protects identity but can still make travel
   unplayable; same-cell credit retention fixes ordinary subquantum progress but
   does not prove enclosure liveness under every adversarial input. The completion
   rate and latency must be judged together.
7. **The finite planet introduces cold global work and broad invalidation.** A new
   coordinate may require census totals, sea level, drainage, cycle climate, and
   moment closure before local Canonical queries settle. Cache keys include the
   whole coordinate, so continuous Egress may regenerate many visible cells even
   though common innovation preserves correspondence.
8. **Continuity remains presentation, not physical history.** WFR genuinely moves
   living measure in the resident annulus, but commuting/matrix Bures blocks
   and heuristic abiotic spectral blends reshape rather
   than horizontally transports a ridge. Rapid rebasing, cache eviction, and
   topology birth/death can still produce artifacts. The fixed two-endpoint cap
   contains memory; it does not prove visual quality.
9. **The spherical basis and chart are new trusted surfaces.** Equal-area cell
   claims, seam transforms, pole/antipode rules, spherical derivatives, drainage
   adjacency, and canonical area/time measures require independent fixtures.
   A seam bug can corrupt both the visible planet and the statistics used to
   steer it.
10. **The implementation remains research-heavy.** Information geometry,
    interval Canonical math, spherical random fields, global physical closures,
    a moment ledger, bounded finite candidate populations, WFR/Bures, clustering, and Build
    compatibility are more machinery than the current runtime. The small fixed
    navigation ontology is the payoff, not evidence that the whole system is
    simple or fast.

## 19. Acceptance criteria for an implementation

V3 is implementable when a prototype demonstrates, without special cases:

1. every certified safe parameter cell has analytic/interval closure coverage,
   and at least 99% of the frozen ordinary corpus opens a complete finite
   spherical law/planet whose water, drainage flux, entity, conservation, and
   simultaneous moment checks pass; bounded unresolved statuses are reserved for
   the predeclared adversarial corpus rather than counted as success;
2. twelve-patch addresses are unique and neighbor/field/topology results agree at
   every seam, pole, and antipode under adversarial fixed-point tests;
3. Canonical addresses, Egress commits, time forcing, fields, sea/coast and river
   topology, canonical candidates/entities, applicability, and ledger statistics
   are bit-identical on native and wasm;
4. $g=\nabla^2A=\operatorname{diag}(Q_0,0)+\operatorname{Cov}_s[(d,c)]$ matches
   independent derivative fixtures, is positive definite on the safe interior,
   and meets named sign/range gates for both fitted cross-block covariance and
   corresponding realized-planet correlations;
5. $\Pi_\mu$ is typed, unique, and idempotent; every admitted mean has a finite
   enclosed dual; no closed-hull target or natural coordinate is passed to it;
6. for **every** steerable abiotic and biotic statistic,
   $|R_a(\hat\theta)-\nabla A(\theta)_a|\le\varepsilon_a$ on training-independent
   interval-certified parameter cells and training-independent coordinates,
   including rare traits, time-conditioned measures, positive-applicability
   margins, and the declared discrete quota error;
7. common innovation ids and lineage slots remain stable across nearby coordinates,
   while discrete gain/mark/topology events obey named rate and correspondence
   margins rather than an impossible blanket smoothness claim;
8. arbitrary input permutations yield bit-identical mode candidates, targets,
   Resonance, and commits; Accentuate and complement Repress obey their exact
   one-sided thresholds, Hold stays at its activation snapshot, and Disable has no
   effect;
9. each selected regime/Attractor mode has one bounded convex minimizer and a
   corridor containing the current state; separated modes remain separate or
   report `AmbiguousModes`, and repeated evidence raises
   cluster precision without moving its center toward an extreme;
10. adversarial rounding cases either enclose one Q24 commit or fail closed with the
    same continuation/status on native and wasm; exact half ties terminate,
    same-cell attempts retain credit, and no partial result changes state;
11. integer Model time reproduces forcing and time-conditioned Impressions, while
    different Visualization simulation histories leave Canonical results unchanged;
12. compatible Visualizations reproduce Build graph, attachments, interaction,
    and material semantics within visual tolerance, while credited-path collision
    geometry/placement is exact in the shared Traveler controller, and loading or
    removal changes no Model/Scope/Reachability result;
13. WFR living transport, commuting/matrix Bures blocks, and explicitly heuristic
    noncommuting spectral blends hit both presentation endpoints,
    rapid interrupted commits rebase with two bounded buffers, fading-slot capture
    is refused correctly, and transport never alters $\hat\theta_\star$;
14. long, fast, turning, and revisiting travel with multiple Yearnings reaches
    fixed memory ceilings through cache eviction and rapid commits;
15. scheduler, worker count, budget, cancellation, cache capacity, resource tier,
    and frame subdivision do not change settled results or fixed-cadence travel
    accounting;
16. the single §13 cold/warm ledger meets the predeclared native/wasm latency,
    completion-rate, allocation, and memory gates without hiding work in
    `Pending`/`Unresolved`; and
17. held-out quality tests and blinded playtests meet the thresholds frozen before
    labels/results were opened for diversity,
    repeated motifs, physical/ecological failures, nearby-world correspondence,
    visible coast/river/species transitions, and recognition of requested intent.

---

## Appendix A: frozen manifest and decoded parameter blocks

$A$, $T$, the joint bank, sphere profile, and canonical operation rules are
immutable fixtures; the coordinate does **not** decode new coefficients for $A$.
The Realization decoder derives physical/field parameters from the law means and
responsibilities. None of the following is additional Model State.

| Block | Contents | Construction constraint |
|---|---|---|
| Law family | $Q_0\succ0$, $q$, joint bank $\{d_r,c_r,\pi_r\}$, safe polytope, regime atlas | minimal/rank and condition gates; $\pi_r>0$ and $\sum_r\pi_r=1$; cross-block constraints; every regime convex and bounded |
| Observable schema | $T$, units/ranges, affine normalizers, membership, applicability, population/time measure, estimator and $\varepsilon_a$ | $\dim T=\dim\theta=k$; prevalence in $[0,1]$; adjacent Scope bands wider than admitted error |
| Sphere/time | radius, twelve-patch chart/ties, neighbor tables, census level/weights, epoch, rotation/orbit/forcing tables | equal-area/seam fixtures; integer time; portable direction and transcendental tables |
| Common innovation | spherical low modes, needlet bands, archetype residuals $G_r$, amplitudes $\gamma_r$, responsibility denominator $Q_R$, plate/regime seeds, slot/candidate hash domains | hashes exclude $\hat\theta$; KL matrix scaling preserves integer $\hat s_r$ quotas within the declared $s_r$ error; bounded covariance deformation and positive local responsibilities |
| Planet closures | gravity/atmosphere/water ranges, sea-level bracket, integer drainage, climate/hydrology/soil coefficients | inventory/flux conservation, dependency closure, residual and work caps |
| Moment ledger | centered scalar controls, monotone brackets, rank apportionment, coupled I-projection, error budgets | every steerable final statistic meets $|R_a-\mu_a|\le\varepsilon_a$ or is unresolved/demoted |
| Ecology/entities | bounded logistic-normal activation, $L_e$, $\lambda_{\mathrm{cap}}$, candidate/lineage caps, prefix quotas, trophic rules, inhibition, connectivity $\kappa_e$ | finite candidate population; common slot identity; canonical tier-independent population |
| Transport | $\delta_W$, $r_n,r_f$, $B_{\max}$, endpoint-buffer and iteration caps, fallback | $0<r_n<r_f<\pi R_P$; at most two endpoint buffers; presentation only |
| Navigation/numerics | Hold stiffness; Scope table; resonance constants; $K_m$, $K_A$, $K_{\mathrm{nav}}$; operation/reduction/interval manifest | safe interior targets; finite caps; adversarial quantization fixtures; fail closed |

The rate $\hat\beta$ belongs to a separately versioned Traveler policy and replay,
not the Model generation identity. The bank, regime atlas, physical decoder, and
schema are fitted offline and held-out-tested, not proven by the Fisher identities.
An implementation is not conforming until the manifest supplies every value,
ordering, cap, approximation, and rounding rule named here and hashes the consumed
fixtures into the appropriate identity.

---

## Appendix B: one navigation tick

Given committed $\hat\theta_\star$, active Yearnings with Hold snapshots,
normalized Attractor clusters, selected/automatic mode policy, canonical Model
time, and accumulated fixed-point spherical arclength:

1. group one-sided thresholds and weights with checked canonical reductions;
2. rank no more than $K_{\mathrm{nav}}$ combined regime/Attractor candidates;
   return `AmbiguousModes` if the
   policy cannot select separated near-equal candidates;
3. for each candidate solve the strictly convex safe mean-space program and
   enclose its dual target;
4. compute Canonical spherical-cap support, susceptibility, and request-force
   disagreement; unresolved Resonance prevents a commit;
5. set $\Delta s=\hat\beta\hat\rho\Delta\ell_W$ and bracket the mixture-curve
   fraction whose Fisher arclength fits that allowance;
6. run the Canonical dual/enclosure path; commit only if every component selects
   one Q24 cell and the objective monotonicity bound passes, otherwise return a
   deterministic continuation/status without consuming state or travel credit;
7. open the new immutable spherical snapshot and emit continuity endpoint data;
8. Visualization rebases its at-most-two-buffer transient at the next fixed
   presentation tick and chooses refinement/fallback independently.

The commit is a pure function of these explicit inputs. Presentation readiness,
frame cadence, and resource tier cannot change it.

---

## Appendix C: machine-checkable invariants (the goal-4 checklist)

The navigation algebra, given the frozen bank, is checkable against identities and
properties — the concrete hooks an AI maintainer or CI gate can assert:

| Property | Assertion | Maps to |
|---|---|---|
| Metric identity | $g=\nabla^2A=\operatorname{diag}(Q_0,0)+\operatorname{Cov}_s[(d,c)]$, including cross blocks | acc. 4 |
| Metric PD | $g\succ0$ throughout the safe set; prevalence susceptibility falls toward an eroded hull facet | acc. 4 |
| KL = Bregman | $\mathrm{KL}(p_{\theta_1}\Vert p_{\theta_2})=B_A(\theta_2,\theta_1)=B_{A^*}(\mu_1,\mu_2)$ (both forms) | §5 |
| Projection typing/idempotence | $\Pi_\mu(\Pi_\mu(\mu))=\Pi_\mu(\mu)$ in $\mathcal P_{\rm safe}$; natural/mean types cannot mix | acc. 5 |
| Order-independence | permute requests/evidence, re-reduce, assert bit-equal modes, target enclosures, Resonance, and commit | acc. 8 |
| Influence semantics | one-sided Accentuate/Repress thresholds and immutable Hold activation snapshot | acc. 8 |
| Reconciliation uniqueness/modes | one minimizer per selected convex mode; separated modes remain separate or ambiguous | acc. 9 |
| Moment closure | every $R_a$ encloses $\nabla A_a$ within $\varepsilon_a$ on the finite applicable census | acc. 6 |
| Common innovation | phase/candidate/slot ids ignore $\hat\theta$ while manifestation/dependency ids consume it | acc. 7 |
| Egress monotone/enclosed | mixture-segment objective is non-increasing; commit enclosure selects one Q24 cell | acc. 10 |
| Spherical closure | chart seams, area totals, water and drainage flux agree exactly/certifiably | acc. 1, 2 |
| Transition rebase | endpoints and repeated rebases converge with two buffers; $\hat\theta_\star$ unchanged | acc. 13 |
| Portable Canonical result | fields, topology, entities, ledger, Resonance, and commits match native/wasm | acc. 3 |

What is *not* proved by these identities: that the fitted bank, regime atlas,
synthetic physics, visible worlds, and Build/player experience are diverse,
plausible, or fun. Held-out quality and blind playtests own that boundary.

---

## Appendix D: imported mathematics and references

- **Information geometry / exponential families.** Fisher metric $=\nabla^2A=
  \operatorname{Cov}[T]$; mean map $\mu=\nabla A$;
  $A^*(\mu(\theta))=\mathrm{KL}(p_\theta\Vert h)$ relative to the normalized base
  measure; KL $=$ Bregman divergence of $A$; dually flat structure — Amari &
  Nagaoka, *Methods of Information Geometry*; Amari 1998 (natural gradient).
  Uniqueness of the Fisher metric — Chentsov (Čencov); continuous case —
  Ay–Jost–Lê–Schwachhöfer. Marginal convex support (a polytope for finite
  categorical support) and the $\nabla A$ interior bijection — Wainwright &
  Jordan. Mirror descent $\equiv$ natural gradient —
  Raskutti & Mukherjee 2015.
- **Maximum entropy / I-projection.** Jaynes; Csiszár (I-projection, uniqueness,
  Pythagorean). Regularised/soft maxent — Dudík, Phillips & Schapire. Ecological
  precedent — Phillips et al. (MaxEnt SDM); Harte, *Maximum Entropy and Ecology*
  (METE).
- **Optimal transport.** Villani, *Optimal Transport: Old and New*; Brenier;
  Benamou–Brenier; McCann displacement interpolation; Agueh–Carlier and
  Álvarez-Esteban et al. 2016 (Gaussian/Bures barycenter fixed point); Cuturi 2013
  (Sinkhorn); Feydy et al. 2019 (Sinkhorn divergence); Jordan–Kinderlehrer–Otto
  1998 and Otto 2001 (Wasserstein gradient flow). Bures–Wasserstein Gaussian
  closed forms are used only under the commuting/finite-matrix conditions in §10.
- **Unbalanced optimal transport (WFR / Hellinger–Kantorovich).**
  Chizat–Peyré–Schmitzer–Vialard (arXiv:1506.06430, 1607.05816);
  Liero–Mielke–Savaré; Kondratyev–Monsaingeon–Vorotnikov. Dynamic
  continuity-with-source form and the length-scale $\delta_W$ as in §10.
- **Spherical address, random fields, and finite populations.** HEALPix NESTED
  twelve-pixel equal-area addressing — Górski et al. Spherical Matérn covariance/spectral
  density; Whittle--Matérn SPDE and its Laplace--Beltrami spectral reading —
  Lindgren, Rue & Lindström 2011. Spherical harmonics and needlet/localized-frame
  constructions — including Narcowich, Petrushev & Ward — provide the global/local
  basis. V3's ecology uses a finite logistic-normal Bernoulli candidate population,
  not an unbounded Poisson/Cox count law.
  Determinantal/inhibited-process precedent — Macchi; Hough et al.

Determinism caveat (repeated from §12): imported mathematics does not specify a
portable numeric program. Every operation that influences a Canonical field,
topology, statistic, entity, Resonance value, or committed $\hat\theta$ follows
the frozen native/wasm operation-and-enclosure manifest. WFR/Bures/Sinkhorn
presentation may remain platform-approximate because it cannot affect those
outputs.
