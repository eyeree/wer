# Phase 7 — Browser Runtime: Implementation Plan

This is the lower-level plan required by section 21 of
[`implementation-plan.md`](implementation-plan.md) before Phase 7 work begins
(it is `browser-runtime-plan.md` in section 21's list, plus the browser slices
of `persistence-plan.md`, `job-system-plan.md`, and `renderer-plan.md`, and the
section 19 portability requirements collected into one delivery). It expands
the Phase 7 scope in section 20 — Wasm runtime integration, Web Worker
scheduling, shared memory where available, browser persistence, browser asset
streaming, suspension and recovery, WebGPU feature tiers, browser-specific
memory budgets, startup benchmarking, reduced compatibility profiles — into
concrete interfaces, algorithms, and milestones, grounded in the seams every
earlier phase deliberately cut for exactly this moment: the executor-agnostic
`TaskExecutor` trait ("the shape Web Workers need", phase-6-plan §13), the
abstract `Storage` trait whose only user is the vault ("callers must treat it
as potentially asynchronous", `storage.rs`), the surface-from-a-closure
renderer ("targets `wgpu` … and uses only WGSL shaders", `renderer/src/lib.rs`),
the pure `ResourceTier::detect` ("Phase 7 feeds browser inputs … into the same
table", `tier.rs`), the travel-fueled convergence that makes pausing free
(ADR 0006), and the schedule independence that makes *any* executor legal
(ADR 0018).

Two deliverables in this plan go beyond section 20's list, at the owner's
direction:

- **The information panel moves to HTML.** The browser shell renders the
  panel as real DOM, not pixels. That forces the panel's *content* out of
  `platform-native/src/panel.rs` (where it is entangled with an 8×8 bitmap
  font) into a platform-neutral, serializable **panel model** that both
  shells render — HTML in the browser, the existing bitmap strip natively
  (§6.4, ADR 0021).
- **A documentation page** ships with the browser app: a high-level,
  non-technical overview of the project *and* a low-level description of the
  world model as currently implemented. Writing the two documents is two
  **independent tasks** (D1, D2 — §15.2) with no dependency on each other or
  on any engineering milestone.

Read [`AGENTS.md`](AGENTS.md) first. This plan does not repeat the
crate-boundary rule, the determinism invariant, or the CI contract — it
assumes them and calls out where Phase 7 stresses each. One sentence of
orientation up front, because it governs everything below: **Phase 7 changes
no generated output for any input.** Like Phase 6, this phase moves the same
world to a new place — this time a browser tab — and its determinism
obligation is to *prove* the world survived the trip, not to re-bless around
the differences.

---

## 1. Goals and non-goals

### 1.1 The question Phase 7 must answer

Phases 0–6 built and hardened the world model natively while paying the
browser tax continuously: the neutral crates have compiled for
`wasm32-unknown-unknown` in CI since Phase 0, every platform capability the
core needs is a trait, the renderer is WGSL/WebGPU-shaped, SIMD has scalar
twins, GPU output is derived-only, and ten integer parity exports pin the
cross-platform identity surface. But the browser side of that bargain is
still a smoke test: `platform-web` computes `origin_feature_hash` into the
console and stops. No region streams, no frame renders, no key moves the
player, nothing persists. Phase 7 asks:

> Can the **same world model** — bit-identical identity surface, same layers,
> same steering, same records — run as a real, playable, persistent
> application in a modern desktop browser, **scaling simulation density and
> cache sizes to device capabilities** rather than assuming native desktop
> resources? (section 20, Phase 7)

This is the phase where section 3.2's claim — "browser support is a planned
platform target, not a late-stage port" — is either vindicated or exposed.
The plan's architecture bet is that Phase 7 is mostly *assembly*: the neutral
crates already contain the game; what is missing is a browser-shaped shell
around them, plus the honest engineering at the three real friction points
(§1.6).

### 1.2 Success criterion (from section 20)

> The browser version preserves the same world model and core experience
> while scaling simulation density and cache sizes to device capabilities.

Decomposed into machine-checkable properties:

- **Same world model:** all ten parity exports return their golden values
  *executed as wasm in CI* (today only the native side of the parity claim is
  machine-checked; §11.2 closes that gap), shared record bytes are identical
  (`record_codec_sample`), shared-anchor steering is identical
  (`shared_steer_sample`), and a browser save→load→settle round trip is
  state-hash exact *within the browser runtime* (§9.2 — cross-platform
  equality remains the integer surface only, exactly as Phases 2–6 defined
  it).
- **Core experience:** every interaction in the README key table — movement,
  bias nudges, anchor drop/capture/clear, transition mode, discoveries,
  summon, preserves, routes, channels, overlays, save/load — is reachable in
  the browser shell, against the same `RegionMap`, vault, and steering code,
  with the panel (now HTML) showing the same telemetry and cursor readout.
- **Scaled to the device:** `ResourceTier::detect` consumes browser inputs
  (worker count, adapter class, memory hints — §6.7); each compatibility
  profile (§1.5) sustains its tier's world with bounded, draining
  backpressure (`deferred_*` counters, the Phase 6 stability discipline) and
  memory plateaued under the tier's cache ceilings.
- **Output-identical:** `WORLD_ALGORITHM_VERSION` stays at 2, every
  `algorithm_revision` stays 0, `RECORD_FORMAT_VERSION` stays 1, and **zero
  golden fixtures are re-blessed**. Native behavior is unchanged: the
  extraction refactor (§4.1) is proven behavior-preserving by the untouched
  harness corpus.

### 1.3 Goals

- **A shell-neutral application core** (§4.1, the phase's enabling refactor):
  hoist the game-session state machine out of `platform-native/src/main.rs`
  (`World`: map + field + anchors + bias + player + vault + recorder +
  tracker), the input **command vocabulary**, the CPU **map composer** and
  channel palettes (`viz.rs`), and the new **panel model** (§6.4) into a new
  platform-neutral crate, `crates/app-core`. Native becomes a thin
  winit/wgpu/filesystem shell; the browser becomes a thin
  canvas/worker/IndexedDB shell; the game lives once, between them. This is
  the same move Phase 6 made for the executor (hosting `LaneExecutor` in
  `tools` so harnesses drive production code), applied to the whole shell.
- **Wasm runtime integration** (section 20): `platform-web` grows from smoke
  test to application — canvas surface, async renderer init, a
  `requestAnimationFrame` frame loop, keyboard/mouse input mapped to the
  shared command vocabulary, and the GPU map path (the Phase 6 atlas +
  `compose_map.wgsl` compose, which is already WebGPU-shaped) rendering to
  the canvas.
- **The HTML information panel** (owner requirement; §6.4, ADR 0021): the
  panel's content becomes `PanelModel` — a versioned, serde-serializable
  tree of sections and toned rows built in `app-core` from the same inputs
  `PanelInfo` carries today. The browser renders it as DOM (a real HTML
  sidebar with CSS, selectable text, and its own scrollbar); native renders
  the same model through the existing bitmap-font strip so headless
  `--screenshot` output keeps working. Shells become dumb renderers of one
  model; the panel can never again drift between platforms.
- **Web Worker scheduling** (section 20; the job-system browser slice, §6.2):
  generation jobs become **nameable pure data** (`JobSpec`) with a neutral
  `execute_spec` twin of today's closure body, so a `WorkerExecutor` in
  `platform-web` can serialize specs to a pool of Web Workers (postcard bytes
  over `postMessage`, results transferred back) while the native
  `LaneExecutor` keeps running the same specs inside closures. ADR 0019.
  Cancellation and supersession keep their existing main-thread mechanics
  (job-id gate; tokens checked before posting).
- **Browser persistence, suspension, and recovery** (section 20; the
  persistence-plan browser slice, §6.3): a `BrowserStorage` backend — a
  synchronous in-memory mirror over the existing `Storage` trait with an
  ordered **write-behind journal** draining into IndexedDB (ADR 0020) — so
  `Vault<BrowserStorage>` works unmodified. Autosave, `visibilitychange`
  flush, device-loss recovery through the renderer's existing
  surface-source-closure pattern, and safe tab throttling (rAF pauses ⇒
  travel pauses ⇒ the world pauses; ADR 0006 makes this free).
- **WebGPU feature tiers, memory budgets, and reduced compatibility
  profiles** (section 20; §6.7, §1.5): browser `TierInputs`
  (`hardwareConcurrency`, adapter probe, `deviceMemory`, URL-param overrides
  mirroring `WER_TIER`/`WER_CACHE_MB`) into the same pure `detect`; a
  profile matrix that degrades gracefully — no WebGPU ⇒ 2D-canvas CPU
  compose (the shared composer from `app-core`), no workers ⇒
  `InlineExecutor`, both ⇒ still a correct, slower world.
- **Browser asset streaming and startup benchmarking** (section 20; §6.8,
  §12): `WebAssembly.instantiateStreaming`, a recorded wasm size budget, a
  `simd128`-enabled release build (stable `-C target-feature=+simd128`; the
  ADR 0016 scalar twins remain the no-SIMD fallback build), and
  `performance.mark` instrumentation of fetch→compile→init→first-frame→
  settled-window, reported in the panel and recorded in
  [`docs/perf-baseline.md`](docs/perf-baseline.md).
- **The documentation page** (owner requirement; §6.9): `web/docs.html`
  presenting two documents — `docs/overview.md` (non-technical, what the
  project is and what the player does) and `docs/world-model.md` (the model
  as implemented: layers, hashing, steering, records, tiers) — rendered
  client-side by a small vendored Markdown renderer so the canonical sources
  stay plain Markdown, readable on GitHub. Authoring the two documents is
  tasks **D1** and **D2**, independent of each other and of every milestone
  (§15.2).

### 1.4 Non-goals (explicitly deferred)

- **Networking and servers.** Still none. The sharing model remains
  file-based `wer-atlas` bundles; the browser gets bundle export/import
  through the file picker / download APIs (§6.3) — the same files, no wire.
  The "community atlas" service is a later phase.
- **Shared-memory wasm threads.** `SharedArrayBuffer` + wasm atomics would
  let the closure-based executor run unchanged, but threaded `std` for
  `wasm32-unknown-unknown` requires nightly `build-std`, and the pinned
  toolchain is stable (rust-toolchain.toml). The descriptor executor (§6.2)
  is the portable baseline; shared memory is a future optimization behind
  the same `TaskExecutor` seam, explicitly allowed for by ADR 0019 and
  section 20's "shared memory where available" — *where available* is
  currently *not on stable Rust*, and the plan says so rather than
  pretending.
- **Mobile browsers.** Section 20 says modern *desktop* browsers. Touch
  input, mobile GPU tiers, and small-screen layout are out.
- **A real 3D renderer.** Same as Phase 6: the render surface is the debug
  map (GPU-composed atlas + refinement). `renderer-plan.md` still precedes
  the terrain renderer, on either platform.
- **New world features.** No new layers, records, anchor kinds, or
  possibility dimensions. The vector is still one scalar per domain.
- **An HTML UI beyond the panel and docs page.** No menus, settings screens,
  or onboarding flow. The browser shell reaches key-command parity with
  native and stops; UI/UX investment belongs to a product phase, not a
  runtime phase.
- **OPFS.** Considered for persistence (sync access handles are
  worker-only and fast), rejected for now: it would force vault I/O onto a
  dedicated worker and an async bridge *anyway*, for no capability the
  write-behind IndexedDB mirror lacks at vault scale (deviations only, KBs
  to low MBs). Revisit if profiling shows journal drain cost that matters.
- **Timing-gated CI.** Unchanged from Phase 6: CI gates counts, bytes,
  hashes, and invariants. Browser wall-clock (startup marks, frame times) is
  measured locally and recorded in the baseline doc.

### 1.5 Compatibility profiles (reduced profiles, made concrete)

The profile is detected at startup, shown in the panel, and overridable by
URL params. Every profile runs the **same world model**; profiles select
execution and presentation substrates only (ADR 0018 licenses exactly this).

| Profile | Requires | Executor | Map | Tier ceiling |
|---|---|---|---|---|
| **Full** | WebGPU + workers ≥ 2 | `WorkerExecutor` | GPU compose + refinement | by device (Mid/High) |
| **Single-thread** | WebGPU only | `InlineExecutor` | GPU compose, refinement off | Low |
| **Compat** | 2D canvas only | `InlineExecutor` | CPU compose → `putImageData` | Low |

There is no profile without persistence: IndexedDB is universal. If even
that fails (private-mode quota, storage denied), the vault runs on the
`MemoryStorage` mirror alone and the panel says so — the run works, nothing
survives the tab.

### 1.6 The three honest friction points

Everything else in this phase is assembly. These three are design work, and
each gets an ADR:

1. **Jobs are closures; workers need data.** `TaskExecutor::submit` takes
   `Box<dyn FnOnce() + Send>`, and results return through an
   `mpsc::Sender<JobResult>` captured inside the closure. A closure cannot
   cross a Web Worker boundary without shared memory (non-goal). Resolution:
   make the job's *content* a first-class value (`JobSpec`) with a pure
   neutral evaluator, keep the closure path as the native wrapper, add an
   opt-in spec-level submission path to the trait (§6.2, ADR 0019).
2. **`Storage` is synchronous; IndexedDB is not.** The trait's docs
   anticipated this ("callers must treat it as potentially asynchronous"),
   and the vault's design already absorbs it: it stores only deviations
   (small), loads everything at `open`, and flushes through a budgeted dirty
   queue. Resolution: a fully-loaded in-memory mirror that satisfies the
   sync trait, with an ordered write-behind journal to IndexedDB and a
   stated durability window (§6.3, ADR 0020).
3. **The panel is pixels; the browser wants DOM.** The panel's content is
   currently expressed only as bitmap-font draw calls. Resolution: a
   serializable panel model, two dumb renderers (§6.4, ADR 0021).

---

## 2. Where this sits in the subsystem-plan map

| Section 21 plan | Phase 7 coverage |
|---|---|
| `browser-runtime-plan.md` | **This document** — the whole phase. |
| `job-system-plan.md` | Browser slice — the descriptor transport and `WorkerExecutor` (§6.2); the native `LaneExecutor` is untouched. |
| `persistence-plan.md` | Browser slice — `BrowserStorage`, durability semantics, autosave/suspend (§6.3); formats and the vault are untouched. |
| `renderer-plan.md` | Browser bring-up slice — canvas surface, WebGPU tiers, compat fallback (§6.5, §6.7); still no terrain renderer. |
| `region-streaming-plan.md` | Consumed unchanged — `RegionMap` does not learn it is in a browser. |
| `determinism-and-versioning-plan.md` | The parity surface finally machine-checked *on both platforms* (§9.1, §11.2). |

Generation math is **untouched**: no layer changes, no hashing change, no
fold-order change, no record change. Phase 7 changes *where* the same world
runs, how its jobs travel, where its bytes rest, and how its panel is drawn.

---

## 3. Architecture overview

```text
                 unchanged world model (Phases 1–6)
   ┌──────────────────────────────────────────────────────────────┐
   │ world-core: hashing · layers L0..L8 · genomes · steering ·   │
   │             records/codec        (wasm-clean since Phase 0)  │
   │ world-runtime: RegionMap · Vault<S> · Budget · ResourceTier  │
   │             + NEW: JobSpec/execute_spec seam (§6.2)          │
   └──────────────────────────────────────────────────────────────┘
                  │
   ┌──────────────▼───────────────────────────────────────────────┐
   │ app-core (NEW, neutral): the game, shell-agnostic (§4.1)     │
   │   session.rs  WorldSession<S: Storage> — the state machine   │
   │   command.rs  Command enum — the input vocabulary            │
   │   viz.rs      Channel · MapComposer · overlays (moved)       │
   │   panel.rs    PanelModel + build() (§6.4)                    │
   └───────┬──────────────────────────────────────┬───────────────┘
           │                                      │
   ┌───────▼───────────────┐            ┌─────────▼─────────────────┐
   │ platform-native (thin)│            │ platform-web (grows up)   │
   │  winit · keymap       │            │  canvas · keymap · rAF    │
   │  LaneExecutor (tools) │            │  WorkerExecutor (§6.2)    │
   │  FileStorage  (tools) │            │  BrowserStorage (§6.3)    │
   │  Hud: PanelModel →    │            │  PanelModel → JSON → DOM  │
   │       bitmap strip    │            │  (HTML panel, §6.4)       │
   └───────┬───────────────┘            └─────────┬─────────────────┘
           │                                      │
   ┌───────▼──────────────────────────────────────▼───────────────┐
   │ renderer: wgpu/WGSL — native surface OR HtmlCanvasElement;   │
   │ GPU map compose + refinement (derived-only, ADR 0017)        │
   └──────────────────────────────────────────────────────────────┘
        web shell extras: index.html (app) · docs.html (§6.9)
        workers: N × wer-worker.js, each its own wasm instance
```

Four commitments organize everything:

1. **One game, two shells.** Any behavior implemented in a shell is a bug
   waiting to diverge. Movement, capture selection, save/load choreography,
   panel content, map composition — all live in `app-core`; shells own only
   event sources, executors, storage backends, and blitting.
2. **Jobs are pure data.** A generation job is a value: serializable,
   inspectable, executable anywhere by a pure function. The closure path is
   a wrapper around the value, not the other way round (ADR 0019). This is
   the browser-portability requirement of section 19 ("generation jobs must
   be resumable and interruptible") taken to its logical form.
3. **The mirror is authoritative in-session; the journal is the disk.**
   Browser persistence never blocks a frame and never tears a record; its
   durability window is stated, bounded, and shrunk at the moments that
   matter (autosave, hide, save-command) rather than hidden (ADR 0020).
4. **The panel is a model.** Shells render `PanelModel`; nothing about the
   world is expressed panel-first in pixels or DOM (ADR 0021).

---

## 4. Data layout and crate moves

### 4.1 `crates/app-core` — the extraction

New platform-neutral crate (deps: `world-core`, `world-runtime`, `serde`,
`log`; `[lints] workspace = true`; joins the wasm CI check). Contents move
from `platform-native`, changing home but not behavior:

- **`session.rs`** — `WorldSession<S: Storage>`: today's `World` struct from
  `main.rs` (map, field, anchors, bias, player/last-player, transition mode,
  capture category/polarity, `Vault<S>`, recorder, tracker, budget) plus its
  methods (movement integration, `update()` orchestration incl. budgeted
  vault flush, save/load choreography with the zero-travel settle, capture,
  discovery record/summon, preserve and route toggles). Generic over
  `Storage`; takes `&dyn TaskExecutor` per tick exactly as `RegionMap` does.
  The executor and storage are *constructor inputs* — the session spawns
  nothing and opens nothing (the crate-boundary rule; enforced as always by
  the wasm CI check).
- **`command.rs`** — `enum Command { Move(dir), Sprint(bool), Nudge(dim, sign),
  ResetBias, DropAnchor(kind), CaptureCategory, CapturePolarity, Capture,
  Transition, ClearAnchors, SaveSession, LoadSession, RecordDiscovery,
  SummonDiscoveries, TogglePreserve, RecordRoute, ToggleRouteAttraction,
  CycleChannel, Overlay(which), ToggleGpuCompose, ToggleRefinement, … }` —
  the input vocabulary. Each shell owns only its physical keymap → `Command`
  table (browser and native keep the same bindings; the README table becomes
  the shared spec).
- **`viz.rs`** — `Channel`, the palettes, `MapComposer`, overlay drawing
  (moved verbatim from `platform-native`). This gives the browser compat
  profile (§1.5) and the native screenshot path one composer — the CPU
  composer remains the presentation correctness reference (phase-6-plan
  §6.5) on both platforms.
- **`panel.rs`** — `PanelModel` and its builder (§6.4). The bitmap renderer
  does *not* move; it stays in `platform-native` as one of the two dumb
  renderers.

`platform-native` keeps: winit shell, keymap, `Hud` (bitmap renderer, now
model-driven), tier-input gathering, renderer wiring, `--screenshot`,
`--inline`. `tools` keeps `LaneExecutor` and `FileStorage` (native platform
services). The harnesses (`wer-ledger`, `wer-anchor`, `wer-vault`,
`wer-scale`, continuity replay) are the proof the extraction preserved
behavior: they pass unmodified or the refactor is wrong (§11.1).

### 4.2 `JobSpec` — jobs as data

The data today's dispatch closure captures, made nameable (world-runtime,
neutral, `serde` + `postcard` like every other portable byte surface):

```rust
// world-runtime/src/job.rs — NEW (§6.2, ADR 0019)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobSpec {
    /// Regenerate one layer of one region: the coord/layer identity, the
    /// realized possibility input, the dependency hash the output is keyed
    /// by, and the upstream tile planes the layer reads (owned copies of
    /// the Arc'd immutable data — see §6.2 on cost).
    Regen(RegenSpec),
    /// Compute one macro drainage tile (ADR 0009 inputs).
    Macro(MacroSpec),
}

/// Pure evaluator: the body of today's dispatch closure, extracted.
/// Same math, same order, same outputs — the closure path calls this too.
#[must_use]
pub fn execute_spec(spec: &JobSpec) -> JobResult;
```

`JobResult` already exists (the mpsc payload); it gains `serde` derives.
Postcard encode/decode of spec and result are the worker wire format —
the same codec discipline the vault uses, applied to transient job bytes
(these bytes are *not* persisted and carry no format-version obligation
beyond "same build on both ends", which the browser guarantees: the worker
loads the same wasm module).

### 4.3 Browser storage layout

IndexedDB database `wer-vault` (version 1), one object store `records`:
key = the existing vault key namespace as a string
(`meta/store`, `session/current`, `disc/<id:016x>`, `route/…`, `pres/…`,
`seen/…` — already ASCII, already sorted-order-compatible), value = the
record's envelope bytes as an `ArrayBuffer`. One record per put; a put is
atomic (old-or-new, the `Storage` contract). The vault's own format
(`RECORD_FORMAT_VERSION` = 1, envelope, postcard) is **unchanged** — an
exported browser bundle and a native vault dir hold byte-identical record
values, which is what makes `wer-atlas` interop (§6.3) trivial.

### 4.4 Deliberately rejected

- **Rewriting `Storage` as an async trait.** It would infect the vault, the
  session, every harness, and native code that has no async runtime — to
  serve one backend whose workload (small, sparse, load-once/write-behind)
  doesn't need it. The mirror design (§6.3) contains the asynchrony at the
  backend boundary, which is where the trait docs always said it lived.
- **A second wasm memory shared between workers via SAB.** Non-goal (§1.4);
  stable toolchain forbids threaded std, and the descriptor path must exist
  anyway as the no-isolation fallback (COOP/COEP headers are not guaranteed
  on arbitrary static hosting).
- **Rendering the panel with egui (or any immediate-mode GUI) on both
  platforms.** It would solve the divergence problem while adding a
  rendering dependency heavier than the panel itself, and would surrender
  the actual requirement: a real HTML panel (selectable text, native
  scrolling, CSS, a11y) in the browser.
- **Bundlers / npm dependency tree for the web shell.** The shell is one
  page, one panel renderer, one worker script, one docs page. TypeScript
  compiled by `tsc` alone (no bundler, ES2022 modules) keeps the no-server,
  no-build-magic property the smoke test established; the only web-side
  toolchain additions are `tsc` and the existing `wasm-bindgen` CLI.

---

## 5. Public interfaces

### 5.1 `world-core`

**No changes.** No new parity exports are expected: the ten existing exports
already pin identity, steering, and codec; Phase 7 adds transport and shells,
not portable vocabulary. (If browser bring-up uncovers a genuinely new
portable surface, it lands as export #11 with a golden on both platforms —
but the plan's expectation is zero.)

### 5.2 `world-runtime`

```text
world-runtime/src/
    job.rs       # NEW: JobSpec, RegenSpec, MacroSpec, execute_spec (§4.2)
    task.rs      # TaskExecutor gains ONE defaulted method (§6.2):
                 #   fn submit_spec(&self, priority: TaskPriority,
                 #                  spec: JobSpec,
                 #                  results: Sender<JobResult>,
                 #                  cancel: Arc<AtomicBool>) -> bool
                 #   { false }   // false = "use the closure path"
    stream.rs    # dispatch_regen: builds JobSpec, offers it via submit_spec,
                 #   falls back to the existing closure (which now calls
                 #   execute_spec) — same outputs either way
    vault.rs     # unchanged (Vault<S: Storage> already generic)
    tier.rs      # unchanged types; detect() unchanged — browser inputs are
                 #   gathered platform-side into the same TierInputs (§6.7)
```

`InlineExecutor` and `LaneExecutor` do not implement `submit_spec`; they keep
the closure path (their default `false` is the fallback working as designed).
`FrameStats` grows nothing world-side; browser-shell telemetry (journal
depth, worker queue, startup marks) is shell-side panel content.

### 5.3 `crates/app-core` (new)

The §4.1 surface: `WorldSession<S>`, `Command`, `Channel`/`MapComposer`,
`PanelModel`/`PanelInput`. All `#[derive(Debug)]`, wasm-clean, no
filesystem/thread/GPU/browser API anywhere in the crate.

### 5.4 `platform-web`

```text
crates/platform-web/
    src/lib.rs        # parity exports (unchanged) + app entry points:
        #[wasm_bindgen] pub fn boot(canvas: HtmlCanvasElement,
        #                           opts: JsValue) -> Promise   // async init
        #[wasm_bindgen] pub struct WebApp;   // owns WorldSession<BrowserStorage>
        impl WebApp {
            pub fn frame(&mut self, dt_ms: f64) -> ...   // rAF tick
            pub fn command(&mut self, cmd_json: &str);   // keymap → Command
            pub fn cursor(&mut self, x: f32, y: f32);
            pub fn panel_json(&mut self) -> String;      // PanelModel (§6.4)
            pub fn startup_report_json(&self) -> String; // §6.8 marks
        }
    src/executor.rs   # WorkerExecutor (wasm-gated): implements TaskExecutor,
                      # submit_spec = true path (§6.2)
    src/store.rs      # BrowserStorage: sync mirror + write-behind journal
                      # (wasm-gated IndexedDB glue; mirror logic is
                      # target-independent and unit-tested natively, §11.3)
    src/worker_entry.rs  # #[wasm_bindgen] run_job(bytes: &[u8]) -> Vec<u8>
                         #   = postcard decode → execute_spec → encode
    web/
        index.html    # the app: canvas + <aside id=panel> + docs link
        docs.html     # the documentation page (§6.9)
        src/*.ts      # shell glue: boot, keymap, rAF loop, panel renderer,
                      # worker pool host, storage init — tsc, no bundler
        wer-worker.js # worker bootstrap: instantiate shared module, loop
        vendor/       # tiny Markdown renderer for docs.html (checked in)
```

`web-sys` features grow accordingly (`HtmlCanvasElement`, `Window`,
`Document`, `Worker`, `MessageEvent`, IndexedDB types, `Performance`,
`Storage` events, visibility). All still gated under
`cfg(target_arch = "wasm32")` so native workspace checks stay browser-free.

### 5.5 `renderer`

- `Renderer::new` is already async and already takes a surface-source
  closure — on wasm the closure yields
  `wgpu::SurfaceTarget::Canvas(HtmlCanvasElement)`. What changes:
  `probe_adapter()`'s spin-loop `pollster_block` is native-only poison on
  wasm; it becomes `#[cfg(not(target_arch = "wasm32"))]`, and the browser
  path probes through the async API it already has.
- The `renderer` crate **joins the wasm32 CI check** (it was never checked;
  wgpu compiles for wasm, and any native-only leakage gets caught the same
  way the neutral crates' leaks do).
- No API additions. The GPU map (`GpuMap`, `MapTileUpload`, `GpuMapParams`)
  is used as-is from the web shell; ADR 0017's no-readback surface is
  unchanged and now enforced in a second shell.

### 5.6 `platform-native`, `tools`

Native shrinks (§4.1) but its behavior, flags, env vars, and key bindings
are identical before and after. `tools` gains nothing; every harness keeps
running against `app-core`'s session where it previously duplicated shell
logic (the vault harness's save/load choreography, notably, becomes a call
into the same `WorldSession` code the shells use — one less copy of the
truth).

---

## 6. Algorithms and designs

### 6.1 The browser frame loop

`requestAnimationFrame` drives `WebApp::frame(dt)`. Inside: clamp dt (as
native does), drain queued `Command`s, `session.tick(...)` (which runs
`RegionMap::update` with the profile's executor and the tier's budget),
compose (GPU path: atlas delta uploads + `compose_map.wgsl` draw; compat
path: `MapComposer` → `putImageData`), then every Nth frame (~10 Hz)
`panel_json()` → DOM update (§6.4). Budgets are count/cost-based
(ADR 0018), so a 30 Hz laptop, a 144 Hz desktop, and a throttled background
tab produce the same world at different pacing — the property Phase 6
proved is exactly the property browser frame variability needs.

When the tab is hidden, rAF stops; no frames ⇒ no travel ⇒ no convergence
(ADR 0006: fueled by travel, not wall-clock) ⇒ **suspension is free**. On
`visibilitychange → hidden` the shell additionally snapshots the session and
drains the journal (§6.3) so an OS-killed background tab loses at most
nothing. On `pageshow`/resume, the loop simply restarts; a lost WebGPU
device or resized canvas rebuilds through the renderer's existing
surface-source/`acquire_frame` recovery machinery.

### 6.2 The `WorkerExecutor` (Web Worker scheduling, ADR 0019)

**The seam.** `dispatch_regen` builds the `JobSpec` it was about to capture,
then:

```rust
if !executor.submit_spec(priority, spec.clone(), self.results.clone(), cancel.clone()) {
    let tx = self.results.clone();
    executor.submit(priority, Box::new(move || {
        if cancel.load(Relaxed) { return; }
        let _ = tx.send(execute_spec(&spec));
    }));
}
```

Native executors take the closure branch — whose body is now the *same
function* the workers run, so the spec path is exercised by every native
test by construction. The `mpsc::Sender`/`Receiver` pair stays: on wasm the
"channel" is main-thread-only (the `onmessage` callback that sends and the
`integrate_finished` pass that receives both run on the main thread's event
loop), which `std::sync::mpsc` handles fine without threads.

**The pool.** The TS host spawns `min(hardwareConcurrency - 1, 4)` workers
(cap revisited from measurement), passing each the compiled
`WebAssembly.Module` (structured clone shares compiled code; each worker
gets its own instance and memory — no shared state, which is what makes
this trivially correct). `WorkerExecutor` keeps three FIFO lanes mirroring
`LaneExecutor`'s priority discipline, posts postcard-encoded specs
(`Uint8Array`, transferred) to idle workers, and on `onmessage` decodes the
`JobResult` and `send`s it into the map's channel. The existing job-id gate
in `integrate_finished` remains the correctness backstop for late or
superseded results — unchanged code, doing for worker messages exactly what
it did for thread messages.

**Cancellation.** The `Arc<AtomicBool>` token is checked before a queued
spec is posted (queued-but-unposted jobs cancel for free, the common
supersession case). In-flight-on-a-worker jobs are not interrupted — they
finish and get dropped by the id gate, same as a native worker that already
dequeued. `jobs_cancelled`/`results_dropped` telemetry works unchanged.

**Cost honesty.** Spec transport *copies* upstream tile planes into the
message (structured clone; a job needs the handful of upstream channels its
layer reads, ~4 KB per channel). At `max_regen_cost` scale this is hundreds
of KB per storm frame — measured in M5 and recorded in the baseline doc. If
it matters, the mitigation ladder is: per-worker tile caching keyed by
dep-hash (workers already hold nothing; a small LRU makes repeat deps free),
then shared memory (deferred, §1.4). Results transfer back (ownership moves,
no copy).

### 6.3 `BrowserStorage` (persistence, ADR 0020)

```rust
pub struct BrowserStorage {
    mirror: MemoryStorage,                      // in-session authority
    journal: VecDeque<JournalOp>,               // ordered write-behind
    drained: Rc<...>,                           // IndexedDB glue handle
}
enum JournalOp { Put(Vec<u8>, Vec<u8>), Remove(Vec<u8>) }
```

- **Open (async, before `Vault::open`):** read every `records` entry from
  IndexedDB into the mirror. The vault is deviations-only; this is KBs to
  low MBs and happens once, inside the startup sequence (§6.8), in parallel
  with renderer init.
- **Sync trait:** `load`/`keys_with_prefix`/`contains` read the mirror;
  `store`/`remove` mutate the mirror *and* append to the journal. The
  `Storage` contract (atomic, old-or-new) holds at the record level: one
  journal op = one IndexedDB `put`/`delete` = atomic.
- **Drain:** an idle-time task (and every `Vault::flush` call site already
  paced by `max_persist_ops`) commits journal ops to IndexedDB **in order**,
  batched into transactions. Coalescing is allowed (a later put to the same
  key supersedes an earlier one) because the vault's dirty queue already
  coalesces the same way.
- **Durability window (the ADR's core statement):** the mirror is
  authoritative for the session; a crashed tab loses at most the un-drained
  journal tail — never a torn record, never a reordering across records.
  The window is actively shrunk at the moments that matter: explicit save
  (`O`) drains fully before reporting success; autosave (every ~30 s of
  activity) snapshots + drains; `visibilitychange → hidden` drains.
  The panel shows journal depth next to the vault line — the browser twin
  of the native `dirty` count.
- **Bundle interop:** export = serialize the store's records into the
  existing `wer-atlas` bundle bytes and hand them to a download; import =
  file picker → bundle bytes → the existing CRDT merge (ADR 0014, order
  never matters). Same bytes a native `wer-atlas` produces and consumes —
  the sharing model's browser expression, no server, no new format.

### 6.4 The HTML panel (ADR 0021)

**The model** (app-core, serde):

```rust
pub const PANEL_SCHEMA: u16 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct PanelModel { pub schema: u16, pub title: String,
                        pub sections: Vec<PanelSection> }
#[derive(Debug, Serialize, Deserialize)]
pub struct PanelSection { pub header: Option<String>, pub rows: Vec<PanelRow> }
#[derive(Debug, Serialize, Deserialize)]
pub enum PanelRow {
    /// One or two label/value cells on a line (the `pair` idiom).
    Cells(Vec<PanelCell>),
    /// Full-width text (status lines, "none active", key help).
    Text { text: String, tone: Tone },
    /// A key-binding help line.
    Key { keys: String, action: String },
}
#[derive(Debug, Serialize, Deserialize)]
pub struct PanelCell { pub label: String, pub value: String, pub tone: Tone }
#[derive(Debug, Serialize, Deserialize)]
pub enum Tone { Value, Label, Active, Alert, Key }
```

`PanelModel::build(&PanelInput) -> PanelModel` transcribes today's
`draw_panel` content — TIMINGS, tier/ceiling, EXEC/POOL, regions/cache,
jobs/deferred, organisms, resonance/mode, vault (+ journal depth in the
browser), pinned violations, REGEN BY LAYER, channel/player, BIAS, ANCHORS,
CURSOR (including the ecology block), KEYS — section by section, with the
semantic emphasis (`Active` for nonzero bias, `Alert` for violations/issues)
that today lives in color constants. `PanelInput` is `PanelInfo` renamed and
moved, minus nothing.

**Native renderer:** `Hud::draw_panel` becomes a ~80-line walk over the
model — `Cells` uses the existing two-column `pair` layout, tones map to the
existing palette constants, rules separate sections. Pixel output is
intended to be visually identical; it is presentation, not a golden surface,
and `--screenshot` continues to capture it headlessly.

**Browser renderer:** `panel_json()` (serde_json; ~10 Hz, and only when the
model changed — a cheap hash gate) → a small TS renderer into
`<aside id="panel">`. First render builds the DOM (`<section>`, header,
rows as flex lines, tone → CSS class); subsequent renders diff per-row and
update `textContent` only (no innerHTML churn, no allocation storm). Dark
CSS matching the native palette; the panel is real DOM: selectable,
scrollable, styleable, and screen-reader-visible. The KEYS section renders
from the same model, so the two shells can never document different
bindings.

**The ADR (0021):** the panel is a versioned declarative model built in
neutral code; shells are dumb renderers; new telemetry lands in the model
once or it doesn't land at all.

### 6.5 Rendering in the browser

The Full and Single-thread profiles run the Phase 6 GPU map unchanged:
`Renderer::new` with a canvas surface target, `GpuMap` atlas with dep-hash
delta uploads, `compose_map.wgsl` (WGSL, already WebGPU-portable by
construction), refinement octaves per tier. The overlay stays CPU-drawn and
uploaded, as native does. The panel is *not* composed into the frame in the
browser — it is DOM beside the canvas (§6.4); the canvas shows the map
alone, letterboxed by the existing `letterbox_viewport` math (which the
cursor-picking code inverts, unchanged).

The Compat profile uses `MapComposer` (now in app-core) into an
`ImageData` → `putImageData` on a 2D context — the same pixels the native
screenshot path produces. Refinement requires the GPU path and is simply
absent here.

### 6.6 Input

`keydown`/`keyup` on the document, `mousemove` on the canvas, mapped by a
TS keymap table to `Command` JSON → `WebApp::command`. The bindings are the
README table verbatim (WASD/arrows, 1–8/Shift, Z, E/Q, T/Y/K, R, C, O/L, B,
I, P, J, U, F, V, G/N/X/M, `,`/`.`). Browser-reserved combos are avoided by
construction (all bindings are bare keys). `Esc` is repurposed to blur the
canvas rather than quit.

### 6.7 Tiers, profiles, budgets

`TierInputs` gathering, browser edition (`tier.rs` itself unchanged):

- `cores` ← `navigator.hardwareConcurrency` (workers = cores − 1, capped).
- `adapter` ← no WebGPU or fallback adapter ⇒ `Cpu`; otherwise
  `Integrated` (browser adapter info is deliberately vague; we do not
  guess discrete — a wrong `High` hurts more than a modest `Mid`).
  `WER_TIER`-equivalent URL param `?tier=low|mid|high` overrides, exactly
  like the env var.
- Memory hint: `navigator.deviceMemory < 4` clamps to Low;
  `?cache_mb=` overrides the field-cache ceiling like `WER_CACHE_MB`.

The profile (§1.5) is orthogonal detection (WebGPU present? workers
usable?) and composes with the tier: profile picks substrates, tier picks
capacity. Both are logged at boot, shown in the panel, and included in the
startup report. Budgets stay count/cost-based; nothing browser-side may
introduce a wall-clock-adaptive budget (ADR 0018 forbids outcomes depending
on machine speed).

### 6.8 Startup, assets, benchmarking

Startup sequence (all async stages overlapped where independent):

```text
fetch + WebAssembly.instantiateStreaming(wasm)     mark: wasm-init
├─ Renderer::new(canvas)  (Full/Single profiles)   mark: gpu-init
├─ BrowserStorage open ← IndexedDB full read       mark: store-open
└─ worker pool spawn + module share (Full)         mark: pool-ready
first frame presented                              mark: first-frame
window settled (deferred_* drained at spawn)       mark: settled
```

`performance.mark`/`measure` for each; `startup_report_json()` surfaces
them; the panel shows `boot`/`settle` lines; the numbers join
`docs/perf-baseline.md` in a new **Browser** section alongside the wasm
binary size. Asset streaming is deliberately boring: the app is one wasm
binary (shaders are embedded WGSL strings, fonts are the panel's CSS), so
"streaming" means `instantiateStreaming` (requires the
`application/wasm` MIME type — the README's serve instructions say so),
compression left to the host, and a size budget: the release wasm is built
with `-C target-feature=+simd128` (stable; the `wide` kernels light up per
ADR 0016 — bit-identity is per-platform and untouched), `opt-level`
inherited, size recorded per milestone. A no-simd fallback build command is
documented but not shipped by default (all evergreen desktop browsers have
had wasm SIMD since 2021).

### 6.9 The documentation page

`web/docs.html`: a static page, styled like the panel, with a two-entry
table of contents rendering two Markdown documents fetched at load and
rendered by a small vendored renderer (`web/vendor/`, checked in, zero
network dependencies — headings, paragraphs, lists, links, code, tables,
emphasis; nothing more):

- **[`docs/overview.md`](docs/overview.md) — the project, for anyone** (task
  **D1**, §15.2): what the Infinite World is, in words a curious visitor
  understands. The one continuous journey through possibility space; the
  orb; travel that gradually transforms the world instead of loading a new
  one; photographing discoveries into anchors that steer what comes next;
  expeditions, routes, preserves, and the shareable atlas. Honest about
  status: what is playable today (the debug-map prototype) versus the
  vision (`Infinite_World_Exploration_Project_Overview.md` is the source
  material, *filtered through what actually exists*). No jargon, no code,
  no ADR numbers. Target length ~1,000–1,500 words.
- **[`docs/world-model.md`](docs/world-model.md) — the model, as
  implemented** (task **D2**, §15.2): the low-level description of the
  current machine, written against the code, not the plans. The possibility
  vector (eight domains, one scalar each) and the control-point field; the
  nine-layer dependency graph with dep-hash staleness (ADR 0008) and
  integer hashing (ADR 0003); stable drainage topology (ADR 0009); rosters,
  genomes, food webs, and L8 (ADR 0010); anchors, order-independent
  combination, plausibility projection, resonance (ADRs 0011–0012); the
  vault — deviations only, quantized shareable records, CRDT merge, the
  session snapshot (ADRs 0013–0014); routes as derived weak anchors
  (ADR 0015); the Phase 6 substrate — lanes, pools, ceilings, bit-identical
  SIMD, derived-only GPU, schedule independence, tiers (ADRs 0016–0018);
  and the Phase 7 additions once landed (ADRs 0019–0021). States what is
  deliberately absent (networking, 3D renderer, one-scalar domains).
  Cites files and ADRs the way AGENTS.md does. Target length ~2,500–4,000
  words.

The build step that copies `docs/overview.md` and `docs/world-model.md`
into `web/generated/docs/` joins the README's wasm-bindgen build snippet
(one `cp` line), so the canonical sources live in `docs/` — readable on
GitHub, versioned with the code they describe — and the served page never
drifts from them. `index.html` links to `docs.html` from the header.

**D1 and D2 are writing tasks, not engineering tasks.** They depend on no
milestone (the page shell in M7 renders whatever exists, and both documents
describe the *already-landed* Phases 0–6 model), and not on each other.
They can be drafted on day one of the phase, by different people, and
reviewed like code: D2's acceptance review checks every claim against the
source; D1's checks that no unimplemented vision is presented as shipped.

---

## 7. Scheduling and budgets

The update pipeline keeps its exact pass order and semantics — `RegionMap`
does not know it is in a browser. What differs per platform is entirely
inside the seams: which executor `submit`s (LaneExecutor threads vs. worker
`postMessage` vs. inline), which storage the vault wraps, and what drives
the tick (winit redraw chain vs. rAF). Budgets are per-frame counts, so
browser frame-rate variability changes pacing only (ADR 0018); the
backpressure discipline (deferred counters bounded under pressure, draining
after it) carries over as the browser stability gate, run in the Single-
thread profile at Low tier — the worst legitimate case — as well as Full.

One browser-specific budget rule: the journal drain and the panel DOM
update are shell work, and both are paced (drain batches bounded per idle
slice; panel at ~10 Hz, change-gated) so neither can eat the frame.

---

## 8. Threading model

Native: unchanged. Browser: the main thread owns *all* world state — the
session, the map, the mirror, the panel model — and is the only
`mpsc` sender/receiver toucher besides worker callbacks, which the event
loop serializes onto the same thread. Workers are share-nothing wasm
instances that receive pure `JobSpec` bytes and return pure `JobResult`
bytes; there is no shared memory, no atomics (beyond the advisory
cancellation flag, which on wasm is main-thread-only anyway), and no lock.
This is the strictest possible reading of the single-writer discipline the
codebase has had since Phase 1 — the browser doesn't weaken the model, it
enforces it structurally.

---

## 9. Determinism and versioning

### 9.1 No version bumps, no re-blesses — the phase invariant

Phase 7 changes **no generated output for any input**: no layer math, no
hashing, no fold order, no record bytes, no steering math.
`WORLD_ALGORITHM_VERSION` stays **2**, every `algorithm_revision` stays 0,
`RECORD_FORMAT_VERSION` stays **1**, and every golden fixture and harness
expectation passes **unmodified** — through the app-core extraction, the
spec seam, and both shells. A re-bless in a Phase 7 diff is a determinism
bug by definition.

The parity claim gets *stronger*: the ten exports' goldens are currently
asserted natively only; Phase 7 runs them **as wasm in CI** (§11.2), turning
"native and wasm must agree" from a convention checked by hand in a browser
console into a machine gate.

### 9.2 What equality means across the platform boundary (unchanged rules)

The cross-platform contract is the **integer surface**: feature hashes,
seeds, drainage topology, genome/food-web fingerprints, codec bytes, and
the declared float-deterministic steering samples. Full settled *state*
hashes are same-platform properties: `f32` transcendentals (`ln` in
`river_intensity`) may differ between native libm and wasm, exactly as they
may between OSes — Phases 2–6 drew this boundary and Phase 7 does not move
it. Concretely gated:

- Browser save→load→settle is state-hash exact **within the browser
  runtime** (the vault harness's central property, run as a wasm test).
- Records, bundles, quantized buckets, and steering-from-records are
  byte/bit-identical **across** platforms (already golden; now golden on
  both sides).
- Schedule independence (ADR 0018) extends to the new executor: settled
  state hashes are invariant across Inline vs. WorkerExecutor at any pool
  size — asserted in the browser test lane with a scripted settle (§11.4),
  and natively by construction (the closure path now runs `execute_spec`,
  so native CI exercises the spec body every time).

### 9.3 New ADRs

- **ADR 0019 — Generation jobs are pure data; executors may serialize
  them.** A job is a `JobSpec` value evaluated by a pure, neutral
  `execute_spec`; the closure API is a wrapper. Any executor — inline,
  threaded, worker-pool, or a future shared-memory pool — is legal iff
  settled state is unchanged (extends ADR 0018 to transport). Job bytes are
  transient and carry no cross-version compatibility obligation.
- **ADR 0020 — Browser persistence is a write-behind mirror with a stated
  durability window.** The sync `Storage` trait is satisfied by an
  in-memory mirror loaded fully at open; an ordered journal drains to
  IndexedDB; record-level atomicity always holds; the tail-loss window is
  bounded and actively closed on save/hide/autosave. Never a torn record,
  never cross-record reordering.
- **ADR 0021 — The information panel is a serializable model; shells are
  dumb renderers.** Panel content is built once, in neutral code, as a
  versioned `PanelModel`; the native bitmap strip and the browser HTML
  panel render it without adding or reinterpreting content.

---

## 10. Debug visualization and tools

- **The HTML panel** is itself the browser's debug surface (§6.4), with the
  browser-specific lines: profile + tier, journal depth, worker
  queued/in-flight per lane, startup marks, wasm size stamp.
- **Native panel** unchanged in content (now via the model), so the two
  shells are visually comparable line by line — the panel *is* the parity
  eyeball for shell telemetry.
- **`wer-inspect`, `wer-atlas`, harness bins:** unchanged. `wer-atlas`
  bundles gain a second producer/consumer (the browser's export/import) —
  round-tripping a browser-exported bundle through native `wer-atlas
  validate` is the interop smoke test.
- **URL params** as the browser's env vars: `?tier=`, `?cache_mb=`,
  `?profile=compat|single|full` (force-degrade for testing), `?inline=1`
  (the `--inline` A/B), `?cpu_map=1` (the `WER_CPU_MAP` twin).

---

## 11. Testing strategy

### 11.1 Existing fixtures and harnesses: the regression net

`cargo test --workspace` — determinism goldens, parity goldens (native
side), continuity replay, ledger, ecology, anchor, vault, scale harnesses —
passes **unmodified at every milestone**. The app-core extraction (M1) and
the spec seam (M5) are the two refactors that touch load-bearing code; the
harness corpus plus the zero-re-bless invariant is what proves them
behavior-preserving. The scale harness's schedule-independence scenarios
re-run against the closure-wrapping-spec path by construction.

### 11.2 Wasm executed in CI (the new lane)

A new CI job (**web**) that goes beyond the existing compile-checks:

1. Build the release wasm (`+simd128`) and run `wasm-bindgen` — the
   artifact build itself becomes a gate (today only `cargo check` gates).
2. `tsc --noEmit`-equivalent check of the web shell sources.
3. `wasm-bindgen-test` under Node (no browser, no GPU needed) running:
   - the **ten parity exports against their goldens** — the cross-platform
     identity surface, finally asserted on both sides;
   - codec round-trips and a vault open/mutate/flush cycle over the
     `BrowserStorage` mirror (IndexedDB glue stubbed by `MemoryStorage`
     drain — the journal logic is target-independent);
   - `JobSpec` postcard round-trip + `execute_spec` output equality against
     the closure path for a sampled spec set;
   - a scripted settle: Inline executor, small window, save→load→settle
     state-hash exactness inside wasm (§9.2);
   - `PanelModel` build + JSON round-trip.

### 11.3 New native unit tests

- `app-core` session tests: command dispatch, save/load choreography
  (zero-travel settle), capture selection — the logic that used to hide in
  `main.rs` becomes testable for the first time.
- `BrowserStorage` mirror/journal (target-independent core): ordering,
  coalescing, drain batching, `Storage`-contract conformance against the
  same suite `MemoryStorage`/`FileStorage` satisfy.
- `PanelModel::build` golden-ish snapshot (JSON) for a fixed `PanelInput` —
  guards accidental content loss during the panel refactor (a snapshot, not
  a determinism golden; updating it is a review event, not a version bump).
- `submit_spec` fallback: an executor returning `false` produces identical
  integration results to one returning `true` over a stub transport.

### 11.4 Browser-level testing

- **Headless browser smoke** (local + optional non-gating CI): wasm-bindgen
  test runner in headless Chromium for boot-to-first-frame in the Compat
  profile (no WebGPU in CI). WebGPU paths are exercised locally, like the
  Phase 6 GPU map — CI has no GPU, and the CPU compose path remains the
  correctness reference on both platforms.
- **Manual matrix** (recorded in the sign-off doc, not CI): current Chrome,
  Firefox, Safari — boot, profile detection, worker settle, save/reload
  persistence, bundle export→native import, panel correctness, suspend
  (background 10 min) → resume.
- **Worker schedule independence:** the §11.2 scripted settle re-run in
  headless Chromium at pool sizes 0 (inline) / 2 / max — settled hashes
  equal (local gate; the transport is also covered by the native
  construction argument in §9.2).

### 11.5 CI summary

Native job: unchanged. Wasm check job: adds `renderer` and `app-core` to
the compile-check list. New web job: §11.2. Docs tasks add no CI (Markdown
is reviewed, not linted).

---

## 12. Profiling and metrics

- **Startup marks** (§6.8) — wasm-init, gpu-init, store-open, pool-ready,
  first-frame, settled — in the panel, in `startup_report_json`, and
  snapshotted into `docs/perf-baseline.md` (**Browser** section: machine +
  browser version labeled, like the native table).
- **Steady-state:** the existing `FrameStats`/pass_ms telemetry flows into
  the HTML panel untouched (`pass-timing` stays native-only; the wasm build
  reports zeros for pass_ms exactly as designed in Phase 6 — browser
  frame-level timing comes from `performance.now` around update/compose in
  the shell, which is where the shell always measured).
- **Worker transport:** specs posted, bytes cloned per frame, results
  transferred, queue depths — the §6.2 cost-honesty numbers, panel-visible
  and baseline-recorded.
- **Wasm size:** bytes (raw + gzipped) recorded per milestone; unexplained
  growth is a review flag, not a gate.

---

## 13. Native and browser constraints

Where Phase 7 stresses the standing obligations: the neutral crates
(`world-core`, `world-runtime`, now `app-core`) still spawn no threads,
touch no filesystem, open no sockets, and call no browser APIs —
`WorldSession` receives its executor and storage; `JobSpec`/`execute_spec`
are pure; `PanelModel` is data. All wasm-bindgen, web-sys, IndexedDB,
Worker, and canvas code is confined to `platform-web` under the existing
`wasm32` cfg gate, so native workspace builds stay browser-free. The
renderer stays WGSL-only and readback-free (ADR 0017) in its second shell.
Storage stays behind the trait with one new backend; the executor stays
behind the trait with one new implementation; `tier.rs` stays a pure
decision now fed by a second gatherer — every Phase 7 addition lands in a
seam an earlier phase left open, which is the architectural claim of
section 3.2 paying out. The one place Phase 7 *tightens* a native
convention: `probe_adapter`'s blocking helper becomes explicitly
native-only (§5.5), removing the last spin-wait a wasm build could reach.

---

## 14. Risks (mapping section 23)

| Risk | Phase 7 manifestation | Mitigation |
|---|---|---|
| 23.4 Platform divergence | The phase exists to retire this risk — but the shells themselves can diverge (input, panel, save flow) | One game, two shells (§4.1): session/commands/panel/composer in app-core; README key table as shared spec; panel model (ADR 0021); harnesses drive the shared session. |
| 23.5 Determinism drift | wasm f32 libm differences read as "the browser is wrong"; worker scheduling leaks into state | The integer parity surface asserted in wasm CI (§11.2); the §9.2 equality taxonomy stated up front; ADR 0018/0019 gates for the new executor; jobs pure by construction. |
| 23.6 Memory growth | wasm32's single linear memory + atlas + mirror + journal in one 4 GB space; no OS paging mercy | Existing byte ceilings tier-scaled down for browser tiers; mirror is deviations-only; journal bounded by drain pacing; `deviceMemory` clamp (§6.7); plateau gates re-run in-browser (§11.4). |
| 23.1 Continuity | Throttled tabs, slow devices, or worker latency cause visible pops or a never-settling window | Travel-fueled convergence makes throttling safe (ADR 0006); budgets count-based (ADR 0018); backpressure drain gates at the Single-thread/Low worst case (§7); pinned-violation detector runs in both shells. |
| 23.2 Scope risk | Browser UI/UX is a bottomless pit; the docs page grows a docs *site* | Non-goals fence (§1.4): panel + docs page only, no menus; docs are two Markdown files + one static page + a vendored renderer; TypeScript-no-bundler toolchain cap (§4.4). |
| 23.3 Dependency explosion | n/a — no invalidation-machinery change | Ledger harness unchanged in `cargo test`. |

Phase-specific risks: **IndexedDB durability folklore** (browsers may drop
transactions at pagehide — mitigated by draining at `visibilitychange:
hidden`, the reliable signal, plus autosave; the window is stated, not
wished away); **worker transport cost** (measured in M5 with a named
mitigation ladder, §6.2); **browser API churn** (the shell uses only
boring, universally-shipped APIs: rAF, workers, IndexedDB, canvas, WebGPU
with a no-WebGPU profile).

---

## 15. Incremental milestones and tasks

### 15.1 Engineering milestones (sequential where dependent)

Each keeps CI green (native + wasm32 + the new web job once it exists) and
every Phase 2–6 fixture and harness passing unmodified.

- **M1 — Extract the core.** Create `crates/app-core`; move
  session/commands/viz out of `platform-native` (§4.1); native shell
  refactored onto it with byte-identical behavior (flags, env vars, keys,
  screenshots); `app-core` + `renderer` join the wasm32 CI check;
  `probe_adapter` blocking helper cfg'd native-only. *Exit:* native
  indistinguishable before/after; all harnesses green unmodified; wasm
  checks green with the two new crates.
- **M2 — Boot the browser.** `WebApp` + `boot()`; canvas renderer init;
  rAF loop; keymap → commands; `InlineExecutor`; Low tier; GPU map path in
  the browser; Compat profile (2D-canvas CPU compose); startup marks; the
  **web CI job** (artifact build + tsc check + Node parity/codec tests —
  §11.2's first half). *Exit:* playable single-threaded world in Chrome and
  Firefox; ten parity goldens asserted as wasm in CI; boot marks recorded
  in the baseline doc.
- **M3 — The HTML panel.** `PanelModel` + builder in app-core; native `Hud`
  re-rendered from the model; browser DOM renderer + CSS + change-gated
  10 Hz updates; cursor readout wired from canvas mouse; ADR 0021;
  model-snapshot test. *Exit:* both shells render the same model; native
  screenshot output still headless-testable; panel content reviewably
  identical line-for-line between shells.
- **M4 — Persist and survive.** `BrowserStorage` (mirror + journal +
  IndexedDB glue); vault open at boot; save/load commands; autosave;
  `visibilitychange` drain; device-loss/resize recovery exercised; bundle
  export/import via file picker; ADR 0020; mirror/journal unit suite +
  in-wasm save→load→settle exactness test. *Exit:* reload the tab, load
  the session, settle — state-hash exact; browser-exported bundle passes
  native `wer-atlas validate`; kill-tab tail-loss bounded as stated.
- **M5 — Workers.** `job.rs` (`JobSpec`/`execute_spec`); `submit_spec`
  seam with closure fallback (native path now runs `execute_spec` inside
  the closure); `WorkerExecutor` + pool host + worker entry; cancellation
  before-post; transport telemetry; ADR 0019; spec/closure equality tests;
  scripted-settle pool-size invariance (local headless). *Exit:* Full
  profile settles a teleport storm with workers doing the kernel work and
  the main-thread update pass time dropping by the measured delta; settled
  hashes invariant across pool sizes; transport bytes/frame recorded.
- **M6 — Tiers, profiles, budgets, benchmarks.** Browser `TierInputs`
  gathering; profile detection + forced-degrade params; memory clamps;
  URL-param overrides; startup report finalized; baseline doc Browser
  section complete (boot marks, transport costs, wasm size, per-profile
  settle behavior). *Exit:* all three profiles pass the stability gates
  (bounded, draining backpressure; memory plateau under ceiling) on the
  manual matrix; detection table documented in AGENTS.md.
- **M7 — Docs page and sign-off.** `docs.html` + vendored Markdown
  renderer + build-step copy; integrate whatever D1/D2 drafts exist (the
  page renders the files as they are — D1/D2 completion is tracked
  independently, §15.2); README browser instructions rewritten (build,
  serve, MIME note, params); AGENTS.md Phase 7 architecture/commands
  update; sign-off record in `docs/`. *Exit:* §1.2's success criterion
  holds with evidence; the manual matrix is recorded; CI is green across
  all three jobs.

### 15.2 Independent documentation tasks (parallel to everything)

These are deliberately **not** milestones in the M-sequence: they have no
code dependencies, may start immediately, may be done by different authors,
and block only the *content completeness* of M7's page (which ships
regardless and renders whatever is merged).

- **D1 — Write `docs/overview.md`** (the high-level, non-technical
  overview; audience, scope, and length per §6.9). *Source material:*
  `Infinite_World_Exploration_Project_Overview.md`, filtered to distinguish
  shipped prototype from vision. *Acceptance:* a reader with no engine
  background understands what the project is, what the player does, and
  what exists today; zero unimplemented features presented as shipped;
  zero code references required to follow it.
- **D2 — Write `docs/world-model.md`** (the low-level description of the
  model as currently implemented; coverage per §6.9). *Source material:*
  the code, `AGENTS.md`, ADRs 0001–0018 (0019–0021 appended as they land),
  the phase plans as history — but every claim verified against source,
  not plans. *Acceptance:* a new contributor can predict, from this
  document alone, which layer a change touches, what invalidates when
  possibility drifts, what persists versus regenerates, and which surfaces
  are cross-platform contracts; every file/ADR citation resolves; a review
  pass confirms no statement contradicts the code as of the commit that
  merges it.

**Phase 7 is done when** M1–M7 are complete, D1 and D2 are merged, CI is
green across the native, wasm-check, and web jobs (every Phase 2–6 golden
and harness unmodified, the ten parity goldens passing *as wasm*, the
in-browser save/load/settle exactness and pool-size invariance gates
passing), and the success criterion holds with evidence: the same world
model — same identity surface, same records, same steering — runs as a
persistent, playable application in modern desktop browsers, degrading
gracefully across the Full/Single-thread/Compat profiles, scaling density
and caches to the device through the same tier table as native, with the
information panel rendered as HTML from a model both shells share, a
documentation page telling both the human story and the machine story, and
not one golden fixture re-blessed — the browser delivery the architecture
promised in section 3.2, arriving through the seams every phase since
Phase 0 kept open for it.
