# Phase 4 — Anchors and Player Steering: Implementation Plan

This is the lower-level plan required by section 21 of
[`implementation-plan.md`](implementation-plan.md) before Phase 4 work begins
(it covers the ground of `anchor-system-plan.md`, and the transition/resonance
slice of the possibility-space and entity-realization plans). It expands the
Phase 4 scope in section 20 into concrete interfaces, data layouts, algorithms,
and milestones, grounded in the landed Phase 3 stack
([`phase-3-plan.md`](phase-3-plan.md), ADRs 0007–0010) and the travel-fueled
convergence model ([ADR 0006](docs/adr/0006-travel-fueled-convergence.md)).

Read [`AGENTS.md`](AGENTS.md) first. This plan does not repeat the
crate-boundary rule, the determinism invariant, or the CI contract — it assumes
them and calls out where Phase 4 stresses each.

---

## 1. Goals and non-goals

### 1.1 The question Phase 4 must answer

Phase 1 shipped a deliberately blunt anchor sketch (`anchor.rs`): two kinds —
`Emphasize` and `Suppress` — a per-domain `mask`, radial falloff, and a
two-rule `project_plausible`. Its stated purpose was to prove *the seam between
steering and constraints exists*, not to model steering. Phase 3 then wired all
eight possibility domains into generation and grew the world's organism-level
richness — genomes, rosters, food webs, near-field organisms — but explicitly
deferred **trait-capture anchors and the resonance graph** to Phase 4
(phase-3-plan.md §1.4). Anchors are still dropped as bare `Emphasize`/`Suppress`
pulls toward the fixed bounds `1`/`0`, disconnected from anything the player
actually discovers. Phase 4 asks:

> Can the player **intentionally steer** the world's evolution — capturing the
> traits of the living things and places they choose to remember, emphasizing or
> suppressing them, combining several anchors — so that outcomes are
> **intentional yet surprising** and remain **ecologically coherent**, without
> breaking the continuity, determinism, or invalidation precision the earlier
> phases won? (section 20, Phase 4)

This is the direct validation of the game's **core vision statement**
(Overview): *one continuous journey through possible worlds, steered by carrying
forward the characteristics of discoveries*. It is where the possibility-space
machinery stops being a debug slider and becomes the game. It stresses the
**continuity risk** (section 23.1) hardest of any phase so far — a strong anchor
is a lever on the whole far field — and the answer must be machine-checkable.

### 1.2 Success criterion (from section 20)

> Players can intentionally steer world evolution while outcomes remain
> surprising and ecologically coherent.

Decomposed into machine-checkable properties (asserted by the anchor harness,
§12.3):

- **Intentional:** an anchor *captured from a discovery* and emphasized
  measurably moves the far-field target toward that discovery's traits in the
  masked domains, and the world converges there as the player travels; a
  suppress anchor (anti-anchor) moves it away. The effect is monotone in
  `strength` and fades with distance (falloff).
- **Selective:** steering moves *only* the masked trait categories; unmasked
  domains are untouched — the Phase 2/3 invalidation-precision property, now
  driven by real captured anchors rather than the debug bias slider.
- **Coherent:** the steered-and-projected target always satisfies the expanded
  section 8 plausibility constraints, and the Phase 3 ecology coherence
  invariants (§12.3 there) still hold in *every* steered world — you cannot
  steer the world into an implausible or incoherent ecosystem.
- **Surprising:** constraint projection and anchor combination produce outcomes
  that are *not* the naive per-domain target — relaxation reshapes a captured
  vector, and combining two anchors yields an emergent blend neither captured
  alone. (Checked weakly: projection provably alters some steered vectors, and a
  combined steer differs from either single steer.)
- **Continuity & determinism preserved:** convergence stays travel-fueled
  (ADR 0006) and is additionally resonance-gated so change can never bank up out
  of sight; the continuity replay still passes; steering and capture are
  deterministic and reproducible per run; two-run state-hash equality holds.

### 1.3 Goals

- **Anchor capture** (Overview, section 8): produce an anchor's *trait target*
  from a live discovery — an organism, a rock/landform, a river, an atmospheric
  condition — rather than from the fixed bounds. The captured target is the
  discovery's habitat possibility signature in the masked domains, nudged toward
  what makes *this* discovery distinctive (its deviation from its habitat's
  baseline), so remembering a giant bioluminescent creature pulls the world
  toward large, glowing life, not merely toward "more ecology."
- **Trait masks, emphasis, and suppression** (section 8): generalize the Phase 1
  `Anchor` from `{Emphasize, Suppress}`-toward-a-bound into a captured
  `target` vector plus a per-anchor polarity — emphasize (pull toward target),
  suppress (push away, the anti-anchor), leave-neutral (unmasked). Map the
  in-fiction trait categories (Overview) onto the domain mask honestly.
- **Constraint projection** (section 8): grow `project_plausible` from two rules
  into the real section 8 rule set — vegetation vs rainfall, animal scale vs
  primary productivity, canopy vs soil depth and wind, ice vs temperature,
  wetland vs hydrology — as a fixed, ordered, bounded iterative relaxation. This
  is the second embodiment of section 8's "rule-based constraints and iterative
  relaxation, not ML" (the first was Phase 3's food web).
- **Anchor combination** (section 8): a real combination algorithm turning many
  overlapping anchors into one steering vector — **order-independent** by
  construction, replacing the Phase 1 sequential contraction whose order
  sensitivity was only "mild."
- **Transition controls and resonance** (section 14, Overview "Player Avatar"):
  a transient, locally-built **resonance graph** over the near-window features
  (Phase 3's organisms and aggregate fields), yielding a scalar transition
  capability that **gates convergence** — dense, diverse, anchor-compatible
  surroundings let the player steer strongly; sparse ones hold the world still.
  Plus a distinct deliberate transition-movement mode (Overview, Movement).
- **Debug visibility**: anchor influence fields, the per-domain steering vector
  and its projection at the cursor, the resonance graph overlay, a capture
  readout, and an **anchor harness** that is the phase's machine-checkable
  sign-off — the analogue of the invalidation ledger and ecology harness.

### 1.4 Non-goals (explicitly deferred)

- **Persistence and sharing of anchors** (Phase 5). Captured anchors are
  run-local; the `Storage` trait stays unused. Published expeditions, shared
  anchors, and the community atlas's trait vocabulary are Phase 5 (section 13,
  Overview "Social Features").
- **Routes through possibility space** and the route-attraction field (Phase 5,
  section 13). Phase 4 steers a single explorer's world; it does not record or
  replay paths.
- **Growing the possibility vector.** The 8-domain, one-scalar-per-domain
  `PossibilityVector` is unchanged. Phase 4 captures *into* those eight scalars;
  several distinct in-fiction trait categories therefore collapse onto one
  scalar domain (§4.4). Per-category sub-trait vectors (separate hue vs
  branching within Aesthetics/Morphology) await the vector's growth in a later
  fidelity phase — this is the honest boundary of what Phase 4 can capture.
- **Photography as a real mechanic.** "Capture" in Phase 4 is a debug action
  that anchors the feature under the player/cursor; the camera, framing, and the
  photo-to-anchor UX loop (Overview) are a later game-facing phase. The
  *procedural* content of a capture — the trait target — is what Phase 4 builds
  and validates.
- **Cross-platform anchor identity.** A captured target reads `f32` organism
  expression and environment tiles, so it is **presentation-grade** exactly as
  the habitat signature is (ADR 0010). The portable surface Phase 4 adds is the
  pure `steer` / `project_plausible` / capture *math* (integer-free but
  float-deterministic and golden-fixtured); which *world* a capture yields is
  per-run, per-platform.
- **Real resonance VFX and avatar physics.** The renderer still presents one
  composed debug texture; resonance arcs and the orb are debug overlays, not
  meshes. Line-of-sight is approximated (§7.5), not raycast against geometry
  (there is none yet).
- **GPU steering fields, SIMD** (Phase 6).

---

## 2. Where this sits in the subsystem-plan map

| Section 21 plan | Phase 4 coverage |
|---|---|
| `anchor-system-plan.md` | **Core of Phase 4** — capture, trait masks, combination, projection (§4, §5, §7). |
| `possibility-space-plan.md` | Steering + projection over the existing vector; no new dimensions (§4, §7.2). |
| `entity-realization-plan.md` | Second slice: the resonance graph reads Phase 3's near-field organisms (§6.3, §7.5). |
| `region-streaming-plan.md` | Resonance-gated convergence; a transition mode input (§8.2, ADR 0012). |
| `determinism-and-versioning-plan.md` | New presentation-side surfaces; **no** world-version bump (§9.1). |

The environmental and ecology layers (terrain … ecology L8), the layer
dependency graph, the roster cache, and near-field realization are **unchanged**
in kind: Phase 4 changes *which possibility buckets a region targets*, not the
function from a bucket to a tile. It touches generation only through the
existing `target`/`current`/`converge` machinery.

---

## 3. Architecture overview

Phase 4 adds no generation layer. It grows the **steering front-end** that
computes each region's target vector, and a **transition gate** that governs how
fast realized state moves toward that target:

```text
  Discovery (Phase 3 output)                     Player intent
  ─────────────────────────                      ─────────────
   near-field Organism  ─┐                        trait mask
   habitat signature     ├─▶ capture ─▶ Anchor{ target, mask,     (§4, §5)
   aggregate tiles      ─┘              polarity, strength,
                                        falloff, source }
                                                 │
  Steering front-end (per region, per frame, presentation-side)
  ────────────────────────────────────────────────────────────────
   base = field.sample(coord) + player bias
   steered  = steer(base, anchors, center)      order-independent   (§7.2)
   target   = project_plausible(steered)        section-8 rules     (§7.3)
                                                 │
  Transition gate (near window, transient)
  ────────────────────────────────────────────────────────────────
   resonance = f(near organism density/diversity,               (§6.3, §7.5)
                 anchor compatibility, distance)
   converge_rate = travel × resonance × converge_per_unit       (ADR 0012)
                                                 │
  Existing Phase 2/3 machinery (unchanged)
  ────────────────────────────────────────────────────────────────
   converge → quantized bucket flips → dep-hash staleness →
   topological regen of exactly the declared readers → tiles →
   near-field re-realization
```

Four commitments organize everything, each continuing an earlier commitment:

1. **Steering is presentation-side; identity is untouched.** Anchors, capture,
   combination, projection, and resonance all move a region's `target`/`current`
   possibility vector — runtime `f32` state that has *always* been presentation,
   not identity (possibility.rs). They change *which quantized bucket* a region
   drives, never the deterministic tile-for-a-bucket function. So Phase 4 changes
   no generated output for identical inputs, and `WORLD_ALGORITHM_VERSION` does
   not bump (§9.1) — the same discipline Phase 3 followed by appending, not
   altering.

2. **Capture reads the world; the target is a nudge, not a snapshot.** An anchor
   captured from a discovery targets its habitat's possibility signature *plus a
   bounded deviation* toward what makes the discovery distinctive (§7.1). This
   is what makes steering feel like carrying a memory forward rather than
   copy-pasting a location. Like the habitat signature it reads, capture is
   presentation-grade (ADR 0010).

3. **Combination is order-independent.** Overlapping anchors combine by a
   weighted, commutative rule (§7.2), so the steered vector is a pure function of
   the *set* of anchors and the position — not of placement order. This retires
   the Phase 1 caveat ("order sensitivity is mild but deterministic") and makes
   the two-run replay equality robust to any future reordering (persistence load
   order, shared anchors) without a determinism trap.

4. **Transition never banks change out of sight.** Resonance *multiplies* the
   travel-fueled convergence rate; it never adds. A stationary player's world is
   still perfectly still (zero travel ⇒ zero convergence, ADR 0006), and a
   player in a barren region simply cannot transition — the world holds until
   they reach richer ground. Resonance can only slow or enable transformation,
   never manufacture it, so the stand-still cliff ADR 0006 closed stays closed
   (ADR 0012).

---

## 4. Anchors (world-core)

### 4.1 The generalized anchor

`anchor.rs` grows the Phase 1 struct into the full section 8 shape (trait target,
trait mask, strength, polarity, scope, falloff, source metadata):

```rust
/// What an anchor does to the masked dimensions relative to its captured target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorKind {
    /// Pull masked dimensions toward `target` (make the remembered trait more present).
    Emphasize,
    /// Push masked dimensions away from `target` (the anti-anchor: suppress the trait).
    Suppress,
}

/// Where an anchor was captured from — metadata for legibility now, and the
/// seed of a persistent/shareable discovery record in Phase 5 (§1.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorSource {
    Organism { species: u64 }, // a captured creature (its stable Species::id)
    Landform,                  // rock formation / terrain character
    River,                     // hydrology feature
    Atmosphere,                // climate / sky / weather
    Manual,                    // debug-placed, no discovery (the Phase 1 behaviour)
}

/// A placed steering influence: a captured trait target with smooth radial falloff.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    /// Where the anchor sits in continuous world space.
    pub world_pos: (f64, f64),
    /// The captured possibility target the masked dimensions are pulled toward
    /// (Emphasize) or away from (Suppress). Only masked dimensions are read.
    pub target: PossibilityVector,
    /// Bitmask of affected `PossibilityDomain`s (bit = `domain.index()`).
    pub mask: u8,
    /// Direction of influence relative to `target`.
    pub kind: AnchorKind,
    /// Peak influence at the anchor's center, `0..=1`.
    pub strength: f32,
    /// World-space radius beyond which the anchor has no effect (its scope).
    pub falloff_radius: f64,
    /// What this anchor was captured from (metadata; run-local in Phase 4).
    pub source: AnchorSource,
}
```

`influence(at)` (the C1 radial falloff) and `affects(domain)` are unchanged from
Phase 1. `domain_mask(&[...])` is unchanged. The Phase 1 `Emphasize`/`Suppress`
semantics become the special case `target = 1.0`/`target = 0.0` across the mask —
so the Phase 1 debug keys keep working by constructing `Anchor { target:
neutral-or-bound, source: Manual, .. }` (§8.3), and no behaviour the earlier
phases relied on is lost.

### 4.2 Capture

New module `world-core/src/capture.rs`. Capture is split into a **pure core**
here (given a habitat baseline and a trait deviation, build the target) and a
**runtime gatherer** (read the tiles/organisms to produce those inputs, §6.3):

```rust
/// A bounded per-domain deviation describing what makes a discovery distinctive
/// relative to its habitat baseline — e.g. an unusually large, luminous organism
/// yields positive Morphology and Aesthetics deviations. Values in `[-1, 1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TraitDeviation {
    pub dims: [f32; POSSIBILITY_DIMS],
}

/// Build a captured anchor target: the habitat's possibility signature nudged by
/// the discovery's deviation, on the masked dimensions only. `gain` bounds how
/// far a capture can pull past its habitat (the "distinctiveness" strength).
///
/// Pure and float-deterministic; the target is presentation-grade because
/// `baseline` and `deviation` derive from f32 tiles/organisms (ADR 0010).
#[must_use]
pub fn capture_target(
    baseline: PossibilityVector,
    deviation: TraitDeviation,
    mask: u8,
    gain: f32,
) -> PossibilityVector;

/// The trait deviation of an organism relative to its habitat's expected
/// expression: distinctive size → Morphology, luminance/hue → Aesthetics,
/// activity/aggression → Behavior, trophic role/pressure → Ecology (§7.1).
#[must_use]
pub fn organism_trait_deviation(
    expressed: Expressed,
    genome: Genome,
    habitat_baseline: PossibilityVector,
) -> TraitDeviation;
```

`capture_target` is the honest heart of "carry forward the characteristics of
what you remember": the anchor does not target the raw discovery, and it does not
target the fixed bound — it targets *the world that would make this discovery
typical*, pushed a bounded step toward what made it stand out.

### 4.3 Combination (order-independent) and projection

`steer` is rewritten from the Phase 1 sequential contraction into an
order-independent weighted combination, and `project_plausible` from two rules
into the section 8 set (§7.2, §7.3). Signatures are unchanged, so every caller
(`stream.rs::target_for`, the inspector, the harness) is source-compatible:

```rust
/// Combine a base field sample with every nearby anchor into a steered vector.
///
/// Order-independent: the result is a pure function of the *set* of anchors and
/// `at`, not of slice order (ADR 0011). Each domain's masked emphasize anchors
/// contribute a total-influence-weighted pull toward their combined target;
/// suppress anchors contribute a weighted push away from theirs; the base is
/// blended toward the combined desired value by the saturating total weight.
#[must_use]
pub fn steer(base: PossibilityVector, anchors: &[Anchor], at: (f64, f64)) -> PossibilityVector;

/// Project a steered vector back inside plausible bounds via the section 8 rule
/// set, applied as a fixed, ordered, bounded iterative relaxation (§7.3).
#[must_use]
pub fn project_plausible(v: PossibilityVector) -> PossibilityVector;
```

### 4.4 Trait categories map onto the eight domains

The Overview lists richer trait categories than the vector has dimensions. Phase
4 maps them honestly, collapsing several categories onto a shared scalar domain
and recording the collapse as a known limitation (§1.4):

| Overview trait category | Possibility domain | Note |
|---|---|---|
| Coloration | Aesthetics | hue/luminance share one scalar in Phase 4 |
| Morphology, scale, branching patterns | Morphology | three categories → one scalar |
| Behavior | Behavior | |
| Ecological traits | Ecology | drives vegetation + L8 pressure |
| Climate affinity | Climate | |
| (Rock / landscape) | Geology | slow domain; capture allowed, drift stays stable topology |
| (River / wetness) | Hydrology | |
| (Atmosphere / ocean) | Planetary | |

`capture.rs` exposes a `TraitCategory → mask bit` mapping so the shell and
harness name categories in the fiction's terms while the math stays in the eight
domains. When the vector grows (later phase), the collapsed categories split
without touching the anchor algebra — the mask simply widens.

---

## 5. Public interfaces

### 5.1 `world-core` additions

```text
world-core/src/
    anchor.rs    # Anchor grows target/source; steer order-independent;
                 #   project_plausible → section-8 rule set (§4.1, §4.3, §7.2–3)
    capture.rs   # NEW: TraitDeviation, capture_target, organism_trait_deviation,
                 #   TraitCategory ↔ mask mapping (§4.2, §4.4)
    possibility.rs # unchanged (8 domains, one scalar each)
```

`lib.rs` re-exports `capture_target`, `organism_trait_deviation`,
`TraitDeviation`, `TraitCategory`, `AnchorSource` alongside the existing anchor
exports. All new code is pure, wasm-clean, in the `#[inline] #[must_use]` style.

### 5.2 `world-runtime` changes

```text
world-runtime/src/
    resonance.rs # NEW: transient resonance graph + gate over the near window (§6.3, §7.5)
    stream.rs    # capture_at(); resonance folded into convergence rate;
                 #   transition-mode flag; resonance exposed for viz (§7.5, §8.2)
    budget.rs    # + max_resonance_nodes (resonance graph node cap, §8.3)
    region.rs    # unchanged (target/current/converge already carry steering)
    realize.rs   # unchanged (resonance reads its organisms, does not change them)
```

Key additions:

```rust
// stream.rs — capture the feature at a world position into a run-local anchor.
// Reads the cell's current possibility vector (the habitat baseline), the nearest
// realized organism (for an Organism capture) or the aggregate/terrain tiles, and
// calls world-core capture. `None` if nothing capturable is resident there.
pub fn capture_at(
    &self,
    world_pos: (f64, f64),
    category_mask: u8,
    kind: AnchorKind,
    strength: f32,
    falloff_radius: f64,
) -> Option<Anchor>;

// resonance.rs — a transient, locally-built graph (section 14: NOT stored globally).
#[derive(Debug, Clone)]
pub struct Resonance {
    /// Transition capability at the player, `0..=1` — the gate multiplier.
    pub strength: f32,
    /// Contributing near-field features (organisms/landforms) within reach.
    pub nodes: Vec<ResonanceNode>,
    /// Per-domain compatibility with the active anchor set (why steering here
    /// pulls where it does), for the influence viz.
    pub anchor_compatibility: f32,
}
pub fn resonance_at(&self, player: (f64, f64), anchors: &[Anchor], budget: &Budget) -> Resonance;
```

`RegionMap::update` gains a `transition_mode: bool` argument threaded from the
shell (§8.2); `RegionState`, `GenerationStatus`, job ids, and the results
channel are unchanged. Resonance is computed once per frame as a pure read of the
settled caches (like near-field realization) and folded into `converge`'s rate.

### 5.3 `renderer`, `platform-native`, `tools`, `platform-web`

- **Renderer:** unchanged — still one composed texture.
- **`platform-native` (`main.rs`, `viz.rs`, `panel.rs`):** capture keys (anchor
  the organism/feature under the cursor with the selected trait-category mask and
  polarity); category-selection and polarity-toggle keys; a transition-mode
  toggle. New viz overlays: an **anchor influence field** channel (summed
  influence, tinted by dominant steered domain), the **resonance graph** overlay
  (arcs from the player to contributing near-field nodes, brightness = strength),
  and a **steering-vector readout** at the cursor (base → steered → projected per
  domain). The panel shows each active anchor's source, target, mask, polarity,
  and strength; the resonance strength and transition mode; and a capture flash.
- **`tools`:** `wer-inspect --steer X Y` dumps, for a scripted anchor set, the
  base / steered / projected possibility vectors at the position and which
  domains moved — the steering analogue of `--layers`. A new **anchor harness**
  binary drives the §12.3 steering scenarios (the Phase 4 sign-off).
- **`platform-web`:** exports one parity sample — `steer_sample()` (the steered +
  projected vector for a fixed base and a fixed scripted anchor set) — pinned to
  the native golden. This is the *portable* steering math surface; capture from a
  live world is deliberately not a parity export (§9.3), like signature
  derivation (ADR 0010).

---

## 6. Data layout

### 6.1 Anchors

Anchors live where they do today: a `Vec<Anchor>` owned by the app shell (and by
the harness/inspector for scripted runs), passed by slice into `update`. The
struct grows from Phase 1's ~40 bytes to carry a `PossibilityVector` target (32
bytes) and a small `source` enum — a few dozen bytes each, and a world holds at
most a handful of active anchors, so anchors are not a memory concern and are
never cached in the region map. Combination cost is `O(anchors × masked domains)`
per region per frame, evaluated inside the existing unbudgeted `retarget` pass
(§8.1).

### 6.2 Resonance

The resonance graph is **transient and per-frame** (section 14: "generated
locally using spatial queries … not stored as a global graph"). It is rebuilt
each frame from the near-window organisms and aggregate tiles, capped at
`max_resonance_nodes` contributing features, and dropped at end of frame — a
bounded few KB that never accumulates. Only the scalar `Resonance.strength` (and,
for viz, the node list for the current frame) is retained.

### 6.3 What capture and resonance read

Both are pure reads of already-settled state — they add no cache and no new tile:

- **Capture** reads, at the capture cell: the region's `current` possibility
  vector (the habitat baseline), the covering `CellEcology` (roster + dominant
  species + aggregate pressures, phase-3-plan.md §11) for an Organism capture, or
  the terrain/geology/hydrology/climate channels for a Landform/River/Atmosphere
  capture. `organism_trait_deviation` compares the captured organism's expressed
  genome to its habitat's roster baseline.
- **Resonance** reads the near-window `organisms` map (Phase 3 Tier B) and the
  aggregate `diversity`/`biomass` channels, plus the active anchor set for the
  compatibility term.

Neither ever mutates a tile or enters the results channel — the Phase 3
guarantee that the main thread is the only cache writer is preserved.

---

## 7. Algorithms

### 7.1 Capture (§4.2)

`capture_at` classifies the capture cell and gathers the baseline
(`region.current`) and a deviation:

- **Organism capture:** find the nearest realized organism to `world_pos` in the
  region's `organisms` list; compute `organism_trait_deviation` — expressed size
  above/below the habitat's mean realized size → Morphology deviation; luminance
  and hue distance from the roster's mean → Aesthetics; activity/aggression →
  Behavior; the cell's herbivore/predator pressure and the species' trophic role
  → Ecology. Each deviation is bounded to `[-1, 1]`.
- **Landform / River / Atmosphere capture:** deviation is the corresponding
  channel's departure from its neighbourhood mean (a steep, hard massif → high
  Geology; a wide wet river → high Hydrology; a cold clear sky → low Climate),
  masked to that category's domain.

`capture_target` then sets, for each masked domain, `target_i = clamp(baseline_i
+ gain · deviation_i, 0, 1)` and leaves unmasked domains at neutral (they are
never read by `steer`). The result is a full `PossibilityVector` but only its
masked entries matter. Presentation-grade throughout (reads f32).

### 7.2 Combination / steering (§4.3, order-independent)

For each domain `i` in the union of anchor masks, gather the anchors that affect
`i` with positive influence `w_a = anchor.influence(at)`:

- **Emphasize** anchors contribute a weighted target: `num += w_a · target_{a,i}`,
  `den += w_a`, `W⁺ += w_a`. Their combined desired value is `num / den`.
- **Suppress** anchors contribute a weighted *reflected* target (push away):
  reflect `target_{a,i}` about the base to `2·base_i − target_{a,i}` (clamped),
  accumulate the same way into `W⁻` and a combined suppress-desired.

The domain result blends the base toward the combined emphasize-desired by a
saturating weight `s⁺ = 1 − ∏(1 − w_a)` over the emphasize anchors, then toward
the reflected suppress-desired by `s⁻`, and clamps to `[0, 1]`. Because both the
weighted means and the saturating products are symmetric functions of the anchor
set, `steer` is **order-independent** — a property asserted directly (§12.4),
retiring the Phase 1 order caveat. The saturating (rather than additive) blend
keeps the result in `[0, 1]` without the sequential contraction, and keeps a
single strong anchor from being diluted by many weak far ones.

### 7.3 Constraint projection (§4.3)

`project_plausible` clamps to `[0, 1]`, then applies the section 8 rules as a
**fixed, ordered, bounded relaxation** — a small constant number of passes
(enough for the coupled rules to settle; no convergence loop, mirroring the food
web's single-pass discipline, phase-3-plan.md §7.4). Rule order is part of the
deterministic contract and is golden-fixtured. The rule set (superseding the
Phase 1 two-rule sketch):

| # | Constraint (section 8) | Rule |
|---|---|---|
| 1 | Wetland vs hydrology | surface wetness (Hydrology) capped by planetary ocean fraction (kept from Phase 1) |
| 2 | Vegetation vs rainfall | Ecology capped by available moisture (Hydrology + Climate) (kept, widened) |
| 3 | Animal scale vs primary productivity | Morphology (body scale) capped by a function of Ecology (productivity) |
| 4 | Canopy vs soil depth and wind | Ecology (canopy component) capped by Geology-derived soil/exposure proxy |
| 5 | Ice vs temperature | Climate cold + low Planetary jointly bound Hydrology's liquid-water expression |

Rules are ordered so later rules see earlier relaxations (rule 3 reads the
already-capped Ecology). The output is *plausible by construction*, which is
exactly what §12.3's coherence invariants assert — and it is where "surprising"
comes from: a naive captured target (huge animals in a barren world) is
relaxed into a coherent nearby world (moderately larger animals, and only where
productivity allows), not the literal impossible one.

### 7.4 The steering front-end wiring (unchanged shape)

`stream.rs::target_for` already computes `project_plausible(steer(base, anchors,
center))` for every region each frame (`retarget`). Phase 4 changes only what
`steer` and `project_plausible` *do*; the plumbing — base = field sample + player
bias, per-region evaluation, farthest-first travel-fueled convergence toward the
target — is untouched. A captured anchor thus rides the *identical* path a debug
bias does today: it moves the target, convergence drifts `current` toward it as
the player travels, bucket flips dirty exactly the declared reader layers
(ADR 0007), and near-field organisms re-realize from the new aggregates
(phase-3-plan.md §7.6). This is why Phase 4 needs no generation change.

### 7.5 Resonance (§6.2, section 14)

Each frame, `resonance_at` builds the transient graph over the near window:

- **Nodes:** the near-field organisms within a resonance radius of the player
  (Phase 3 Tier B), plus a few aggregate landform/terrain features, capped at
  `max_resonance_nodes` by nearest-first (deterministic order).
- **Density & diversity terms:** node count and species entropy among the nodes
  (dense, varied surroundings → high resonance; a bare ice sheet → near zero).
- **Distance term:** a smooth falloff so faraway nodes contribute less
  ("distance" in section 14).
- **Anchor-compatibility term:** how well the local ecology matches the active
  anchor targets — steering toward a world the player is *near an example of*
  resonates more strongly ("compatibility with active anchors", section 14).
- **Line-of-sight:** approximated by an aggregate occlusion proxy (dense canopy
  between player and a node attenuates it); not a geometric raycast (§1.4).

`strength ∈ [0, 1]` combines these bounded terms. It is a pure read of settled
caches, order-independent, and — like realization — trivially parallelizable if
profiling ever demands it (§10).

### 7.6 Resonance-gated convergence (ADR 0012)

`converge`'s rate becomes `converge_per_unit · travel · resonance.strength`,
clamped to `converge_rate_cap`. Consequences, each preserving an ADR 0006
invariant:

- Zero travel ⇒ zero rate regardless of resonance: the stationary world is still
  perfectly still (resonance multiplies, never adds).
- Zero resonance ⇒ zero rate regardless of travel: in a barren region the player
  moves but the world holds — "sparse environments make transition difficult or
  impossible" (Overview). Change resumes when they reach richer ground; because
  travel gates too, nothing banked up while they crossed the barren stretch.
- Rich, anchor-compatible surroundings ⇒ full rate up to the cap: steering is
  strongest exactly where the world is densest, which is where the player wants
  to dwell and shape.

Transition mode (§8.2) additionally scales `converge_per_unit` down for
deliberate, slow reality-transition movement versus fast free exploration
(Overview, Movement) — free movement surveys, transition movement steers.

---

## 8. Scheduling and budgets

### 8.1 Steering rides the existing per-frame passes

Capture is a one-shot, event-driven action (a keypress / future photo), not a
scheduled job. Combination and projection run inside the existing unbudgeted
`retarget` pass — a few extra arithmetic operations per resident region per
frame, already the cheapest pass in `update` (a few hundred bilinear samples
today). No new job type, no scheduler change.

### 8.2 Transition mode and the update signature

`RegionMap::update` gains `transition_mode: bool` (and continues to take
`travel`, which the shell already computes from displacement). In free mode the
convergence coefficient is unscaled; in transition mode it is scaled down for
slow deliberate steering. The shell toggles the mode on a key; the replay and
harness script it. Everything else in the step order (integrate, evict, load,
retarget, converge, dispatch, realize) is unchanged; resonance is computed
between `retarget` and `converge` so the rate sees the current frame's gate.

### 8.3 Budgets

`Budget` gains `max_resonance_nodes` (a cap on resonance graph size, so a dense
biome does not build an unbounded graph — the analogue of
`max_realize_organisms`). `FrameStats` grows `resonance_strength`,
`resonance_nodes`, and `anchors_active` — the raw material for the panel and the
harness. No generation budget changes: Phase 4 dispatches the *same* layer jobs
Phase 3 does, only with steered targets, so `max_regen_cost` and the per-layer
costs are unchanged and re-validated by the §13 benches.

---

## 9. Determinism and versioning

### 9.1 No world-version bump

Phase 4 changes **no existing layer's generated output for identical inputs**: it
alters only which quantized buckets a region *targets*, never the deterministic
function from a bucket to a tile. Terrain … Ecology tiles are bit-identical to
Phase 3 for the same buckets, so every existing golden fixture stays blessed and
`WORLD_ALGORITHM_VERSION` **stays at 2** — the same append-don't-alter discipline
Phase 3 followed (phase-3-plan.md §9.1). The steering functions `steer` and
`project_plausible` *are* changing behaviourally, but they compute presentation
state (a region's target vector), which has never been a golden-fixtured world
identity — their outputs get **new** Phase 4 fixtures (§12.1), and updating them
is not a re-bless of any Phase 2/3 identity fixture. (A casual re-bless of an
environmental or ecology fixture during Phase 4 is a determinism bug, per
AGENTS.md.)

### 9.2 What is deterministic vs presentation-grade

- **Portable, golden-fixtured, wasm-parity-tested:** `steer` and
  `project_plausible` as pure functions of `(base, anchor set, position)` —
  float-deterministic and identical on native and wasm for the *same inputs*
  (the `steer_sample()` parity export, §12.5). `capture_target` given an explicit
  `(baseline, deviation, mask, gain)`.
- **Presentation-grade, per-platform, replay-hash-checked only:** which anchor a
  live `capture_at` produces (it reads `f32` organism expression, `CellEcology`,
  and environment tiles, inheriting the habitat signature's knife-edge residual,
  ADR 0010), the resonance strength (reads the presentation-grade organism set),
  and therefore which steered world a run produces. Deterministic and
  reproducible within a run and platform; not asserted cross-platform.

### 9.3 The identity ledger (extended)

Phase 4 adds no integer world identity. It adds one portable *math* surface
(`steer_sample`) and two presentation-grade surfaces (live capture, resonance),
extending the ledger exactly as Phase 3 did with `genome_sample` (portable) and
signature derivation (presentation-grade). The community atlas's cross-platform
anchor/trait vocabulary is a Phase 5 problem, solved then with the same move ADR
0010 names — quantize the classification inputs into portable bands before
capture.

### 9.4 New ADRs

- **ADR 0011 — Anchors capture trait targets and combine order-independently.**
  Records generalizing `{Emphasize, Suppress}`-toward-a-bound into `target` +
  polarity; that captured targets are the habitat baseline nudged by a bounded
  discovery deviation (not a snapshot, not a bound); that `steer` is rewritten as
  an order-independent weighted combination (retiring the Phase 1 order caveat so
  future load/share order cannot perturb the replay); and that captured targets
  are presentation-grade (ADR 0010 lineage), run-local until Phase 5.
- **ADR 0012 — Resonance gates transition; it multiplies the travel-fueled rate.**
  Extends ADR 0006. Records that `converge_rate = travel × resonance × k`, so
  resonance can slow or enable transformation but never manufacture it (the
  stand-still cliff stays closed); that the resonance graph is transient and
  locally built (section 14), never a global stored structure; and that
  transition mode scales the deliberate-steering rate distinctly from free
  movement. Notes the one-way door: anything later wanting resonance to *drive*
  change (rather than gate it) must revisit this ADR.

---

## 10. Threading model

Unchanged in kind from Phase 3 (§10 there). The two new computations are pure
main-thread reads of settled state:

- **Steering** (combination + projection) is per-region pure arithmetic in the
  existing `retarget` pass — no shared mutable state, trivially parallel if ever
  needed, but cheap enough to stay serial.
- **Resonance** is a pure read of the near-window organism set and aggregate
  tiles (like realization), main-thread in Phase 4, Web-Worker-compatible by
  construction (section 19): each frame's graph is an independent function of the
  current caches and anchor set.

Capture is an event handler that reads the resident caches and returns an owned
`Anchor`; it never mutates the map. Sequencing repeats the earlier phases'
de-risking: every milestone lands and passes the replay under `InlineExecutor`
first; the threaded path is re-validated by the same tests afterward.

---

## 11. Debug visualization and tools

- **Map channels / overlays** (`viz.rs`): an `Influence` channel (summed anchor
  influence, tinted by the dominant steered domain, so an anchor's reach and
  which trait it pushes read at a glance); a `Resonance` overlay (arcs from the
  player to contributing near-field nodes, brightness = strength — the visible
  "orb resonating with nearby reality", Overview); and a per-cursor steering
  readout painting base → steered → projected so the effect of an anchor and its
  projection is legible. Capture flashes the captured feature.
- **Panel**: each active anchor (source, target vector, mask as trait-category
  names, polarity, strength, falloff); resonance strength and node count;
  transition mode; and the cursor cell's base/steered/projected vectors.
- **`wer-inspect --steer X Y`**: for a scripted anchor set, dump base / steered /
  projected vectors at the position and the per-domain deltas and which
  constraint rules fired — the steering analogue of `--layers`, making the
  capture→steer→project chain *legible*.
- **Anchor harness** (`tools`): headless runner for the §12.3 steering scenarios
  — the Phase 4 sign-off tool, alongside the still-passing invalidation ledger
  and ecology harness.

---

## 12. Testing strategy

### 12.1 Golden determinism fixtures (extend `determinism.rs`)

New known-answer fixtures (no existing fixture re-blesses, §9.1): `steer` output
for a fixed base and a fixed scripted anchor set (covering emphasize, suppress,
and overlap); `project_plausible` output for fixed steered vectors that trip each
section-8 rule; `capture_target` for a fixed `(baseline, deviation, mask, gain)`;
and `steer`'s order-independence encoded as *the same* golden for two anchor
orderings.

### 12.2 Continuity replay (extend, must stay green)

The Phase 3 script and assertions run unchanged, plus a steering leg:

- A scripted **capture → emphasize → travel** sequence: after the player
  captures an organism and travels, the far-field target in the masked domains
  moves measurably toward the capture, and two runs produce a bit-identical
  state hash (steering is reproducible).
- **Selectivity**: unmasked domains' realized values are unchanged by the anchor
  (within epsilon) — the precision property under real anchors.
- **Resonance-gated continuity**: with resonance forced to zero (barren scripted
  region), a travelling player produces no convergence; with resonance high,
  bounded convergence — and neither banks a discontinuity at the pinned boundary
  (the ADR 0006 assertion, now through the resonance gate).

### 12.3 Anchor harness (the Phase 4 success criterion)

Scenario families over a settled window, each machine-checked:

**Intentional / selective steering:**

| Scenario | Expected effect |
|---|---|
| Emphasize a captured organism (Morphology/Aesthetics mask) | far-field target moves toward the capture in *those* domains; unmasked domains unchanged; effect monotone in strength |
| Suppress the same capture (anti-anchor) | far-field target moves *away* from the capture in the masked domains |
| Two anchors, different captures | combined steer differs from either alone (emergent blend); result independent of placement order |
| Falloff | a region beyond `falloff_radius` is untouched; influence decreases monotonically with distance |

**Coherence (hold for every steered region):**

| Invariant | Assertion |
|---|---|
| Plausible target | every projected target satisfies all section-8 rules (§7.3 post-conditions) |
| Ecology still coherent | the Phase 3 §12.3 coherence invariants hold in the steered world (steering cannot break the food web) |
| Diversity retained | a strongly-steered window still meets the Phase 3 diversity floor (over-steering does not flatten all habitats to one) |
| No stable-trio steer under fast domains | emphasizing Ecology/Morphology/Behavior/Aesthetics never moves terrain/geology/drainage (precision, extended) |

**Transition / resonance:**

| Scenario | Expected effect |
|---|---|
| Dense vs barren region, equal travel | dense region converges faster (higher resonance) |
| Stationary player | zero convergence regardless of resonance or anchors (ADR 0006) |
| Anchor-compatible surroundings | resonance strength higher than for incompatible surroundings |

Plus a budget test: a dense biome builds a resonance graph of ≤
`max_resonance_nodes` and steering adds no per-frame regen cost beyond what the
same bucket flips cost in Phase 3.

### 12.4 Unit tests

`steer` order-independence (permuted anchor slices give identical output);
`steer` bounded to `[0, 1]`; emphasize raises toward / suppress lowers away from
`target` on masked dims only; each section-8 projection rule fires at its
boundary and is idempotent (projecting a projected vector is a fixed point);
`capture_target` neutral deviation reproduces the baseline on the mask;
`organism_trait_deviation` bounded to `[-1, 1]`; `TraitCategory ↔ mask` mapping
round-trips; resonance strength bounded and monotone in node density; the
resonance-gated rate is zero when travel *or* resonance is zero.

### 12.5 Native ↔ wasm parity

`platform-web` exports `steer_sample()` (steered + projected vector for a fixed
base and fixed anchor set), pinned to the native golden in the existing parity
test. Live `capture_at` and resonance are **not** exported — they are
presentation-grade by decision (§9.2, ADR 0010/0011), and asserting them
cross-platform would bake in a guarantee Phase 4 does not make.

### 12.6 CI

The existing contract, unchanged: fmt, clippy `-D warnings`, native check+test,
wasm32 check of the neutral crates + `platform-web`. New benches build in CI but
are not timing-gated.

---

## 13. Profiling and metrics

- Per-frame steering time (combination + projection over the resident window),
  resonance build time and node count, and capture latency.
- Criterion benches: `steer` over a representative anchor set and window,
  `project_plausible` over vectors that trip every rule, `capture_target` /
  `organism_trait_deviation`, and `resonance_at` for a dense near region. These
  confirm steering stays inside the unbudgeted `retarget` pass and calibrate
  `max_resonance_nodes`.
- Panel/telemetry grow anchor count, resonance strength/nodes, and transition
  mode; the steering pass time joins the per-pass breakdown.

---

## 14. Native and browser constraints

Unchanged obligations, restated where Phase 4 stresses them: all anchor, capture,
steering, projection, and resonance code is pure and wasm-clean (CI-enforced);
`Vec<Anchor>` and the transient resonance graph are bounded and non-accumulating
(section 19's "no large monolithic allocations"); steering and resonance are
resumable/interruptible by construction (they are stateless per-frame reads);
resonance parallelizes if ever needed (§10). The `platform-web` shell grows only
the one `steer_sample` parity export. Capture and resonance never touch the
filesystem or threads — they read the same in-memory caches the render pass does.

---

## 15. Risks (mapping section 23)

| Risk | Phase 4 manifestation | Mitigation |
|---|---|---|
| 23.1 Continuity | A strong anchor yanks the far field into a visible cliff | Travel-fueled *and* resonance-gated convergence (ADR 0006/0012); projection keeps targets plausible; near pinned; replay asserts no boundary discontinuity under steering (§12.2). |
| 23.3 Dependency explosion | An anchor touching many domains regenerates broadly | The mask limits steered domains; the Phase 2 dep-hash/budget machinery is unchanged; the harness machine-checks that a masked steer touches exactly the declared readers (§12.3). |
| 23.5 Determinism drift | Combination order or capture float diverges native vs wasm | `steer`/`project` order-independent and parity-tested (`steer_sample`); live capture and resonance declared presentation-grade with the ADR 0010 upgrade path (§9.2). |
| 23.6 Memory growth | Resonance graph or anchor list grows unbounded | Transient per-frame graph capped at `max_resonance_nodes`; anchors are a handful, never cached (§6). |
| 23.4 Platform divergence | Resonance assumes threads / geometry for LOS | Pure per-frame read, main-thread, Web-Worker-compatible; LOS approximated from aggregate occlusion, not raycast (§7.5). |

The phase-specific risk: **legibility-vs-surprise tension** — projection and
combination strong enough to guarantee coherence can make steering feel either
mechanical (no surprise) or unpredictable (no intent). Mitigation: the harness
asserts *both* an intentional-response property (masked domains move toward the
capture, monotone in strength) *and* a surprise property (projection provably
reshapes naive targets; combined anchors blend emergently) — neither can be won
by sacrificing the other, the same both-floors discipline Phase 3 used for
coherence-vs-diversity.

---

## 16. Incremental milestones

Each keeps CI green (including wasm32), keeps the continuity replay, the Phase 2
invalidation ledger, and the Phase 3 ecology harness passing, and preserves the
crate-boundary and determinism invariants. No milestone re-blesses a Phase 2/3
fixture (§9.1).

- **M1 — Generalized anchors + capture core.** `anchor.rs` grows `target` /
  `source`; `steer` rewritten order-independent; `capture.rs` with
  `capture_target`, `organism_trait_deviation`, and the `TraitCategory ↔ mask`
  mapping; ADR 0011; steer/capture goldens + the `steer_sample` parity export;
  order-independence unit test. Pure world-core, no runtime change. *Exit:*
  order-independent steering; capture math deterministic; parity native == wasm;
  Phase 1 Emphasize/Suppress reproduced as the bound-target special case.
- **M2 — Constraint projection.** `project_plausible` grown to the section-8 rule
  set as a fixed bounded relaxation; per-rule and idempotence unit tests;
  projection goldens. *Exit:* every projected target satisfies all rules and is a
  fixed point; the naive-vs-projected difference (surprise) is demonstrable.
- **M3 — Capture wired into the runtime + shell.** `capture_at`; capture /
  category / polarity keys; anchor influence viz + panel; `wer-inspect --steer`.
  *Exit:* capturing an organism and emphasizing it measurably moves the far-field
  target in the masked domains and only those; the world converges there as the
  player travels; unmasked domains and the stable trio are untouched.
- **M4 — Resonance + transition controls.** `resonance.rs`; resonance-gated
  convergence; transition-mode toggle; `max_resonance_nodes`; resonance overlay;
  ADR 0012. *Exit:* dense regions transition faster than barren ones under equal
  travel; a stationary player still produces zero change; no boundary
  discontinuity appears under gating.
- **M5 — Anchor harness + sign-off.** The intentional/selective/coherence/
  transition scenario harness (§12.3); the steering replay leg; benches
  calibrating the steering pass and `max_resonance_nodes`. *Exit:* every §12.3
  property holds over a settled, steered window — intentional response,
  selectivity, coherence, diversity retention, and resonance gating hold
  simultaneously.

**Phase 4 is done when** M1–M5 are complete, CI is green (native + wasm32,
goldens, parity, continuity replay, Phase 2 ledger, Phase 3 ecology harness,
Phase 4 anchor harness), and the success criterion holds with evidence: the
player can capture the traits of discoveries and steer the world toward or away
from them, outcomes are intentional (masked domains move, monotone in strength)
yet surprising (projection and combination reshape naive targets) and remain
ecologically coherent (section-8 plausibility + the Phase 3 invariants hold in
every steered world), while continuity, determinism, and invalidation precision
stay exactly as tight as the earlier phases left them — the steering foundation
Phase 5's routes, shared anchors, and community atlas will build on.
