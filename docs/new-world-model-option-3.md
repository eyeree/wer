# New World Model Option 3: Possibility as a statistical manifold of world-laws

## Status and purpose

This document proposes a third complete Model for the concepts in
[`conceptual-model.md`](conceptual-model.md), a sibling to
[`new-world-model-option-1.md`](new-world-model-option-1.md) (a latent cube
decoded into a procedural planet) and
[`new-world-model-option-2.md`](new-world-model-option-2.md) (a plane with a
travel-gated relaxation "wake"). It is a design, not a description of landed
behavior; the current prototype is documented in
[`world-model.md`](world-model.md), and references to it are comparisons, not
compatibility requirements.

The name **V3** identifies this proposed contract, not the current value of
`WORLD_ALGORITHM_VERSION`.

Options 1 and 2 converge on a common shape: a compact latent coordinate, a
frozen decoder into generator parameters, an analytic *pullback* metric built by
weighting a Jacobian, and a travel-gated gradient Egress. They differ mainly in
World Space (planet vs. plane), in how the metric weight is chosen, and in the
continuity trick (a reported sensitivity descriptor vs. a lagged-coordinate
crossfade). V3 shares that overall skeleton deliberately — the Egress *step*, the
per-attribute request fusion, and the one-point Impression are the same shape as
Option 2, and this document says so where it happens — but it changes the
*foundation* underneath the skeleton, and two commitments make it its own
proposal.

**The V3 thesis, in one paragraph.** A world is not a bundle of parameters; it is
a *probability law over what a Traveler can observe*. Points of Possibility are
therefore members of an **exponential family of world-laws**, and one convex
**free-energy function** $A(\theta)$ generates the navigation subsystem: its
Hessian *is* the Fisher information metric on Possibility (the unique metric
invariant under sufficient statistics, up to scale, by Chentsov's theorem, once
the observables are fixed — computed as an exact algebraic identity, never a
differentiated probe summary); its gradient *is* the vector of prevalences that
Scope acts on; and it turns Yearning reconciliation into a convex maximum-entropy
program with a unique minimizer. Continuity is handled at a *second* geometric
level by **unbalanced optimal transport** (the Wasserstein–Fisher–Rao metric):
the world's *living and biome content are spatial mass distributions*, so when the
law changes they genuinely *transport and grow* across World Space — forests
spread, species ranges migrate, blooms and extinctions happen in place — rather
than only lagging a coordinate. These two geometries, information geometry for
*navigation* and optimal transport for *continuity*, are kept strictly separate;
they are the two ideas that make V3 its own proposal.

The design has five goals, in addition to the four shared with Option 1:

1. every representable coordinate is a *valid, well-posed probability law* by
   construction (validity is membership in a convex moment set, not a clamp);
2. Scope/prevalence is a *first-class coordinate* — the thing a Yearning asks for
   is literally a mean coordinate of the world-law, so it cannot be a spatial
   falloff even in principle;
3. Yearning reconciliation is a *strictly convex program with a unique minimizer*,
   so the destination is well-defined and, given a canonical input reduction,
   order-independent;
4. the navigation algebra, *given the frozen parameter bank*, reduces to small
   closed-form operations that an AI maintainer can check against identities
   (metric $=\nabla^2A$, KL $=$ Bregman, idempotent projection) rather than
   against a screenshot; and
5. continuity is a physical *transport* of realized living content with a provable
   per-frame cost bound.

Goal 4 is deliberately scoped: the algebra is identity-checkable, but the frozen
parameter bank that instantiates $A$ is a *fitted* object validated by
world-quality tests, not by an identity (§3.4, §13).

---

## 1. Overview of the construction

The Model is the tuple

$$
\mathfrak M=\big(M,\ \mathcal D,\ A,\ \varphi,\ g,\ \mathcal W,\ \Pi\big),
$$

| Symbol | Name | Role | § |
|---|---|---|---|
| $M$ | Model identity | family + version + public seed; scopes every hash | 2.2 |
| $\mathcal D$ | Decoder | maps the compact coordinate to the coefficients of $A$ and the field hyperparameters | 3 |
| $A$ | Free energy (log-partition) | the single generating function of the navigation subsystem | 3 |
| $\varphi$ | Observables / mean map | $\varphi(\theta)=\nabla A(\theta)$: the prevalences Yearnings act on | 4 |
| $g$ | Information metric | $g(\theta)=\nabla^2A(\theta)$: the (Chentsov-unique) metric on Possibility | 5 |
| $\mathcal W$ | Realization | the World Space fields/organisms drawn from the law | 6 |
| $\Pi$ | Feasibility projection | Bregman projection onto the valid moment set | 3.3 |

The running coordinate $\theta$ ranges over the manifold $\Theta$; it is the
**Model State** (§2.2), carried over the static structure $\mathfrak M$, not a
component of it.

Around this the **Traveler** carries a canonical coordinate $\theta_\star(t)$
moved by **Egress** (§7–8), a World Space position $x_\star(t)$ moved by
**Exploration**, and a derived **transition state** that morphs the realized
world along an optimal-transport path (§10) — never an authoritative coordinate.

The pipeline for one navigation tick, with the Model/Traveler boundary marked:

```text
Yearnings (Impressions + Influence + Scope + weights)  +  community Attractors
        │
        ▼  MODEL: canonical reduction to aggregates (π, μ̄, θ_A)  →  one convex program
   reconciled target law  θ⁺ = argmin over the marginal polytope      (§7)
        │
   MODEL: resonance ρ (susceptibility + ecological support)           (§9)
        │
        ▼  TRAVELER POLICY: step length Δs = β · ρ · ‖Δx_traveler‖     (§8.3)
   θ⋆ ← Π( θ⋆ + Δs · û(θ⋆→θ⁺) )              (canonical world-law advances)
        │
        ▼  VISUALIZATION drives, MODEL supplies geodesic data:
   unbalanced optimal transport on the streaming annulus              (§10)
     ecology/biome measures: TRANSPORT across World Space + GROW/FADE (WFR)
     terrain/climate fields:  in-place spectral reshaping (Bures)
        │
        ▼  deterministic, lazy, cached by quantization bucket
   𝒲( θ̂, x )  →  terrain, climate, hydrology, soils, biome, food web, organisms
```

The reconciliation and Resonance are Model math (information geometry, no
Visualization dependence, satisfying invariant 12). The step-length coupling
$\Delta s=\beta\rho\lVert\Delta x\rVert$ is a **Traveler policy** (invariant 5);
$\beta$ is a game-feel dial, not part of Model identity. The transport is a
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

### 2.1 A world is a law over observations

Fix a set of **observable features** $T(x)=(T_1,\dots,T_k)$ — real functions of
"a thing a Traveler can encounter" $x$ (a patch of terrain, a climate sample, an
organism). Examples: local relief, aridity, drainage density, canopy fraction,
an organism's body-scale, branching count, hue. A **world-law** is the
probability distribution of $T$ that the Traveler would measure by sampling the
whole world — the world's *statistical fingerprint*.

V3 restricts world-laws to a **minimal exponential family**

$$
p(x\mid\theta)=h(x)\,\exp\!\big(\langle\theta,T(x)\rangle-A(\theta)\big),
\qquad
A(\theta)=\log\!\int h(x)\,e^{\langle\theta,T(x)\rangle}\,dx,
$$

with $\theta\in\Theta\subseteq\mathbb R^{k}$ the **natural (canonical)
parameter** — the compact Possibility Coordinate — $h$ a fixed base measure, and
$A$ the **log-partition** (equivalently **free energy**), convex, and strictly
convex because the family is minimal (§3). Because the family is minimal, the
number of natural parameters equals the number of sufficient statistics: **$\dim
\theta=\dim T=k$**, and the mean map below is a map $\mathbb R^k\to\mathbb R^k$.
Richer *presented* attributes a Visualization might show are deterministic
functions of these $k$ coordinates, not additional sufficient statistics.

This is exactly the Jaynes maximum-entropy law: among all distributions with
prescribed feature averages, $p(\cdot\mid\theta)$ is the least-committal one, and
$\theta$ are the Lagrange multipliers. It is also the object macro-ecology already
uses — Harte's Maximum-Entropy Theory of Ecology and the MaxEnt
species-distribution models of Phillips, Dudík and Schapire derive the prevalence
of traits across a landscape as an exponential-family maximum-entropy law over
ecological sufficient statistics. V3 makes that the definition of a world.

Crucially, the *law* is the shareable, navigable meaning of the world; the
*specific terrain a Traveler walks on* is one deterministic **sample** of that
law, fixed by hashing $\theta$ (§6.2). The law is what is close between nearby
worlds; the sample is what can differ sharply through emergence. This split —
law for meaning, sample for the ground underfoot — is what lets V3 satisfy both
halves of the conceptual model's continuity clause at once: *"nearby points in
Possibility produce related Realizations"* (nearby laws are $\mathrm{KL}$-close)
while *"emergent or chaotic behaviour may still create sharp local differences"*
(one sample of a close law can still be locally very different).

### 2.2 The coordinate and the Model State

The **Model State** is exactly $(M,\theta)$; everything else is derived. $M$ is
the Model identity — family, major version, minor version, and a public $128$-bit
world-family seed — folded into every hash domain (as in Option 1's $M$). A
practical first instance uses $k\approx 40$ natural parameters, split into a
**prevalence block** $\theta_{p}$ (bounded traits — §3.2) and a **scalar block**
$\theta_{s}$ (unbounded magnitudes), grouped to mirror the prototype's eight
possibility domains but giving each domain a small sub-vector rather than one
scalar (Appendix A).

For determinism the coordinate is stored in fixed point,

$$
\hat\theta_j=\big\lfloor 2^{B}\,\theta_j\big\rceil\in\mathbb Z,\qquad B=24,
$$

(round-to-nearest, ties by a fixed rule). $\hat\theta$ is the portable identity of
a world. This mirrors the prototype's $Q=4096$ possibility quantisation but
promotes it from a per-region device to *the* single global world address and
widens it so the metric geometry of §5 has room to express fine navigation.

### 2.3 Dual coordinates: natural $\leftrightarrow$ mean

Because $A$ is convex, it has a Legendre–Fenchel dual
$A^*(\mu)=\sup_\theta[\langle\theta,\mu\rangle-A(\theta)]$, and the two are
conjugate: $\mu=\nabla A(\theta)$, $\theta=\nabla A^*(\mu)$, $A^{**}=A$. $A^*$ is
the **negative entropy of the law relative to the base measure** $h$ (equal to
$-H(p_\theta)$ up to the base-measure constant; exactly $-H$ when $h$ is uniform).
The two coordinate systems have direct meaning:

- **Natural coordinates $\theta$** are the *knobs* — what Egress moves.
- **Mean (expectation) coordinates $\mu=\nabla A(\theta)=\mathbb E_\theta[T]$**
  are the *prevalences* — the average of every observable across the world. Scope
  acts here, directly (§4, §7).

This duality is the engine of the whole design: it reappears in the metric (§5),
in reconciliation (§7), in Egress (§8), and in Attractors (§11), each a different
reading of the same $A$/$A^*$ pair.

### 2.4 Theoretical, Representable, Reachable

- **Theoretical Possibility** is the natural-parameter domain
  $\Theta=\{\theta:A(\theta)<\infty\}$, convex.
- **Representable Possibility** is the fixed-point lattice
  $\hat\theta\in2^{-B}\mathbb Z^{k}\cap\Theta$.
- **Reachable Possibility** from $\theta_0$ is the set connected to $\theta_0$ by
  an admissible Egress path (§8) that stays in $\Theta$ and everywhere has
  positive Resonance (§9). Generally a proper, path-connected subset.

A fourth, physically meaningful set is specific to the law view: the **marginal
polytope**

$$
\mathcal P=\{\mu:\mu=\mathbb E_p[T]\text{ for some }p\}=\operatorname{conv}\{T(x)\},
$$

the set of *achievable prevalences*. $\nabla A$ is a bijection from $\Theta$ onto
the **interior** of $\mathcal P$ (Wainwright–Jordan). The prevalence directions of
$\mathcal P$ are bounded (a trait's average lies in a fixed hull, §3.2); the
scalar directions are unbounded. On the *prevalence* boundary — a trait pushed to
a degenerate extreme (perfectly pervasive, or perfectly absent) — the natural
parameter diverges and the law becomes deterministic, so $g=\operatorname{Cov}[T]$
*degenerates* ($\lambda_{\min}\!\to\!0$) in that direction. This boundary is not a
wall to clamp against; it is exactly where Resonance collapses (§9), giving the
game a principled reason why "make everything the same" is unreachable.

---

## 3. The free energy and the feasible manifold

### 3.1 One function generates the navigation subsystem

From $A$ alone the Model gets its mean map, its metric, and its reconciliation
objective. V3 therefore specifies $A$ as a fixed, versioned, convex closed form.
So that the prevalence directions have a genuine bounded boundary while the scalar
directions stay well-conditioned, $A$ is built in two blocks:

$$
A(\theta)=\underbrace{\tfrac12\,\theta_s^\top Q_0\,\theta_s+\langle q,\theta_s\rangle}_{\text{scalar block }A_s(\theta_s)}
\;+\;\underbrace{\log\!\sum_{r=1}^{R}\pi_r\,\exp\!\big(\langle\theta_p,c_r\rangle\big)}_{\text{prevalence block }A_p(\theta_p)} .
$$

- The **scalar block** is a mild convex form with $Q_0\succ0$ (frozen), modelling
  unbounded magnitudes (relief energy, aridity level). $Q_0$ is the metric floor,
  and it acts *only* on scalar coordinates.
- The **prevalence block** is a categorical/softmax log-partition over a frozen
  bank of $R$ **world archetypes** $c_r$ (reference prevalence configurations)
  with priors $\pi_r>0$. It has **no additive quadratic**, so its mean map lands
  in the bounded hull $\operatorname{conv}\{c_r\}$ and can genuinely saturate at
  the hull boundary.

Both blocks are $C^\infty$ and convex, so $A$ is convex with closed-form
derivatives. Writing $s_r(\theta_p)=\pi_r e^{\langle\theta_p,c_r\rangle}/\sum_{r'}
\pi_{r'}e^{\langle\theta_p,c_{r'}\rangle}$ for the softmax weights,

$$
\nabla A=\big(Q_0\theta_s+q,\ \textstyle\sum_r s_r c_r\big),
\qquad
\nabla^2A=\begin{pmatrix}Q_0&0\\[2pt]0&\ \operatorname{Cov}_{s}[c]\end{pmatrix},
\quad
\operatorname{Cov}_{s}[c]=\sum_r s_r c_rc_r^\top-\Big(\sum_r s_r c_r\Big)\Big(\sum_r s_r c_r\Big)^\top .
$$

The metric is $Q_0$ on the scalar block (a permanent positive-definite floor,
well-conditioned navigation) and the softmax covariance of the archetype bank on
the prevalence block. As the softmax concentrates on a single archetype (a trait
driven to the hull boundary), $\operatorname{Cov}_s[c]\to0$ and the metric
degenerates *there and only there* — the singular boundary §2.4 needs.
Crucially, **the metric is an algebraic identity in $\theta$**: no autodiff, no
probe set, no finite differences. (This is the single line separating V3's metric
from its siblings; see §5 and §17 for the precise contrast.)

### 3.2 Validity is intrinsic

There is no plausibility clamp cascade. Every $\theta\in\Theta$ yields a valid
law, and every reachable prevalence $\mu_p=\sum_r s_r c_r$ lies in the interior of
$\operatorname{conv}\{c_r\}$ by construction. Physical relationships between
attributes ("no large animals without productivity", "no vegetation without
water") are encoded once, as *correlations in the archetype bank*: an archetype
with high morphology prevalence also has high productivity, so the softmax cannot
place mass on "giant animals, dead world." Infeasible combinations simply are not
in $\mathcal P$, so the flow never reaches them and never has to be projected away
from them. This is validity-by-construction — a different mechanism from, not a
correction of, Option 2's triangular parent-gated chart, which achieves the same
property a different way (§17).

### 3.3 Residual projection $\Pi$

Inputs that arrive off-manifold — an Impression from a newer Model minor version,
a hand-edited coordinate, quantisation drift after an Egress step — are mapped
back by the **Bregman (information) projection** onto the closed feasible set,

$$
\Pi(\mu)=\arg\min_{\mu'\in\overline{\mathcal P}}\ B_{A^*}(\mu',\mu),
$$

which is idempotent, $\Pi(\Pi(\cdot))=\Pi(\cdot)$, and — because $\overline{\mathcal P}$
is convex and $A^*$ strictly convex — has a *unique* value. It is the same
fixed-point property the prototype proves for its $\Pi$, but here it is a
projection onto a genuine convex set of laws whose uniqueness is a one-line
convexity argument an agent can check.

### 3.4 The archetype bank is a fitted fixture, not an identity

The bank $\{c_r,\pi_r\}$, the scalar form $(Q_0,q)$, and the observable schema $T$
together determine $A$, hence the mean map, the metric, the feasible polytope, and
(via §6.3) the realized prevalences. Choosing $T$ and $\{c_r\}$ *is* choosing the
geometry — Chentsov's uniqueness (§5) applies only *after* $T$ and $A$ are fixed,
so this is as much a design surface as Options 1/2's weight matrices, merely
relocated into a bank of frozen data. It is fitted once, offline, from a corpus of
accepted worlds (bootstrapped from hand-authored seed worlds, then grown), subject
to: $Q_0\succ0$; archetypes spanning the desired feasible worlds with the physical
inequalities baked in as correlations; prevalence ranges matching the Scope band
$[\tau_{\min},\tau_{\max}]$; and the LGCP calibration of §6.3. **Its correctness is
validated by held-out world-quality tests, not by an identity** — this is the part
of the system goal 4 does *not* cover, and it is stated plainly rather than hidden
behind "the metric is handed to us." Once fitted, the bank is hashed into the
Model major identity and becomes an immutable fixture (Appendix A).

---

## 4. Observables — the Realization/attribute contract

The Model exposes the fixed **observable schema** $T=(T_1,\dots,T_k)$ and the
**mean map** $\varphi(\theta)=\nabla A(\theta)=\mathbb E_\theta[T]$ — one language
for three subsystems: what Yearnings push on, what Impressions capture, and the
coordinates of the feasible marginal polytope.

Two attribute *kinds* matter for Yearnings, distinguished as in the conceptual
model:

- **Scalar attributes** name a world-wide magnitude (sea fraction, mean warmth) —
  a scalar-block mean.
- **Prevalence attributes** name *how widespread* a trait is across the world's
  species or regions. This is where V3 is structurally clean: a prevalence *is*,
  by definition, a prevalence-block mean $\mu_a=\mathbb E_\theta[T_a]\in[0,1]$ —
  the fraction of the sampled population expressing the trait. Scope therefore
  cannot be a spatial falloff even in principle (invariant 11); it targets a
  coordinate of $\theta$'s image. "Make branching plants pervasive" sets a target
  for the mean of the branching-indicator statistic.

**Query and accuracy contract.** The Realization is queried through immutable
snapshots (the trait surface of §14) at three accuracy tiers, exactly as Option 1
specifies: **Preview** (fewer octaves/iterations), **Interactive** (bounded error
against a supplied tolerance), and **Canonical** (fixed octave counts, fixed
solver iterations, canonical rounding, portable transcendentals — the reference
result an Impression commits, §12). Every approximate result carries
componentwise error bounds and an integer dependency key; refinement narrows the
bounds without changing canonical identity. A Visualization declares which
attribute groups and accuracy tiers it consumes; a Model/Visualization pair is
compatible iff the consumed groups are present at compatible versions. Adding
statistics to $T$ is backward compatible; changing a statistic's meaning bumps the
Model major version (§12).

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
coordinatise observations, *once the observable schema $T$ is chosen*. The
siblings both *build* a metric and must choose its weighting; V3 does not choose a
metric at all, only the observables and $A$ (§3.4), after which the metric is
determined and its only free constant is an overall scale (a game-feel dial for how
"far" a given amount of world-change feels).

Local distance is the metric length $d_g(\theta,\theta+\delta)^2\approx\delta^\top
g(\theta)\delta$, and the phenomenon the conceptual model asks for falls out: a
small numeric move where $\operatorname{Cov}[T]$ is large (the law responds
strongly) is metrically *far*; a large numeric move in a flat direction of the
covariance is metrically *near*. High-sensitivity / continuity-risk directions are
the large-eigenvalue directions of $g$, read off for free.

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

Cost: $g$ is $k\times k$ with $k\lesssim48$, block-diagonal (§3.1), assembled in
closed form. Egress needs $g^{-1}\nabla$ against it; §8 and §13 give an exact
low-rank / matrix-free form so the solve is cheap.

---

## 6. Realization: the World Space field drawn from the law

### 6.1 The law fixes the *statistics*; a stack of fields realizes them

Given a coordinate $\theta$ and a World Space position $x\in\Omega$ (a plane in V3;
the construction is stated on a generic domain $\Omega$ and transfers to a sphere
$S^2$ for a planet later), the world is a deterministic stack of fields

$$
\mathcal W(\theta,x)=\big(z,\ \kappa,\ h,\ u,\ b,\ e,\ \Lambda\big):\ \
\text{terrain}\to\text{climate}\to\text{hydrology}\to\text{soils}\to\text{biome}\to\text{food web}\to\text{organisms},
$$

the same dependency chain the overview lists and the prototype implements. The
possibility input is the *global* $\theta$, so there is exactly one world; spatial
structure comes entirely from $x$.

Each abiotic layer is a **Gaussian random field** whose second-order law is
decoded from $\theta$. V3 uses the **Matérn** family because it is the standard
two-and-a-half-parameter model of a spatially correlated field and because its
distances have closed forms (§10). Its covariance and spectral density are

$$
C(r)=\sigma^2\,\frac{2^{1-\nu}}{\Gamma(\nu)}(\kappa r)^\nu K_\nu(\kappa r),
\qquad
S(k)\ \propto\ \big(\kappa^2+\lvert k\rvert^2\big)^{-(\nu+d/2)},
\qquad
\kappa=\frac{\sqrt{2\nu}}{\ell},
$$

with variance $\sigma^2$ (relief energy), range $\ell$, and smoothness $\nu$
decoded smoothly from $\theta$. The Whittle–Matérn SPDE representation
$(\kappa^2-\Delta)^{(\nu+d/2)/2}(\tau z)=\mathcal W_{\!wn}$ (Lindgren–Rue–Lindström,
$\mathcal W_{\!wn}$ white noise) gives a local Markov characterisation and the
*offline* ground-truth path (§6.2).

### 6.2 Law versus sample: lazy, deterministic synthesis

The *law* (the decoded Matérn spectrum) is what navigation uses. The *sample* the
Traveler walks on is produced lazily by **summed hashed-gradient noise tuned to
that spectrum**. A multi-octave fBm with octave frequencies $f_i=f_0L^i$
(lacunarity $L$) and amplitudes $a_i=r^i$ has Hurst regularity $H$ when the octave
gain satisfies $-\ln r/\ln L=H$; to match a Matérn field of smoothness $\nu$, set
$H=\nu$ (i.e. $r=L^{-\nu}$) and $f_0\approx\kappa/2\pi$. The realized field then has
the Matérn power-spectrum exponent $2\nu+d$ (the amplitude-slope $-2\ln r/\ln L=2\nu$
plus the spatial dimension $d$). The primitive is the same hashed-gradient noise
as the prototype's `terrain.rs`, so per-sample cost is $O(1)$, bit-deterministic,
and cross-platform by the existing rules.

Two honest limits. (i) The correspondence is **stationary and isotropic**: a
domain-warp can modulate range and grain slowly across the world, but a fully
nonstationary SPDE covariance has *no* $O(1)$-per-sample lazy certificate — the
lazy path is scoped to (locally) stationary Matérn, and this is also what the
closed-form transport of §10 requires. (ii) The match is asymptotic in the
high-frequency slope and the range; circulant-embedding and FEM/GMRF solves are
the offline "blessing" path that certifies the stationary match and never run
per-frame.

This is the concrete meaning of "law for navigation, sample for the ground": a
small Egress step reshapes the whole spectrum smoothly, while whether any given
ridge survives is a property of the sample and may change sharply — emergence,
exactly as intended.

### 6.3 Organisms and vegetation as point processes

The living layers are **marked point processes** whose intensity is decoded from
the law. Placement uses a **log-Gaussian Cox process**: intensity
$\lambda(x)=\exp(Z_\theta(x))$ for a Gaussian suitability field $Z_\theta$, so
dense forests and sparse steppes are the high/low-intensity regimes of one field.
Individual organisms are a deterministic hash-thinning of $\lambda$ (accept a
candidate at $p$ iff $\mathrm{hash}(p)/2^{64}<\lambda(p)/\lambda_{\max}$); marks
(species, body-scale, hue) are hashed samples from the law's trait distribution.
Regular inhibited spacing uses a hash-based Poisson-disk proxy for a determinantal
point process (exact DPP sampling is $O(N^3)$ and sequential).

The intensity is a genuine **spatial mass distribution over World Space** — which
is what makes the continuity transport of §10 physical for the living layers.

**The prevalence tie is a calibrated constraint, not an automatic identity.** The
whole-world trait prevalence
$\int_\Omega\lambda(x)\pi_a(x)\,dx/\int_\Omega\lambda(x)\,dx$ and the law's mean
$\mu_a=\nabla A(\theta)_a$ are properties of two *separately decoded* objects (the
LGCP intensity and the free energy $A$). Equality is a **decoder moment-matching
constraint** the fitting step (§3.4) must impose — that the LGCP trait marginals
reproduce $\nabla A$ across $\Theta$ — and it holds only in the large-domain,
large-$N$ limit: a finite hash-thinned sample gives an $O(1/\sqrt N)$ estimate,
the lazy-fBm variance is slightly biased (§6.2), and $\mathbb E[\lambda\pi]/
\mathbb E[\lambda]$ equals the ratio of expectations only ergodically. So the tie
is a calibrated, harness-verified equality within a stated tolerance (acceptance
criterion 3), not "the same number by construction." It is still the tightest tie
any of the three proposals draws between what a Yearning asks for (a mean
parameter) and what the Traveler counts on the ground.

### 6.4 Determinism and caching by bucket

$\mathcal W$ never consumes a live float $\theta$; it consumes the bucketed
$\hat\theta$. A *tile* is a pure function of
$(\hat\theta,\text{region},\text{layer},\text{version})$, hashed to a dependency
key as world-model.md §2.6 does, with the global $\hat\theta$ replacing the
per-region vector, so sub-bucket Egress advances the coordinate, metric, and
Resonance while regenerating nothing. The dependency-hash graph, per-layer
`algorithm_revision`, cache ceilings, and farthest-first eviction carry over
unchanged; only the *inputs* to the hash change.

---

## 7. Yearnings → a reconciled target law

Reconciliation produces a single **target law** by a strictly convex program whose
inputs are order-independent aggregates. This unifies what §8 moves toward — there
is one objective and one destination, with community Attractors folded in.

### 7.1 Per-attribute requests

Each active Yearning $y$ has weight $w_y>0$, source Impressions, and for each
usable attribute $a$ an Influence intention and a Scope level, emitting at most one
soft request — a target mean $\bar\mu_{y,a}$ and a precision $\pi_{y,a}\ge0$:

| Influence | target mean $\bar\mu_{y,a}$ | precision $\pi_{y,a}$ |
|---|---|---|
| **Accentuate** | Scope-implied prevalence, biased **above** the captured value | $w_y$ |
| **Repress** | Scope-implied prevalence, biased **below** the captured value | $w_y$ |
| **Hold** | the *current* prevalence $\mu_a(\theta_\star)$ | $w_y\,\eta_{\text{hold}}$ (stiff) |
| **Disable** | — | $0$ |

Scope maps to a target through a fixed monotone table
$\text{singular}\to\text{common}\to\text{pervasive}$, kept strictly interior —
$\tau(s)=\tau_{\min}^{1-s}\tau_{\max}^{s}$ with $0<\tau_{\min}<\tau_{\max}<1$ (so
"pervasive" asks for, say, $80\%$, never $100\%$; the last approach to totality is
where Resonance vanishes, §9). Hold and Disable are distinct as the conceptual
model requires: Disable contributes nothing ($\pi=0$); Hold pins the attribute to
its current mean with a stiff precision that *competes* with other requests rather
than dominating them.

The per-attribute request table and the fusion below are the **same shape as
Option 2** — this is a shared mechanism, stated as such.

### 7.2 A convex maximum-entropy program (order-independent)

Aggregate requests on each attribute by precision-weighted fusion, and community
Attractors by their pseudo-count natural parameter $\theta_{\!A}$ (§11.2):

$$
\pi_a=\sum_y\pi_{y,a},
\qquad
\bar\mu_a=\frac{\sum_y\pi_{y,a}\,\bar\mu_{y,a}}{\pi_a}\ (\pi_a>0),
\qquad
\theta_{\!A}=\sum_i n_i\,\theta_i .
$$

These reductions are **commutative and associative in exact arithmetic**, but IEEE
float addition is not associative, so V3 accumulates each contribution in fixed
point (or in a canonical order keyed by Yearning content-id) *before* the solve.
Given the deterministic aggregates, the reconciled target is the minimizer, over
the marginal polytope, of the strictly convex objective in **mean coordinates**:

$$
\mu^{+}=\arg\min_{\mu\in\overline{\mathcal P}}\ \Big[
\underbrace{B_{A^*}\!\big(\mu,\mu(\theta_\star)\big)}_{\text{KL to the current law}}
+\underbrace{\tfrac12\sum_a\pi_a\big(\mu_a-\bar\mu_a\big)^2}_{\text{soft moment fit}}
-\underbrace{\langle\theta_{\!A},\mu\rangle}_{\text{attractor pull}}\Big],
\qquad \theta^{+}=\nabla A^*(\mu^{+}).
$$

This objective is **genuinely strictly convex**: $B_{A^*}(\cdot,\mu_\star)$ has
Hessian $\nabla^2A^*=g^{-1}\succ0$ on $\operatorname{int}\mathcal P$, the moment
term is a convex quadratic, and the attractor term is linear; over the convex set
$\overline{\mathcal P}$ the minimizer $\mu^{+}$ exists and is unique for *any*
requests, including contradictory ones. Order-independence is then a genuine
theorem — a unique minimizer of a strictly convex function does not depend on the
order in which the (deterministically reduced) terms were formed — and it is
machine-checkable: permute the Yearning list, re-reduce in the canonical order,
re-solve, assert bit-equality of the quantised $\hat\theta^{+}$.

Posing the fit in mean coordinates matters. The earlier natural-coordinate form
$\tfrac12\sum_a\pi_a(\nabla A(\theta)_a-\bar\mu_a)^2$ is a nonlinear least-squares
penalty whose exact Hessian carries an indefinite third-derivative term; it is
*not* convex for large residuals. The mean-coordinate form above is convex by
construction. Two honesties about how this relates to Option 2: the **moment-fit
term** is a precision-weighted quadratic in mean coordinates — the same shape as
Option 2's weighted least squares (Option 2's attributes are also prevalences), so
the *compromise between conflicting Accentuate/Repress requests is Euclidean in
mean space, as in Option 2*. What is genuinely different is (i) the **proximal
term is the exact $\mathrm{KL}$** to the current law, not a coordinate-distance
penalty; (ii) the feasible set is the **marginal polytope**, so "Model validity
takes precedence over literal satisfaction" is automatic — an impossible
combination is simply not in $\mathcal P$ and the flow settles at the
$A^*$-Bregman-closest feasible law; and (iii) the whole thing is one **convex
program with a unique minimizer**, where Option 2 relies on a sequential
Emphasize-first/Suppress-last blend that needs a canonical raw-bit sort to recover
order-independence. V3 still needs canonicalization — but only of the *input
reduction*, not of the blend.

---

## 8. Egress dynamics

Egress is a single Resonance- and travel-gated step from the current coordinate
$\theta_\star$ toward the reconciled target $\theta^{+}$ of §7. There is one
objective and one destination; §8 is only the commit.

### 8.1 The step direction

The step ascends toward $\theta^{+}$ in the information metric. The metric-
normalized direction is

$$
d=g(\theta_\star)^{-1}\,\nabla_\theta\Big[-\tfrac12\,\mathrm{KL}\big(p_{\theta}\Vert p_{\theta^{+}}\big)\Big]\Big|_{\theta_\star}
\ =\ \tfrac12\big(\theta^{+}-\theta_\star\big),
\qquad
\hat d=\frac{d}{\lVert d\rVert_g},
$$

using the exponential-family identity
$\nabla_\theta\mathrm{KL}(p_\theta\Vert p_{\theta^{+}})=-g(\theta)(\theta^{+}-\theta)$,
so the natural-gradient direction of the proximal objective is *exactly* the
displacement toward $\theta^{+}$ (the scalar $\tfrac12$ is absorbed by
normalization). No separate utility is ascended — the attractor pull and the
stay-near-current proximal term already live inside the §7 program that produced
$\theta^{+}$.

### 8.2 Computing $\nabla A^*$ and the solve

Two quantities need the metric: forming $\theta^{+}=\nabla A^*(\mu^{+})$ (inverting
the mean map) and taking the metric-normalized step. Neither has a closed form for
a softmax family; both are computed by damped Newton iterations
$\theta\leftarrow\theta-g(\theta)^{-1}(\nabla A(\theta)-\mu)$ at a fixed iteration
budget. Each iteration assembles $g$ and solves a $k\times k$ system, so — stated
honestly — the mirror/dual route costs *several* metric solves, not zero; it does
**not** avoid the solve. Its genuine benefit is that $\nabla A^*$ lands inside the
marginal polytope, so the target stays feasible without a separate projection in
the interior (near the boundary, $\Pi$ of §3.3 or a damped step guards it — this is
not projection-free unconditionally). The Raskutti–Mukherjee equivalence (mirror
descent with mirror map $A$ $\equiv$ natural-gradient descent on the dual manifold)
is used here as a *correctness identity* for the dual step, not as a cost argument.

Because $g$ is $Q_0\oplus\operatorname{Cov}_s[c]$ and the prevalence block is
$\operatorname{Cov}_s[c]=C(\operatorname{diag}s-ss^\top)C^\top$ with
$C=[c_1\cdots c_R]$, the solve uses the matrix-free low-rank form: each
metric–vector product is $O(Rk)$ and a fixed-iteration conjugate gradient avoids
forming or factoring the dense Hessian (§13).

### 8.3 Resonance- and travel-gated step (Traveler policy)

The step length couples Egress to Exploration and gates it by Resonance
(invariants 4, 5; conceptual model §Egress capability and resonance). This
coupling is a **Traveler/gameplay
policy**, not Model structure — the Model exposes only $\hat d$ and $\rho$:

$$
\Delta s=\beta\,\rho\,\max\big(\lVert\Delta x_\star\rVert,0\big),
\qquad
\theta_\star\leftarrow\Pi\!\big(\theta_\star+\Delta s\,\hat d\big),
$$

with $\Delta x_\star$ the Traveler's World Space displacement this frame,
$\rho\in[0,1]$ Resonance (§9), and $\beta$ a **tunable game-feel rate** owned by
the Traveler layer — *not* hashed into Model identity. Zero travel or zero
Resonance gives exactly zero Egress. This is the prototype's
$\text{travel}\times\text{resonance}$ convergence rule (world-model.md §2.5) and
Option 2's Egress step lifted to the global coordinate; the Egress *dynamics* are
deliberately shared with Option 2, and V3's distinctness in this subsystem is
entirely inherited from the metric it rides on and the target it aims at, not from
the step itself.

### 8.4 Reachability and settling

Integrating §8.3 from $\theta_0$ produces an Egress path that stays in $\Theta$ and
moves only where $\rho>0$; its image over all admissible Yearning schedules is
Reachable Possibility (§2.4). Reachability depends only on Model quantities
($A$, $g$, and the portable core of $\rho$, §9) — never on the Visualization
(invariant 12). Between commits the pre-quantization step is monotone in the §7
objective (a descent of the reconciliation free energy, a testable property); the
committed step is monotone up to one quantization quantum, which is the honest form
of the invariant a harness can assert.

---

## 9. Resonance

Resonance must be "a property of the Traveler's interaction with a Model and its
current Realization … not a property of a particular Visualization." V3 defines it
from Model fields only, as two factors,
$\rho=\rho_{\text{support}}\cdot\rho_{\text{align}}\in[0,1]$.

**Support** — is there enough living, connectable world around the Traveler? A
spatial average of the Model's ecological connectivity intensity $\kappa_{\!e}$
(derived by $\mathcal W$ from productivity/vegetation — a canonical field, *not*
rendered organism instances):

$$
\rho_{\text{support}}=\operatorname{clamp}\!\Big(\tfrac{1}{\pi r_n^2}\!\int_{\lVert x-x_\star\rVert\le r_n}\!\kappa_{\!e}(\theta_\star,x)\,dx,\,0,\,1\Big).
$$

This factor is the **same construction as Option 2's** $\rho_{\text{support}}$
(differing only in evaluating at $\theta_\star$); it is a spatial ecology average,
not a susceptibility, and V3 claims no novelty for it. To keep Reachability
Visualization- and tier-independent (invariant 12), the *reachability-determining*
use of $\rho_{\text{support}}$ — the $\rho>0$ support test and the $\rho\ge\rho_{\min}$
gate — is computed as a **canonical, portable quadrature at a fixed resolution
independent of resident tiles and resource tier**; only the smooth rate scaling
that multiplies $\Delta s$ may use the cheaper resident-tile reduction and be
presentation-grade.

**Alignment** — this is the genuinely new, exponential-family part. It reports
whether the *net requested move* is one the law can actually make, and whether the
Yearnings agree:

$$
\rho_{\text{align}}
=\underbrace{\frac{v^\top g(\theta_\star)\,v}{v^\top g(\theta_\star)\,v+\varepsilon_\rho}}_{\text{can the law respond?}}\cdot
\underbrace{\exp\!\Big(-\frac{1}{\sigma_0^2}\sum_a\pi_a\operatorname{Var}_y[\bar\mu_{y,a}]\Big)}_{\text{do the Yearnings agree?}},
\qquad v=\frac{\theta^{+}-\theta_\star}{\lVert\theta^{+}-\theta_\star\rVert}.
$$

The **response** factor uses the law's susceptibility $v^\top g\,v=
\operatorname{Var}_\theta[\langle v,T\rangle]$ in the requested direction. As a
prevalence is driven toward its degenerate boundary the law becomes deterministic,
$\operatorname{Cov}[T]\to0$, so $g$ collapses in that direction and the factor
$\to0$: the law can no longer respond, and Resonance vanishes — the principled
reason "make everything the same" stalls. (This is the correct boundary physics:
susceptibility *vanishes* at a deterministic limit; it does not diverge. The
mean-coordinate cost of further prevalence gain, $1/\lambda_{\min}(g)$, diverges
correspondingly — the dual reading of the same fact.) The **agreement** factor
falls when Yearnings on an attribute disagree (large precision-weighted variance of
their targets) — "low Resonance indicates ambiguity, incompatibility, or
insufficient local support."

Resonance is thus simultaneously a **rate scale** (it multiplies $\Delta s$), a
**confidence signal** (its factors report *why* movement is slow — barren
surroundings, a saturated request, or conflicting desires), and a **threshold**
under the trivial policy $\rho<\rho_{\min}\Rightarrow\Delta s=0$. It never touches
rendered geometry, frame timing, or hardware. Half of it (support) is shared in
form with Option 2; the alignment half is the law-susceptibility content unique to
V3.

---

## 10. Continuity as unbalanced optimal transport (the "drift")

This is the second, distinct geometry of V3. Egress changes the world-law; the
realized world must transition without a global reload. V3 does not merely *report*
the coming change (Option 1's sensitivity descriptor) and does not only *lag* the
coordinate (Option 2's crossfade). For the layers where content is a genuine
spatial mass distribution — the **living and biome intensities** — it **moves the
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
=\inf_{v,\alpha}\ \tfrac12\!\int_0^1\!\!\int_\Omega\big(\lvert v\rvert^2+\delta_W^2\,\alpha^2\big)\,\varrho\,dx\,dt
\quad\text{s.t.}\quad
\partial_t\varrho+\nabla\!\cdot(\varrho\,v)=\varrho\,\alpha .
$$

The transport field $v$ is horizontal — **content migrates across World Space**.
The reaction field $\alpha$ is vertical — **content blooms and fades in place**.
The length-scale $\delta_W$ sets the crossover: below separation $\pi\delta_W$ mass
is transported; beyond it, mass is destroyed and recreated rather than dragged
implausibly far. This is exactly the Continuity requirement — near-field content
slides smoothly, distant content converges in place — and $\delta_W$ is a physical
game-feel dial. (A clarification the theory forces: the "Fisher–Rao" *inside* WFR
is the Hellinger metric on square-roots of spatial mass — a different object from
the information metric $g=\nabla^2A$ of §5, which lives on the compact coordinate.
V3 keeps them explicitly separate: information geometry navigates Possibility; WFR
transports the Realization. The free energy generates the former, never the
latter.)

### 10.2 What is closed-form, and what genuinely transports

- **Living and biome intensities ($\lambda(x)$, biome-mixture fields) are spatial
  measures**, so unbalanced WFR applies literally: the transport field $v$
  migrates forest belts and species ranges across $\Omega$, and the reaction field
  $\alpha$ grows and fades them where prevalence changes. Locally the grow/fade
  part is closed-form, $\lambda_t=\big((1-t)\sqrt{\lambda_0}+t\sqrt{\lambda_1}\big)^2$;
  the migration part transports the suitability field. Where a non-Gaussian biome
  boundary must reshape, an **unbalanced Sinkhorn / scaling iteration** at a
  *fixed* iteration count supplies a deterministic transient approximation. This is
  the layer where V3's "content physically moves" claim is real, and it is the
  living heart of the game.
- **Abiotic fields (terrain, climate) are Gaussian**, and their morph is the
  **balanced Bures–Wasserstein displacement of the field's law**. For stationary
  fields this is the per-frequency amplitude interpolation
  $s_t(k)=\big((1-t)\sqrt{s_0(k)}+t\sqrt{s_1(k)}\big)^2$ plus a trend-mean
  interpolation. Stated honestly: with the sample's phase fixed by its hash seed,
  this is an **in-place spectral reshaping** — relief re-textures, roughness and
  anisotropy and level morph — *not* a rigid horizontal slide of a specific ridge.
  Genuine horizontal migration of an abiotic feature would need optimal transport
  of elevation as a spatial mass (not closed-form); V3 does not claim it on the
  fast path and does not need it, because the "large changes appear in the
  distance and resolve on approach" experience comes from the streaming annulus
  (§10.3), where the far field is realized directly at the new law.

### 10.3 The streaming annulus and the Model/Visualization split

The transition is computed only in the streaming annulus between the pinned near
zone and the resolved far zone, so per-frame transport work is $O(\text{band})$,
not $O(\text{window})$. Near the Traveler the transport rate is zero (content is
pinned — invariant 2's "nearby content retains realization history"); the far
field realizes directly at $\theta_\star$ ("newly encountered regions are realized
according to the newer Model State"); the band between is where content is
mid-transport. Large changes therefore appear first in the distance (a biome
colour spreading over the horizon, a forest advancing) and resolve on approach.

The **Model supplies continuity information** — the transport endpoints, the WFR
length-scale $\delta_W$, the Bures affine map for the abiotic spectra, and the
grow/fade rates — while the **Visualization owns the transient morph state and
performs the blend** (invariant 7; the conceptual model assigns boundary blending
to the Visualization). The morph is presentation-grade and discarded once the new
$\hat\theta$ tiles are resident; history lives entirely here, and the canonical
coordinate is always the single $\theta_\star$. An Impression captured
mid-transition therefore always samples the *canonical* realization
$\mathcal W(\theta_\star,\hat x)$, never the transient transport buffer; if the
captured subject has no canonical counterpart (a fading organism), capture snaps to
the nearest canonical entity or is refused, per the thin-classifier rule of §11.1.

---

## 11. Impressions, Attractors, and dual-space travel

### 11.1 Impressions

An Impression is a small, exact record —
$I=(M,\hat\theta,\hat x,\hat t?,\{(\text{attr id},\hat\mu_a)\})$: Model identity,
the quantised global coordinate, a quantised World Space point, an optional
canonical time, and captured mean-parameter values of the subject. As in **Option
2**, the single-global-coordinate decision makes an Impression literally one point
$(\hat\theta,\hat x)$ — this compactness is shared, not V3-specific; what is
V3-specific is only that the captured values are mean parameters $\hat\mu_a$ that
seed Yearning targets (§7.1). A Traveler with a compatible Impression re-derives
the same law by decoding $\hat\theta$ and the same subject by sampling
$\mathcal W$ at $\hat x$ (invariants 6, 9). Capture a subject by its canonical
attribute values when its classifier margin is thin (Option 1's rule).

### 11.2 Attractors as conjugate pseudo-counts

Published Impressions accumulate into a **community pull in natural-parameter
space**. Each cluster $i$ of visits near coordinate $\theta_i$ contributes a
pseudo-count natural parameter, and they add:
$\theta_{\!A}=\sum_i n_i\theta_i$, with the pseudo-count $n_i$ growing with
independent visit count, published Builds, and subscriptions. This is a
product-of-experts / conjugate-prior update — adding a term is exactly the Bayesian
effect of $n_i$ more "votes" for the law $\theta_i$ — and it enters reconciliation
directly as the linear term of §7.2,
so Attractors and Yearnings compromise in one convex program. With grid-bucketed,
id-keyed, union-merged counters it is CRDT-mergeable (union-by-id, idempotent), so
the prototype's atlas-bundle sharing (ADR 0014) carries over and strength is
removable when its records are (invariant 14).

Diffuse-to-precise resolution is automatic: a lone visit is a weak, low-precision
pull (a broad bias); many independent visits raise $n_i$ until the induced target
law's metric radius falls below one Possibility quantum, at which point its
minimizer *is* an exact destination equivalent to an Impression. For summarising an
expedition's *realized terrain*, a **Bures/Wasserstein barycenter** of the
published Gaussian field covariances $C_i$ (the globally convergent fixed point
$C=\sum_i w_i(C^{1/2}C_iC^{1/2})^{1/2}$) gives a canonical "average landscape" —
this barycenter applies only to realized field covariances, never to the world-laws
$\theta_i$, whose community average is the natural-parameter sum $\theta_{\!A}$.

### 11.3 Dual-space travel

An Attractor may specify both a Possibility region and a World Space location; the
two distances are independent (invariant 3): the information metric $d_g$ on
$\theta$ and the Euclidean $\lVert x-x_{\text{target}}\rVert$ on $\Omega$.
Coordinated arrival is a rate controller — choose Exploration speed and Yearning
weighting so normalised progress matches,

$$
\frac{d_g(\theta_\star,\theta_{\text{target}})}{\text{egress rate}}
\ \approx\
\frac{\lVert x_\star-x_{\text{target}}\rVert}{\text{explore speed}},
$$

adjusting the two rates without ever equating the two metrics. Exactness depends on
the Attractor's precision (its induced metric radius).

---

## 12. Determinism, identity, and versioning

V3 keeps the prototype's three grades of determinism (world-model.md §2.9), scoped
carefully to the law/sample split.

1. **Portable decode identity.** A *given* world address $\hat\theta$ decodes
   identically on native and wasm: all permanent identities — region hashes,
   species ids, record ids, tile dependency keys — are integer folds (SplitMix64)
   over $\hat\theta$, integer World Space indices, layer id, feature index, and
   version, and no float feeds the *decode* of a fixed $\hat\theta$. This is the
   guarantee an Impression relies on.
2. **Live navigation reproducibility is conditional.** The value of the *next*
   $\hat\theta$ is produced by a float pipeline (the reconciliation solve, the
   $\nabla A^*$ inversion, rounding), which is exact on one target but only
   presentation-grade across targets. Two devices running identical Yearnings and
   travel are guaranteed the same committed world only when the commit pipeline runs
   in **canonical mode** — fixed iteration counts, portable transcendental
   approximations, and a specified rounding rule, with the input reductions of §7.2
   done in fixed point. Otherwise live-navigated $\hat\theta$ is portable in format
   but not bit-reproducible across platforms, and cross-platform play relies on
   sharing the committed $\hat\theta$ (a scripted/shared address), exactly as the
   prototype's anchor-reduction contract does (ADR 0011/0013).
3. **Settled state.** Tiles are pure functions of their integer dependency key, so a
   quiescent scripted endpoint is independent of scheduler, worker count, budget,
   cancellation, and cache capacity (ADR 0018).

The float layers that are always presentation-grade and never cross the identity
boundary: the metric solve and $\nabla A^*$ inversion, the Bures matrix square root,
Sinkhorn/scaling iterations, and organism samples. Two version axes as before: the
Model major version changes the generated world's identity (any change to $A$, $T$,
or $\mathcal W$ altering output for fixed inputs), per-layer `algorithm_revision`
confines changes, and `RECORD_FORMAT_VERSION` changes serialized schemas. An
Impression stores $M$ so it is never silently reinterpreted (invariant 8).

---

## 13. Performance — why it runs in real time

Let $k$ be the coordinate dimension ($\le48$), $R$ the archetype count ($\sim256$),
$T_{\text{res}}$ resident tiles ($\sim10^3$), $n^2$ samples per tile.

The dominant navigation cost is assembling and solving against the prevalence-block
metric $\operatorname{Cov}_s[c]=C(\operatorname{diag}s-ss^\top)C^\top$. Assembling
it densely is $O(Rk^2)$ and recurs at every Newton iterate (the softmax weights
$s$ move with $\theta$), so a naïve dense reconcile ($\sim8$ Newton steps) plus the
$\nabla A^*$ inversion ($\sim4$ steps) is a few $\times10^6$ FMAs — of order
**hundreds of microseconds**, not tens. That is still under $3\%$ of a $16.6$ ms
frame, so real time holds; the honest figure is a few hundred µs per navigation
tick, not the tens of µs an $O(Rk+k^2)$ mislabel would suggest.

The **matrix-free** form removes even that: never form $\operatorname{Cov}_s[c]$;
each metric–vector product is $C(\operatorname{diag}s-ss^\top)(C^\top v)=O(Rk)$, and
a fixed-iteration conjugate gradient solves $g\,d=\text{rhs}$ without a dense
factorization. At $R=256,k=48$ a metvec is $\sim2.5\times10^4$ FMAs and a
fixed-iteration solve is a handful of those — tens of µs per solve, and the fixed
iteration count preserves determinism (canonical mode, §12).

| Step | Matrix-free cost | Note |
|---|---|---|
| Evaluate $A,\nabla A$ (softmax over $R$ archetypes) | $O(Rk)$ | one pass over the bank |
| Reconcile (§7): $\sim8$ CG-based Newton steps | $O(Rk)$ per metvec, fixed iters | strictly convex, unique target |
| Invert $\nabla A^*$ for $\theta^{+}$ | $\sim4$ Newton steps, same metvec | benefit is feasibility, not cost |
| Attractor pseudo-counts over nearest clusters | $O(k\cdot\text{clusters})$ | conjugate sum |
| Resonance (susceptibility + portable support quadrature) | $O(Rk+\text{quad})$ | portable core fixed-resolution |

**Realization** is identical in character and cost to any streaming procedural-
terrain engine: pointwise hashed noise per sample, GPU-parallelisable, cached per
tile, regenerated only on a dependency-key change; $\mathcal W$ reads a bucketed
$\hat\theta$ constant, adding no per-sample possibility cost. **Continuity
transport** is closed-form grow/fade plus suitability transport for the living
layers and per-frequency Bures for the abiotic spectra, restricted to the streaming
annulus, so it is $O(\text{band})$; the optional unbalanced-Sinkhorn path runs at a
fixed low iteration count on the coarse biome grid only. No cost grows with explored
area or world size.

---

## 14. Rust realization sketch

The neutral core (no threads, filesystem, or GPU — the crate-boundary rule of
`AGENTS.md`) exposes the free energy and its derivatives; everything else is built
on them.

```rust
/// Compact possibility coordinate = natural parameters of a world-law (fixed point).
/// Split into a bounded prevalence block and an unbounded scalar block (§3.1).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Coord { pub q: [i32; K] }            // q_j = round(2^B * theta_j)

/// Mean/prevalence coordinates = the observables Yearnings act on (dim = K, minimal family).
#[derive(Clone, Copy, Debug)]
pub struct Means { pub mu: [f32; K] }

pub trait Model {
    fn free_energy(&self, t: Coord) -> f32;                 // A(theta)   (convex)
    fn mean_map(&self, t: Coord) -> Means;                  // mu = grad A(theta)
    fn metvec(&self, t: Coord, v: &[f32; K]) -> [f32; K];   // g·v  matrix-free, O(Rk)  (§8.2,§13)
    fn dual(&self, mu: &Means) -> Coord;                    // theta = grad A*(mu)  (~4 Newton solves)
    fn project(&self, t: Coord) -> Coord;                   // Pi: Bregman projection onto feasible set
    fn realize(&self, t: Coord, region: RegionIx, layer: Layer) -> Tile;  // 𝒲, cached by key
    fn dep_key(&self, t: Coord, region: RegionIx, layer: Layer) -> u64;   // integer identity
    fn resonance(&self, t: Coord, target: Coord, near: NearField) -> f32; // support × alignment (§9)
    /// Continuity information only; the Visualization owns the morph state (§10.3, invariant 7).
    fn transport_plan(&self, old: Coord, new: Coord) -> TransportPlan;
}

/// One reconciled target law. Order-independent: aggregates reduced in canonical/fixed-point
/// order, then a strictly convex program with a unique minimizer (§7.2).
pub fn reconcile_target(
    m: &impl Model, theta: Coord, yearnings: &[Yearning], attractors: &AttractorField,
) -> Coord {
    let (pi, mu_bar, theta_a) = reduce_canonical(theta, yearnings, attractors); // §7.2 fixed-point sums
    soft_maxent_mean_space(m, theta, &pi, &mu_bar, &theta_a)                     // §7.2 unique μ⁺ → θ⁺
}

/// One Egress commit: a gated metric-normalized step toward the target. β is a Traveler dial.
pub fn egress_step(m: &impl Model, theta: Coord, target: Coord, resonance: f32, travel: f32) -> Coord {
    let dir = unit_metric_step(m, theta, target);           // §8.1 metric-normalized toward θ⁺
    let ds  = BETA * resonance * travel.max(0.0);            // §8.3  (Traveler layer, not Model identity)
    m.project(step_along(theta, &dir, ds))
}
```

Suggested neutral crate split:

```text
world-model-v3-core   fixed point, hashes, coordinates, free energy A and its derivatives, metvec
world-model-v3-fields Matern GRFs, LGCP/point processes, canonical entities, prevalence integrals
world-model-v3-nav    reconciliation, gated Egress, resonance, attractor pseudo-counts
world-model-v3-flow   WFR/Bures transport-plan data (endpoints, maps, rates) — no morph state
world-model-v3-api    capability and Realization contract types
```

Jacobians and Hessians are algebraic (from the block $A$), not autodiff; the metric
is applied matrix-free, never densely factored; all ordered maps and integer
tie-breaks reproduce the prototype's count-budgeted, run-stable scheduling.

---

## 15. Conceptual invariants — conformance

| # | Invariant | How V3 satisfies it |
|---|---|---|
| 1 | One point in Possibility = one complete world | $\theta$ is one global world-law; $\mathcal W(\theta,\cdot)$ is the whole world (§2, §6). |
| 2 | One canonical point; nearby content keeps history | canonical $\theta_\star$; history is the transient WFR morph state, never authoritative; capture reads the canonical realization (§10.3). |
| 3 | Possibility and World Space independent metrics | information metric $g$ on $\theta$ (§5) vs. Euclidean/World-Space transport (§10, §11.3). |
| 4 | Egress = Possibility, Exploration = World Space | distinct flows $\theta_\star$, $x_\star$ (§1, §8). |
| 5 | Egress coupled to Exploration but owned by neither | coupling is the **Traveler policy** $\Delta s=\beta\rho\lVert\Delta x_\star\rVert$; $\beta$ is a tunable dial, not Model identity (§8.3). |
| 6 | Realization carries stable meaning | integer $\hat\theta$ + versioned observable schema $T$/$\mathcal W$ (§4, §12). |
| 7 | Simulation belongs to Visualization | $\mathcal W$ yields fields/laws only; the Model supplies transport *information*, the Visualization owns the morph (§10.3). |
| 8 | Identical Model inputs reproduce Realization | decode of a fixed $\hat\theta$ is a pure function of $(\text{version},\hat\theta,x,\hat t)$ (§12 grade 1). |
| 9 | Impression addresses meaningful across Visualizations | $(\hat\theta,\hat x,\text{version})$ + captured $\hat\mu$ decode identically; capture is canonical (§11.1, §10.3). |
| 10 | Yearnings weighted, order-independent | unique minimiser of a strictly convex mean-space program over $\mathcal P$, on canonically reduced aggregates (§7.2). |
| 11 | Scope = prevalence, not spatial falloff | prevalence *is* a mean parameter $\mu_a=\mathbb E_\theta[T_a]$; no falloff appears anywhere (§4, §6.3). |
| 12 | Visualization does not change Reachable Possibility | reachability depends only on $A,g$ and the **portable core** of $\rho$ at fixed quadrature (§8.4, §9). |
| 13 | Builds are optional Visualization content | Builds attach to Impressions and only strengthen Attractors; never enter $\theta$, $A$, or $\mathcal W$. |
| 14 | Attractor evidence historical, abuse-resistant, removable | CRDT-merged pseudo-count clusters; removable weights $n_i$ (§11.2). |

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
- **How strongly may Hold resist?** A stiff but finite precision in the convex
  program; overridden only by greater aggregate precision or by feasibility. (§7)
- **Is Resonance threshold / rate / precision?** All three, from
  $\rho=\rho_{\text{support}}\rho_{\text{align}}$: an ecological-support term
  (shared with Option 2) and a law-susceptibility/agreement term (new). (§9)
- **Dual-space coordinated arrival?** A rate controller equalising normalised
  progress in $d_g$ and Euclidean space (restated from the conceptual model, not
  further resolved). (§11.3)
- **When does an Attractor become exact?** When accumulated conjugate pseudo-counts
  shrink the induced target law's metric radius below one Possibility quantum.
  (§11.2)
- **Continuity risk / chaotic divergence between nearby states?** The law/sample
  split localises it: nearby laws are $\mathrm{KL}$-close, while one sample may
  diverge sharply, and $g$ reports where. (§2.1, §5)

Left untouched (and §16 claims no more): canonical-time membership and
Build-reproduction fidelity. Attractor abuse-resistance is inherited from the
prototype's mechanism, not re-derived.

---

## 17. Relationship to the current implementation and to the sibling proposals

V3 keeps the engineering mechanisms the prototype has proven — integer SplitMix64
hashing and versioned identities; lazy coordinate-derived content; declared
dependencies and dependency-hash-gated integration; bucketed quantisation as the
tile-invalidation event; bounded caches, pools, deterministic scheduling, and
cancellation; the three grades of determinism; the CRDT/atlas sharing laws; and the
neutral/platform crate boundaries with native/wasm verification. It changes the
*semantics*:

| Concern | Current prototype | Proposed V3 |
|---|---|---|
| Point in Possibility | per-region 8-vector + authoritative current state | one global exponential-family world-law $\theta$ |
| Validity | ordered `project_plausible` clamp cascade | intrinsic: every $\theta$ is a valid law; validity = the marginal polytope |
| Distance | component differences of scalars | Fisher information metric $g=\nabla^2A$ (Chentsov-unique given $T$) |
| Steering | Emphasize-first/Suppress-last blend + raw-bit sort | one convex mean-space maxent program; unique minimizer |
| Scope | not represented as global prevalence | a mean parameter $\mu_a=\mathbb E_\theta[T_a]$ |
| Resonance | near-organism density/diversity gate | ecological support × law-susceptibility/agreement |
| Continuity | per-region current/target lerp history | unbalanced-OT transport of living/biome mass; Bures spectral reshaping for abiotic |
| Ecology | habitat-signature roster, ≤12 species | log-Gaussian Cox intensity + marked point process; prevalence = calibrated intensity marginal |
| Attractors | route/anchor weak steering | conjugate pseudo-counts in natural-parameter space, folded into reconciliation |

**Honest distinctness from the siblings.** V3 shares its overall skeleton with the
others, and the probabilistic structure is genuinely load-bearing in **five**
subsystems while **five** are the same mechanism as Option 2 with a probabilistic
reading:

- *Genuinely V3* — the **metric** is the exact Hessian identity $g=\nabla^2A=
  \operatorname{Cov}[T]$ with no weight matrix and no rank-floor (contrast: Option 1
  differentiates a 256-probe *realized moment summary*; Option 2 differentiates an
  *analytic attribute chart* exactly with dual numbers, with a hand-chosen weight
  $S$ and an $\varepsilon$-floor — V3 has neither the probe nor the weight nor the
  floor); **Scope** is literally a mean parameter; **ecology** is an LGCP whose
  intensity marginal is (calibrated to be) the mean map; **Attractors** are
  conjugate pseudo-counts; and **continuity transports living/biome mass** across
  World Space under unbalanced OT.
- *Shared in mechanism with Option 2, stated as such* — the per-attribute request
  fusion (§7.1); the Euclidean-in-mean compromise term inside the otherwise-convex
  reconciliation (§7.2); the ecological-support half of Resonance (§9); the
  one-point Impression record (§11.1); and the travel×resonance natural-gradient
  Egress *step* (§8.3), whose distinctness is inherited entirely from the metric it
  rides on.

Both siblings also achieve validity-by-construction and prevalence-as-global-scalar
by their own means (Option 2's triangular parent-gated chart; Option 1's total
smooth decoder); V3's marginal-polytope-from-an-archetype-bank is a different route
to the same properties, not a correction of a deficiency they lack.

Compatibility is neither required nor implied. Current anchors, preserves, routes,
eight-component signatures, and generated regions cannot be reinterpreted as V3
addresses. A migration tool could embed a current observation as a Yearning and
search for a similar V3 law, but the result would be a new Impression, not the same
world.

---

## 18. Acceptance criteria for an implementation

V3 is implementable when a prototype demonstrates, without special cases:

1. any coordinate $\hat\theta$ opens a valid deterministic world-law and world;
2. a canonical address reproduces selected fields, prevalences, and entities on
   native and wasm to the specified rounding;
3. the calibrated prevalence tie holds: the realized fraction of organisms with a
   trait equals $\mu_a=\nabla A(\theta)_a$ within a stated tolerance covering
   finite-$N$ sampling, the fBm-vs-Matérn variance bias, and the ergodic gap (§6.3);
4. the metric equals $\nabla^2A$ (block $Q_0\oplus\operatorname{Cov}_s[c]$) to
   finite-difference tolerance and is SPD in the interior of $\Theta$;
5. after canonical fixed-point reduction of the aggregates, arbitrary Yearning
   permutations produce bit-identical quantised Egress steps and Resonance (§7.2);
6. singular/common/pervasive requests move planetary prevalence monotonically,
   saturating (not clamping) as the prevalence hull is approached;
7. conflicting Accentuate/Repress/Hold terms yield a unique, bounded compromise (the
   convex program has one minimizer);
8. a local Realization query has cost independent of travel history and world size;
9. a long high-speed traversal stays within fixed memory ceilings, and per-frame
   transport stays $O(\text{band})$;
10. Resonance falls to zero as a request approaches a degenerate prevalence, and
    Egress stalls rather than clamps;
11. the living-layer transport reaches both endpoints, travels at bounded metric
    speed, and never alters $\theta_\star$;
12. schedules, cancellation, worker count, and cache capacity do not change settled
    results; and
13. measured navigation stays inside a 60 Hz host's background budget on the
    supported native and browser reference machines.

---

## Appendix A: decoded parameter blocks

The frozen decoder produces the coefficients of $A$ and the field hyperparameters.
These are derived data, not additional Model State.

| Block | Contents | Construction constraint |
|---|---|---|
| Free energy — scalar | $Q_0\succ0$ (metric floor on scalar block); linear $q$ | $Q_0$ PD; smooth in the scalar coordinates |
| Free energy — prevalence | archetype bank $\{c_r\}$; priors $\{\pi_r\}$ | $\pi_r>0$, $\sum\pi_r=1$; archetypes span feasible worlds; physical inequalities baked in as correlations; prevalence hull matches $[\tau_{\min},\tau_{\max}]$ |
| Observable schema | sufficient statistics $T$ (with $\dim T=\dim\theta=k$); per-attribute kind | prevalence statistics bounded in $[0,1]$; $T$ affinely independent (minimal family) |
| Terrain/geology spectra | Matérn $(\sigma^2,\ell,\nu)$ + slow anisotropy per abiotic layer, as functions of $\theta$ | $\sigma^2,\ell,\nu>0$; (locally) stationary for the lazy path; smooth in $\theta$ |
| Hydrology/soils | drainage, retention, nutrient response coefficients | bounded positive rates; stable integer flow topology |
| Ecology/organisms | LGCP mean/covariance fields; trait-mark laws; connectivity field $\kappa_{\!e}$; **LGCP↔$\nabla A$ prevalence calibration** | log-intensity fields; prevalence integrals equal $\nabla A$ to tolerance (§6.3) |
| Transport | WFR length-scale $\delta_W$; near/far radii $r_n,r_f$; grow/fade rate | $\delta_W>0$; $0<r_n<r_f$ |
| Navigation | metric scale; Hold stiffness $\eta_{\text{hold}}$; resonance constants $\varepsilon_\rho,\sigma_0,\rho_{\min}$; Scope band $[\tau_{\min},\tau_{\max}]$ | positive; $\eta_{\text{hold}}>1$; $0<\tau_{\min}<\tau_{\max}<1$ (interior) |

The step rate $\beta$ is **not** in this manifest: it is a Traveler/gameplay dial
(§8.3, invariant 5). The archetype bank and observable schema are *fitted* offline
from a corpus of accepted worlds and validated by held-out world-quality tests, not
by an identity (§3.4). An implementation is not conforming until a **parameter
manifest** supplies every archetype, matrix entry, statistic definition, range
endpoint, solver iteration count, transcendental approximation, and rounding rule
named here; the manifest is hashed into the Model major identity and becomes an
immutable test fixture.

---

## Appendix B: one navigation tick

Given current canonical state $\theta_\star$, active Yearnings $Y$, Attractor field,
and Traveler travel $\Delta x$:

1. reduce $Y$ and the Attractors to canonical fixed-point aggregates
   $(\pi_a,\bar\mu_a,\theta_{\!A})$ (§7.2);
2. solve the strictly convex mean-space maxent program for the unique target
   $\mu^{+}$; set $\theta^{+}=\nabla A^*(\mu^{+})$ (fixed-iteration Newton, §8.2);
3. evaluate $g(\theta_\star)$ matrix-free; form the metric-normalized unit direction
   $\hat d$ toward $\theta^{+}$ (§8.1);
4. compute Resonance $\rho$ = portable ecological support × law susceptibility/agreement
   (§9);
5. set $\Delta s=\beta\rho\max(\lVert\Delta x\rVert,0)$ (Traveler policy); integrate
   one step, error-feedback-round to $\hat\theta$, apply $\Pi$ (§8.3);
6. open the new immutable snapshot; hand the old/new coordinate pair to the
   Visualization as a transport plan for the streaming annulus (§10.3);
7. the Visualization drives the transient morph and decides where to refine.

The tick is a pure function of its explicit inputs. The gameplay layer may scale
$\Delta s$ by physical distance, coordinate dual-space arrival, or refuse Egress
while stationary; none of those policies changes the Model's definition of
Possibility or Reachability.

---

## Appendix C: machine-checkable invariants (the goal-4 checklist)

The navigation algebra, given the frozen bank, is checkable against identities and
properties — the concrete hooks an AI maintainer or CI gate can assert:

| Property | Assertion | Maps to |
|---|---|---|
| Metric identity | $g(\theta)=\nabla^2A(\theta)$ to finite-difference tolerance; block $Q_0\oplus\operatorname{Cov}_s[c]$ | acc. 4 |
| Metric PD | $g\succ0$ on interior samples; $\lambda_{\min}(g)\to0$ only at the prevalence hull | acc. 4, 10 |
| KL = Bregman | $\mathrm{KL}(p_{\theta_1}\Vert p_{\theta_2})=B_A(\theta_2,\theta_1)=B_{A^*}(\mu_1,\mu_2)$ (both forms) | §5 |
| Projection idempotent | $\Pi(\Pi(\mu))=\Pi(\mu)$; feasible set is $\overline{\mathcal P}$ | §3.3 |
| Order-independence | permute $Y$, re-reduce canonically, re-solve, assert bit-equal $\hat\theta^{+}$ | acc. 5 |
| Reconciliation uniqueness | one minimizer of the strictly convex mean-space program | acc. 7 |
| Scope monotonicity/saturation | prevalence moves monotonically toward the Scope target and saturates | acc. 6 |
| Egress monotone | pre-quantization step non-decreasing in the §7 objective; committed step within one quantum | §8.4 |
| Transport fidelity | living-layer morph hits both endpoints; $\theta_\star$ unchanged during transition | acc. 11 |
| Portable decode | fixed $\hat\theta$ decodes bit-identically native/wasm | acc. 2, §12 |

What is *not* on this list, and is validated by world-quality tests instead: the
fitted archetype bank, the observable schema, and the LGCP prevalence calibration
(§3.4, §6.3). Naming this boundary honestly is part of the design.

---

## Appendix D: imported mathematics and references

- **Information geometry / exponential families.** Fisher metric $=\nabla^2A=
  \operatorname{Cov}[T]$; mean map $\mu=\nabla A$; Legendre dual $A^*=$ negative
  entropy; KL $=$ Bregman divergence of $A$; dually flat structure — Amari &
  Nagaoka, *Methods of Information Geometry*; Amari 1998 (natural gradient).
  Uniqueness of the Fisher metric — Chentsov (Čencov); continuous case —
  Ay–Jost–Lê–Schwachhöfer. Marginal polytope and $\nabla A$ bijection onto its
  interior — Wainwright & Jordan. Mirror descent $\equiv$ natural gradient —
  Raskutti & Mukherjee 2015.
- **Maximum entropy / I-projection.** Jaynes; Csiszár (I-projection, uniqueness,
  Pythagorean). Regularised/soft maxent — Dudík, Phillips & Schapire. Ecological
  precedent — Phillips et al. (MaxEnt SDM); Harte, *Maximum Entropy and Ecology*
  (METE).
- **Optimal transport.** Villani, *Optimal Transport: Old and New*; Brenier;
  Benamou–Brenier; McCann displacement interpolation; Agueh–Carlier and
  Álvarez-Esteban et al. 2016 (Gaussian/Bures barycenter fixed point); Cuturi 2013
  (Sinkhorn); Feydy et al. 2019 (Sinkhorn divergence); Jordan–Kinderlehrer–Otto
  1998 and Otto 2001 (Wasserstein gradient flow). Bures–Wasserstein Gaussian closed
  forms as in §10.
- **Unbalanced optimal transport (WFR / Hellinger–Kantorovich).**
  Chizat–Peyré–Schmitzer–Vialard (arXiv:1506.06430, 1607.05816);
  Liero–Mielke–Savaré; Kondratyev–Monsaingeon–Vorotnikov. Dynamic
  continuity-with-source form and the length-scale $\delta_W$ as in §10.
- **Random fields and point processes.** Matérn covariance/spectral density;
  Whittle–Matérn SPDE — Lindgren, Rue & Lindström 2011. Log-Gaussian Cox process —
  Møller, Syversveen & Waagepetersen. Determinantal point processes — Macchi;
  Hough et al.

Determinism caveat (repeated from §12): these results are used at the *law and
navigation* level, where their float outputs are presentation-grade; only a
committed $\hat\theta$ and integer-hashed identities cross the sharing boundary,
and cross-platform live reproducibility requires canonical mode.
