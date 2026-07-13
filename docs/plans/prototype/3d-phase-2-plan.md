# Phase 3D-2 — Ground Collision and Terrain Following: Implementation Plan

This is the lower-level plan for the second phase of
[`3d-design.md`](3d-design.md) (§4 there): a walk mode that keeps the camera
riding the terrain surface, toggled against the existing fly mode with `F`.
No water (3D-3), no organisms (3D-4), no lateral collision, no jumping, no
character-controller physics (design §8).

Read [`AGENTS.md`](../../../AGENTS.md) first. This plan does not repeat the
crate-boundary rule, the determinism invariant, or the CI contract — it
assumes them, and it builds on the **landed** 3D-1 implementation
(`platform-native/src/pov.rs`, `renderer/src/pov.rs`), not on the 3D-1 plan's
pre-implementation guesses. Where the two diverged (left-button-drag look
instead of cursor grab, baked per-vertex lighting, the `--pov-script`
headless harness, `POV_SKIRT_DROP = 128`), this plan follows the code.

One sentence of orientation, because it governs everything below: **3D-2 is
presentation-side camera state only** (design §4.2). The walk camera reads
heights the 3D-1 mesher already computed and stored
(`ChunkEntry::heights`, kept for exactly this phase); nothing touches world
state, generation, hashing, saves, or the vault beyond the player-position
recentering the shell already does for fly mode. `WORLD_ALGORITHM_VERSION`
stays at 2, every `algorithm_revision` stays at its current value, zero
golden fixtures are re-blessed, and **no crate outside `platform-native` is
touched at all** — not even the renderer.

---

## 1. Goals and non-goals

### 1.1 Goals (design §4.3, restated as deliverables)

- A **`ground_height` query** on `PovChunkManager`: barycentric
  interpolation over the same 64×64 chunk lattice that is drawn, using the
  CPU-side `heights` array each `ChunkEntry` already carries — the camera
  never visually sinks into or floats above the rendered triangles, by
  construction. Pure, unit-testable, exact at vertices, bounded mid-cell.
- An **analytic fallback** for the loading frontier: where no chunk exists
  yet, walk mode stands on halo-sampled `elevation()` — correct to within
  one mesh cell, and transient.
- **Walk kinematics**: eye held `EYE_HEIGHT = 1.7` units above the ground,
  horizontal WASD movement in the yaw plane, a vertical-rate clamp so cliff
  faces read as steep ramps rather than teleports. No gravity, no falling
  state, no slide, no step logic. Walking below `z = 0` is allowed (sea
  floor; water is 3D-3).
- **`F` toggles walk ↔ fly**, keeping position and orientation both ways;
  `Tab`/`Escape`/`F12` behave as today. Map mode remains pixel-identical
  (the existing POV keybinding gate in `handle_press` already guarantees
  this structurally; `F` is added inside the gate).
- **Scripted coverage**: `--pov-script` gains `walk` / `fly` instructions so
  headless captures and the F12 dump can reproduce walk-mode poses.

### 1.2 Non-goals (deferred to later 3D phases or design §8)

- Water surfaces, wading, swimming (3D-3 — the sea floor is just ground).
- Lateral collision, jumping, crouching (`Space`/`LShift` are consumed and
  ignored in walk mode, reserved per design §2.1).
- Any change to the mesher, the chunk lifecycle, the renderer, or the
  shaders. `ground_height` is a new *reader* of data 3D-1 already produces.
- Making walk heights agree with the *analytic* surface everywhere — they
  agree with the **rendered** surface everywhere, which is the design's
  explicit choice (§4.1): the mesh is a piecewise-linear approximation of
  `elevation()`, and the camera must ride the mesh, not the function.

## 2. Contracts this phase must not break

- **Determinism.** No new identity, no new persistence, no generation-path
  change. Every height the walk camera uses is either a mesh vertex the
  3D-1 mesher computed (itself authoritative halo-sampled `elevation()`,
  ADR 0016/0027) or a barycentric combination of three of them —
  presentation-only float math. Nothing feeds back (ADR 0017).
- **Crate boundaries.** All changes live in `platform-native`
  (`pov.rs`, `main.rs`, `dump.rs`). Neutral crates, the renderer, and the
  wasm CI job are untouched by construction.
- **CI.** Lands green on the full matrix: `fmt --check`, `clippy` with
  `-D warnings`, `cargo test --workspace` with **no golden fixture
  changes**, wasm check unaffected.
- **Map mode.** The `handle_press` POV gate (`main.rs`, the 3D-1 §8.4
  guarantee) is extended with one arm (`KeyCode::KeyF`); no map binding
  changes, no map state is reachable from POV.

## 3. New and touched surfaces

| Surface | Change |
|---------|--------|
| `crates/platform-native/src/pov.rs` | `ground_height` on `PovChunkManager` (barycentric over `ChunkEntry::heights`; drop the `#[allow(dead_code)]`); `analytic_ground` helper (refactor of `entry_ground`); `PovCamera` walk state (`walk`, `walk_speed`) and walk-mode movement basis; new constants (§5.2); `PovInstr::Walk` / `PovInstr::Fly` + parser arms; unit tests. |
| `crates/platform-native/src/main.rs` | `F` in the POV key gate; walk branch in `apply_pov_movement` (terrain following each frame, vertical-rate clamp); walk/fly + ground info in the once-per-second POV log line; doc-comment controls table. |
| `crates/platform-native/src/dump.rs` | `state.txt` camera block gains move mode and current ground height; the view-mode string distinguishes walk/fly. |
| `README.md` | One line in the POV controls note (`F` toggles walk/fly). |
| `docs/perf-baseline.md` | One-line note in the POV section: walk mode adds one O(1) height query per frame — no measurable frame-time change (verified on the llvmpipe reference environment). |

Nothing else. In particular `renderer/src/pov.rs` is read (its published
topology constants and `chunk_indices` order define the triangles
`ground_height` must match) but not modified.

## 4. The ground-height query (design §4.1)

### 4.1 Why the render lattice, and which copy of it

Collision height comes from the **drawn mesh, not the analytic function**.
`PovChunkManager::sync` swaps a `ChunkEntry` in atomically with the upload
that replaces its GPU buffer (`pov.rs`, the integration loop), so
`ChunkEntry::heights` is at every moment the CPU twin of exactly the
vertices on screen — including mid-drift, when the mesh is briefly stale
relative to new possibility buckets. Riding the stale mesh until the remesh
lands is correct: the eye tracks what the player *sees*, and the correction
arrives as one bounded step when the swap happens (the "possibility-drift
step" the design's exit criteria explicitly permit).

`heights` is the 65×65 core vertex lattice, row-major `j * POV_GRID + i`,
chunk-local spacing `SPACING = 4.0`, `z` stored as the same `f32` the GPU
vertex carries — so vertex-exact agreement with the rendered triangles is
bit-level, not approximate. Skirt vertices are not in `heights` and are
irrelevant to collision (they hang *below* the perimeter).

### 4.2 Triangle selection must match `chunk_indices`

`renderer::pov::chunk_indices` splits every cell along the **v00→v11
diagonal** (south-west to north-east): triangles `[v00, v10, v11]` and
`[v00, v11, v01]`, CCW from above. The query must use the same split or
mid-cell heights will disagree with the drawn surface by up to the cell's
diagonal curvature. With fractional cell coordinates `(fx, fy) ∈ [0, 1]²`:

```text
fx ≥ fy  (south-east triangle v00,v10,v11):
    h = h00 + fx · (h10 − h00) + fy · (h11 − h10)
fx < fy  (north-west triangle v00,v11,v01):
    h = h00 + fy · (h01 − h00) + fx · (h11 − h01)
```

Both expressions agree on the diagonal (`fx = fy`) and on the shared
vertices, so the interpolant is continuous across the split, across cell
edges, and — because adjacent chunks' border columns are identical
halo-sampled elevations in steady state (ADR 0027, overlapping absolute
halos) — across region borders. A unit test (§8, test 3) pins the split
against the actual `chunk_indices()` topology rather than restating it, so
a future diagonal flip in the renderer fails the test instead of silently
de-synchronizing collision from visuals.

### 4.3 Signature and lookup

```rust
impl PovChunkManager {
    /// Height of the rendered terrain surface under a world position, from
    /// the resident chunk's CPU-side height lattice (the drawn mesh's twin).
    /// `None` when the covering region has no chunk yet (loading frontier).
    #[must_use]
    pub fn ground_height(&self, wx: f64, wy: f64) -> Option<f32>
}
```

Locate region (`RegionCoord::from_world` — floor semantics put a border
position in exactly one region, whose chunk owns that border column) →
`chunks.get(&coord)?` → chunk-local `lx = wx − ox`, cell
`i = (lx / SPACING).floor()` clamped to `POV_MESH_RES − 1`, fraction
`fx = lx / SPACING − i` (and likewise `j`, `fy`) → the §4.2 interpolation
over four `heights` reads. O(1), no allocation, pure over `&self` — called
once per frame in walk mode and freely from tests.

The `#[allow(dead_code)] // consumed by 3D-2's ground_height` on
`ChunkEntry::heights` comes off in the same commit.

### 4.4 Analytic fallback at the frontier

```rust
/// The authoritative halo-sampled terrain height under a world position —
/// the mesh-free fallback for walk mode at the loading frontier, and the
/// shared core of `entry_ground`. Presentation-only; never an identity.
#[must_use]
pub fn analytic_ground(map: &RegionMap, world: (f64, f64)) -> f64
```

This is today's `entry_ground` body **minus the `SEA_LEVEL` floor**:
halo-sampled `PossibilityVector` when the covering region is resident
(neutral otherwise), through `world_core::elevation`. Walking the sea floor
is allowed (design §4.2), so the fallback must not clamp; `entry_ground`
keeps its clamp and becomes `analytic_ground(..).max(SEA_LEVEL as f64)` —
a pure refactor, pinned by a test.

The shell composes the two: `pov_chunks.ground_height(x, y)` first,
`analytic_ground` when it returns `None`. The fallback differs from the
eventual mesh by at most the piecewise-linear approximation error of one
4-unit cell; when the chunk lands, the vertical-rate clamp (§5.3) absorbs
the correction as a small ramp, not a pop.

## 5. Walk kinematics (design §4.2)

### 5.1 Camera state

`PovCamera` gains:

```rust
/// Walk mode: terrain following at eye height (3D-2). Fly mode when false.
pub walk: bool,
/// Walk speed, world units per second (scroll-adjusted, like `speed`).
pub walk_speed: f64,
```

`speed` (fly) is untouched; each mode keeps its own scroll-tuned speed and
the wheel adjusts whichever is active. Deliberately a `bool`, not an enum —
the design has exactly two modes and reserves nothing that needs a third.

### 5.2 Constants

```rust
/// Eye height above the ground surface in walk mode (design §4.2, §1.1
/// scale reference: person-scale in a world where 1 unit ≈ 1 m).
pub const EYE_HEIGHT: f64 = 1.7;

/// Base walk speed, world units per second — a brisk run at person scale.
/// `POV_FLY_SPEED` (40) is survey speed, wrong for eye-level ground travel.
pub const POV_WALK_SPEED: f64 = 6.0;

/// Scroll-wheel walk-speed bounds (fly keeps `POV_SPEED_RANGE`).
const POV_WALK_SPEED_RANGE: (f64, f64) = (1.0, 60.0);

/// Vertical-rate clamp as a multiple of the current walk speed: the eye
/// approaches `ground + EYE_HEIGHT` at ≤ this × walk_speed, both up and
/// down, so a cliff face is climbed as a ≤ ~71° effective ramp and a ledge
/// is descended as a quick drop — never a teleport (design §4.2). Scaling
/// with walk speed keeps the feel constant under the scroll multiplier.
const POV_CLIMB_FACTOR: f64 = 3.0;
```

### 5.3 Per-frame integration

Walk mode replaces the fly branch of `apply_pov_movement` (`main.rs`) —
same held-key sampling, different basis and a follow step:

1. **Horizontal move.** `W`/`S` along the horizontal yaw direction
   `(cos yaw, sin yaw, 0)` — *not* `forward()`, which pitches; looking at
   your feet must not stop you (design: "move along ground plane").
   `A`/`D` use the existing `right()` (already horizontal). Normalize the
   combined direction, scale by `walk_speed · dt`. `Space`/`LShift` are
   ignored (reserved).
2. **Follow step.** `target = ground(x, y) + EYE_HEIGHT` where `ground` is
   `pov_chunks.ground_height` with the `analytic_ground` fallback (§4.4).
   Then `pos.z += (target − pos.z).clamp(±POV_CLIMB_FACTOR · walk_speed · dt)`.
   On flat and ordinary terrain the clamp never engages and following is
   exact ("hard terrain following"); it engages only on cliff faces,
   frontier-fallback corrections, remesh swaps, and drift steps — precisely
   the cases the design wants softened.
3. **Recentering.** `world.player = (pos.x, pos.y)` exactly as fly mode
   does today (`main.rs`, the frame branch) — streaming, retarget, and
   realization already recenter on the camera; walk changes nothing there.

**Snap cases** (set `z = target` directly, no clamp): toggling `F` into
walk (an instant grounding from any fly altitude — a clamped descent from a
600-unit survey height would be a 30-second elevator; flagged in §11 in
case review prefers the descent), and the scripted `pos:` instruction.
Toggling `F` back to fly keeps position and orientation unchanged (design
§4.2) — no snap, no state reset.

### 5.4 What walk mode deliberately does not do

No gravity or airborne state (the follow step *is* the vertical model), no
slide on steep slopes, no step-height logic, no lateral collision (you walk
through organisms and cliffs — the clamp just makes the cliff take time),
no wading/swimming behavior below `z = 0`. Each of these is either design
§8 deferred work or a later phase's concern; adding any of them here would
be scope creep into character-controller territory the design explicitly
declines.

## 6. Shell integration

### 6.1 Input

`handle_press`'s POV gate (`main.rs`) gains one arm:

```rust
KeyCode::KeyF => self.pov_camera.walk = !self.pov_camera.walk, // + snap/log
```

(routed through a small `toggle_walk` helper on `App` so the snap-to-ground
and log line live in one place). `Tab`, `Escape`, `F12`, drag-look, and the
wheel are unchanged; the wheel handler picks `walk_speed` vs `speed` by
mode. Map-mode bindings are untouched — the gate structure already
guarantees it.

### 6.2 Telemetry

The once-per-second POV log line (`frame_pov`) replaces its fixed
`fly {:.0}u/s` tail with the active mode:
`walk 6u/s (ground 142.3, mesh)` / `(ground 142.1, analytic)` / or the
existing fly form — the mesh-vs-analytic tag is the observable for the
frontier-fallback exit criterion. No counters change; `PovCounters` is
untouched (nothing new is scheduled or uploaded in this phase).

### 6.3 F12 dump

`state.txt`'s camera block (`dump.rs`) gains `mode: walk|fly` and, in walk
mode, the current ground height and source (mesh/analytic). The view-mode
string at the top becomes `pov (3D walk camera)` / `pov (3D fly camera)`.
Screenshot path unchanged.

### 6.4 `--pov-script`

Two new no-argument instructions, parsed like `settle`:

- `walk` / `fly` — set the mode through the same toggle path the live `F`
  key uses (including the snap-to-ground on `walk`).
- `move:f[,r[,u]]` in walk mode applies the displacement in the walk basis
  (`f` along horizontal yaw, `r` strafe, `u` ignored) and then snaps
  `z = ground + EYE_HEIGHT` — scripted captures want the settled pose, not
  a clamped animation, and the runner's inline executor means chunks are
  already settled when the snap resolves. In fly mode `move` is unchanged.

Example, which becomes the manual-verification recipe and a doc-comment:

```sh
wer --pov-script "pos:300,-10; walk; move:200; snap:walk-a.ppm; \
                  mouse:400,0; move:200; snap:walk-b.ppm"
```

The parser tests extend accordingly; unknown-instruction and arity errors
keep their existing shapes.

## 7. Performance posture

One `ground_height` call per frame (O(1): a hash lookup, four `f32` reads,
a handful of multiplies) plus, transiently at the frontier, one
`analytic_ground` (a single `elevation()` sample — the same cost as the
existing `entry_ground` on POV entry). No new allocations, threads, jobs,
uploads, or GPU work of any kind. `docs/perf-baseline.md` gets a one-line
no-change note rather than a new table; the `pass-timing` picture is
unaffected.

## 8. Testing

Unit tests inline in `pov.rs` (`#[cfg(test)]`, the existing module — the
`settled_map` fixture already builds a fully-settled multi-region window):

1. **Vertex exactness.** For a settled chunk, `ground_height` at every core
   lattice position returns exactly `heights[j * POV_GRID + i]`, which the
   3D-1 tests already pin to halo-sampled `elevation()` — the design's
   "exact agreement at vertices", asserted at the consumer.
2. **Mid-cell boundedness and planarity.** At interior points of a cell,
   the result lies within `[min, max]` of that cell's four corner heights,
   and equals the plane through the containing triangle's three vertices to
   f32 round-off (evaluated independently from the barycentric weights).
3. **Topology agreement with the renderer.** For a grid of probe points,
   find the triangle of `renderer::pov::chunk_indices()` (core triangles
   only) whose 2D projection contains the probe, evaluate its plane, and
   assert equality with `ground_height`. This is the guard that collision
   and visuals share one diagonal (§4.2); it fails loudly if either side
   changes its split.
4. **Continuity.** Along a transect crossing cell edges, the diagonal, and
   a region border (two settled adjacent chunks), consecutive samples at
   ±ε differ by O(ε · slope) — no jumps. Border continuity is exact in
   steady state by ADR 0027; the test asserts it numerically.
5. **Frontier fallback.** A position whose region has no chunk returns
   `None`; `analytic_ground` there equals `elevation()` under the halo
   vector (and the neutral vector off-map); `entry_ground` still equals
   `analytic_ground.max(SEA_LEVEL)` (pins the §4.4 refactor).
6. **Walk kinematics** (pure math on `PovCamera` + a stub ground fn):
   pitch does not affect walk displacement; the follow step is exact when
   the target is within the clamp and rate-limited when beyond it, both
   directions; `F`-toggle snap grounds immediately; fly→walk→fly round-trip
   preserves x, y, yaw, pitch; wheel adjusts only the active mode's speed
   within its range.
7. **Script parsing.** `walk`, `fly`, mixed sequences, and the walk-mode
   `move` snap semantics (driven through the parsed instructions against a
   settled map, following the existing script tests).

CI: all of the above run in plain `cargo test --workspace`; no golden
fixture is added or changed; the wasm check is unaffected (no neutral-crate
diff exists); clippy/fmt clean under `-D warnings`.

Manual verification on the reference environment (WSL2/llvmpipe, X11):
enter POV, press `F`, walk across a region border and over a ridge line
(no sinking, no floating, no border pop); walk off a cliff (fast ramp, not
teleport); walk into the loading frontier and watch the log tag flip
analytic → mesh with at most a small step; scroll the walk speed; `F` back
to fly at altitude and `F` again to snap down; `Tab` to map and confirm it
renders as before; F12 in walk mode and check `state.txt`.

## 9. Milestones

Each lands independently green on the full CI matrix.

- **M1 — The query.** `ground_height`, `analytic_ground` (+ `entry_ground`
  refactor), dead-code allow removed, tests 1–5. No behavior change in the
  running app. *Exit:* all query tests green, including the
  `chunk_indices` topology-agreement test.
- **M2 — Walk mode.** `PovCamera` walk state + constants, the walk branch
  of `apply_pov_movement`, `F` in the POV gate, wheel routing, snap cases,
  telemetry line, tests 6. *Exit:* live walk/fly toggling works end to end
  on the reference environment; map mode untouched.
- **M3 — Harness and sign-off.** `walk`/`fly` script instructions + tests
  7, F12 dump fields, README controls line, perf-baseline note, the §8
  manual walkthrough, design-doc exit-criteria check (§10).

## 10. Phase exit criteria (design §4.3, restated checkable)

- [ ] `F` toggles walk/fly; walk mode holds the eye exactly `EYE_HEIGHT`
      above the rendered surface across region borders, skirt edges, and
      chunk boundaries — no pops beyond the possibility-drift step itself
      (clamped into a short ramp, §5.3).
- [ ] `ground_height` unit tests: vertex-exact, mid-cell bounded,
      cross-border continuity within the drift step — plus the
      renderer-topology agreement test this plan adds.
- [ ] Frontier fallback: walk mode works over unmeshed regions on analytic
      ground and corrects by at most one mesh cell when the chunk lands.
- [ ] Fly mode, map mode, `--screenshot`, and all existing `--pov-script`
      instructions behave byte-identically to 3D-1.
- [ ] `cargo test --workspace` green with no golden fixture changes; wasm
      check unaffected; clippy/fmt clean under `-D warnings`.

## 11. Risks and open questions

- **Diagonal drift between collision and rendering.** The single real
  correctness hazard in this phase. Mitigated structurally by test 3, which
  derives the expected surface from `chunk_indices()` itself instead of a
  copied formula.
- **Snap vs. descent on `F` into walk.** This plan snaps (§5.3) — instant
  grounding from any altitude. If review prefers a bounded descent for
  drama, it is a one-line change (route the toggle through the clamp), but
  the default is chosen for utility: `F` is how you get to the ground.
- **Visual slope vs. walked slope.** 3D-1's `NORMAL_EXAGGERATION = 1.5`
  makes shading read steeper than the true surface the camera walks.
  Cosmetic only — heights are unexaggerated — but worth knowing when a
  slope "looks 45°" and climbs like 30°. No action; noted so it isn't
  misdiagnosed as a collision bug.
- **Clamp feel at extreme scroll speeds.** At `walk_speed = 60` the
  vertical clamp allows 180 u/s; terrain following can feel bouncy over
  sharp ridges at that speed. `POV_CLIMB_FACTOR` is the single tuning knob;
  tune on the reference environment at M2, not in review.
- **Eviction under the camera.** Cannot occur inside the radius window
  (`capacity = window + POV_CHUNK_SLACK`, farthest-first — the camera's
  region is by definition the nearest), so `ground_height` under the
  camera only returns `None` at the genuine loading frontier. Stated here
  so the fallback isn't mistaken for an eviction workaround.
