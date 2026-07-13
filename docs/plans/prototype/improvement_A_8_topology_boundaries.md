# Improvement A.8 — Stable topology and ordinary region boundaries

**Status:** Completed

## 1. Purpose and roadmap scope

This plan implements item A.8 in the prioritized improvement roadmap in
[`docs/world-model.md`](../../world-model.md): **Make stable topology and
ordinary region boundaries satisfy their stated guarantees**. It closes:

- finding 9, because the elevation which decides drainage directions and
  accumulation order currently crosses several floating-point thresholds
  before it becomes an integer; and
- finding 19, because a Terrain tile currently applies one region-wide
  Planetary/Geology vector and Hydrology/Soils compute one-sided slopes at tile
  edges.

The implementation is one deliberately versioned correction. It does not try
to solve apron-truncated drainage accumulation (finding 20), every remaining
verification gap (roadmap A.13), GPU refinement parity (finding 34), richer
terrain, erosion, or the later browser runtime.

The implementation must be developed in the dedicated A.8 worktree, committed
as one commit, fast-forward merged to `main`, and pushed before A.9 begins. Do
not edit `docs/plans/prototype/implementation-plan.md` or any
`docs/plans/**/phase-N-plan.md` file. Those are historical plans.

## 2. Current behavior and why a combined change is required

### 2.1 Float-derived permanent routing

`world_core::drainage::routing_elevation_cm` currently performs this chain:

```text
control-point integer seeds
  -> f32 RNG values and f32 possibility bilerp
  -> float plausibility/project/requantize
  -> f64/f32 Perlin coordinates, fade, dot products, and fBm sum
  -> f32 possibility relief/sea-level scaling
  -> f32 * 100 and round()
  -> i32 centimeters
  -> integer flow routing and accumulation order
```

The final direction pass is integral, but a half-centimeter float threshold can
still change an edge and an entire downstream tree. Compiling one fixed sample
for wasm does not make that structurally portable.

### 2.2 Region-constant Terrain inputs

`world_runtime::generate_layer` reconstructs one dequantized possibility vector
from the central region and passes it to every Terrain cell. Adjacent regions
therefore multiply the same continuous relief by different constants right up
to their boundary. A loose base-field difference already permits a height
step; different steering, convergence history, session restoration, or a
preserve can make it much larger.

The current `slope_at` then reads only the local elevation tile. Interior cells
use centered differences, while the first and last rows/columns use one-sided
differences. That derivative discontinuity feeds both Hydrology and Soils and
can then alter wetness, fertility, and biome classification.

### 2.3 Why not use a cache-local border taper

A self-contained alternative would write

```text
terrain_p(x) = base_field_p(x)
             + C1_edge_bump(x) * (region_current - region_base)
```

where the bump and its first derivative vanish on all four borders. It avoids
cross-region dependency keys, but forces every edge and corner back to the
anchor-free world. Legitimate steering is attenuated on a visible 256-unit
grid, and every region develops the same base-world frame regardless of its
neighbors. It also still needs a new analytic derivative implementation if
slope is to avoid local-tile one-sided differences.

Use a **3 by 3 authoritative possibility halo** instead. Region possibility
samples live at region centers; bilinear interpolation of those absolute
samples is continuous through the boundary between centers and preserves both
neighbors' histories. The same snapshot supplies a one-cell elevation ghost
ring, making slope a normal centered difference everywhere. This is more
runtime bookkeeping than a taper, but it preserves the intended steering
model and the bookkeeping can be made exact with dependency hashes and one
central invalidation helper.

## 3. Decisions and invariants

The implementation must preserve all of the following.

1. Drainage routing elevation contains no `f32`, `f64`, float conversion,
   float comparison, or float rounding before an `i32` centimeter result is
   returned. The separate `DrainageTile::accum_bilinear` expression path may
   remain floating point because it is downstream presentation/simulation and
   cannot change topology.
2. The fixed routing evaluator is a pure function of world algorithm version,
   possibility-field spacing, integer region coordinate, and fixed integer
   constants. The enclosing cache key additionally carries the Terrain and
   Drainage algorithm revisions.
3. Routing retains the existing eight-neighbor, strictly downhill,
   cardinal-times-10/diagonal-times-7, coordinate-hash tie-break, local-minimum,
   apron, and accumulation rules. Only the source elevation algorithm changes.
4. Ordinary Terrain uses the current quantized Planetary and Geology buckets
   of the absolute 3 by 3 authoritative region-center neighborhood. Parked
   authority counts as authority; field capacity never changes Terrain input.
5. A missing absolute halo coordinate has one exact fallback: its anchor-free
   `PossibilityField` sample, projected/requantized under the existing base
   rules, then reduced to Planetary and Geology buckets. The lookup is by
   absolute coordinate, so every overlapping halo sees the same fallback.
   Presence itself does not fold into the key: loading authority with identical
   buckets is content-inert.
6. Terrain reconstructs bucket centers and bilinearly samples those region
   center values at every core and ghost position. The interpolation cell is
   selected from the absolute world position, not from the tile's local side,
   so evaluating the same boundary position through either overlapping halo
   performs the same fetches and operations.
7. Terrain produces both Elevation and a stored Slope channel in one job under
   one Terrain dependency hash. Slope is a Terrain output, not an undeclared
   read performed later by Hydrology or Soils.
8. Slope uses centered differences over a one-cell elevation ghost ring on all
   four sides and corners. Neighbor core samples and corresponding ghost
   samples must be bit-identical for the same world position.
9. A Terrain dependency key includes all 18 ordered halo buckets (row-major
   absolute offsets `dy=-1..=1`, then `dx=-1..=1`, Planetary before Geology),
   plus the existing world/layer/revision/coordinate/resolution fields.
   Duplicate bucket values remain folded in their positions.
10. A change to one authoritative region's Planetary or Geology bucket
    invalidates Terrain and its declared transitive dependents for every
    field-active region whose 3 by 3 halo contains it. Climate-only and other
    fast-domain changes do not fan out through the Terrain halo.
11. Loading, unloading, restoring, or materially preserve-snapping authority
    invalidates that same 3 by 3 consumer closure. A false-positive notification
    is legal because the exact dependency key clears it without regeneration.
12. In-flight work remains advisory. A neighbor change alters the recursively
    expected key, retires matching Terrain/dependent dispatches where possible,
    and causes any orphan result to fail ADR 0019 integration validation.
13. Macro drainage keys include possibility-field spacing. Switching the field
    configuration cannot reuse a macro tile generated from a different base
    field.
14. `WORLD_ALGORITHM_VERSION` remains 2. Terrain and Drainage are independently
    versioned layer changes, so both `LayerDecl::algorithm_revision` values move
    from 0 to 1. All other layer revisions remain 0.
15. Re-bless only fixtures which actually include the corrected Terrain tile
    key/output, Drainage key/output, downstream derived output, or a new A.8
    parity fold. Origin feature hashes, gradient/control/lithology seeds,
    standalone constant-vector `elevation`, genomes, records, anchor signatures,
    route attraction, and record bytes must remain unchanged.
16. Native and wasm execute the same parity probes. Merely compiling wasm is no
    longer described as a parity test.
17. The GPU atlas remains derived presentation and retains its existing packed
    13-channel layout. Slope is authoritative CPU input for Hydrology/Soils but
    is not uploaded or exposed as a map view in this item.

## 4. Fixed-point drainage routing elevation

### 4.1 Isolate the identity-grade evaluator

Add `crates/world-core/src/routing.rs` (or an equivalently isolated module) for
the no-float elevation path and re-export only the API needed by drainage and
tests. Keeping it separate makes accidental float use conspicuous in review.
The public shape is:

```rust
pub fn routing_elevation_cm(
    field: &PossibilityField,
    region_x: i32,
    region_y: i32,
) -> i32;
```

`drainage.rs` may continue to re-export this function for source compatibility,
but both the scalar single-cell call and the macro 50 by 50 fill must call this
one evaluator. Delete the SIMD-float relief fill from the topology path; do not
retain two routing-elevation implementations.

### 4.2 Fixed representation and arithmetic policy

Use signed Q30 for noise coordinates, gradient components, interpolation
weights, relief, and possibility scaling:

```text
ONE = 1 << 30
```

Use `i64` for stored Q30 values and `i128` for every multiply, weighted sum,
and scale conversion. Define a small private arithmetic vocabulary rather than
scattering shifts:

- `mul_q30(a, b)`;
- `lerp_q30(a, b, t)`;
- `round_div_signed(numerator, denominator)`; and
- checked/narrowed conversion to `i64`/`i32` with debug assertions and tests
  near the supported `i32` bounds where the required neighborhood remains in
  range.

Choose and document one exact signed rounding rule (round to nearest, ties away
from zero) in those helpers. Every negative intermediate must use that helper;
do not rely on implementation-looking signed shifts or truncating `/` to imply
the contract. Addition and multiplication must be checked during development;
the final implementation can use proven-bounded operations with comments that
show the bound.

### 4.3 Integer possibility-field buckets

Add an integer-only sampling helper on `PossibilityField`, scoped for routing:

```rust
pub(crate) fn routing_bucket(
    &self,
    region: RegionCoord,
    domain: PossibilityDomain,
) -> u16;
```

Derive a control-point component from the exact same `control_point_seed` and
SplitMix stream as the ordinary field:

1. advance the stream in stable `PossibilityDomain::ALL` order;
2. take the high 24 bits used by `Rng::next_f32`, but retain them as an integer
   in `[0, 2^24)`;
3. bilinearly combine the four values using non-negative integer weights
   `(cell-fx)*(cell-fy)`, etc., over denominator `cell_regions^2`;
4. map the exact rational value to one of 4096 buckets with floor semantics;
   and
5. construct the bucket center in Q30 as `(2 * bucket + 1) / 8192`, which is
   exact at Q30 (`(2 * bucket + 1) << 17`).

The helper deliberately defines the permanent topology field directly. It does
not call `PossibilityField::control_point`, `sample`, `project_plausible`,
`PossibilityVector::quantize`, or any float API. Current plausibility rules do
not alter Planetary or Geology; a future rule which wants to alter stable
topology must gain an integer counterpart and a deliberate Drainage revision.

Use wide intermediates so supported nonzero `cell_regions` values do not
overflow their weight products. Preserve Euclidean division for negative
coordinates. Add tests at lattice points, off-lattice positive and negative
coordinates, spacing 1/default/non-power-of-two, and large coordinates.

### 4.4 Integer hashed-gradient fBm

Retain the Terrain identity inputs and spectral shape, but make their arithmetic
explicitly integral:

- use `terrain::gradient_seed(ix, iy, octave)` for the fixed eight-direction
  lookup;
- axial gradients are `(±ONE, 0)` / `(0, ±ONE)`;
- diagonal components use one named Q30 `1/sqrt(2)` constant, pinned by a unit
  test;
- derive octave offsets from the same high 20 seed bits. Since the existing
  offset is `bits / 2^14` lattice units, it is exact in Q30 by shifting the
  integer bits 16 places;
- express a level-0 region center in octave lattice Q30 directly from
  `(2 * region + 1) * 2^octave / 32`, without first constructing a world-space
  float;
- use Euclidean Q30 floor/fraction extraction for negative positions;
- evaluate corner dot products, quintic fade, x/y lerps, and the named Q30
  `sqrt(2)` scale through the common arithmetic helpers; and
- combine five octaves with exact integer weights `16, 8, 4, 2, 1` and divisor
  31, which is the current normalized `1, 1/2, ..., 1/16` spectrum.

This fixed evaluator is a versioned Drainage algorithm, not a claim of bitwise
equality with the old float Terrain presentation. Its job is to provide stable
terrain-shaped ordering for rivers.

### 4.5 Convert relief and P/G to centimeters

Apply the existing conceptual Terrain coupling in Q30:

```text
tectonic = 0.5 + G
sea_shift = 120 * (P - 0.5)
elevation = 600 * relief * tectonic - sea_shift
```

Convert Q30 world units to centimeters with the shared signed rounding rule,
then range-check/narrow to `i32`. Add known-answer fixtures for relief,
Planetary/Geology buckets, and final centimeters so an innocent constant or
rounding change cannot silently reroute the world.

### 4.6 Drainage dependency key

Change the dependency API to include the field recipe:

```rust
pub fn drainage_dep_hash(
    macro_coord: RegionCoord,
    drainage_revision: u16,
    terrain_revision: u16,
    field_cell_regions: u32,
) -> u64;
```

Fold `field_cell_regions` in a documented fixed position after the two layer
revisions and before coordinate-derived output, or provide a dedicated drainage
fold that documents the full order. Update `drainage_dep_hash_default` to use
`PossibilityField::DEFAULT_CELL_REGIONS`. Tests must show that macro coordinate,
both revisions, and field spacing independently change the key.

`RegionMap` must remember the active `PossibilityField` recipe before its first
integration pass. At the top of `update`, synchronize the stored field. On a
spacing change:

- retire every in-flight macro job;
- make every field-active Hydrology closure depend on the new macro key;
- invalidate field-active Terrain closures because missing-halo fallback
  buckets may change; and
- let exact keys clear false positives where a complete authoritative halo is
  field-independent.

All expected-key, dispatch, and integration paths must consult the stored
recipe. A custom field cannot accidentally integrate a default-field macro.

## 5. Continuous Terrain possibility halos

### 5.1 Snapshot type

Add a small, owned, `Debug + Clone + PartialEq + Eq` snapshot in
`world-runtime/src/generate.rs`:

```rust
pub struct TerrainPossibilityHalo {
    center: RegionCoord,
    // row-major dy -1..=1, dx -1..=1; [Planetary, Geology]
    buckets: [[[u16; 2]; 3]; 3],
}
```

Give it explicit constructors/accessors rather than exposing index arithmetic
throughout the runtime. It must:

- reject/non-support non-level-0 centers;
- fetch a bucket pair by absolute level-0 coordinate within the halo;
- return a complete `PossibilityVector` with only P/G replaced when a core
  Terrain helper needs the existing elevation scaling; and
- produce the exact 18-value dependency-key sequence.

Add `terrain_halo: Option<TerrainPossibilityHalo>` to `LayerInputs`. It is
`Some` only for Terrain. Terrain generation with a missing halo is a failed
input snapshot just like a missing declared tile: return no channels and leave
the scheduler able to retry. Other layers must not inspect it.

### 5.2 Authoritative and fallback values

Build a halo on the main runtime thread at both expected-key calculation and
job submission. For each absolute coordinate:

1. if `RegionMap::regions` contains authority, use its `current` P/G buckets
   regardless of `GenerationStatus`; otherwise
2. sample the currently stored `PossibilityField` at that coordinate, apply
   the existing anchor-free base projection/requantization, and use its P/G
   buckets.

Do not read a neighboring field tile or cache admission status. Do not use the
neighbor's `target`: Terrain represents realized `current` history. Do not use
anchors or bias in fallback; a missing coordinate has no realized steered
history yet. Loading it later replaces the fallback and notifies dependents.

Expected-key calculation and `submit_layer` must call the same halo builder.
The submitted owned snapshot must be the object whose 18 buckets produced the
recorded dispatch hash.

### 5.3 Per-cell interpolation

For a world position `(x, y)`, compute center-lattice coordinates:

```text
u = x / REGION_SIZE - 0.5
v = y / REGION_SIZE - 0.5
x0 = floor(u), fx = u - x0
y0 = floor(v), fy = v - y0
```

Fetch the P/G bucket centers at `(x0,y0)`, `(x0+1,y0)`, `(x0,y0+1)`, and
`(x0+1,y0+1)` from the absolute halo, then bilerp in one fixed x-then-y
operation order. This helper is the only presentation Terrain possibility
sampler. It handles core cell centers, a shared exact boundary, and the one-cell
ghost ring for every supported nonzero resolution.

When an axis lands exactly on a region center (`fraction == 0`), fetch that
single center on the axis rather than an unused second endpoint. This keeps the
resolution-1 ghost ring inside the 3 by 3 snapshot without changing the value;
pin the exact-center branch in tests. Do not fetch an out-of-halo value and
multiply it by zero.

Add tests which construct overlapping left/right and bottom/top halos from one
absolute bucket map and assert exact `to_bits()` equality for P, G, and final
elevation at the same boundary and corner positions. Include negative world
coordinates and deliberately opposing P/G buckets.

### 5.4 Terrain dependency hash

Add `terrain_dep_hash` beside `layer_dep_hash`, or a typed equivalent, rather
than pretending the central region's two buckets describe the output:

```rust
pub fn terrain_dep_hash(
    region: RegionCoord,
    terrain_revision: u16,
    halo_buckets: &[u16; 18],
    resolution: u16,
) -> u64;
```

Use the standard dependency-hash basis/world/layer/revision/coordinate/
resolution prefix, followed by all 18 values in the order fixed in section
3. Existing downstream layer hashes continue to fold the resulting Terrain
hash as a declared input. Add mutation tests for every one of the 18 slots,
resolution, coordinate, and revision.

`expected_layer_hash_inner` special-cases only `LAYER_TERRAIN` to build this
typed key. All other layers keep the existing `layer_dep_hash` path. This is
also the key used to accept/reject both Elevation and Slope because they are
one atomic Terrain output.

## 6. One Terrain job produces Elevation and Slope

### 6.1 Channel layout and provenance

Append, do not insert, the new channel:

```rust
pub const CHANNEL_SLOPE: usize = 13;
pub const CHANNEL_COUNT: usize = 14;
```

Appending preserves every existing channel index. `layer_channels(TERRAIN)`
becomes `[CHANNEL_ELEVATION, CHANNEL_SLOPE]`. Hydrology and Soils retrieve the
Slope input tile supplied through their existing declared Terrain dependency;
delete `slope_at` and all local one-sided reconstruction.

Both Terrain output tiles carry the identical Terrain dependency hash and
integrate atomically with the existing multi-channel result behavior. Missing
either output makes `RegionTiles::layer_hash(TERRAIN)` incomplete, so dependency
repair regenerates both.

### 6.2 Rolling ghost rows

Avoid a full `(n+2)^2` long-lived scratch allocation. For x coordinates from
cell `-1` through `n`, keep three elevation rows (previous/current/next) and
rotate them while producing each core row:

1. call the existing differential-tested `simd::fbm_row` for the `n+2` world x
   positions at the current y;
2. for each lane, sample P/G from the immutable halo and apply
   `terrain::elevation_from_relief` in its fixed scalar expression order;
3. copy current indices `1..=n` into Elevation; and
4. compute Slope from `(right-left)/(2*step)` and
   `(next-previous)/(2*step)`, then `sqrt(dx*dx + dy*dy)`.

The first and last core cells use ghost samples at real neighboring cell-center
positions; no denominator or operation-order branch exists at a tile edge.
The same code runs for every row and column.

Keep fBm's SIMD optimization. Region-varying P/G makes the old
`elevation_row(xs, y, one_p)` inappropriate, but the dominant hashed-noise row
kernel remains shared. Add a scalar reference patch generator in tests and
assert exact Elevation/Slope bits against the rolling-row path over odd/even
resolutions, negative coordinates, uniform halos, adversarial halos, and all
four borders. If a new row scaling helper is vectorized, it must gain an ADR
0016 scalar twin and differential test; a scalar possibility-scaling tail is
acceptable unless measurement proves it material.

### 6.3 Border and downstream oracle tests

Add non-vacuous focused tests at resolution 8 and the production resolution:

- the right ghost sample of a left tile equals the first core sample of its
  right neighbor at the exact same world position, and vice versa;
- analogous top/bottom and diagonal-corner samples match bit-for-bit;
- stored edge slope equals a manually computed central difference which reads
  the corresponding neighbor core elevation, not the old one-sided formula;
- the chosen fixture proves the old one-sided value is different;
- Hydrology wetness/river and Soil depth/fertility at each boundary cell equal
  a direct oracle supplied with the stored cross-border Slope bits; and
- Biome at the same cells equals direct classification of those corrected
  inputs.

Neighboring cell values need not be identical—they are different world
positions. The exact guarantee is that overlapping evaluation of the same
position is identical and every boundary cell is generated from the same
global centered stencil a monolithic patch would use.

## 7. Cross-region invalidation and lifecycle handling

### 7.1 One central helper

Add a `RegionMap` helper which receives an absolute source coordinate whose
P/G authority/fallback may have changed. It visits the nine potential Terrain
consumers centered at `source + (-1..=1, -1..=1)` in deterministic row-major
order. For each field-active resident consumer it:

- sets `dependents_closure(LAYER_TERRAIN)` dirty;
- moves `Ready` to `Generating`;
- retires in-flight Terrain and transitive dependent jobs using the existing
  cancellation machinery; and
- retires canonical/presentation organism publications only through the
  existing L8 invalidation path, not through a new ad hoc identity rule.

Parked consumers need no field dirty state because activation already starts a
complete generation epoch, but parked **sources** remain valid halo authority
for active neighbors.

Where practical, compare the old and new P/G bucket pair first. Calling the
helper on an unchanged pair is still correct; exact expected keys must clear the
false-positive hint. Correctness must not rely on avoiding notifications.

### 7.2 Convergence

During convergence, retain the existing per-region direct-domain dirty mask.
Additionally collect every coordinate whose Planetary or Geology bucket
changed. After releasing mutable region borrows, call the halo invalidation
helper for each collected source in coordinate order. A fast-domain-only flip
must leave neighboring Terrain keys and dirty masks unchanged.

### 7.3 Preserve winner changes

`apply_effective_preserve_signature` already compares old and snapped buckets.
When P/G changes, invalidate the nine Terrain consumers in addition to the
snapped region's existing declared-domain dirtiness. Same-bucket normalization
must retain in-flight work and exact tiles. A winner-owner change with the same
signature remains inert under A.2.

Removing the last preserve without changing `current` does not alter halo
content and needs no Terrain fan-out. Later ordinary convergence will notify if
P/G actually crosses a bucket.

### 7.4 Load, unload, and session restoration

On authority insertion, compare the old fallback P/G pair with the inserted
`current` pair and notify the halo closure if they differ (or notify
unconditionally and rely on keys). This applies to ordinary initial load and a
preserved initial load.

Before/after complete authority removal beyond `unload_radius`, switch that
absolute coordinate back to fallback and notify the same closure. Field
capacity parking is explicitly not removal and must not notify or change a
halo value.

`restore_region` can replace, insert, or park an authority record. Capture its
old effective pair (authority or fallback), install the snapshot, compare the
new pair, and notify. Session order reversal must settle to the same Terrain
keys and bits.

### 7.5 Late work and recovery regressions

Add focused runtime tests for:

- a queued Terrain result dispatched from the old neighbor halo, followed by a
  neighbor P/G flip; the result is rejected and the corrected job settles;
- the same scenario with cancellation disabled;
- a field-active tile beside a parked authoritative neighbor, proving tight and
  roomy field-cache configurations use the same halo and settle identically;
- authority load replacing fallback and unload restoring fallback;
- preserve snap, same-bucket preserve normalization, and session restore;
- field spacing change invalidating macro topology and only Terrain tiles whose
  effective fallback inputs changed (false positives may clear); and
- a Climate-only source flip which does not invalidate neighboring Terrain.

## 8. Ordinary history-divergent seam integration test

Add a private `world-runtime::stream` test or a focused tools integration test
which uses two adjacent, ordinary, non-preserved regions. Establish authority,
then drive or explicitly inject two different realized P/G bucket histories
through the same internal mutation path used by convergence. Do not use a
preserve as a shortcut; assert both coordinates are not overridden.

Settle the map and require:

1. the two Terrain dependency keys contain the shared absolute halo values;
2. evaluating the shared border through either submitted halo yields exact P,
   G, and Elevation bits;
3. each stored edge Slope matches the combined two-region central stencil;
4. the boundary Hydrology, Soil, and Biome samples match the direct corrected
   oracle;
5. reversing which ordinary region acquired its history first produces the
   same settled keys and samples; and
6. parking the neighbor's fields without dropping its authority changes none
   of the above.

Use deliberately distant P/G buckets and choose a border where relief and
slope are nonzero. Assertions must state the actual differing inputs and prove
the test would have caught the old constant-vector height step/one-sided slope.

## 9. Native/wasm parity must execute

### 9.1 Expanded topology probe

Keep `drainage_routing_sample`, updating its deliberate golden for the fixed
algorithm. Add a broader `drainage_topology_sample` export which folds:

- fixed routing elevations over a coordinate matrix spanning positive,
  negative, control-cell-boundary, and non-power-of-two-field cases; and
- direction, accumulation, and content hash for at least three macro tiles on
  both sides of the origin (every 48 by 48 cell contributes through the content
  hashes).

Fold counts/field spacings/coordinates before values with a new fixed basis so
a missing loop cannot collide with a shorter sample. This is an additive
identity golden in `world-core/tests/determinism.rs` and `platform-web`; it does
not replace focused flow tests.

### 9.2 Wasm test harness

Pin the harness dependencies instead of installing an unversioned latest tool:

- workspace/target-specific dev dependency `wasm-bindgen-test = "=0.3.76"`
  (matching the locked wasm-bindgen 0.2.126 family); and
- CI runner `wasm-pack` exactly `0.13.1`, installed with an exact version
  (`cargo install wasm-pack --version 0.13.1 --locked`) or an equivalently
  version-pinned installer.

Add `crates/platform-web/tests/wasm_parity.rs`, compiled only on wasm32, with
`#[wasm_bindgen_test]` tests configured for Node. Invoke every public parity
probe, not just drainage:

- origin feature;
- terrain gradient and possibility control seeds;
- lithology;
- single-cell and many-cell drainage topology;
- genome and food web;
- steering and canonical anchor signature;
- record codec/shared steering; and
- route attraction.

Compare each to the same named constants used by native platform-web tests.
Keep constants in one shared source or an explicit shared assertion helper so
native and wasm expectations cannot drift independently.

After the existing three wasm `cargo check` commands, CI must run:

```sh
wasm-pack test --node crates/platform-web
```

The job name and documentation must say “wasm check + parity test.” Node is a
real wasm engine for these pure exports; a browser renderer/runtime remains out
of scope.

If any existing float parity probe disagrees when actually executed, fix the
portable implementation or narrow the documented parity surface with an ADR.
Do not delete the assertion merely to land the runner.

## 10. Versioning and fixture procedure

### 10.1 Revisions

Make these exact table changes in `world-core/src/layer.rs`:

```text
Terrain  algorithm_revision: 0 -> 1
Drainage algorithm_revision: 0 -> 1
all other revisions: remain 0
WORLD_ALGORITHM_VERSION: remain 2
```

Terrain's new per-cell halo and Slope output are one layer-local algorithm
change. Drainage's fixed evaluator is a separate layer-local algorithm change.
Downstream hashes change automatically through declared dependencies; do not
bump every dependent layer merely because its inputs changed.

### 10.2 Deliberate re-blessing

Before editing expected values, run the focused tests once and record the
failures. Review every changed fixture against the intended key/data flow. Then
update:

- Terrain dependency-hash known answers and any complete Terrain tile/output
  fixtures;
- Drainage dependency hash, routing elevations, direction strings,
  accumulation rows, tile content hash, and platform-web routing sample;
- the new many-cell topology fold;
- downstream settled hashes or exact derived samples which intentionally read
  corrected Terrain/Slope; and
- payload/cost benchmark numbers after measurement.

Do not change unrelated seed, hash, steering, genetics, record, route, codec,
or standalone constant-vector elevation goldens. A diff which changes one of
those is an implementation error unless separately explained and approved.

### 10.3 Architecture record

Add accepted ADR 0027, “Fixed-point drainage and halo-sampled Terrain
boundaries,” and index it in `docs/adr/README.md`. It must:

- build on ADRs 0003, 0008, 0016, 0017, 0019, and 0023;
- supersede the float-elevation part of ADR 0009 while retaining its macro
  topology, local direction, apron, and expression decisions;
- define the integer routing inputs/rounding and field-spacing key;
- define the 3 by 3 realized-current halo, anchor-free missing-authority
  fallback, 18-bucket fold, and Slope provenance;
- state that Terrain/Slope remain presentation-grade floats while Drainage
  routing elevation is identity-grade integer math;
- document neighbor invalidation and parked-authority behavior; and
- record the layer revision choices and wasm execution gate.

Do not edit the accepted text of ADR 0009. Its index status may say
“Superseded in part by 0027,” with the new ADR carrying the explanation.

## 11. Cache, pool, renderer, and performance consequences

### 11.1 Field payload and pool

With Slope appended, a complete 32 by 32 region contains 14 `f32` channels,
one `u8`, and one `u16` per cell:

```text
32^2 * (14 * 4 + 1 + 2) = 60,416 bytes
```

This replaces the current 56,320-byte logical payload. Because
`full_region_payload_bytes` uses `CHANNEL_COUNT`, admission, scale ceilings,
pool plateau math, and tests should update through the shared helper rather
than acquire another literal. Terrain jobs now request two pooled `f32`
outputs. Add/adjust pool hit/miss and complete-payload tests.

The three rolling ghost rows are job scratch and are not falsely described as
part of the cache payload ceiling. Finding 32 remains open: the target is still
logical payload, not total heap.

### 11.2 Layer costs and benchmarks

Extend the Criterion suite with:

- fixed single routing elevation;
- whole fixed-point drainage macro tile;
- Terrain tile with a uniform halo; and
- Terrain tile with an adversarial varying halo including Slope.

Run the relevant benchmarks on the same local machine before and after. Keep
the existing M4 measurements as historical entries and append an A.8
measurement note to `docs/plans/prototype/perf-baseline.md`. Recalibrate the
Terrain and Drainage `LayerDecl::cost` values to the repository's approximately
25-microsecond unit using measured full-job time (round conservatively upward),
not taste. Cost is scheduling metadata and does not fold into hashes.

The fixed routing path will no longer use `simd::fbm_row`; record that expected
tradeoff honestly. Optimize integer rows only if profiling shows it is needed
and the integer result remains exact.

### 11.3 GPU and headless presentation

Do not expand the four `rgba32float` atlas planes or WGSL channel selectors for
Slope. In `platform-native/src/gpumap.rs`:

- keep the existing 13 display-channel-to-plane mapping;
- explicitly skip `CHANNEL_SLOPE` during packing rather than indexing past a
  13-element table;
- keep biome/dominant presentation-presence bits and shader masks unchanged;
  and
- include the Terrain hash in `region_key` as before (Elevation and Slope have
  one provenance key, so no independent slope upload is needed).

Update packing tests to prove all 13 presented floats, biome, and dominant are
present while Slope is intentionally CPU-only. CPU map, screenshots, replay,
and POV terrain continue reading Elevation. Hydrology/Soils read Slope before
the renderer. There is no GPU readback or authoritative GPU dependency.

## 12. Concrete file changes

| File | Required change |
|---|---|
| `crates/world-core/src/routing.rs` | New integer-only Q30 routing elevation evaluator, integer possibility sampling bridge, exact rounding helpers, and known-answer/bound tests. |
| `crates/world-core/src/lib.rs` | Declare/re-export the fixed routing API needed by drainage/parity tests. |
| `crates/world-core/src/possibility_field.rs` | Add integer control-component/bilinear routing-bucket sampling without calling float field APIs. |
| `crates/world-core/src/drainage.rs` | Replace float/SIMD routing fill with the fixed evaluator; preserve direction/accumulation rules; update docs/tests and compatible re-export. |
| `crates/world-core/src/dephash.rs` | Add typed 18-bucket Terrain key; fold field spacing into Drainage key; extend field-sensitivity tests. |
| `crates/world-core/src/layer.rs` | Bump Terrain and Drainage revisions to 1 and recalibrate measured costs. |
| `crates/world-core/tests/determinism.rs` | Deliberately update affected Terrain/Drainage goldens and add the multi-coordinate topology golden; prove unrelated goldens unchanged. |
| `crates/world-core/benches/generation.rs` | Benchmark fixed routing/macro generation and the corrected Terrain-facing primitives as feasible. |
| `crates/world-runtime/src/generate.rs` | Add Terrain halo snapshot/sampler, append Slope, rolling ghost-row Terrain output, remove one-sided slope reconstruction, and make Hydrology/Soils consume Slope. |
| `crates/world-runtime/src/lib.rs` | Re-export `CHANNEL_SLOPE` and any test/tool-facing halo APIs that genuinely need to be public. |
| `crates/world-runtime/src/stream.rs` | Store/synchronize field recipe; build halo for key+dispatch; centralize nine-consumer invalidation across convergence/load/unload/preserve/session; reject stale neighbor jobs; add lifecycle and ordinary-history seam tests. |
| `crates/world-runtime/benches/update.rs` | Add or adapt full Terrain/settle cases so measured layer costs include halo/Slope work. |
| `crates/platform-native/src/gpumap.rs` | Skip CPU-only Slope in the unchanged atlas layout; retain correct presence masks and tests. |
| `crates/platform-web/src/lib.rs` | Add broad topology parity probe, share golden assertions, and update deliberate fixed-routing expected values/wasm export. |
| `crates/platform-web/tests/wasm_parity.rs` | Execute every parity probe under wasm-bindgen-test in Node. |
| `Cargo.toml`, `crates/platform-web/Cargo.toml`, `Cargo.lock` | Pin/add target-only `wasm-bindgen-test` consistently with centralized dependencies. |
| `.github/workflows/ci.yml` | Install pinned wasm-pack 0.13.1 and execute Node wasm parity after all wasm checks. |
| `crates/tools/src/replay.rs` and relevant tools tests | Carry the appended channel through snapshots/hashes and add exact ordinary boundary assertions without weakening existing replay bounds. |
| `crates/tools/src/scale.rs` | Let shared payload math absorb Slope and keep schedule/cache equality gates meaningful under neighbor invalidation. |
| `docs/adr/0027-fixed-point-drainage-and-halo-terrain.md` | Record the accepted replacement decisions. |
| `docs/adr/README.md` | Index ADR 0027 and mark ADR 0009 partially superseded in the index only. |
| `docs/plans/prototype/perf-baseline.md` | Append measured post-A.8 cost/payload results without rewriting Phase 6 history. |
| `README.md` | Document fixed topology, actual wasm parity command/gate, and the unchanged versioning rule. |
| `AGENTS.md` | Correct machine-facing topology/parity/channel claims if they would otherwise remain stale. |
| `docs/world-model.md` | Update the formal model, channel/payload/slope/Terrain/Drainage/runtime/cache/verification descriptions; mark A.8 and findings 9/19 completed without overclaiming A.13/20/33/34. |

If implementation shows another source file with a hard-coded 13-channel array,
presence bit, payload, or local slope calculation, update it as part of this
same coherent change. Do not edit historical implementation or phase plans.

## 13. Test matrix and validation

### 13.1 Focused tests during implementation

Run at minimum:

```sh
cargo test -p world-core routing
cargo test -p world-core drainage
cargo test -p world-core --test determinism
cargo test -p world-runtime generate
cargo test -p world-runtime stream
cargo test -p platform-native gpumap
cargo test -p platform-web
cargo test -p tools --test continuity
```

Focused assertions must cover:

- integer routing buckets and Q30 helpers, including negative and extreme
  coordinates;
- fixed elevation and macro topology known answers;
- every Drainage key input and every Terrain halo key slot;
- overlapping same-position halo/border/corner exactness;
- centered edge slope and downstream Hydrology/Soil/Biome oracles;
- ordinary history-divergent neighbors and parked authority;
- load/unload/preserve/session/convergence/field-change invalidation;
- stale result rejection with cancellation on/off;
- payload/pool and GPU atlas omission; and
- native parity constants plus executed wasm parity constants.

### 13.2 Harnesses

Run the gameplay and scale sign-off tools because Terrain and every downstream
layer intentionally change:

```sh
cargo run --bin wer-ledger
cargo run --bin wer-anchor
cargo run --bin wer-vault
cargo run --release --bin wer-scale
cargo run --release --bin wer-scale -- --report
```

The exact settled hashes may be deliberately new, but equality across executor,
worker count, budget, cancellation, amortization, tier, and tight/roomy field
capacity remains mandatory. Route/capture/tier shared-record invariants from
A.5-A.7 must remain exact.

### 13.3 Full CI-equivalent gate

From a clean worktree with the pinned toolchain:

```sh
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets
RUSTFLAGS="-D warnings" cargo check --workspace
RUSTFLAGS="-D warnings" cargo test --workspace
RUSTFLAGS="-D warnings" cargo check -p world-core -p world-runtime -p platform-web \
  --target wasm32-unknown-unknown
wasm-pack test --node crates/platform-web
```

The CI workflow must perform the last command with wasm-pack 0.13.1. A local
environment without that pinned binary is not grounds to skip the committed CI
step; install the exact version and run it before completion.

Run `git diff --check` and inspect `git status --short`. Confirm neither
`docs/plans/prototype/implementation-plan.md` nor any `phase-N-plan.md` appears
in the diff.

## 14. Documentation completion edits

Update `docs/world-model.md` in the same commit so it describes implemented
truth:

- the state diagram and dependency discussion identify Slope as a Terrain
  output consumed by Hydrology/Soils;
- section 3.8 lists 14 float channels, 60,416 payload bytes, and centered ghost
  slope;
- Terrain describes the 3 by 3 current/fallback region-center halo and
  per-cell bilerp instead of a constant vector;
- Drainage describes the Q30 integer field/noise/scaling/rounding evaluator and
  field-aware key, without claiming it is the exact float Terrain surface;
- streaming/cache sections describe nine-consumer halo invalidation and parked
  authority;
- same-math performance text distinguishes SIMD float Terrain presentation
  from scalar exact fixed routing;
- verification says wasm probes run under Node in CI and names the broad
  topology fold;
- roadmap item A.8 becomes **Completed** and links this plan;
- findings 9 and 19 gain resolved status/link plus a concise account of the
  implemented guarantees; and
- findings 20, 32, 33, and 34 remain open and accurately scoped.

Suggested roadmap completion text:

> **Completed: Make stable topology and ordinary region boundaries satisfy
> their stated guarantees** ([Improvement A.8](plans/prototype/improvement_A_8_topology_boundaries.md);
> findings 9 and 19). Drainage routing elevation is now an entirely integer
> fixed-point function with field-aware keys and executed native/wasm topology
> probes. Terrain samples an exact 3 by 3 realized-current P/G halo, emits a
> centered ghost-derived Slope channel, and invalidates every affected
> neighbor closure across authority lifecycle changes. Exact ordinary
> history-divergent border tests cover Terrain through Biome.

Adjust wording to match final names and measured results, but do not claim that
accumulation is globally window-independent, all CPU/GPU presentation is
bit-equal, or the complete A.13 verification roadmap is finished.

## 15. Implementation order

1. Add failing integer routing bucket/Q30 helper tests and implement the
   isolated fixed evaluator.
2. Switch macro Drainage generation to that evaluator, add field spacing to
   keys, bump Drainage revision, and deliberately update focused goldens.
3. Define `TerrainPossibilityHalo` and its exact interpolation/key tests.
4. Append `CHANNEL_SLOPE`, implement rolling ghost rows, and convert
   Hydrology/Soils to the stored input.
5. Specialize Terrain expected keys and submission snapshots.
6. Add the central nine-consumer invalidation helper and route convergence,
   preserve, load, unload, session restore, and field-recipe changes through it.
7. Add stale-job, parked-authority, and ordinary history-divergent seam tests.
8. Update GPU atlas packing/presence behavior and logical payload tests.
9. Add broad parity probes, pinned wasm-bindgen-test/wasm-pack wiring, and run
   the actual Node wasm suite.
10. Measure Terrain/Drainage jobs, recalibrate costs, and append the performance
    ledger entry.
11. Add ADR 0027 and update README, AGENTS, and the world-model completion and
    finding text.
12. Run focused tests, harnesses, every native/wasm CI gate, diff checks, and a
    final fixture/version audit.
13. Create one commit for A.8 only, fast-forward merge it to `main`, verify a
    clean tree and one-commit topology, then push `main` to `origin` before
    starting A.9.

## 16. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Fixed-point code still hides a float conversion | Isolate it in `routing.rs`, make every API/input integral except the field recipe object, review the module for float types/casts, and execute wasm known answers. |
| Signed Q arithmetic differs at negative coordinates | Centralize round/floor helpers, use Euclidean decomposition, and pin negative/control-boundary/extreme-coordinate fixtures. |
| Q30 intermediate overflow | Use `i128` multiplies/sums, document bounds, and test near-boundary supported i32 coordinates plus maximum buckets. |
| Routing accidentally continues through old SIMD float fill | Remove the old fill and require macro tile plus scalar sample to call one fixed evaluator; compare all grid cells in a test. |
| Halo content changes without dirty hints | Route every authority mutation through one nine-consumer helper; retain ADR 0019 expected-key integration checks and add queued stale-result tests. |
| Parked fields are mistaken for absent authority | Build halos from `regions`, not `RegionCache` or status; tight/roomy capacity tests require identical outputs. |
| Fallback differs by observing tile | Define it by absolute coordinate and stored field recipe only; overlapping-halo tests include missing coordinates. |
| Terrain key omits one neighbor | Use a typed fixed-size 18-value fold and mutate every slot in tests. |
| New Slope becomes an undeclared dependency | Produce it atomically from Terrain and pass it through existing declared Terrain edges to Hydrology/Soils. |
| Slope channel corrupts GPU packing/presence bits | Append at index 13, explicitly skip it in the unchanged 13-channel atlas map, and test CPU/GPU presence semantics. |
| Extra channel silently violates cache ceilings | Use the shared payload helper, update the exact 60,416-byte assertion, and rerun scale plateau gates. |
| Integer routing is materially slower | Benchmark honestly, recalibrate Drainage cost, retain bounded 50² work, and optimize only with exact integer equivalence. |
| Fixture re-bless masks unrelated drift | Bump only Terrain/Drainage revisions, list allowed fixtures first, and require unrelated goldens to remain byte/bit exact. |
| Wasm CI still only compiles | Add a wasm-bindgen-test integration suite and explicit pinned `wasm-pack test --node` workflow step. |
| Runner/tool updates drift CI | Pin wasm-bindgen-test 0.3.76, wasm-pack 0.13.1, and commit the lockfile. |
| Scope overclaims drainage seams | Keep apron accumulation finding 20 open; A.8 guarantees fixed routing decisions and ordinary Terrain/Slope boundaries, not globally identical truncated accumulation. |

## 17. Definition of done

A.8 is complete only when all of the following are true:

- routing elevation is structurally integer-only from control seed through
  final centimeters;
- the scalar and macro paths share that one evaluator;
- Drainage keys include field spacing and Terrain/Drainage revisions are 1
  while world version stays 2;
- Terrain snapshots and hashes all 3 by 3 P/G authority/fallback buckets;
- Terrain and Slope are atomic outputs, with centered ghost differences at
  every cell and no remaining Hydrology/Soils one-sided slope path;
- slow-domain authority changes invalidate the exact nine-consumer closure
  across convergence, preserve, load, unload, session restore, parking, and
  field-recipe transitions;
- same-position border/corner values are bit-identical and ordinary
  history-divergent downstream seam oracles pass;
- macro topology and all other parity probes execute under real wasm in CI;
- only intended goldens changed under the documented layer revision process;
- logical payload, pool, GPU omission, benchmarks, and scheduler costs are
  updated and verified;
- ADR 0027, README/AGENTS, and `docs/world-model.md` describe the landed truth,
  roadmap A.8 and findings 9/19 are marked completed/resolved, and neighboring
  open findings remain open;
- all focused tests, harnesses, native CI gates, wasm checks, and Node wasm
  tests pass with warnings denied;
- forbidden historical plan files are untouched; and
- the dedicated worktree contains one A.8 commit which is fast-forward merged
  and pushed before the next roadmap item begins.
