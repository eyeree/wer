# Phase 7 - Browser Runtime and Web Renderer: Implementation Plan

This is the lower-level plan required by section 21 of
[`implementation-plan.md`](implementation-plan.md) before Phase 7 work begins
(it covers the ground of `browser-runtime-plan.md`, the browser slice of
`renderer-plan.md`, and the browser portability commitments of section 19).
It expands the Phase 7 scope in section 20 - Wasm runtime integration, Web
Worker scheduling, browser persistence, browser asset streaming, WebGPU
feature tiers, browser-specific memory budgets, startup benchmarking, and
reduced compatibility profiles - into concrete milestones that can be
implemented and tested independently.

Read [`AGENTS.md`](../../../AGENTS.md) first. This plan does not repeat the
crate-boundary rule, the determinism invariant, or the CI contract - it assumes
them and calls out where Phase 7 stresses each. One sentence of orientation up
front, because it governs everything below: **Phase 7 delivers the existing
world model through a static browser application without changing generated
world output.** `WORLD_ALGORITHM_VERSION`, layer `algorithm_revision`s, and
golden fixtures change only if a separate, intentional generation change
requires it; this phase should not require one.

The native 3D renderer is still ongoing and is specified separately in
[`3d-design.md`](3d-design.md) and its phase plans. Phase 7 must not block on
the full native 3D stack landing. It starts by bringing the existing top-down
debug-map experience to the browser, then shares renderer interfaces with the
native 3D work as those interfaces stabilize.

---

## 1. Goals and non-goals

### 1.1 The question Phase 7 must answer

Phases 1-6 built and optimized the native world model while keeping
`world-core`, `world-runtime`, renderer shaders, storage, and task execution
portable enough for a later browser target. The repository already has a
minimal `platform-web` wasm parity shell, but it is not a playable browser
runtime: it does not stream regions, drive the production scheduler, render the
map, persist browser state, expose the debug controls, or package a deployable
static site. Phase 7 asks:

> Can the browser version preserve the same world model and core exploration
> experience while scaling simulation density, cache size, renderer path, and
> worker strategy to browser capabilities?

The answer must be usable, not just compilable: a user should be able to open a
static site, pan/travel through the world, steer possibility, inspect region
state in HTML, read the world-model documentation, read help for controls, and
continue after refresh when browser persistence is enabled.

### 1.2 Success criterion

> The browser version preserves the same world model and core experience while
> scaling simulation density and cache sizes to device capabilities.

Decomposed into testable properties:

- **Static deployable app:** the browser build emits ordinary static files
  (`index.html`, JS, wasm, CSS, generated docs, help, assets) that run under
  any static HTTP host, including GitHub Pages. There is no server component,
  dynamic build step, filesystem dependency, socket, or native service at
  runtime.
- **Local runnable app:** the same output can run locally through a simple
  static file server. Development may use a watch server, but the production
  artifact remains static.
- **Same world model:** browser parity probes still match native for every
  exported deterministic surface, and the browser app's region/layer hashes
  match the native inspector for fixed scripted positions after settling.
- **Responsive interaction:** the main thread remains responsive while regions
  generate, docs render, and WebGPU uploads proceed. Worker and inline
  execution paths produce the same settled hashes.
- **Tiered degradation:** WebGPU, worker count, shared memory, storage
  availability, and memory budget are detected at startup and mapped to
  explicit browser resource tiers. Lower tiers reduce radius, density,
  refinement, and cache ceilings; they do not change identity or persistence
  semantics.
- **Documented UI:** the web app includes a generated documentation page from
  [`docs/world-model.md`](../../world-model.md), plus a help page documenting
  keyboard, mouse, button controls, feature tiers, storage behavior, and known
  browser requirements.

### 1.3 Goals

- **A static web application shell** under `crates/platform-web/web` with
  stable routes for the viewer, world-model documentation, and help. The app is
  authored as browser-native TypeScript/JavaScript plus wasm-bindgen output,
  with no backend assumptions.
- **A reproducible web build pipeline** that builds wasm, copies static assets,
  converts `docs/world-model.md` into HTML, emits a manifest for cache busting,
  and produces one deployable directory. The conversion should be build-time by
  default so the deployed docs page needs no markdown parser on the hot path.
- **Browser renderer bring-up** for the existing top-down map UI first. The
  initial deliverable may use the CPU composer uploaded to a canvas/WebGPU
  texture; the WebGPU atlas/refinement path from Phase 6 follows once the
  renderer abstraction is shared cleanly. 3D POV integration is a later Phase 7
  milestone that consumes the still-ongoing 3D renderer API rather than
  reimplementing it.
- **HTML UI panels and controls.** The native right-side info panel becomes
  browser HTML, not pixels inside the map. Configuration must be available
  through buttons, toggles, segmented controls, sliders, and selects in
  addition to keyboard shortcuts. The panel updates from structured state
  snapshots, not from per-frame DOM rewriting.
- **Browser task execution** behind the existing `TaskExecutor` boundary:
  start with deterministic inline execution, then add a Web Worker executor
  with cancellation/supersession, bounded queues, and a no-`SharedArrayBuffer`
  fallback. Shared memory is an optimization for cross-origin isolated hosts,
  not a requirement for correctness.
- **Browser storage** behind the existing `Storage` trait semantics:
  IndexedDB first, OPFS only if it materially improves large blob handling.
  Storage is sparse record persistence, not generated geometry.
- **Browser resource tiers and startup benchmarking** that choose cache
  ceilings, worker counts, map radius, organism density, upload budgets, and
  refinement defaults conservatively.
- **Suspension and recovery** for page visibility changes, tab discard, reload,
  failed WebGPU device creation/loss, interrupted storage writes, and worker
  shutdown.
- **CI coverage** for static asset generation, wasm parity, docs generation,
  no-warning wasm checks, headless browser smoke tests, and deterministic
  browser settle scripts.

### 1.4 Non-goals

- **Changing generation output.** Phase 7 is a platform/runtime phase. Browser
  constraints may expose bugs, but fixes should preserve output unless a
  separately documented algorithm change intentionally bumps the relevant
  version boundary.
- **Replacing the native shell.** Native remains the fastest debug and
  profiling environment. Browser UI should share core surfaces where practical,
  but the native app does not need to become HTML.
- **Shipping the final 3D game renderer on day one.** 3D work is tracked by
  [`3d-design.md`](3d-design.md). Phase 7 should define the browser integration
  seam and eventually host POV mode, but the independently testable first
  browser renderer is the map.
- **Browser networking, accounts, hosted worlds, multiplayer, or community
  services.** Atlas bundles may be downloaded/uploaded as files, but no server
  enters the architecture.
- **Service-worker offline mode as a correctness requirement.** A service
  worker may be added after the static build is stable, but the app must work
  without one. Cached generated world data is explicitly non-authoritative.
- **GPU-authoritative simulation.** WebGPU output remains presentation-only
  unless a future ADR proves a portable readback/compute contract. No browser
  rendering path writes back into world state, persistence, identity, or tests.

---

## 2. Architecture overview

Phase 7 adds a browser platform shell, not a new world model:

```text
Static site
  index.html
  assets/app.js + app.css
  generated/platform_web.js + platform_web_bg.wasm
  docs/world-model.html
  help/index.html
        |
        v
Browser app shell (TypeScript/JS)
  routing, canvas, DOM panels, buttons, help/docs chrome
  input normalization, resize, visibility, device/tier detection
        |
        v
platform-web wasm facade
  AppHandle, frame/update APIs, snapshots, command queue
  optional WebGPU canvas hookup, panic/log bridge
        |
        v
world-runtime / renderer / world-core
  RegionMap, TaskExecutor, Storage, atlas/map renderer surfaces
```

The browser shell owns browser APIs: DOM, Canvas, WebGPU adapter/device setup,
Workers, IndexedDB/OPFS, URL routing, file import/export, and page lifecycle.
Neutral crates still do not touch those APIs directly. The wasm facade should
export coarse application operations rather than many tiny getters; otherwise
the JS/wasm boundary becomes a frame-time cost.

### 2.1 Static site layout

The build emits a directory such as `target/web-dist/`:

```text
target/web-dist/
  index.html
  help/
    index.html
  docs/
    world-model.html
  assets/
    app.js
    app.css
    manifest.json
  generated/
    platform_web.js
    platform_web_bg.wasm
```

GitHub Pages can publish that directory directly or publish an equivalent
checked-out artifact from CI. The site must not rely on path-root deployment:
all asset URLs should work when hosted under a repository subpath. The app
should use ordinary links (`./`, `./help/`, `./docs/world-model.html`) rather
than requiring a client-side router for static hosting.

### 2.2 Documentation generation

The world-model page is generated from
[`docs/world-model.md`](../../world-model.md) during the web build. Build-time
conversion is preferred over runtime conversion because it:

- keeps the viewer startup path small;
- avoids shipping a markdown parser to every user;
- lets CI fail when documentation conversion breaks;
- allows generated anchors/table-of-contents metadata to be checked.

Recommended implementation: add a small Rust build tool in `crates/tools` or an
`xtask`-style binary that uses a centralized markdown dependency such as
`pulldown-cmark`, wraps the result in the site's documentation template, and
copies it to `target/web-dist/docs/world-model.html`. The converter should
preserve headings, tables, code blocks, and math source text. Math rendering can
be deferred or handled with progressive enhancement; the raw formulas must
remain readable without network access.

CI should compare the generated output against a fresh conversion, or at least
run the converter and fail on warnings/errors, so edits to `docs/world-model.md`
cannot silently rot the web documentation page.

### 2.3 Help page

The help page is a hand-authored static page generated or copied by the same
build. It documents:

- keyboard controls, grouped by map mode and POV mode when POV is available;
- mouse controls, wheel behavior, click/drag behavior, and canvas focus;
- button equivalents for every viewer configuration shortcut;
- what each map channel/overlay/refinement toggle means;
- storage behavior, reset/export/import controls, and privacy implications;
- browser requirements for WebGPU, workers, shared memory, and reduced modes;
- known unsupported features such as networking and unfinished 3D phases.

The help content should be versioned with the controls. A CI smoke test should
assert that every registered keyboard command has a help entry and, where
appropriate, a visible button/control.

---

## 3. Browser UI model

### 3.1 Viewer layout

The first screen is the usable viewer, not a landing page:

```text
+---------------------------------------------------+----------------------+
| toolbar: mode/channel/tier/storage/help/docs       |                      |
+---------------------------------------------------+ HTML info panel      |
|                                                   |                      |
| canvas: map first, POV later                      | controls + telemetry |
|                                                   | region/cursor info   |
|                                                   | anchors/routes       |
+---------------------------------------------------+----------------------+
| status bar: loading, worker backlog, storage, fps, warnings              |
+-------------------------------------------------------------------------+
```

On narrow screens the info panel becomes a collapsible drawer. The canvas keeps
stable dimensions during panel open/close and device-pixel-ratio changes.

### 3.2 HTML info panel

The native map currently paints its right-side information panel as part of the
debug UI. In the browser, this becomes structured HTML fed by a wasm snapshot:

```rust
pub struct WebInfoSnapshot {
    pub frame_index: u64,
    pub world_pos: [f64; 2],
    pub region: [i32; 2],
    pub possibility: [f32; 8],
    pub target: [f32; 8],
    pub active_channel: u8,
    pub cursor_cell: Option<WebCellInfo>,
    pub cache: WebCacheStats,
    pub executor: WebExecutorStats,
    pub storage: WebStorageStats,
    pub warnings: WebWarnings,
}
```

The exact type can evolve, but the contract should be:

- wasm produces one compact snapshot when state changes or at a capped cadence;
- JS diffs the snapshot against the previous snapshot and updates only changed
  DOM nodes;
- high-frequency values such as FPS can update at 4-10 Hz, not every frame;
- cursor/cell readouts update on pointer movement and settled-tile changes,
  not on unrelated render frames;
- the panel never blocks rendering or generation.

Use stable DOM nodes with `data-field` bindings, not `innerHTML` replacement of
the whole panel. This keeps layout thrash low and makes Playwright assertions
straightforward.

### 3.3 Button controls

Every configuration shortcut exposed in the browser should have a visible
control:

- map channel selection as tabs or a segmented control;
- compose/refinement toggles as icon/text toggle buttons;
- resource tier as a select or segmented control;
- worker mode as a select: auto, inline, workers, workers + shared memory when
  available;
- storage controls: enable/disable, save now, export bundle, import bundle,
  reset local vault;
- route/preserve/anchor debug controls once those features are exposed;
- help/docs links as ordinary links, not hidden keyboard affordances.

Keyboard shortcuts remain for speed, but buttons are the discoverable and
testable control surface. Shortcut state and button state must share one
command registry so they cannot drift.

### 3.4 Input and focus

The canvas receives pointer and keyboard input only when focused or actively
captured. Form controls in the panel must not leak keystrokes into movement or
steering. The command registry should normalize physical keys, pointer deltas,
wheel events, touchpad scroll, resize, and visibility changes before they reach
wasm.

---

## 4. Renderer strategy

### 4.1 Milestone order

The browser renderer lands in layers:

1. **Canvas boot and resize:** create the page layout, WebGPU availability
   detection, fallback error UI, device-pixel-ratio handling, and a blank
   canvas clear.
2. **CPU-composed map upload:** reuse the existing CPU map composition path or
   a neutral equivalent and upload the resulting pixels to the browser canvas.
   This proves world streaming, input, and UI before GPU atlas complexity.
3. **WebGPU map atlas:** port/share Phase 6 `GpuMap` atlas uploads and WGSL
   composition in the browser. Delta uploads stay dependency-hash keyed.
4. **Refinement and feature tiers:** enable/disable WGSL refinement by tier and
   expose controls for compose/refinement toggles.
5. **POV/3D integration:** once the native 3D renderer API from
   [`3d-design.md`](3d-design.md) is stable, add browser canvas hosting for
   the same renderer surfaces. POV mode must remain derived presentation only.

Each layer is shippable and testable without the later layers.

### 4.2 Renderer ownership

`crates/renderer` should remain WebGPU/wgpu portable. The platform shell
creates or receives browser-specific canvas/surface handles and passes them
into renderer initialization; the renderer owns pipelines, buffers, textures,
and draw calls. World-aware packing stays in the platform shell or a neutral
packing helper, following the existing `AtlasManager`/`GpuMap` split.

If direct `wgpu` canvas setup from wasm is not yet ergonomic for all paths,
the first CPU-composed milestone may upload through browser 2D canvas APIs as a
temporary platform-web surface. That path must be explicitly marked as a
bootstrap fallback and should not become the only renderer.

### 4.3 Device loss and fallback

Browser WebGPU can fail at adapter request, device request, shader/pipeline
creation, or device loss. The app should:

- show a clear reduced-mode message when WebGPU is unavailable;
- keep deterministic world parity tests available even without WebGPU;
- fall back to CPU-composed canvas where practical;
- recreate GPU resources after device loss without changing world state;
- record renderer tier and failure reason in the info panel.

---

## 5. Wasm runtime facade

### 5.1 Export shape

The `platform-web` crate should grow from parity exports into an app facade:

```rust
#[wasm_bindgen]
pub struct WebApp { /* opaque */ }

#[wasm_bindgen]
impl WebApp {
    #[wasm_bindgen(constructor)]
    pub fn new(config: JsValue) -> Result<WebApp, JsValue>;

    pub fn update(&mut self, dt_ms: f64, input: JsValue) -> Result<JsValue, JsValue>;
    pub fn render_cpu_map(&mut self) -> Result<JsValue, JsValue>;
    pub fn apply_command(&mut self, command: JsValue) -> Result<JsValue, JsValue>;
    pub fn info_snapshot(&self) -> Result<JsValue, JsValue>;
    pub fn shutdown(&mut self);
}
```

This is illustrative, not a locked API. The important rule is that JS sends
batched input/commands and receives batched render or info snapshots. Avoid
per-cell or per-field JS calls in the frame loop.

### 5.2 Serialization at the JS boundary

Use compact typed arrays and explicit structs for hot paths. `serde-wasm-bindgen`
or `JsValue` objects are acceptable for low-frequency configuration, help
metadata, and snapshots. Pixel buffers, tile uploads, and worker messages
should use transferable `ArrayBuffer`s where possible.

### 5.3 Logging, panic, and diagnostics

`console_error_panic_hook` stays enabled in debug and release web builds. Rust
`log` output should bridge to `console.debug/info/warn/error`, with optional
in-app diagnostic capture for the status panel. Panic or worker failure should
surface as a visible recoverable error instead of a silent blank canvas.

---

## 6. Web Worker scheduling

### 6.1 Execution modes

Implement browser execution in three independently testable modes:

1. **Inline:** all work runs on the main wasm instance. Slow but simplest,
   deterministic, and required for debugging and browsers with worker/module
   limitations.
2. **Worker pool without shared memory:** jobs are serialized to workers and
   results are transferred back. This works on ordinary static hosts without
   cross-origin isolation.
3. **Worker pool with shared memory:** enabled only when the page is
   cross-origin isolated and the browser supports `SharedArrayBuffer`.
   This reduces copies but must produce identical settled state.

Mode selection is automatic but user-overridable from the UI and URL query
parameters.

### 6.2 Job contract

Browser jobs should reuse the production scheduler concepts from Phase 6:
priority lanes, cancellation tokens, supersession, bounded queues, and
amortized integration. A job message should include:

- stable region/layer identifiers;
- input dependency hashes/revisions;
- priority lane and estimated cost;
- cancellation generation;
- serialized inputs or shared-buffer ranges;
- requested output kind.

The main thread integrates results only if their dependency key is still
current. Cancelled or superseded worker results are discarded without side
effects.

### 6.3 Worker determinism tests

The browser harness should run fixed scripts in inline mode, worker mode, and
worker+shared-memory mode when available, then compare settled state hashes,
layer dependency hashes, and persistence records. Worker count and budget scale
must not affect final settled output.

---

## 7. Browser storage

### 7.1 IndexedDB storage backend

Implement a `platform-web` storage adapter matching the existing `Storage`
trait semantics over IndexedDB:

- key/value bytes;
- versioned database name and object store;
- atomic-enough per-record writes using IndexedDB transactions;
- explicit flush/settle operation for tests;
- graceful handling of quota, blocked upgrades, private mode, and user-denied
  persistence;
- no generated geometry, tiles, or organisms stored.

The vault record codec remains the canonical persistence boundary. Browser
storage stores the same bytes native stores, unless a future migration
explicitly changes the record format for every platform.

### 7.2 Import/export

The static app should support file-based atlas import/export:

- export current vault or selected atlas bundle as a downloadable file;
- import a bundle through a file picker or drag-and-drop;
- validate before merge and report record counts/errors;
- preserve CRDT merge laws from Phase 5.

No network service is introduced.

### 7.3 Suspension and recovery

On `visibilitychange`, page freeze, or before unload:

- stop scheduling low-priority generation;
- request cancellation of stale worker jobs;
- flush pending vault writes when possible;
- persist a compact session snapshot;
- resume by reloading sparse records and regenerating deterministic state.

Reload recovery is part of Phase 7 sign-off: a scripted browser run saves,
reloads, settles, and compares against uninterrupted execution.

---

## 8. Resource tiers

Browser tier selection combines:

- WebGPU availability and adapter limits;
- worker support and `navigator.hardwareConcurrency`;
- shared-memory availability;
- rough memory budget from configuration and conservative defaults;
- startup benchmark results for generation and upload throughput;
- user overrides through URL parameters and controls.

Suggested tiers:

| Tier | Intended browser/device | Defaults |
|---|---|---|
| `WebLow` | no shared memory, weak GPU, low memory | CPU map fallback or small atlas, inline/1 worker, small radii, Low runtime tier |
| `WebMid` | ordinary desktop browser | WebGPU atlas, 2-4 workers, moderate cache, Mid runtime tier |
| `WebHigh` | strong desktop with stable WebGPU | refinement on, larger atlas/cache, more workers, High runtime tier |

Tier changes are runtime configuration changes only. They may change how much
work is visible or cached, never which identities or records exist.

---

## 9. Milestones

Each milestone should be small enough to review and test independently.

### 9.1 Phase 7-1 - Static site scaffold and build

Deliverables:

- `crates/platform-web/web` reorganized into a real static app structure;
- reproducible build command that emits `target/web-dist`;
- wasm-bindgen build integrated into that command;
- asset copying with cache-busted names or a generated manifest;
- local static-server instructions in `README.md`;
- GitHub Pages-compatible paths.

Tests:

- CI builds the static directory from a clean checkout;
- a headless browser loads `index.html` from a local server and sees the app
  chrome plus the existing origin-feature-hash parity result;
- no network requests are made outside the static directory.

### 9.2 Phase 7-2 - Generated docs and help pages

Deliverables:

- build-time markdown conversion of `docs/world-model.md` to
  `docs/world-model.html`;
- static help page with current browser controls and feature descriptions;
- shared site header/nav linking viewer, docs, and help;
- CI check that docs generation succeeds.

Tests:

- generated docs contain expected headings from `docs/world-model.md`;
- help page contains every command registered in the browser command registry;
- Playwright smoke opens viewer, docs, help, then returns to viewer without
  losing app state.

### 9.3 Phase 7-3 - Browser app facade and inline world loop

Deliverables:

- `WebApp` wasm facade with initialization, update, commands, snapshots, and
  shutdown;
- inline `TaskExecutor` mode;
- browser resource-tier detection stub;
- structured info snapshot surfaced in HTML;
- keyboard and button command registry.

Tests:

- wasm parity suite remains green;
- browser settle script at fixed positions matches native inspector hashes;
- button controls and keyboard shortcuts dispatch the same commands;
- info panel updates only on snapshot changes or capped telemetry cadence.

### 9.4 Phase 7-4 - CPU-composed map renderer in browser

Deliverables:

- top-down map canvas rendering using CPU-composed pixels from the existing
  presentation logic or a neutral helper;
- pan/travel, zoom/view controls where applicable, channel selection, and
  cursor inspection;
- HTML info panel replacing the native painted panel for browser use;
- responsive layout and collapsible info panel.

Tests:

- screenshot/pixel smoke confirms nonblank map output on desktop and narrow
  viewports;
- fixed world/camera/channel renders match a checked browser image hash within
  an explicitly documented presentation tolerance;
- no DOM overlap or text overflow in the toolbar/panel at target widths.

### 9.5 Phase 7-5 - WebGPU map atlas and refinement

Deliverables:

- browser-hosted `wgpu`/WebGPU renderer path for the Phase 6 map atlas;
- dependency-hash-keyed delta uploads;
- compose/refinement toggles exposed as buttons and shortcuts;
- CPU map fallback retained for unsupported devices or debugging;
- device-loss handling.

Tests:

- GPU and CPU map paths show the same authoritative region/tile data;
- delta-upload counters are zero in steady state;
- WebGPU unavailable/device-lost tests show fallback UI rather than a blank
  page;
- WGSL validation remains in native tests and browser smoke.

### 9.6 Phase 7-6 - Web Worker executor

Deliverables:

- worker bundle and message protocol;
- inline, worker, and shared-memory-when-available execution modes;
- cancellation and supersession;
- bounded worker queues and backlog telemetry;
- UI override and URL parameters for worker mode.

Tests:

- fixed scripts settle to identical hashes in every available execution mode;
- cancellation storm test drains queues and does not integrate stale results;
- main-thread responsiveness smoke stays within the chosen interaction budget
  during generation pressure.

### 9.7 Phase 7-7 - Browser vault storage and recovery

Deliverables:

- IndexedDB-backed storage adapter;
- save/load/session snapshot integration;
- export/import atlas bundle controls;
- storage status and quota/error reporting in the info panel;
- visibility/reload recovery.

Tests:

- save -> reload -> settle equals uninterrupted browser run;
- browser-imported atlas bundle matches native `wer-atlas` validation;
- quota/private-mode failures are reported and leave runtime state consistent;
- IndexedDB bytes decode with the same record codec native uses.

### 9.8 Phase 7-8 - Browser tier tuning and startup benchmarks

Deliverables:

- startup benchmark measuring generation, upload, and frame pacing;
- tier selection using adapter/device/worker/storage signals;
- cache ceilings and runtime budgets wired to `WebLow`, `WebMid`, `WebHigh`;
- UI for tier override and diagnostics.

Tests:

- tier changes never change settled world hashes for fixed scripts;
- low-tier settings stay within configured cache byte ceilings;
- benchmark failures fall back to conservative defaults.

### 9.9 Phase 7-9 - POV/3D browser integration

Deliverables:

- browser mode switch that hosts the shared 3D renderer once native 3D APIs are
  stable enough to reuse;
- pointer-lock/mouse-look browser controls with visible button equivalents for
  mode/config toggles;
- help page updated for POV controls;
- WebGPU feature gating for depth, buffers, and shader limits.

Tests:

- map mode remains unchanged when POV is unavailable;
- POV canvas renders nonblank terrain on supported devices;
- device-loss and unsupported-feature paths return to map mode cleanly;
- no generated output changes and no readback path exists.

### 9.10 Phase 7-10 - Deployment hardening

Deliverables:

- CI artifact for the static site;
- optional GitHub Pages deployment workflow;
- production build with release wasm, panic/log settings, and cache headers
  suitable for static hosting;
- browser compatibility matrix documented in help or README;
- final Phase 7 sign-off script.

Tests:

- deployed artifact runs from a repository subpath;
- Playwright smoke runs against the production artifact;
- full CI remains green: fmt, clippy with warnings denied, native tests,
  wasm32 checks, wasm-pack tests, static build, docs generation, browser smoke.

---

## 10. Determinism and versioning

Phase 7 must treat browser differences as runtime differences, not world-model
differences:

- integer identity surfaces remain the source of permanent identity;
- parity exports are extended when new browser-facing deterministic surfaces
  are added;
- worker mode, worker count, resource tier, storage backend, renderer path, and
  frame cadence must not change settled hashes;
- WebGPU output is derived presentation and is never read back into the world;
- browser persistence uses the existing record versioning and codec;
- no golden fixture is re-blessed merely because browser execution exposed a
  mismatch.

If a browser-only fallback cannot preserve the same output, it must be marked
as presentation-only or disabled for authoritative state.

---

## 11. Failure handling

The app should make failures visible and recoverable:

- wasm initialization failure: show diagnostic and build instructions hint;
- WebGPU unavailable: use CPU map fallback or show reduced-mode message;
- device loss: release renderer resources, keep world state, recreate when
  possible;
- worker crash: cancel outstanding jobs, fall back to inline or smaller pool;
- storage unavailable/quota exceeded: continue in memory and show status;
- docs/help generation failure: fail the build, not the deployed page;
- stale static assets: manifest/version mismatch prompts a reload;
- panic: show captured message and stop scheduling new work safely.

---

## 12. Profiling and telemetry

Browser telemetry should mirror native concepts where practical:

- frame time, update time, render time, upload time;
- generated regions/layers per frame and backlog by priority lane;
- worker queue depth, cancellation count, stale-result discard count;
- wasm memory size and explicit cache bytes;
- IndexedDB operation count, pending writes, failures;
- WebGPU adapter tier, texture size, buffer allocation, device loss count;
- DOM update cadence and info-panel diff count.

Telemetry is visible in the HTML info panel and exportable as a JSON diagnostic
snapshot. CI should gate deterministic counts/hashes, not wall-clock timing on
shared runners.

---

## 13. Documentation updates

Phase 7 should update:

- [`README.md`](../../../README.md): local web build/run instructions, static
  deployment notes, browser requirements;
- [`AGENTS.md`](../../../AGENTS.md): current Phase 7 status once work lands;
- [`docs/world-model.md`](../../world-model.md): only when the model changes,
  not for presentation-only browser UI;
- this plan's milestone checklist as milestones complete;
- help page controls whenever command registry changes.

The generated documentation page must remain a build artifact of
`docs/world-model.md`, not a forked copy.

---

## 14. Phase sign-off

Phase 7 is complete when:

- the static site builds from a clean checkout and runs locally;
- the same artifact can be deployed to GitHub Pages;
- the browser viewer streams and renders the map, exposes HTML info and
  controls, and includes docs/help pages;
- browser inline/worker execution, storage reload, and resource-tier scripts
  settle to the same hashes as native where expected;
- unsupported WebGPU/storage/worker cases degrade visibly and safely;
- full native and wasm CI remains green with warnings denied;
- no generated-output version boundary changed unless a separate intentional
  algorithm change documented and tested it.
