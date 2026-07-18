# bwq World Model Option 2 — A Proposed Mathematical Design for the Model

This document proposes a concrete mathematical design for the **Model** as
defined in [`conceptual-model.md`](conceptual-model.md). It is a *complete
system*: it fixes what a Model State is, what a Possibility Coordinate is, how
Egress moves through Possibility, how Yearnings are reconciled, how Resonance
and Attractors are computed, and how a Realization is derived — all with enough
mathematical precision to be implemented in Rust and to run in real time.

It **references** the current implementation described in
[`world-model.md`](world-model.md) but is deliberately **not** compatible with
it and does not reuse its structures. Where this design departs from the
prototype, the departure is called out and justified. The most important
departure is stated up front so the rest of the document reads against it.

> **Central departure.** In the current prototype, "possibility" is a
> **per-region** eight-vector: the streamed world is a *spatial field of
> possibility states*, and "the world" is never one point. The conceptual model
> says the opposite — invariant 1: *"One point in Possibility represents one
> complete physical world,"* and §Model State: *"A Model State describes an
> entire world, not one local region."* This design takes that literally. The
> canonical Possibility Coordinate is a **single compact global vector** for the
> whole world. All spatial variation — mountains, climate, rivers, ecosystems —
> is a *deterministic function of that one coordinate and a World Space
> position*. Per-place "realization history" becomes an explicitly derived
> **continuity field**, not an authoritative per-region state, exactly as
> invariant 2 requires.

Notation: $\|v\|$ is the Euclidean norm; $\|v\|_g=\sqrt{v^\top g\,v}$ is the norm
in metric $g$; $\lfloor\cdot\rfloor$ is floor; $\mathrm{clamp}(z,a,b)$ clamps to
$[a,b]$. Vectors are columns. Integer/fixed-point quantities carry a hat,
$\hat\theta$.

---

## 1. Overview of the system

The Model is the 6-tuple

$$
\mathcal M=\big(\Theta,\ \mathcal D,\ \phi,\ g,\ W,\ \Pi\big),
$$

with the following pieces, each defined in its own section:

| Symbol | Name | Role | §|
|---|---|---|---|
| $\Theta$ | Possibility manifold | the space of Model States; a point $\theta\in\Theta$ is one world | 2 |
| $\mathcal D$ | Decoder | expands a compact coordinate $\theta$ into rich generator parameters $m=\mathcal D(\theta)$ | 3 |
| $\phi$ | Observables | model-facing attributes $\phi(\theta)\in\mathbb R^A$ that Yearnings act on and Impressions capture | 4 |
| $g$ | Possibility metric | a Riemannian metric giving non-Euclidean distance and neighborhoods | 5 |
| $W$ | Realization field | $W(\theta,x)$ produces the local world at World Space position $x$ | 6 |
| $\Pi$ | Plausibility projection | maps any coordinate onto the feasible (valid) manifold | 3.3 |

Around this static structure, the **Traveler** carries dynamic state and drives
two coupled flows:

* a **canonical possibility coordinate** $\theta_\star(t)\in\Theta$ moved by
  **Egress** (§7–8);
* a **World Space position** $x_\star(t)\in\mathbb R^2$ moved by **Exploration**;
* and a derived **continuity field** $\Xi(x,t)\in\Theta$ — the effective
  coordinate actually used to realize each place — that lags $\theta_\star$ near
  the Traveler and equals it far away (§6).

The full pipeline for one frame:

```text
Yearnings (Impressions + Influence + Scope + weights)
        │
        ▼  weighted least-squares reconciliation (order-independent)
   desired attribute vector  φ̄ , weight W
        │
        ▼  natural-gradient step on the plausibility manifold
   egress direction d = g⁻¹ ∇U         (U = yearning + attractor utility)
        │
   resonance ρ  ×  travel ‖ẋ‖  ──────► egress step length Δs
        │
        ▼
   θ⋆ ← Π( θ⋆ + Δs · d̂ )              (canonical world coordinate advances)
        │
        ▼  travel-gated relaxation
   Ξ(x)  effective coordinate field    (near lags, far tracks θ⋆)
        │
        ▼  deterministic, pointwise, cached by quantization bucket
   W( Ξ(x), x )  →  terrain, climate, hydrology, soils, ecology, organisms
```

Everything below the "desired attribute vector" line is pure Model math with no
dependence on the Visualization, satisfying invariant 12 (Visualization choice
does not change Reachable Possibility).

---

## 2. Possibility as a manifold

### 2.1 The coordinate

A **Possibility Coordinate** is a point on a smooth $k$-dimensional manifold

$$
\Theta \;=\; \mathbb T^{k_c}\times\prod_{j=1}^{k_b}[0,1],
\qquad k=k_c+k_b,
$$

a product of $k_c$ **cyclic** coordinates (a torus — no boundaries, wraps
seamlessly, so "no discernible boundaries or enumerable collection of separate
universes," per §Possibility) and $k_b$ **bounded** coordinates (planetary
magnitudes that have physical floors and ceilings). A practical first instance
uses $k\approx 24\text{–}48$. These are *latent* coordinates: they are the
compact address, not the world itself.

The coordinate is stored in fixed point for determinism:

$$
\hat\theta_j=\big\lfloor 2^{B}\,\theta_j\big\rfloor\in\mathbb Z,\qquad B=32,
$$

with cyclic axes reduced modulo $2^{B}$ and bounded axes clamped to
$[0,2^{B})$. The integer vector $\hat\theta$ is the portable identity of a world
(§10). This mirrors the prototype's $Q=4096$ possibility quantization
(`world-model.md` §2.3) but promotes it from a per-region device to **the**
global world address, and widens it to 32 bits so that the metric geometry of
§5 has room to express fine navigation.

### 2.2 Model State

The **Model State** is the rich structure

$$
m=\mathcal D(\theta)\in\mathcal S,
$$

produced by the **decoder** $\mathcal D$ (§3). It contains every parameter the
Realization field needs: planetary constants, spectral parameters for each
terrain/climate/hydrology basis, ecological rate constants, and the species
generative parameters. $\mathcal S$ is large; $\Theta$ is small. The map
$\mathcal D$ is the "compact Possibility Coordinate mapped through constraints
and relationships to a much richer Model State" of the conceptual model.

$\mathcal D$ is required to be **Lipschitz and piecewise-$C^1$**: nearby
coordinates give nearby Model States. This is what makes Possibility feel like a
continuum and makes Egress produce gradual change (§Movement in Possibility).
Emergent sharpness is still allowed because $W$ (§6) may amplify a smooth
parameter change into a locally sharp world change; continuity of $\mathcal D$
bounds the *parameter* drift, not every realized pixel.

### 2.3 Theoretical, Representable, Reachable

The conceptual model's three sets (§Kinds of Possibility) get exact meanings:

* **Theoretical Possibility** $=\Theta$, the whole manifold.
* **Representable Possibility** $=\{\theta:\hat\theta\ \text{is the exact
  fixed-point image of }\theta\}$ — the $2^{Bk}$-point lattice actually
  addressable at bit width $B$. Everything off the lattice is theoretical only.
* **Reachable Possibility from** $\theta_0$ $=\mathcal R(\theta_0)$, the set of
  coordinates connected to $\theta_0$ by an admissible Egress path (§8): a
  piecewise-geodesic that stays on the feasible manifold $\mathcal F$ (§3.3) and
  everywhere has enough Resonance (§9) to move. $\mathcal R$ is generally a
  proper, path-connected subset — a state can be representable yet unreachable,
  exactly as the conceptual model states.

---

## 3. The decoder and the feasible manifold

### 3.1 Attribute-first construction

Rather than let $\theta$ range over arbitrary parameter space and then filter
out invalid worlds, we build $\Theta$ so that **every representable coordinate
is already a valid world.** Validity is not enforced by a runtime search; it is
baked into the chart. This is the clean replacement for the prototype's
`project_plausible` clamp cascade (`world-model.md` §3.4): instead of projecting
after the fact, the coordinate system only parameterizes the feasible set.

Let $\mathcal F\subset\mathbb R^A$ be the **feasible attribute manifold** — the
set of attribute vectors that describe a coherent world. It is cut out by
plausibility relations $g_i(\phi)\le 0$ that encode the same physics the
prototype hard-codes, but now as global inequalities on world-level attributes:

$$
\begin{aligned}
\text{(water)}\quad & \phi_{\text{water}} \le \sigma\!\left(a_1+a_2\,\phi_{\text{insolation}}+a_3\,\phi_{\text{landmass}}\right),\\
\text{(vegetation)}\quad & \phi_{\text{veg}} \le \sigma\!\left(b_1+b_2\,\phi_{\text{water}}+b_3\,\phi_{\text{warmth}}\right),\\
\text{(productivity)}\quad & \phi_{\text{npp}} \le \sigma\!\left(c_1+c_2\,\phi_{\text{veg}}\right),\\
\text{(body scale)}\quad & \phi_{\text{morph}} \le \sigma\!\left(d_1+d_2\,\phi_{\text{npp}}\right),\\
&\quad\vdots
\end{aligned}
$$

with $\sigma$ a smooth saturating link (logistic). These are the conceptual
"relationships and constraints among parameters." Behavior and Aesthetics
attributes are unconstrained, as in the prototype.

### 3.2 The chart

$\mathcal D$ factors as

$$
\theta \;\xrightarrow{\ \mathcal C\ }\; \phi\in\mathcal F \;\xrightarrow{\ \mathcal G\ }\; m,
$$

where $\mathcal C:\Theta\to\mathcal F$ is a smooth, surjective **coordinate
chart** of the feasible manifold and $\mathcal G$ turns feasible attributes into
generator parameters. Because $\mathcal C$ maps *onto* $\mathcal F$, moving
freely in $\theta$ can never leave feasibility — the coupling between attributes
is intrinsic to the chart. Concretely $\mathcal C$ is a triangular map that
respects the dependency order above:

$$
\phi_{\text{water}}=\sigma\!\big(a_1+a_2\phi_{\text{insolation}}+a_3\phi_{\text{landmass}}\big)\cdot\theta_{\text{water}},
$$

i.e. each dependent attribute is expressed as its own ceiling **times a free
coordinate in $[0,1]$**. Independent attributes (insolation, landmass,
tectonics, hue, behavior) are affine images of their coordinates; dependent ones
are gated by their parents. This is invertible layer by layer, so $\mathcal
C^{-1}$ exists and Impressions can be encoded back to coordinates (§11). The
triangular form is the mathematically honest version of the prototype's ordered
one-pass projection, and it is $C^1$ so §5's Jacobian exists.

### 3.3 Residual projection $\Pi$

For inputs that do arrive off-manifold — an imported Impression from a newer
Model version, a hand-edited coordinate, numeric drift — define the plausibility
projection as the nearest feasible point in the metric of §5:

$$
\Pi(\theta)=\arg\min_{\theta'\in\mathcal C(\Theta)}\ \|\theta-\theta'\|_g .
$$

Because $\mathcal C$ is triangular, $\Pi$ is computed in one forward sweep
(clamp each coordinate to its parent-induced range), so $\Pi$ is $O(k)$ and
idempotent, $\Pi(\Pi(\theta))=\Pi(\theta)$ — the same fixed-point property the
prototype proves for its $\Pi$, but here it is a projection onto a real manifold
rather than a clamp cascade with an implicit feasible set.

---

## 4. Observables — the Realization/attribute contract

The Model exposes a fixed vector of **model-facing attributes**

$$
\phi:\Theta\to\mathbb R^A,\qquad \phi(\theta)=\mathcal C(\theta),
$$

partitioned into groups. These are simultaneously (a) what Yearnings push on,
(b) what Impressions capture, and (c) the coordinates of the feasible manifold,
so the three subsystems share one language. A representative schema:

| Group | Example attributes | Kind |
|---|---|---|
| Planetary | insolation, axial tilt, day length, sea fraction, landmass | scalar |
| Climate | mean warmth, seasonality, aridity | scalar |
| Geology | relief, tectonic activity, hardness spectrum | scalar |
| Hydrology | drainage density, standing-water fraction | scalar |
| Ecology | primary productivity, biome-mixture simplex, diversity | **prevalence** |
| Morphology | body-scale distribution mean/spread | **prevalence** |
| Behavior | activity, aggression, sociality | **prevalence** |
| Aesthetics | hue field, luminance, chroma | scalar/field |

Two attribute *kinds* matter for Yearnings:

* **Scalar attributes** describe a world-wide magnitude (e.g. sea fraction).
* **Prevalence attributes** describe *how widespread* a trait is across the
  world's species/regions — a value in $[0,1]$ that a Scope request targets
  (§Scope: singular → common → pervasive). A prevalence attribute is realized as
  a *distribution parameter* consumed by $W$, e.g. the fraction of species whose
  morphology exceeds a size threshold. This is the design's answer to the
  conceptual requirement that "Scope describes prevalence in a destination
  world, not physical falloff around a location" (invariant 11): prevalence is
  literally a global attribute of $\theta$, so it cannot be a spatial falloff.

The attribute vector *is* the Realization contract (conceptual §Realization,
§Pluggability). A Visualization declares which attribute groups it consumes; a
Model/Visualization pair is compatible iff the consumed groups are present at
compatible versions. Adding attributes is backward compatible; changing the
meaning of one bumps the Model version (§10).

---

## 5. Distance, neighborhoods, and paths in Possibility

Euclidean distance in $\theta$ is meaningless — the conceptual model insists
distance "need not be Euclidean and need not be uniform" (§Movement in
Possibility). We give Possibility a **Riemannian metric** equal to the pullback
of a perceptual metric on attributes:

$$
g(\theta)=J(\theta)^\top S\,J(\theta)+\varepsilon I,\qquad
J(\theta)=\frac{\partial\phi}{\partial\theta}\in\mathbb R^{A\times k},
$$

where $S=\mathrm{diag}(s_1,\dots,s_A)\succ0$ weights attributes by how strongly
they change the *realized* world, and $\varepsilon I$ keeps $g$ positive
definite where $J$ loses rank. Distance between coordinates is the geodesic
length

$$
d_\Theta(\theta_a,\theta_b)=\min_{\gamma:\,a\to b}\int_0^1\|\dot\gamma\|_{g(\gamma)}\,du .
$$

This gives exactly the phenomenon the conceptual model describes: two worlds
that differ by a small numeric $\theta$-amount but a large attribute
consequence are metrically *far* (large $J$), while large numeric moves in a
flat direction of $J$ are metrically *near* (visually subtle). Neighborhoods are
metric balls $\{\theta:d_\Theta(\theta,\theta_0)<r\}$. This directly answers the
open question "what metric, neighborhood relation, or topology best describes
movement through Possibility."

The metric is cheap: $J$ is $A\times k$ with $A,k\lesssim 48$, and we never
invert $g$ densely — Egress needs only $g^{-1}\nabla U$, obtained by solving the
$k\times k$ SPD system $g\,d=\nabla U$ with a Cholesky factorization ($O(k^3)\approx
5\times10^4$ flops, tens of microseconds). Geodesics for long-range route
planning (§11) use a few natural-gradient steps, not a full BVP solve.

---

## 6. Realization: the World Space field

### 6.1 Structure

Given an effective coordinate $\theta$ and a World Space position
$x\in\mathbb R^2$ (a plane in v1; $x\in S^2$ for a planet later), the world is a
deterministic stack of fields

$$
W(\theta,x)=\Big(\underbrace{z}_{\text{terrain}},\ \underbrace{\kappa}_{\text{climate}},\ \underbrace{h}_{\text{hydrology}},\ \underbrace{u}_{\text{soils}},\ \underbrace{b}_{\text{biome}},\ \underbrace{e}_{\text{ecology}},\ \underbrace{\Omega}_{\text{organism density}}\Big),
$$

each layer a function of $\theta$'s decoded generator parameters and of the
layers above it — the same dependency chain the overview lists (climate →
geology → hydrology → soils → vegetation → food web → organisms → local
variation) and the prototype implements (`world-model.md` §2.6). The **new**
part is that the possibility input is the *global* $\theta$, so there is exactly
one world; spatial structure comes entirely from $x$.

Each layer is a **parameterized spatial basis**. For example terrain elevation
is a fixed multi-octave gradient-noise field whose amplitude/frequency/warp are
decoded from $\theta$:

$$
z(\theta,x)=A(\theta)\sum_{o=0}^{O-1}\lambda_o(\theta)\,\mathcal N\!\big(\Lambda_o(\theta)\,R(\theta)\,x+\delta_o(\theta)\big)\;-\;z_0(\theta),
$$

with $\mathcal N$ a hashed-gradient noise (identical primitive to the
prototype's `terrain.rs`), $A$ overall relief, $\lambda_o$ the octave spectrum,
$\Lambda_o$ per-octave frequency, $R$ an anisotropy/warp matrix, and $z_0$ the
sea datum. Because all of $A,\lambda,\Lambda,R,z_0$ are smooth functions of
$\theta$, a small Egress step smoothly reshapes the *whole* terrain spectrum
rather than editing one region's scalar. Climate, hydrology, soils, and ecology
are analogous parameterized fields consuming their upstream layers; hydrology
carries the same intended-stable **integer flow topology** idea as the prototype
(`world-model.md` §3.11), computed in fixed point so river networks do not
flicker under floating-point drift.

### 6.2 Determinism and caching by bucket

$W$ never consumes a live float $\theta$. It consumes the **bucketed**
coordinate

$$
\hat\theta=\big\lfloor 2^{B}\theta\big\rfloor,
$$

and a *tile* — a square block of $n\times n$ samples over a region of World
Space — is a pure function of $(\hat\theta,\ \text{region index},\ \text{layer},\
\text{version})$, hashed to a dependency key exactly as `world-model.md` §2.6
does, but with the global $\hat\theta$ replacing the per-region vector. A tile is
regenerated **only** when its key changes, i.e. only on a $\hat\theta$-bucket
crossing or a World Space region change. Sub-bucket Egress changes the canonical
coordinate (and the metric, resonance, etc.) but regenerates nothing — the same
"movement inside a bucket does not regenerate tiles" guarantee the prototype
relies on, now global.

### 6.3 The obvious performance hazard, and its fix

A global possibility coordinate has a real danger: if the *entire* visible
window used one $\hat\theta$, then every Egress bucket-crossing would invalidate
**every** visible tile at once — a global reload, which invariant 2 forbids.
The continuity field (§6.4) is precisely what prevents this, and §12 proves the
per-frame regeneration count stays bounded.

### 6.4 The continuity field (the "wake")

Realization does not use the canonical coordinate $\theta_\star$ directly. It
uses a smooth **effective-coordinate field** $\Xi(x,t)\in\Theta$ that lags
$\theta_\star$ near the Traveler and equals it far away. This is the design's
formal home for "places close to the Traveler preserve enough realization
history to make the transition continuous" (§Core experience), and it is
explicitly a *derived continuity artifact*, never the canonical address
(invariant 2; conceptual §Continuity: "Continuity techniques must not change the
Traveler's canonical Possibility or World Space address").

$\Xi$ evolves by **travel-gated relaxation** toward the canonical coordinate.
Writing $s$ for cumulative travel arclength ($ds=\|\dot x_\star\|\,dt$) and
$d(x)=\|x-x_\star\|$ for distance to the Traveler:

$$
\frac{d}{ds}\,\Xi(x,s)=\mu\big(d(x)\big)\,\big(\theta_\star-\Xi(x,s)\big),
$$

with a relaxation profile $\mu$ that is **zero in the near zone and grows
outward**:

$$
\mu(d)=\mu_\infty\,\mathrm{smootherstep}\!\left(\frac{d-r_n}{r_f-r_n}\right),
\qquad
\mathrm{smootherstep}(t)=\mathrm{clamp}(t,0,1)^3\big(6t^2-15t+10\big).
$$

Consequences, all matching the conceptual model:

* **Near the Traveler** ($d\le r_n$): $\mu=0$, so $\Xi$ is frozen at whatever it
  last held — the ground under and around the Traveler does not transform. This
  is invariant 2's "nearby content retains realization history."
* **Far field** ($d\ge r_f$): $\mu=\mu_\infty$, so newly streamed distant tiles
  realize at (essentially) $\theta_\star$ — "newly encountered regions are
  realized according to the Traveler's newer Model State" (§Egress).
* **Transition band** ($r_n<d<r_f$): a smooth gradient of effective coordinates,
  the seam the Visualization blends across.
* **Approach freezes detail.** As the Traveler moves toward a place, its $d$
  falls, $\mu\to0$, and its coordinate stops changing — "fine detail resolves as
  the player approaches" (overview). What was tracking the far coordinate a
  moment ago is pinned as it enters the near zone.

Only travel advances $\Xi$ ($ds=0\Rightarrow$ no change), so standing still
never transforms the world — the "movement supplies the transition budget" rule,
promoted from the prototype's convergence gate but now acting on the single
global coordinate.

Implementation of $\Xi$ is *not* a per-pixel PDE. Because $\mu$ depends only on
$d(x)$ and the driver $\theta_\star$ is shared, $\Xi(x,s)$ depends on $x$ only
through the **history of $d(x)$ along the Traveler's path**. We store $\Xi$ at
tile granularity (one $\hat\theta$ per resident region), integrate the scalar
ODE above per region per frame — $O(\text{resident tiles}\times k)$, a few
thousand multiply-adds — and quantize to $\hat\theta$. This is far cheaper than
the field generation it gates.

---

## 7. Yearnings → a desired attribute vector

A **Yearning** (conceptual §Yearnings) is reconciled into an attribute-space
target by a construction that is **order-independent by design** (invariant 10):
the target is the solution of a weighted least-squares problem whose data are
*summed* over Yearnings and Impressions, and summation does not depend on order.

### 7.1 Per-attribute requests

Each active Yearning $y$ has weight $w_y>0$, a set of source Impressions, and for
each usable attribute $a$ an **Influence** intention and a **Scope** level. Each
(Yearning, attribute) pair emits at most one *soft linear request*: a target
value $\bar\phi_{y,a}$ and a precision $\pi_{y,a}\ge0$.

| Influence | target $\bar\phi_{y,a}$ | precision $\pi_{y,a}$ |
|---|---|---|
| **Accentuate** | prevalence/level implied by Scope, biased **above** the Impression's captured value | $w_y$ |
| **Repress** | Scope level biased **below** captured value (an anti-anchor) | $w_y$ |
| **Hold** | the *current* value $\phi_a(\theta_\star)$ | $w_y\cdot\eta_{\text{hold}}$ (stiff) |
| **Disable** | — | $0$ (contributes nothing) |

Scope maps to a numeric prevalence target through a fixed, monotone table
$\text{singular}\to\text{common}\to\text{pervasive}\ \mapsto\ [0,1]$; for scalar
attributes Scope selects magnitude bands. Hold and Disable are distinct exactly
as the conceptual model requires: Disable sets $\pi=0$ (no preference); Hold sets
a **stiff** request pinning the attribute to its current value, which then
*competes* with other requests through the weights rather than dominating them.

### 7.2 Reconciliation (order-independent)

Aggregate all requests into a diagonal precision matrix and a target:

$$
W_a=\sum_{y}\pi_{y,a},\qquad
\bar\phi_a=\frac{\sum_{y}\pi_{y,a}\,\bar\phi_{y,a}}{W_a}\ \ (W_a>0),
$$

with $\bar\phi_a$ undefined and $W_a=0$ when no Yearning references $a$ (the
attribute is free). Both are **sums over Yearnings**, so the result is invariant
to evaluation order and to duplicates being processed in any sequence — the
conceptual guarantee that "the result must be independent of the order in which
Yearnings or Impressions are evaluated" and that "weights express relative
compromise, not processing priority." This is the honest, well-posed replacement
for the prototype's Emphasize-first/Suppress-last sequential blend
(`world-model.md` §2.4), which is order-sensitive by construction and only
recovers order-independence through an elaborate raw-bit canonical sort.

The pair $(W,\bar\phi)$ — a weighted, possibly conflicting *intent* — is the
input to Egress. It never names a destination coordinate; §8 turns it into a
direction that also respects plausibility, the current state, and the local
structure of Possibility, exactly the reconciliation inputs the conceptual model
lists (§Resolving Yearnings).

---

## 8. Egress dynamics

Egress moves the canonical coordinate along the **natural gradient** of a utility
that combines the Yearning intent with community Attractors, projected to stay
feasible, scaled by Resonance and travel.

### 8.1 The utility

$$
U(\theta)=\underbrace{-\tfrac12\big(\phi(\theta)-\bar\phi\big)^\top W\big(\phi(\theta)-\bar\phi\big)}_{\text{yearning fit}}\;+\;\underbrace{\gamma\,\mathcal A(\theta)}_{\text{attractor pull (§11)}},
$$

where $W=\mathrm{diag}(W_a)$ (free attributes contribute nothing), and $\mathcal
A$ is the Attractor potential of §11. Its gradient:

$$
\nabla U(\theta)=-\,J(\theta)^\top W\big(\phi(\theta)-\bar\phi\big)+\gamma\,\nabla\mathcal A(\theta),
\qquad J=\frac{\partial\phi}{\partial\theta}.
$$

### 8.2 Natural-gradient direction

The raw gradient over-weights sensitive coordinates; we ascend in the metric of
§5, giving a **Gauss–Newton / natural-gradient** direction:

$$
d(\theta)=g(\theta)^{-1}\nabla U(\theta),
$$

computed by one Cholesky solve of $g\,d=\nabla U$. With $S=W$ in the metric this
is exactly the Gauss–Newton step of the least-squares problem "move $\phi$ toward
$\bar\phi$," so Egress heads straight at the reconciled intent in *attribute*
terms while spending minimum world-change to get there.

Because $\mathcal C$ is a chart of the feasible manifold, $d$ is automatically
tangent to feasibility and no world becomes invalid; the residual projection
$\Pi$ (§3.3) is applied after the step only to absorb quantization. "Model
validity takes precedence over literal satisfaction of a Yearning" (§Resolving
Yearnings) falls out: the reachable $\phi$ is confined to $\mathcal F$, so an
impossible combination (giant animals with no productivity) is simply not on the
manifold and the flow settles at the feasible point closest — in metric — to the
request.

### 8.3 Resonance- and travel-gated step

The step length couples Egress to Exploration and gates it by Resonance
(invariants 4, 5; §Egress capability):

$$
\Delta s=\beta\,\rho\,\max\big(\|\Delta x_\star\|,0\big),\qquad
\theta_\star \leftarrow \Pi\!\left(\theta_\star+\Delta s\,\frac{d}{\|d\|_{g}}\right),
$$

where $\Delta x_\star$ is the Traveler's World Space displacement this frame,
$\rho\in[0,1]$ is Resonance (§9), $\beta$ a global rate, and $d/\|d\|_g$ the
unit natural-gradient direction (metric-normalized, so step length is measured in
perceptual distance $d_\Theta$). Zero travel or zero Resonance gives exactly zero
Egress — a rich area lets you transform quickly, a barren one nearly freezes you
in place, and merely waiting changes nothing. This is the prototype's
travel×resonance convergence rule (`world-model.md` §2.5) lifted to the global
coordinate and expressed as a single well-posed flow instead of a per-region
lerp.

### 8.4 Reachability

Integrating §8.3 from $\theta_0$ produces an Egress path
$\theta_\star:[0,\infty)\to\Theta$ that stays on $\mathcal F$ and moves only where
$\rho>0$. The image of all such paths under all admissible Yearning schedules is
$\mathcal R(\theta_0)$ (§2.3). Reachability depends only on Model quantities
($\phi,g,\rho$) — never on the Visualization — satisfying invariant 12.

---

## 9. Resonance, defined intrinsically

Resonance must be "a property of the Traveler's interaction with a Model and its
current Realization … not a property of a particular Visualization"
(§Egress capability and resonance). We define it from Model fields only — never
from rendered geometry or organism *instances* (which are the Visualization's).

Two independent factors:

$$
\rho=\rho_{\text{support}}\cdot\rho_{\text{align}}\ \in[0,1].
$$

**Support** — is there enough living, connectable world around the Traveler?
Integrate the Model's ecological connectivity density $\kappa$ (a field derived
by $W$ from vegetation/productivity, *not* from realized organism entities) over
a neighborhood of the Traveler:

$$
\rho_{\text{support}}=\mathrm{clamp}\!\left(\frac{1}{\pi r_n^2}\int_{\|x-x_\star\|\le r_n}\kappa\big(\Xi(x),x\big)\,dx,\ 0,\ 1\right).
$$

This is "dense ecosystems provide many connection points; sparse environments
make transition difficult" (overview) as a spatial average of a canonical Model
field — evaluated on the effective coordinate, so it reflects the world the
Traveler is actually standing in. It is a cheap sum over resident near-zone
tiles (a scalar reduction the map already has in cache).

**Alignment** — is the requested direction locally feasible? Measure how much of
the desired ascent survives the feasibility/metric geometry:

$$
\rho_{\text{align}}=\frac{\|d\|_{g}}{\|d\|_{g}+\delta}\cdot\cos_+\!\angle\big(d,\ \nabla U\big),
$$

with $\cos_+=\max(\cos\angle,0)$ and $\delta$ a softening constant. When the
Yearning asks for something the manifold cannot supply near $\theta_\star$, $d$
is small (the constraints cancel the gradient) and $\rho_{\text{align}}\to0$ —
"low Resonance indicates ambiguity, incompatibility, or insufficient local
support." When the request is cleanly feasible, $\rho_{\text{align}}\to1$.

This resolves the open question "Is Resonance a threshold, a rate limit, a
precision signal, or some combination?" — here it is simultaneously a **rate
scale** (it multiplies $\Delta s$ in §8.3) and a **confidence signal** (its two
factors report *why* movement is slow: barren surroundings vs. incompatible
request). A threshold variant is the trivial policy $\rho<\rho_{\min}\Rightarrow
\Delta s=0$.

---

## 10. Determinism, identity, and versioning

The design keeps the prototype's valuable **three grades of determinism**
(`world-model.md` §2.9), which map cleanly onto the new structure:

1. **Portable integer identity.** The world address is $\hat\theta\in\mathbb
   Z^k$. All permanent identities — region hashes, species ids, record ids,
   tile dependency keys — are integer folds (SplitMix64, as in the prototype's
   `hash.rs`) over $\hat\theta$, integer World Space region indices, layer id,
   feature index, and version. Identical on native and wasm by construction: no
   float feeds an identity.
2. **Same-platform exact content.** Float field tiles, the metric $g$, the
   Egress direction $d$, Resonance $\rho$, and organism expression reproduce
   bit-for-bit for the same inputs on one target but are presentation-grade
   across targets. Egress math is a small, fixed sequence of operations on
   $\hat\theta$-derived inputs, so a *scripted* Egress is cross-platform to the
   quantization boundary, mirroring the prototype's anchor-reduction contract.
3. **Settled state.** Because tiles are pure functions of their integer
   dependency key and $\Xi$ is a deterministic function of the (integer-logged)
   travel path, a quiescent scripted endpoint is independent of scheduler,
   worker count, and budget — the prototype's schedule-independence property,
   preserved.

Two version axes, as in the prototype: `MODEL_VERSION` changes the identity of
the generated world (any change to $\mathcal D,\phi,W$ that alters output for
fixed inputs) and `RECORD_FORMAT_VERSION` changes serialized schemas. An
Impression stores `MODEL_VERSION` so it is never silently reinterpreted as a
different world (conceptual §Determinism and identity). Per-layer
`algorithm_revision`s allow confined changes to invalidate only downstream
layers.

The determinism split answers the conceptual invariant 8 exactly: identical
Model inputs $(\text{version},\hat\theta,x,\text{time})$ reproduce the same
Realization; the Visualization owns everything float and presentation-grade.

---

## 11. Impressions, Attractors, and dual-space travel

### 11.1 Impressions

An **Impression** (conceptual §Impressions) becomes a small, exact record:

$$
I=\big(\hat\theta,\ \hat x,\ \text{model\&viz version},\ \hat t,\ \{(\text{attr id},\ \phi_a\ \text{quantized})\}\big).
$$

Because the canonical coordinate is now a single global vector, an Impression is
*literally one point* $(\hat\theta,\hat x)\in\hat\Theta\times\mathbb Z^2$ — far
simpler than the prototype's per-region anchor with position, mask, polarity,
radius, and falloff. A Traveler with a compatible Impression re-derives the same
Realization and the same canonical subject by decoding $\hat\theta$ and sampling
$W$ at $\hat x$ (invariants 6, 9). Captured attributes seed Yearning targets
(§7.1): "Accentuate this organism's morphology to pervasive" sets
$\bar\phi_{\text{morph}}$ from the captured value and the Scope.

### 11.2 Attractors

Published Impressions accumulate into an **Attractor field** over Possibility —
a kernel density estimate on the coordinate manifold:

$$
\mathcal A(\theta)=\sum_{i\in\mathcal L} c_i\,\mathcal K_{H_i}\!\big(\theta,\theta_i\big),
\qquad
\mathcal K_H(\theta,\theta_i)=\exp\!\Big(-\tfrac12\,d_\Theta(\theta,\theta_i)^2/H_i^2\Big),
$$

over the shared Impression library $\mathcal L$, with per-cluster weight $c_i$
(visit counts, published Builds, personal subscriptions — §Attractor strength)
and bandwidth $H_i$ that **shrinks as evidence accumulates**. This is the exact
mechanism the conceptual model asks for: "A weak Attractor … may indicate only a
broad region and approximate direction. As evidence accumulates, the Attractor
may resolve toward a precise destination." Concretely $H_i=H_0/\sqrt{1+N_i}$ for
$N_i$ independent visits, so a lone visit gives a diffuse bump (bias only) and a
heavily-trafficked coordinate gives a sharp well whose minimizer *is* an exact
destination — answering "when does a diffuse Attractor become precise enough to
expose an exact destination" ($H_i$ below the quantization step).

$\nabla\mathcal A$ enters the Egress utility (§8.1) with weight $\gamma$, so
Travelers drift toward community activity without holding an exact Impression.
The KDE is evaluated only at $\theta_\star$ each frame over the $O(\text{tens})$
nearest clusters (a bucketed spatial index on $\hat\theta$), so it is cheap.

Attractors are historical and abuse-resistant by the same construction as the
prototype: $c_i$ is built from grid-bucketed, id-keyed, union-merged counters
(CRDT laws: commutative, associative, idempotent), removable when the underlying
published records are removed (invariant 14). This design keeps that systems
layer unchanged.

### 11.3 Dual-space travel

An Attractor may specify both a Possibility region and a World Space location.
The two distances are independent (invariant 3): $d_\Theta$ (§5) and Euclidean
$\|x-x_{\text{target}}\|$. Coordinated arrival (conceptual §Dual-space travel)
is a rate controller: choose Exploration speed and Yearning weighting so that

$$
\frac{d_\Theta(\theta_\star,\theta_{\text{target}})}{\text{egress rate}}\approx\frac{\|x_\star-x_{\text{target}}\|}{\text{explore speed}},
$$

adjusting the two rates without ever equating the two metrics. Whether exact
arrival is possible depends on the Attractor's bandwidth $H$ (diffuse → bias
only; sharp → exact), which is the conceptual model's own answer.

---

## 12. Performance — why this runs in real time

Real-time navigation requires that per **frame** the Model do bounded work in
both spaces. Let $k$ = coordinate dimension ($\le48$), $A$ = attributes
($\le48$), $T$ = resident tiles in the streaming window ($\sim10^3$),
$n^2$ = samples per tile.

**Egress (Possibility), per frame — $O(k^3+Ak)$:**

| Step | Cost | Est. |
|---|---|---|
| Evaluate $\phi(\theta_\star)$ and $J$ | $O(Ak)$ | ~2 µs |
| Assemble/factor $g=J^\top S J+\varepsilon I$ (Cholesky) | $O(Ak^2+k^3)$ | ~30 µs |
| Solve $g\,d=\nabla U$ | $O(k^2)$ | <1 µs |
| KDE $\mathcal A,\nabla\mathcal A$ over nearest clusters | $O(k\cdot\text{clusters})$ | ~5 µs |
| Resonance reductions over near tiles | $O(T_{\text{near}})$ | ~5 µs |

Total Egress ≈ **50 µs/frame** — a rounding error against a 16.6 ms budget. This
is the whole point of a *compact* global coordinate: navigating Possibility is a
tiny dense linear-algebra problem, not a sweep over the world.

**Continuity field $\Xi$, per frame — $O(T\cdot k)$:** one scalar ODE step per
resident tile per coordinate, a few thousand multiply-adds, **~20 µs**.

**Realization (World Space):** identical in character and cost to any
streaming procedural-terrain engine — pointwise noise evaluated per sample,
GPU-parallel, cached per tile and regenerated only on a dependency-key change.
The design adds **no** per-sample possibility cost: $W$ reads a bucketed
$\hat\theta$ constant, not a per-sample field.

**The regeneration-rate bound (the critical claim).** The hazard of §6.3 is
that global Egress could regenerate the entire window. It cannot, because $\Xi$
band-limits it. Across the window, $\Xi$ ranges only over
$[\Xi_{\text{near}},\theta_\star]$, and the spread a tile accumulates while
transiting from far zone to near zone is bounded by the Egress that occurs over
that travel distance:

$$
\big\|\theta_\star-\Xi(x)\big\|_g\ \le\ \beta\,(r_f-r_n),
$$

since Egress advances at rate $\le\beta$ per unit travel (§8.3) and a tile
crosses the band in $\ge (r_f-r_n)$ of travel. The number of distinct
$\hat\theta$ buckets present in the window is therefore at most

$$
N_{\text{buckets}}\ \le\ \frac{\beta\,(r_f-r_n)}{\Delta_\theta}+1,
$$

a small constant fixed by the quantization step $\Delta_\theta$ and the band
width — **independent of window size**. Per frame, tiles regenerate only where
$\Xi$ crosses a bucket boundary, which is a thin annulus in the transition band,
so regenerations per frame are $O(\text{band circumference})$, not $O(T)$. Tuning
$\Delta_\theta$ (or the near/far radii) trades transition smoothness against
regeneration load, and the far-field-first ordering means large changes appear
in the distance and resolve as the Traveler approaches — precisely the intended
experience, now with a provable cost bound.

**Concurrency.** Egress and $\Xi$ run on the main thread (microseconds); tile
generation runs on a work-stealing pool keyed by integer dependency hashes, so
results are schedule-independent (§10) and the executor design of the prototype
(`world-model.md` §3.24) transfers unchanged.

---

## 13. Rust realization sketch

The neutral core (no threads, filesystem, or GPU — the crate-boundary rule of
`AGENTS.md`) exposes:

```rust
/// Compact possibility coordinate: fixed-point, cyclic + bounded axes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Coord { pub q: [i32; K] }          // q_j = round(2^32 * theta_j)

/// Model-facing attribute vector (feasible-manifold chart image).
#[derive(Clone, Copy, Debug)]
pub struct Attrs { pub v: [f32; A] }

pub trait Model {
    fn decode(&self, c: Coord) -> ModelState;             // 𝒟
    fn attrs(&self, c: Coord) -> Attrs;                   // φ = 𝒞
    fn jacobian(&self, c: Coord) -> [[f32; K]; A];        // J (analytic or dual-number AD)
    fn metric(&self, c: Coord) -> Sym<K>;                 // g = JᵀSJ + εI
    fn project(&self, c: Coord) -> Coord;                 // Π (triangular clamp)
    fn realize(&self, c: Coord, region: RegionIx, layer: Layer) -> Tile; // W, cached by key
    fn dep_key(&self, c: Coord, region: RegionIx, layer: Layer) -> u64;  // integer identity
}

/// One reconciled Egress step. Order-independent: builds (W, φ̄) by summation.
pub fn egress_step(
    m: &impl Model, theta: Coord,
    yearnings: &[Yearning], attractors: &AttractorField,
    resonance: f32, travel: f32,
) -> Coord {
    let (w, phibar) = reconcile(m, theta, yearnings);     // §7.2  Σ over yearnings
    let grad = utility_grad(m, theta, &w, &phibar, attractors); // §8.1
    let dir  = cholesky_solve(&m.metric(theta), &grad);   // §8.2  g d = ∇U
    let ds   = BETA * resonance * travel.max(0.0);        // §8.3
    m.project(step(theta, unit_g(&dir, &m.metric(theta)), ds))
}
```

Key data structures: a `BTreeMap<RegionIx, ResidentTile>` streaming window with
each resident carrying its quantized `Xi: Coord`; a `TileCache` keyed by
`dep_key` (pure function of `Coord`); an `AttractorField` backed by a bucketed
`BTreeMap<CoordBucket, Cluster>` for the KDE nearest-cluster query. Jacobians
use forward-mode dual numbers over the $A\!\times\!k$ chart — cheap and exact.
All ordered maps and integer tie-breaks reproduce the prototype's
count-budgeted, run-stable scheduling.

---

## 14. Conceptual invariants — conformance

| # | Invariant | How this design satisfies it |
|---|---|---|
| 1 | One point in Possibility = one complete world | $\theta_\star\in\Theta$ is a single global coordinate; $W(\theta,\cdot)$ is the whole world (§2, §6) |
| 2 | One canonical point; nearby content keeps history | canonical $\theta_\star$; history lives in derived $\Xi$, never authoritative (§6.4) |
| 3 | Possibility and World Space independent metrics | $d_\Theta$ from $g$ (§5) vs. Euclidean $x$; independent by construction (§11.3) |
| 4 | Egress = Possibility, Exploration = World Space | distinct flows $\theta_\star$, $x_\star$ (§1, §8) |
| 5 | Egress coupled to Exploration but owned by neither | coupling is the Traveler policy $\Delta s\propto\|\Delta x_\star\|$ (§8.3), outside $\mathcal M$ and Visualization |
| 6 | Realization carries stable meaning | integer $\hat\theta$ + versioned $\phi$/$W$ contract (§4, §10) |
| 7 | Simulation belongs to Visualization | $W$ yields fields/attributes only; no behavior sim in the Model |
| 8 | Identical Model inputs reproduce Realization | pure functions of $(\text{version},\hat\theta,x,\hat t)$ (§10) |
| 9 | Impression addresses meaningful across Visualizations | $(\hat\theta,\hat x,\text{version})$ decodes identically (§11.1) |
| 10 | Yearnings are weighted, order-independent | summed precision/target least squares (§7.2) |
| 11 | Scope = prevalence, not spatial falloff | prevalence attributes are global $\theta$ components (§4) |
| 12 | Visualization does not change Reachable Possibility | $\mathcal R$ depends only on $\phi,g,\rho$ (§8.4, §9) |
| 13 | Builds are optional Visualization content | Builds attach to Impressions; never enter $\theta$ or $W$ |
| 14 | Attractor evidence historical, abuse-resistant, removable | CRDT-merged bucket counters; KDE weights $c_i$ removable (§11.2) |

## 15. Open questions from the conceptual model — answered or narrowed

* **What is a Model State / Possibility Coordinate?** A compact
  $k$-dim manifold point $\theta$ (address) decoded to rich generator parameters
  $m=\mathcal D(\theta)$ (state). (§2–3)
* **What metric/topology for Possibility?** The pullback Riemannian metric
  $g=J^\top S J+\varepsilon I$; geodesic distance; metric-ball neighborhoods.
  (§5)
* **How is continuity risk / sensitivity exposed?** By $J$ and $g$: directions
  with large $\|J\|$ are high-sensitivity; the Visualization can read them to
  pre-warn or slow Egress. (§5)
* **Yearning attributes and Scope across Models?** A fixed, versioned attribute
  vector $\phi$ with scalar and prevalence kinds; Scope is a monotone map into
  prevalence targets. (§4, §7)
* **How strongly may Hold resist?** A stiff but finite precision
  $\eta_{\text{hold}}w_y$; it competes in the least squares and is overridden
  only by greater aggregate weight or by feasibility. (§7.1, §8.2)
* **Is Resonance threshold / rate / precision?** All three views from one
  definition $\rho=\rho_{\text{support}}\rho_{\text{align}}$; used as a rate
  scale, readable as a confidence signal. (§9)
* **Dual-space coordinated arrival?** A rate controller equalizing normalized
  progress in $d_\Theta$ and Euclidean space without merging them. (§11.3)
* **When does an Attractor become exact?** When KDE bandwidth
  $H_i=H_0/\sqrt{1+N_i}$ falls below the quantization step. (§11.2)

## 16. Relationship to the current implementation

Reused ideas (proven and cheap, carried over conceptually, not by code):
integer SplitMix64 hashing for portable identity; quantization buckets as the
tile-invalidation event; the layered generation DAG with per-layer revisions;
travel-gated transformation and a near/transition/far stability ramp;
schedule-independent pure-function tiles; the three grades of determinism; the
CRDT/atlas sharing laws; the neutral-core crate boundary.

Deliberately replaced:

* **Per-region possibility vector → one global coordinate** (§Central
  departure). This is the defining change; it makes "one world = one point"
  literally true and turns Possibility navigation into $O(k^3)$ linear algebra.
* **`project_plausible` clamp cascade → a feasible-manifold chart** so validity
  is intrinsic, with $\Pi$ demoted to a residual projection. (§3)
* **Emphasize-first/Suppress-last sequential blend → weighted least squares**
  that is order-independent without a canonical raw-bit sort. (§7)
* **Per-region lerp convergence → a single natural-gradient Egress flow** with a
  principled non-Euclidean metric. (§5, §8)
* **Organism-instance-derived resonance → an intrinsic Model-field resonance**,
  removing the dependence on presentation-grade near-field entities. (§9)
* **Per-region anchors → single-point Impressions and a KDE Attractor field.**
  (§11)

The result is a smaller conceptual surface with stronger guarantees: one world
coordinate, one reconciliation solve, one Egress flow, one continuity field, all
bounded per frame and all independent of the Visualization.
