# Native/Web Viewer Alignment Refactoring Plan

**Status:** Proposed

This plan aligns the native and browser viewers around one shared viewer host,
one input/action contract, one map presentation implementation, and one
information model. It also adds a third presentation mode in which the map and
POV view are visible side by side.

Read [`AGENTS.md`](../../../AGENTS.md) first. This is a presentation and shell
architecture refactor. It is not a world-generation change: do not bump
`WORLD_ALGORITHM_VERSION`, any layer `algorithm_revision`, or
`RECORD_FORMAT_VERSION`, and do not re-bless determinism fixtures. GPU output
remains derived presentation under ADR 0017, and live rendering gains no
readback API.

The intended end state is:

> Native and web translate environment events through thin adapters, then run
> the same bindings, semantic actions, world/view controller, map presenter,
> POV host, picking code, and panel-data builder. Only window/DOM integration,
> storage/executor adapters, surface creation, and final panel rendering remain
> environment-specific.

---

## 1. Required outcomes

The implementation is complete only when all of the following are true.

1. Raw winit and DOM events are translated into the same platform-neutral
   input events. A shared mapper owns bindings, held state, repeat suppression,
   modifiers, wheel accumulation, focus loss, and the left-button POV-look
   gesture.
2. The mapper emits typed, high-level actions such as `DropAnchor`,
   `ClearAnchors`, `ZoomIn`, `ZoomOut`, `SetPresentation`, and `FocusView`.
   Neither platform adapter mutates viewer state directly.
3. Web buttons enqueue the same typed actions into the same ordered consumer
   as keyboard, mouse, and future controller input. There is no parallel button
   command reducer.
4. Native and web use the same bindings in Map and POV contexts. In both,
   POV look occurs only while the primary mouse button is held. Pointer capture
   may transport a drag, but pointer lock may not bypass this gate.
5. Both platforms use the same CPU map composer and the same GPU atlas
   preparation. Web shows the full native channel and overlay set, including
   realized organisms, routes, preserves, discovery dimming, rings, pinned
   flashes, and the player marker when the corresponding data/capability is
   available.
6. Map cells remain square at every window size and device-pixel ratio. The map
   stays inside its pane, the desktop page does not grow beyond the viewport,
   and every enabled region grid boundary remains at least one physical pixel
   visible when the view is reduced.
7. One shared semantic information model supplies both the native bitmap panel
   and browser DOM panel. The panel is visible in Map, POV, and Split modes on
   both platforms.
8. Moving the pointer over POV reports the nearest visible organism or rendered
   terrain under the pointer. Picking uses CPU camera/geometry data and never
   GPU readback.
9. The browser panel uses horizontal space: at desktop widths its sections are
   arranged in three columns in a full-width dock (or an equivalent measured
   layout), with a deliberate narrow-screen collapse. It is not a 320-pixel
   rail whose content grows the page vertically.
10. `PresentationMode` has `Map`, `Pov`, and `Split`. Split initially uses a
    fixed 50/50 view ratio. Clicking a pane focuses it and gives a visible focus
    indication; view-scoped keyboard and wheel input routes only to that pane.
11. There is one traveler position, one `RegionMap::update`, and one logical
    viewer tick per frame in every mode. Map and POV cannot acquire competing
    streaming centers.
12. The renderer can draw map and POV subviews in one surface frame, with one
    acquire, one submission sequence, and one present. Calling two existing
    whole-surface presentation functions sequentially is not an acceptable
    Split implementation.
13. Native and wasm CI remain warning-clean. Existing deterministic parity and
    phase harnesses remain green.

## 2. Scope boundaries

### 2.1 In scope

- A new cross-platform `viewer-host` crate shared by `platform-native` and
  `platform-web`.
- Typed input, action, layout, focus, controller, map, atlas, inspection, and
  panel-model modules.
- Thin winit and DOM raw-event adapters.
- Browser controls and help metadata derived from or validated against the
  shared action registry.
- Extraction of native map composition, map picking, atlas packing, and panel
  sampling into shared code.
- Removal of the reduced browser map/inspection/controller copies.
- A single browser animation-frame driver for Map, POV, and Split.
- DPR-aware browser canvas resizing and renderer resize handling.
- POV terrain and organism ray picking in `pov-host`.
- A renderer frame API that accepts explicit view rectangles.
- Full native panel display in POV, a horizontally efficient web panel, and
  side-by-side Map/POV presentation.
- Updating native dumps, browser sign-off, README/help, and control
  documentation for the aligned behavior.

### 2.2 Explicitly out of scope

- Any generation, layer dependency, identity, steering-math, or persistence
  format change.
- A new native graphics backend, a DOM-based native UI, or a shared widget
  toolkit. Panel *data* is shared; bitmap and DOM rendering remain distinct.
- GPU picking, depth readback, or any other live-renderer readback surface.
- Independent map and POV worlds or streaming centers in Split mode.
- A complete redesign of `Storage`, Web Workers, or IndexedDB. The viewer
  exposes typed actions/effects and consumes existing platform services; any
  unfinished Phase 7 backend remains a separate backend task.
- Shipping a controller adapter in this change. The normalized input contract
  must admit controller axes/buttons without another viewer reducer, but the
  first implementation covers keyboard, pointer, wheel, and buttons.
- Requiring an adjustable divider for the first Split milestone. Shared,
  clamped split-ratio state and a typed action are included from the start so
  layout has one stable contract, but the initial UI always supplies `0.5`.
  Wiring a web drag handle or native ratio control is a follow-up after fixed
  50/50 behavior is correct.
- Byte-identical GPU pixels across vendors. The shared CPU composer is the
  deterministic presentation reference; GPU parity is semantic and visual.

## 3. Current-state findings

The present code shares core generation and most POV geometry, but the two
viewer shells are separate applications above those seams.

| Area | Native today | Web today | Consequence |
|---|---|---|---|
| Raw input and bindings | `App::window_event`, held `KeyCode` state, `apply_movement`, `apply_pov_movement`, and the large `handle_press` match in `platform-native/src/main.rs` | `commands.js`, `MOVE_KEYS`, `POV_MOVE`, global DOM listeners, and two frame loops in `app.js` | Bindings and behavior drift; there is no common action boundary. |
| Button dispatch | Not applicable | Buttons serialize string commands to `WebAppState::apply_command`, which uses substring matching | Buttons bypass keyboard handling and fragile JSON text becomes an API. |
| POV look | Left-button drag | Pointer lock on click, with drag as a fallback | The primary user gesture is inconsistent. |
| Map composition | Complete `Channel`, `Overlays`, `MapDecor`, and `MapComposer` in native `viz.rs` | A reduced second `compose_map`/`paint_region` in `platform-web/src/lib.rs` | Web lacks organisms and several overlays/channels; fixes must be duplicated. |
| GPU map | Native `AtlasManager` and `renderer::render_map_gpu` | UI state says `webgpu-atlas`, but the map is always copied through a 2D CPU canvas | Reported capability and rendered behavior disagree. |
| Map sizing | Renderer uses `letterbox_viewport` | Fixed `960x540` backing canvas stretched to its CSS box | Aspect distortion, clipped/oversized layout, and unreliable picking. |
| Inspection | Native `sample_cursor` and `pick_organism` | Reduced `inspect_json` plus JS formatting | Data fields and organism behavior differ. |
| Panel model | Semantic structs are embedded in native `panel.rs` beside bitmap rendering | Hand-built Rust JSON plus JS formatting into a fixed 320-pixel rail | Data processing is not shared and the web panel is unnecessarily tall. |
| World/view controller | Native `World` + `App` | Reduced `WebAppState` | Movement, updates, state, telemetry, and action semantics are duplicated. |
| POV streaming | Native copies camera XY to player before the world update | Web moves `pov_camera` but continues updating around unchanged `world_pos` | The visible camera, panel/map position, and streamed authority can diverge. |
| Render frame | Each of `render_map`, `render_map_gpu`, and `render_pov` acquires and presents the whole surface | Map and POV use separate canvas/loop paths | A correct native Split frame cannot be assembled from current public calls. |

Additional complexity to remove while doing this work:

- Native panel construction is repeated by the live frame, map dump, and
  headless screenshot paths.
- Native CPU and GPU overlay preparation repeat sequencing logic.
- Channel names and control metadata appear independently in native Rust, web
  Rust, `commands.js`, HTML controls, help HTML, and module comments.
- Web manually constructs and parses JSON for actions, snapshots, stats, and
  inspection. Hot pixel data already uses bytes; low-rate structured data
  should use typed exact decoding instead of `contains` checks and format
  strings.
- Web allocates a scratch canvas and `ImageData` on every CPU-map draw and then
  scales the image twice.
- Web compares `KeyboardEvent.key` with uppercase letter strings, so ordinary
  unshifted lowercase key events can miss documented shortcuts. It also does
  not consistently suppress one-shot key repeats.
- Native does not clear held keys on `WindowEvent::Focused(false)`, while web
  has a separate blur workaround.
- The changed-while-pinned detector lives inside map composition, so it can
  pause while POV is the only visible view. It belongs to the once-per-frame
  logical presentation update.
- Some shell labels still refer to old phase numbers. Generated binding/help
  metadata should describe the current viewer instead of preserving stale
  titles.

### 3.1 Browser layout baseline

A read-only browser check of the current static build confirmed that this is
not only a palette/composer problem:

- At a 900 by 700 viewport, the map canvas CSS box was 580 by 436 while its
  backing store remained 960 by 540. The internally square map was therefore
  stretched vertically after letterboxing.
- At a 1280 by 577 viewport, the document height grew to 719 pixels and the map
  continued below the window.
- Below the current responsive breakpoint, the information rail moves below
  the viewer but remains a long single vertical stream.

These measurements become explicit browser regression assertions rather than
informal screenshot checks.

## 4. Governing decisions and invariants

### 4.1 Record the architecture before moving code

Add a new ADR (the next available number) and update `docs/adr/README.md`.
Do not edit ADR 0002 or ADR 0017. The new record should state:

1. Cross-platform viewer behavior lives in `viewer-host`; platform crates own
   raw environment adapters and services only.
2. Input is normalized first, mapped through one binding table, and reduced
   through one ordered semantic-action consumer.
3. Map and POV are two presentations of one traveler/world state. Split mode
   performs one logical update and one world update per frame.
4. The renderer records all visible panes before presenting a surface frame.
5. Inspection uses CPU-authoritative/presentation geometry. GPU output never
   becomes gameplay or inspection authority.

If implementation review judges the single-traveler Split rule independently
significant, it may be recorded in a second ADR; it must not be left as an
implicit behavior in shell code.

### 4.2 Crate boundary

Add `crates/viewer-host` (`viewer_host` in Rust), analogous to `pov-host`:

```text
platform-native -- winit, native services, bitmap panel -------+
                                                               |
                                                               v
                                                         viewer-host
                                                               |
platform-web ---- DOM/canvas, browser services, DOM panel -----+
                                                               |
                     +----------------+----------------+---------+
                     v                v                v
                 pov-host          renderer       world-runtime
                                                         |
                                                         v
                                                     world-core
```

`viewer-host` may depend on `world-core`, `world-runtime`, `pov-host`, and
renderer upload/value types. It must contain no `winit`, `web_sys`, DOM,
filesystem, socket, or native-thread API. It is a cross-platform
presentation/controller crate, not a new authority layer. `world-core` and
`world-runtime` must never depend back on it.

`renderer` must not depend on `viewer-host`: `pov-host` already depends on
`renderer`, so the reverse edge would create a crate cycle. Renderer-facing
frame/upload structs stay in `renderer`; `viewer-host` builds or wraps those
values, and the thin shell passes them through without recomputing projection
or world-presentation math.

Start with one crate. Split it only if measured build-time or dependency
coupling becomes a real problem; multiple speculative viewer crates would make
the migration harder without improving the authority boundary.

Suggested modules:

```text
viewer-host/src/
  lib.rs
  action.rs       typed actions, ids, scopes, descriptors
  input.rs        normalized events, bindings, held state, frame intent
  layout.rs       modes, focus, pixel rectangles, map fit, hit routing
  controller.rs   ordered reducer, traveler/camera sync, one viewer tick
  world.rs        common exploration state/update; no concrete platform API
  map.rs          channels, overlays, decor, CPU composer, projection
  atlas.rs        atlas slots, region packing, refinement parameters
  inspect.rs      terrain/ecology/organism inspection models and sampling
  panel.rs        semantic panel snapshot/sections, injected telemetry
```

### 4.3 One traveler and one tick

The shared controller owns a single authoritative exploration XY used by
streaming, travel-fueled convergence, the map center, records, and inspection.

- In POV or POV-focused Split movement, the camera moves first; its XY becomes
  the traveler XY before the single world update.
- In Map or map-focused Split movement, the traveler moves in map axes. In
  Split, apply the same XY delta to the POV camera so the panes remain aligned.
  Preserve yaw/pitch. Preserve fly height; in walk mode, ground the translated
  camera through the existing terrain-following rule.
- Entering POV or Split initializes the camera over the traveler if it has not
  been initialized. Re-entering does not reset orientation unnecessarily.
- After input is reduced, compute travel once, call `RegionMap::update` once,
  then build zero, one, or two presentation packets from that result.
- Map composition and POV chunk synchronization are presentation work and may
  be skipped when their view is hidden, but logical continuity tracking and
  the world update do not depend on which view is visible.

This deliberately rejects independent map and POV navigation. Supporting that
later would require a separate decision about streaming radius, convergence
fuel, capture location, and persistence authority.

### 4.4 Presentation-only picking

POV picking answers what the rendered CPU-side scene says is under the pointer.
It may inspect any displayed organism slot, but ADR 0024 still governs gameplay:
capture and resonance continue through the authoritative slot-0 runtime paths.
Picking never changes an identity or feeds generation.

The first water contract is: select the nearest organism or rendered terrain,
even when a translucent water pass visually overlays that terrain. A future
`Water` hit variant can be added without changing authority; it is not required
for this alignment.

## 5. Shared contracts

### 5.1 Input pipeline

Use this flow on both platforms:

```text
winit WindowEvent ------------------+
                                     |  platform adapter
DOM Keyboard/Pointer/Wheel events ---+-----------------> NormalizedInputEvent
                                                            |
future gamepad adapter -------------------------------------+
                                                            v
                                      shared InputMapper + binding registry
                                                            |
                                      +---------------------+----------------+
                                      |                                      |
                                      v                                      v
                              ordered ViewerAction queue              held InputFrame
                                      |                         (move/look/wheel/pointer)
web buttons -> dispatch typed action -+                                      |
                                      +---------------------+----------------+
                                                            v
                                                    ViewerController
                                                            |
                                                            v
                                                   ViewerEffect queue
                                            (quit/dump/import/export/etc.)
```

The platform adapters translate names and coordinate units only:

- Native maps winit physical key codes, pointer/button phases, physical pixel
  positions, wheel units, resize, and focus changes.
- Web uses `KeyboardEvent.code`, not locale/case-sensitive
  `KeyboardEvent.key`; Pointer Events; CSS-to-physical-pixel conversion; wheel
  delta mode; `blur`; `pointercancel`; and `ResizeObserver`.
- The web adapter may use `setPointerCapture` during a primary-button drag so
  the gesture survives leaving the canvas. It does not enable free-look merely
  because pointer lock exists.

The shared side owns:

- physical controls and modifiers;
- press/release/pulse phases;
- one-shot repeat suppression;
- held movement state and opposing-key cancellation;
- diagonal normalization and sprint intent;
- fractional wheel accumulation and notch thresholds;
- primary-button drag state and pointer positions per pane;
- clearing all held state on blur/focus loss/pointer cancellation;
- input context (`Map`, `Pov`, or `Split` plus focused pane);
- action ordering when multiple events arrive before one frame.

Leave an axis event in the normalized model so a future controller can send
left-stick navigation and right-stick look without inventing another reducer.

### 5.2 Semantic actions and canonical bindings

Use a typed enum rather than string fragments. Exact names can change during
implementation, but the surface should cover at least:

```rust
enum ViewerAction {
    SetPresentation(PresentationMode),
    TogglePrimaryView,
    FocusView(ViewKind),
    SetSplitRatio(f32),
    NudgePossibility { domain: PossibilityDomain, direction: NudgeDirection },
    ResetPossibilityBias,
    DropAnchor(AnchorKind),
    CaptureAnchor,
    CycleCaptureCategory,
    ToggleCapturePolarity,
    ClearAnchors,
    ToggleTransitionMode,
    SaveSession,
    LoadSession,
    RecordLastAnchor,
    SummonDiscoveries,
    TogglePreserve,
    TogglePathTracking,
    ToggleRouteRecording,
    ToggleRouteAttraction,
    ClearRoutes,
    CycleMapChannel,
    SetMapChannel(Channel),
    ToggleOverlay(MapOverlay),
    ZoomIn,
    ZoomOut,
    ToggleGpuCompose,
    ToggleRefinement,
    ToggleWalk,
    TogglePovShadowAo,
    TogglePovDetailNormals,
    TogglePovWater,
    SetPovRenderScale(f32),
    SetResourceTier(ResourceTier),
    RequestTierBenchmark,
    SetWorkerBackend(WorkerBackend),
    CancelSupersededJobs,
    SetMapBackend(MapBackend),
    SetStorageEnabled(bool),
    ResetLocalVault,
    RequestAtlasImport,
    RequestAtlasExport,
    RequestDebugDump,
    RequestExit,
}
```

Continuous movement and look are frame intent rather than thousands of queued
discrete actions. The controller receives normalized map axes or POV
forward/strafe/vertical axes, sprint state, accumulated look delta, and wheel
steps once per frame.

The default binding registry preserves native behavior and resolves conflicts
by context:

| Context | Input | Semantic result |
|---|---|---|
| Focused view | `WASD` / arrows | Held navigation axes for that view. |
| Map | `Shift` + movement | Sprint. |
| Map | `1`-`8` | Nudge the corresponding possibility domain up; `Shift` nudges down. |
| Map | `Z` | Reset possibility bias. |
| Map | `E` / `Q` | Drop Emphasize / Suppress manual anchor. |
| Map | `K` | Capture under the traveler. |
| Map | `T` / `Y` | Cycle capture category / toggle polarity. |
| Map | `R` / `C` | Toggle transition mode / clear anchors. |
| Map | `O` / `L` | Save / load session. |
| Map | `B` / `I` / `P` | Record discovery / summon discoveries / toggle preserve. |
| Map | `H` / `J` / `U` / `Delete` | Path tracking / route recording / route attraction / clear routes. |
| Map | `V` | Cycle map channel. |
| Map | `F` / `G` / `N` / `X` / `M` | Toggle discovered / grid / rings / pinned flash / organisms. |
| Map | `,` / `.` | Toggle GPU compose / refinement. |
| Map | Wheel | Emit `ZoomIn` or `ZoomOut` after shared notch accumulation. |
| POV | `WASD` / arrows | Forward/back/strafe; `Space` / left `Shift` move vertically in fly mode. |
| POV | Primary-button drag | Emit look delta only for the duration of the hold. |
| POV | Wheel | Adjust active walk/fly speed through shared notches. |
| POV | `F` / `B` / `N` / `V` | Walk/fly / shadow+AO / detail normals / water. |
| Any visible view | `F12` | Request the platform diagnostic dump/capture. |
| Any visible view | `Escape` | Request exit where the host supports it. |
| Single view | `Tab` while the view surface is focused | Toggle Map and POV. |
| Split | `Tab` while the view surface is focused | Move focus to the other pane without leaving Split. |
| Split | Primary click in a pane | Focus that pane; a POV primary press also starts the drag-look gesture. |

`Show Map`, `Show POV`, and `Show Split` are visible controls backed by explicit
`SetPresentation` actions. No new keyboard binding is required for Split in the
first milestone. Browser `Tab` remains normal accessibility navigation when a
toolbar/form control owns focus; viewer shortcuts are consumed only when a view
surface is focused or a pointer drag is captured.

Each action has one descriptor containing a stable id, label, scope, default
bindings, value kind, help text, and capability requirements. Native help,
browser controls, and generated help consume or validate against this registry.
Delete `commands.js` as an independent source of truth; a generated artifact is
acceptable, a second hand-edited registry is not.

The runtime/storage variants above cover retained web controls as typed,
capability-gated actions: tier selection/benchmarking, worker selection and
cancellation, CPU/GPU map-backend selection, storage enablement, vault reset,
and atlas import/export. Controls that exist only to exercise a scaffold (for
example synthetic device loss) become explicit test/diagnostic hooks or are
retired; they do not survive as production string commands. A platform may
hide an action whose capability is unavailable, but must not give the same id
a different meaning.

### 5.3 Single consumer and platform effects

`ViewerController::apply_action` is the only reducer for discrete viewer
actions. Buttons call the same dispatcher that the mapper uses. Exact typed
decoding is required at the wasm boundary; `str::contains` is forbidden for
actions and values. Continuous `InputFrame` intent and asynchronous service
responses enter only through `ViewerController::tick`, never through direct
adapter mutation.

Each tick has a tested order: first drain typed service responses in adapter
enqueue order (each carries a monotonic response sequence and its request id),
then drain discrete actions in input order, then sample and apply the frame's
held axes/look/wheel intent, then perform the one traveler/world update and
build presentation output. Thus a completed load is visible before newer user
input in the same frame, while button and key actions retain their relative
event order.

Actions that require environment capabilities return typed effects instead of
calling platform APIs from shared code, for example:

```rust
enum ViewerEffect {
    Exit,
    WriteDebugCapture(DebugCaptureRequest),
    PersistSession(SessionWriteRequest),
    LoadSession,
    OpenAtlasImport,
    DownloadAtlasBundle,
    ConfigureWorkerBackend(WorkerBackend),
    CancelSupersededJobs,
    ConfigureStorage { enabled: bool },
    ResetLocalVault,
    SelectMapBackend(MapBackend),
    RunTierBenchmark,
    ReportWarning(ViewerWarning),
}
```

Pointer capture is not a delayed semantic effect: if the web adapter uses it,
it calls `setPointerCapture(pointerId)` synchronously for the primary pointer
and releases that same id on up/cancel. It still forwards the corresponding
normalized phases through the shared mapper, which alone owns whether a drag
produces POV look.

The exact storage integration should reuse `world-runtime::Storage`/`Vault` and
existing platform adapters. If an asynchronous browser operation completes
later, its typed result returns through a response queue on a later tick. Do
not fake a successful shared state mutation merely because a toolbar button was
clicked.

### 5.4 View layout and focus

Separate what is visible from where keyboard input goes:

```rust
enum PresentationMode { Map, Pov, Split }
enum ViewKind { Map, Pov }

struct ViewLayout {
    mode: PresentationMode,
    focused: ViewKind,
    split_ratio: f32,
}
```

`layout.rs` accepts a physical-pixel content rectangle supplied by the platform
and returns non-overlapping pane rectangles, the square fitted map-content
rectangle, POV aspect, focus-border rectangles, and an optional divider hit
area. The same rectangles drive rendering, pointer routing, and inverse
projection. No platform independently reconstructs the transform.

Rules:

- Map mode shows one map pane and focuses Map.
- POV mode shows one POV pane and focuses POV.
- Split shows both; the initial ratio is exactly 0.5 and is clamped to a safe
  range if adjustability is later enabled.
- The map is aspect-fitted inside its pane; unused space is letterbox, not
  stretched world.
- POV uses its pane aspect for projection and picking.
- Clicking a pane focuses it before processing view-scoped input.
- Global actions remain global; movement, wheel, look, and conflicting
  diagnostic keys are view-scoped.
- If POV is unsupported or its device is lost, `Pov` and `Split` reduce to
  `Map`, focus transfers to Map, the world state remains intact, and the panel
  reports the reason.

### 5.5 Shared map presentation

Move these native types and algorithms to `viewer-host` rather than porting
them again:

- `Channel`, including all native channels and stable id/name metadata;
- `Overlays` and individual overlay ids;
- `MapDecor`;
- `MapComposer`, zoom, player/route/preserve/ring/organism drawing, and
  pixel-to-world inversion;
- `AtlasManager`, `gpu_channel`, region packing, presentation keys, and
  refinement parameter construction.

The shared CPU composer is the headless/correctness reference used by native
screenshots and browser CPU fallback. Both shells call the same function over
the same `RegionMap`; there is no browser twin named `paint_region`.

Map output is described by a shared `MapRenderPacket` containing the source
raster or GPU atlas updates, zoom, projection, overlays, channel, and dirty
keys. Platform code chooses a surface and submits the packet but does not
recompute world-to-pixel math.

Specific parity fixes:

1. Render all currently published `RegionMap::organisms()` with the existing
   expressed-color and marker rules. Inspection may identify an organism at
   the same zoom threshold as native unless the shared design intentionally
   makes the threshold view-size based.
2. Share the complete channel list; do not keep web string indices.
3. Share discovery, routes, preserves, rings, pinned flash, organism, grid, and
   player overlay ordering.
4. Move pinned-revision tracking out of the visible map-composition call and
   update it once per viewer tick, including POV-only frames.
5. Add zoom to GPU map parameters so zoom does not silently switch native or
   web to a different feature path. Base fields and overlays must use the same
   transform.
6. Wire the browser WebGPU map to the existing shared renderer. Report
   `webgpu-atlas` only after it actually renders the map path.
7. Reuse backing buffers and browser image/canvas resources. Do not allocate a
   new scratch canvas and `ImageData` for every unchanged frame.

Grid visibility is a screen-space contract. The shared projection computes the
source-space thickness needed to survive reduction to the destination pane;
CPU composition widens boundary coverage accordingly. The WGSL path uses the
equivalent destination-pixel/derivative-aware threshold. Every visible
interior region boundary must occupy at least one physical pixel at every
supported zoom and DPR.

### 5.6 Browser sizing and layout

Use a single WebGPU stage canvas for Map, POV, and Split when WebGPU is active,
plus a separate 2D canvas used only for CPU Map fallback. Do not maintain one
canvas per mode with independent frame loops.

The web adapter must:

1. Observe the stage's CSS content box with `ResizeObserver`.
2. Set backing dimensions to rounded CSS pixels times current DPR, with a
   documented resource-tier cap if needed.
3. call the wasm/renderer resize path when either CSS size or DPR changes;
4. pass the exact physical content rectangle to shared layout code;
5. set `min-width: 0` and `min-height: 0` on grid/flex descendants so canvas
   intrinsic dimensions cannot enlarge the page;
6. avoid fixed HTML `width`/`height` attributes as long-lived layout authority;
7. preserve the map's fitted square and POV pane aspect during toolbar wraps,
   panel changes, and narrow-screen transitions.

Desktop web layout should use rows rather than the current narrow rail:

```text
+-----------------------------------------------------------------------+
| grouped toolbar / mode buttons / status                               |
+-----------------------------------------------------------------------+
| view deck: Map, POV, or Map | POV                                     |
| (fills remaining bounded viewport space)                              |
+-----------------------------------------------------------------------+
| information dock: section column 1 | column 2 | column 3               |
+-----------------------------------------------------------------------+
```

The information dock has a bounded height and its own overflow. On narrow
screens it collapses deliberately to one column or a user-opened drawer; page
scrolling there is acceptable if explicitly tested. On desktop, the body must
remain viewport-sized and only designated panels scroll.

Group the toolbar by View, Map, Exploration, Runtime, Storage, and POV rather
than keeping every control in one undifferentiated wrapping row. Advanced
runtime/storage diagnostics may live in a disclosure/menu so controls do not
consume the view deck's height.

### 5.7 Shared information model

Move semantic data types out of native `panel.rs` and replace the reduced web
snapshot with one model:

```rust
struct InfoPanelModel {
    frame: FrameInfo,
    view: ViewInfo,
    performance: PerformanceInfo,
    streaming: StreamingInfo,
    steering: SteeringInfo,
    persistence: PersistenceInfo,
    hover: HoverInfo,
    warnings: Vec<ViewerWarning>,
}

enum HoverInfo {
    None,
    Terrain(CellInfo),
    Organism(OrganismInfo),
}
```

`CellInfo` contains the existing world/region/cell coordinates, status,
stability/revision, elevation, temperature, moisture, hardness, river,
wetness, soil, fertility, vegetation, canopy, biome, and ecology/roster/trophic
facts. `OrganismInfo` contains id, slot, species, trophic role, position, form,
and expressed traits. `ViewInfo` includes mode, focused pane, map channel/zoom,
camera pose, walk/fly state and speeds, POV toggles, renderer capabilities, and
split ratio.

Platform-specific measurements such as present time, DOM-update count, surface
format, executor backend, and storage availability enter through a typed
`PlatformTelemetry` input. They are not read through platform APIs in
`viewer-host`.

A shared section builder supplies stable field ids, labels, values, severity,
and column/span hints. Native `Hud` rasterizes those sections with `font8x8`;
web binds them to stable accessible DOM nodes. Styling, scroll behavior, fonts,
and HTML semantics remain environment-specific. This preserves identical data
processing without trying to share widgets.

Low-frequency snapshots may use serde-based exact decoding at the wasm
boundary. Map pixels and GPU uploads remain byte/typed-array paths. Delete the
manual snapshot/stats/inspection JSON formatters once consumers migrate.

The model is produced in all presentation modes. Remove the native special
case that replaces the panel with only an FPS chip in POV; the small chip may
remain as an optional overlay, not as the only information surface.

### 5.8 POV ray picking

Implement picking in `pov-host`, which already owns the exact camera, resident
terrain lattice, and renderer-ready organism visuals.

1. Add `PovCamera::screen_ray` (or equivalent). Convert a pointer inside the
   POV pane to normalized device coordinates and unproject it using the same
   60-degree vertical FOV, handedness, near/far convention, and pane aspect as
   `view_proj`. Keep origin/direction in `f64` for far-world precision.
2. Add a resident-terrain ray query to `PovChunkManager`.
   - Broad-phase against resident chunk XY/height bounds.
   - Traverse the 64 by 64 height-field cells in ray order with 2D DDA or an
     equivalently bounded algorithm.
   - Intersect the same two triangles and `v00 -> v11` diagonal used by the
     renderer and `ground_surface`.
   - Exclude skirts and do not report an invisible analytic frontier as a
     rendered hit.
3. Retain `(id, slot)` beside each `PovOrganismVisual` in
   `PovOrganismManager` and expose closest-hit testing.
   - Transform a ray into the organism's yawed/scaled local space.
   - Intersect boxes as oriented boxes.
   - Use a scaled ellipsoid only as the broad phase for a sphere, then
     intersect the same canonical two-subdivision icosphere triangles that the
     renderer draws. Expose that pure topology from `renderer` for reuse, or
     move it to a lower-level dependency-free geometry module; do not maintain
     a second hand-copied mesh.
   - Use the same position, proportions, scale, yaw, and frame-time bob
     transform as the upload/draw path.
4. Compare positive organism and terrain distances and return the closest.
   Terrain occludes a body behind it; a nearer body wins. Discard hits beyond
   the current POV fog/draw distance so resident but invisible geometry cannot
   populate the panel.
5. Feed a terrain hit's XY through the shared cell sampler and an organism
   hit's identity through the shared organism-info builder.
6. Cache the result and recompute only when pointer position, POV camera,
   resident chunk generation, or organism visual generation changes.

No depth texture is read. This path remains valid in headless tests and obeys
ADR 0017.

### 5.9 Multi-view renderer frame

Refactor `renderer::Renderer` so resource updates and surface presentation are
separate. The target API can be named differently, but must express one frame:

```text
prepare map atlas/overlay uploads
prepare POV chunk/organism uploads
acquire surface once
create one command encoder
clear frame once
draw optional Map packet into map rectangle
draw optional POV packet into POV rectangle
draw native panel/HUD/focus decoration
submit once
present once
```

The safest POV route is a pane-sized offscreen color/depth target followed by a
blit into the destination pane. That prevents a POV clear from erasing Map,
makes reduced-resolution rendering naturally pane-relative, and gives the
camera the correct aspect. Generalize the existing upscale blit to a
destination rectangle. Shadow resources remain independent of pane color size.

Generalize `letterbox_viewport` to fit an image inside an arbitrary parent
`PixelRect`, not only an entire surface. GPU map composition accepts the map
rectangle; POV depth/scaled resources key on the POV rectangle; focus and panel
passes load the existing color result.

Keep `render_map`, `render_map_gpu`, and `render_pov` as temporary single-view
wrappers over the new frame path while callers migrate, then remove wrappers
that preserve duplicate acquire/present logic. The live renderer still exposes
no readback. ADR 0021's separate `PovCapture` remains the only file-bound debug
exception.

## 6. Step-by-step implementation plan

### Milestone 0 - Characterize behavior and accept the ADR

1. Add the ADR described in section 4.1 and its index entry.
2. Record fixed native CPU map fixtures for representative channels and every
   overlay, including an organism-bearing settled region.
3. Record a semantic input trace for Map and POV that covers held movement,
   diagonal normalization, a one-shot key, wheel accumulation, and left drag.
4. Record panel-model source values for terrain, ecology, and an organism from
   existing native sampling before moving types.
5. Extend the browser sign-off harness enough to record current viewport,
   canvas CSS/backing sizes, document scroll size, and renderer status at
   1280x720, 900x700, and 700x700.
6. Keep these as characterization evidence; do not turn nonportable GPU
   screenshots into golden fixtures.

Exit criteria:

- The ownership and single-traveler decisions are accepted.
- The extraction has byte/value/input traces that can detect accidental drift.
- Known intentional layout/grid changes are identified separately from
  unintended world or palette changes.

### Milestone 1 - Add `viewer-host` and shared value contracts

1. Add `crates/viewer-host` to the workspace with workspace lints and
   centralized dependencies.
2. Add `action`, `input`, `layout`, `map`, `atlas`, `inspect`, `panel`,
   `world`, and `controller` module skeletons.
3. Move or introduce value-only types first: `PresentationMode`, `ViewKind`,
   `PixelRect`, `Channel`, `Overlays`, inspection structs, panel field ids,
   platform telemetry, and typed effects.
4. Re-export moved types temporarily from native modules so callers can move
   in small commits.
5. Add compile checks proving `viewer-host` builds natively and for
   `wasm32-unknown-unknown` without winit, web-sys, filesystem, or socket use.

Exit criteria:

- Both shells depend on `viewer-host`.
- The crate graph follows section 4.2 with no reverse core dependency.
- Value-type tests and warning-denying clippy pass before behavior moves.

### Milestone 2 - Land normalized input and typed actions

1. Implement the normalized event types, binding descriptors, contexts,
   held-state machine, wheel accumulator, ordered action queue, and
   `InputFrame`.
2. Encode every binding in section 5.2 once. Add uniqueness checks for stable
   action ids and context collisions.
3. Add a native adapter module that converts winit events to normalized
   events. Make `ApplicationHandler::window_event` enqueue input rather than
   mutating viewer state. Handle `Focused(false)` and pointer cancellation.
4. Add a web adapter that forwards DOM event codes/phases/coordinates to wasm.
   Remove key-casing dependence and suppress repeats according to shared
   semantics.
5. Make toolbar controls dispatch exact typed action ids/payloads through the
   same queue. Remove the generic command listener plus special second export
   dispatch pattern.
6. Initially adapt the native and web existing state objects to consume typed
   actions so the input migration can land before the full controller move.
7. Generate or validate browser help and native key help from action
   descriptors.

Tests:

- Every Map/POV binding, modifier, and context collision.
- One-shot repeat suppression and held movement repetition.
- Press/release ordering, opposite directions, diagonal normalization, and
  focus-loss cleanup.
- Fractional line/pixel wheel deltas producing identical notches.
- Primary-button down/move/up and pointer-cancel; no look without the hold.
- Native and web adapter fixtures yielding the same normalized traces.
- Button and key paths yielding the same action and reducer result.
- Viewer shortcuts do not consume browser Tab while a toolbar/form control is
  focused.

Exit criteria:

- `handle_press`, `commands.js`, `MOVE_KEYS`, and `POV_MOVE` are no longer
  independent binding authorities.
- Web buttons and physical inputs have one consumer.
- The native and web left-drag gesture is identical.

### Milestone 3 - Extract the shared exploration/view controller

1. Split native `World`/`App` into shared exploration state and platform
   services. Move player/last-player, field, bias, anchors, transition state,
   view state, map preferences, POV camera/toggles, and the common update
   sequence into `ViewerController`/`ExplorationWorld`.
2. Keep concrete `FileStorage`, browser persistence, task-executor creation,
   window lifecycle, DOM, and dump/file writing in platform crates. Route them
   through typed services/effects.
3. Move map and POV continuous movement to one shared intent reducer.
4. Implement the single traveler rules from section 4.3 and compute travel
   once.
5. Replace web `WebAppState::update` and `pov_step` behavior with one controller
   tick. In particular, update streaming around the POV camera/traveler rather
   than the stale map position.
6. Replace the web map-movement and POV animation loops with one scheduler. It
   requests another frame while POV is visible, input is held, work is in
   flight, animation is active, or presentation is dirty; idle Map may sleep.
7. Absorb frame/update telemetry exactly once and expose typed frame outputs.

Tests:

- Equivalent action/input scripts produce the same controller presentation
  state on native and wasm hosts.
- Map and POV movement speeds, sprint, fly/walk, vertical movement, and dt
  clamping preserve current behavior.
- POV movement changes traveler, panel position, map center, and streaming
  center together.
- Exactly one world update and one travel value occur per logical frame.
- Persistence/import/export effects are ordered and do not claim success before
  the platform reports it.

Exit criteria:

- `WebAppState` is no longer a second reduced gameplay/viewer controller.
- Map and POV cannot stream around different positions.
- One frame loop drives all modes.

### Milestone 4 - Make the native map presenter canonical

1. Move `viz.rs` map types, composition, overlays, zoom, and inverse picking to
   `viewer-host::map` with their tests.
2. Move `gpumap.rs` atlas slot management, region packing, GPU channel mapping,
   and refinement parameters to `viewer-host::atlas`.
3. Factor shared overlay sequencing so CPU base composition and GPU overlay
   preparation cannot silently reorder or omit features.
4. Move pinned-violation tracking into the once-per-frame controller/presenter
   update.
5. Make native live map, native headless screenshot, native dump, and browser
   CPU fallback call the same composer.
6. Delete web `MAP_CHANNELS`, `compose_map`, and `paint_region` after consumers
   move.
7. Expose every native channel and overlay through shared descriptors; group
   browser controls rather than hand-copying option lists.
8. Add organisms and all available decor to web. Missing platform capability
   means an empty data source with a visible status, not a separate renderer.
9. Add GPU zoom and shared grid-thickness inputs. Keep field/overlay transforms
   identical.
10. Wire the web map through shared atlas preparation and renderer GPU map when
    WebGPU is actually available; retain the canonical CPU fallback.

Tests:

- Shared CPU bytes for every channel on fixed settled fixtures.
- Overlay-specific tests for organisms, routes, preserves, discovered dimming,
  rings, flashes, grid, and player marker, plus combined ordering.
- Organism expressed color/position and zoom-threshold picking.
- Atlas key/delta behavior and zero steady-state uploads.
- CPU/GPU representative cell/palette/overlay comparisons; refinement is
  excluded from exact pixels but remains zero-mean derived presentation.
- A wasm test invokes the same CPU composer rather than a browser twin.

Exit criteria:

- There is one CPU map implementation and one atlas packer.
- Native and web expose the same map feature set for the same data.
- Web renderer status describes the path that actually drew the frame.

### Milestone 5 - Fix viewport, DPR, zoom, and grid behavior

1. Implement rectangle-based layout/map fit and render/pick round trips in
   `viewer-host::layout`.
2. Add the browser `ResizeObserver`/DPR path and wasm renderer resize method.
3. Replace fixed canvas authority with backing dimensions derived from the CSS
   content rectangle and DPR.
4. Apply `min-width: 0`, `min-height: 0`, bounded overflow, and viewport-sized
   desktop grid rules.
5. Use the exact shared physical rectangles for draw and pointer inversion.
6. Implement the minimum-one-physical-pixel grid rule in CPU and WGSL paths.
7. Remove the per-frame scratch-canvas allocation; reuse image resources and
   redraw only when dirty or resized.

Tests:

- Property/table tests over landscape, portrait, tiny, odd pixel sizes, DPR
  1/1.25/1.5/2, zoom 1/2/4/8/16, and future split ratios.
- Every rectangle remains inside its parent and Map content aspect is 1.0.
- World -> pixel -> world round trips within the documented cell tolerance.
- Every enabled interior region boundary has at least one covered physical
  pixel.
- Browser assertions at 1280x720 and 900x700: body does not overflow, backing
  dimensions match CSS times DPR, and the map stays contained.
- A deliberate 700x700 narrow test verifies the specified collapse/drawer
  behavior rather than accidental overflow.

Exit criteria:

- No map stretching or map-bigger-than-window behavior remains.
- Picking and rendering share one transform.
- Grid lines remain visible when the full map is reduced.

### Milestone 6 - Share information processing and redesign the web panel

1. Move `CursorInfo`, `EcologyInfo`, `OrganismInfo`, `PanelInfo`, `VaultInfo`,
   cell sampling, organism conversion, and panel section construction into
   `viewer-host::inspect`/`panel`.
2. Expand the model to include view/focus/camera/POV state and injected
   platform telemetry.
3. Build the model once per state change or capped telemetry interval, not once
   per renderer.
4. Refactor native `Hud` into a renderer of the shared sections. Reuse the same
   model in live frames, screenshots, and dumps.
5. Replace web hand-built snapshot/stats/inspection JSON and JS data assembly
   with exact typed serialization of the shared model.
6. Replace the fixed 320-pixel web rail with the three-column desktop
   information dock described in section 5.6. Use stable accessible nodes and
   update only changed fields.
7. Keep the panel mounted in all modes. Report POV renderer/device loss and
   fallback in the same warning section.
8. Remove stale phase labels and hardcoded control help from the native bitmap
   renderer.

Tests:

- Model fixtures for ready/generating/unloaded terrain, full ecology, map
  organism, no hover, renderer fallback, and persistence warning.
- Native and web renderers expose the same stable field ids and values from one
  model.
- Telemetry cadence changes do not rebuild the whole DOM or map buffers.
- Desktop panel has three section columns; narrow layout collapses without
  clipped labels or horizontal page overflow.
- Panel remains present in Map, POV, and the later Split state.

Exit criteria:

- Panel data/sampling/formatting is shared; only final pixels/DOM differ.
- Native POV has a full panel model ready for the multi-view renderer.
- Web uses horizontal information layout and bounded scrolling.

### Milestone 7 - Add POV terrain and organism hover

1. Implement camera ray construction, terrain DDA/triangle intersection, and
   organism primitive intersection from section 5.8.
2. Retain organism identity alongside renderer-ready visuals without adding a
   second geometry mapping.
3. Add a generation/dirty key for cached hover invalidation.
4. Route pane-relative pointer movement through shared layout to POV picking.
5. Convert the nearest hit into the same `HoverInfo` used by Map.
6. Show POV terrain/organism information in both panel renderers.
7. Confirm primary drag still rotates only while held; hover may update before
   or after the drag according to one documented ordering.

Tests:

- Center ray equals camera forward; corner/edge rays match projection aspect.
- Flat, sloped, diagonal-split, cross-cell, cross-region, and far-origin
  terrain hits.
- Skirts and missing resident chunks do not create false visible hits.
- Yawed boxes, scaled canonical icospheres with ellipsoid broad-phase
  rejection, body-vs-terrain occlusion, nearest body, behind-camera,
  beyond-fog, and sky miss.
- A picked organism's id/slot and expressed panel data match the rendered
  visual source.
- No renderer readback method is added.

Exit criteria:

- Both viewers report organism or terrain under the POV pointer.
- The result agrees with CPU-side rendered geometry and respects occlusion.

### Milestone 8 - Refactor the renderer to one multi-view frame

1. Add explicit pane rectangles and separate resource preparation from surface
   acquisition/presentation.
2. Generalize map composition to an arbitrary map rectangle.
3. Render POV to pane-sized color/depth resources and generalize the scaled blit
   destination.
4. Record map, POV, panel/HUD, and focus passes into one command encoder/frame.
5. Ensure per-pane projection/aspect, viewport, scissor, depth, and clear
   behavior cannot affect the neighboring pane.
6. Retain temporary single-view wrappers, migrate native then web, and remove
   duplicate acquire/present bodies.
7. Preserve surface-loss recreation and browser device-loss reporting.

Tests:

- Device-free layout/pass-plan tests assert non-overlap and correct pass order.
- Instrumented renderer tests or counters prove one acquire/present per frame.
- Map-only and POV-only output paths retain their current data/upload behavior.
- Split-sized POV depth/scaled textures resize only when their pane changes.
- WGSL parse/validation tests remain green.
- Surface/device loss does not mutate world state and falls back to Map.

Exit criteria:

- One renderer call can present Map, POV, or both.
- There is no live readback and no two-present Split workaround.

### Milestone 9 - Ship Split mode and focus routing

1. Enable `PresentationMode::Split` in the shared reducer and both platform
   controls.
2. Use a fixed 50/50 layout first. Initialize POV resources lazily while Map
   remains visible.
3. Draw a clear focus border/title treatment in native and web.
4. On pointer press, focus the hit pane before routing pointer/view input.
5. Route keyboard movement, wheel, and context-colliding keys by focused pane;
   leave global actions global.
6. Apply the one-traveler synchronization rules for map-focused and POV-focused
   movement.
7. Build Map and POV packets from the same post-update world state and present
   them in one renderer frame.
8. On unsupported WebGPU/device loss, atomically change to Map focus and keep
   panel/world state alive.
9. After fixed behavior passes, optionally wire a web drag divider (and later
   a native control) to the already-shared `SetSplitRatio` action. Ratio state
   remains shared even though each pointer adapter is environment-specific.

Tests:

- Mode transition table among Map, POV, and Split, including unsupported POV.
- Clicking each pane changes focus and updates visual focus state.
- `Tab` toggles single views and swaps focus inside Split only when the view
  surface owns keyboard focus.
- Map-focused keys never change POV diagnostics; POV-focused wheel never zooms
  Map; global save/dump remains global.
- One world update, one travel computation, and one surface present per Split
  frame.
- POV movement recenters Map; Map movement translates POV camera consistently;
  walk/fly height rules are preserved.
- Map and POV hover are routed by their own rectangles.
- Resize and optional ratio clamping never overlap panes or place them outside
  the view deck.

Exit criteria:

- Both native and web support Map, POV, and side-by-side Split.
- Focus determines keyboard routing and is visible.
- The two panes remain views of one traveler/world update.

### Milestone 10 - Remove migration scaffolding and harden diagnostics/docs

1. Delete obsolete native `viz.rs`, `gpumap.rs`, and semantic panel types after
   all callers use `viewer-host`; remove the `pov.rs` re-export if it has no
   value.
2. Delete reduced web map/inspect/controller code, manual action parsing,
   independent frame loops, and hand-maintained command/channel registries.
3. Keep platform files small and explicit: raw adapter, services, surface,
   panel renderer, and application lifecycle.
4. Extend native F12 and `--pov-script`/headless support for aligned view state.
   A Split dump composes both views and the panel using the same layout and
   records mode, focus, pane rectangles, camera/traveler pose, and hover. Use
   ADR 0021's file-bound `PovCapture`; do not add live readback.
5. Extend `web-signoff`, static smoke, and browser automation for real input,
   resize, focus, panel, fallback, and Split behavior. Static string checks
   alone are insufficient.
6. Update README controls, web help, crate architecture tables, debug guidance,
   and any current design docs. Historical accepted ADRs and completed phase
   records remain immutable.
7. Measure Map, POV, and Split at Low/Mid/High tiers. Preserve delta-driven
   uploads, capped panel refresh, cached hover, and zero steady-state map/panel
   allocations where practical.

Exit criteria:

- Searches find one binding registry, one map composer, one atlas packer, one
  inspection builder, one controller tick, and no substring command parser.
- Debug captures and help describe all three modes and focus rules.
- Full verification in section 8 passes.

## 7. Expected file impact

| Path | Planned responsibility/change |
|---|---|
| `Cargo.toml` | Add `viewer-host` and any centralized typed-serialization dependency selected for low-rate wasm models. |
| `crates/viewer-host/**` | New shared action/input/layout/controller/world/map/atlas/inspect/panel implementation and tests. |
| `crates/platform-native/src/main.rs` | Shrink to native lifecycle, adapter, services, renderer invocation, and effect handling. |
| `crates/platform-native/src/viz.rs` | Move to `viewer-host::map`, then remove. |
| `crates/platform-native/src/gpumap.rs` | Move to `viewer-host::atlas`, then remove. |
| `crates/platform-native/src/panel.rs` | Retain bitmap `Hud` renderer only; consume shared panel sections. |
| `crates/platform-native/src/dump.rs` | Consume shared model/layout; capture Map/POV/Split without duplicating panel construction. |
| `crates/platform-native/src/pov.rs` | Remove the re-export if native can import `pov-host`/`viewer-host` directly. |
| `crates/platform-web/src/lib.rs` | Keep parity exports and wasm/platform facade; replace `WebAppState` copies with shared controller and typed bridges; add resize/frame APIs. |
| `crates/platform-web/web/assets/app.js` | Thin DOM adapter, canvas/DPR lifecycle, DOM panel renderer, one RAF scheduler, and platform effects. |
| `crates/platform-web/web/assets/commands.js` | Remove as hand-authored authority; generate from shared descriptors only if a static artifact is still useful. |
| `crates/platform-web/web/assets/app.css` | Bounded view deck, full-width three-column info dock, focus styling, narrow collapse, no canvas-driven overflow. |
| `crates/platform-web/web/index.html` | Grouped controls, Map/POV/Split controls, one GPU stage plus CPU fallback, semantic panel section hosts. |
| `crates/platform-web/web/help/index.html` | Generate or validate control/help rows from shared descriptors. |
| `crates/platform-web/web/smoke.mjs` | Validate generated binding/help consistency and static shell; stop treating source-string presence as functional sign-off. |
| `crates/platform-web/tests/wasm_parity.rs` | Keep deterministic probes; add shared composer/controller value tests where portable. |
| `crates/pov-host/src/lib.rs` | Screen rays, resident terrain raycast, organism identity/primitive hit tests, shared movement helpers if still duplicated. |
| `crates/renderer/src/lib.rs` | One multi-view frame API, arbitrary rectangles, one acquire/present, resize integration. |
| `crates/renderer/src/gpumap.rs` | Map zoom/grid/rectangle inputs and draw preparation separated from presentation. |
| `crates/renderer/src/pov.rs` | Pane-sized offscreen color/depth/scaled targets and destination-rect blit. |
| `crates/renderer/shaders/compose_map.wgsl` | Shared zoom transform and minimum-physical-pixel grid rule. |
| `crates/tools/src/bin/web-signoff.rs` | Functional browser layout/input/focus/fallback/Split gates in addition to static artifact checks. |
| `docs/adr/README.md` + new ADR | Record the shared viewer and one-world multi-view boundary. |
| `README.md` and current help/design docs | Document aligned controls, full panels, POV hover, Split focus, fallback, and debugging. |

## 8. Verification strategy

### 8.1 Focused test suites

Add or move focused suites so failures localize cleanly:

- `viewer-host` unit tests for bindings, action reduction, layout, map bytes,
  atlas keys, inspection, panel sections, and single-tick invariants.
- `pov-host` unit tests for camera rays and terrain/organism intersection.
- renderer tests for pass planning, pane resource sizing, and WGSL validation.
- native tests/headless scripts for adapter traces, complete panel rendering,
  Map/POV/Split dumps, and focus-independent world output.
- wasm tests for typed action decoding, shared CPU map invocation, model
  serialization, resize state, and controller traces.
- browser functional tests for real DOM focus, buttons, key codes, LMB drag,
  DPR/resize, panel columns, canvas containment, mode transitions, Split focus,
  and device-loss fallback.

### 8.2 Cross-platform trace gates

Use fixed semantic traces rather than comparing unsynchronized in-flight job
queues:

1. Feed equivalent native and web normalized input traces.
2. Assert identical emitted actions and held `InputFrame` values.
3. Run controller traces with an inline executor and fixed dt.
4. Compare traveler/camera/view/map preferences, action effects, panel-model
   values, and settled world hashes.
5. For lane/worker variants, settle before comparing world state in accordance
   with ADR 0018.

This catches shell drift without pretending arbitrary mid-flight scheduling
state is portable.

### 8.3 Browser sign-off matrix

At minimum, automate:

| Case | Required assertion |
|---|---|
| 1280x720, DPR 1 | No body overflow; bounded toolbar/view/panel; three info columns; square contained map. |
| 900x700, DPR 1 | No aspect distortion; controls remain usable; map stays inside view deck. |
| 700x700, DPR 1 | Intentional narrow collapse/drawer; no horizontal overflow; panel remains reachable. |
| DPR 2 desktop | Backing sizes equal CSS physical sizes within rounding/cap; picking round-trips. |
| Map zoom 1/4/16 | Center is stable, organisms align, enabled grid boundaries remain visible. |
| POV pointer | No look on hover/click-release; look only during primary hold; terrain/organism info updates. |
| Split | Both panes render; click focus routes keys/wheel; one world tick; visible focus border. |
| WebGPU unavailable/lost | Map CPU fallback and panel survive; POV/Split reduce to Map with a warning. |

The repository guidance applies: agent-browser's normal headless Chrome cannot
validate visible WebGPU canvas pixels. Use it for DOM geometry, input, fallback,
`window.__povStatus`, panel values, and diagnostics. Use Windows Chrome over CDP
for real GPU screenshots, and compare a matched pose with the native headless
capture when diagnosing rendering differences.

### 8.4 CI-equivalent gates

Run focused tests throughout, then the full repository gates before marking the
plan implemented:

```sh
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets
cargo check --workspace
cargo test --workspace
cargo check -p world-core -p world-runtime -p viewer-host -p platform-web --target wasm32-unknown-unknown
wasm-pack test --node crates/platform-web
cargo run --bin web-build
cargo run --bin web-signoff
```

Also run the phase sign-off harnesses because shared controller/input changes
can alter which existing world inputs are supplied even though generation math
is unchanged:

```sh
cargo run --bin wer-ledger
cargo run --bin wer-anchor
cargo run --bin wer-vault
cargo run --release --bin wer-scale
```

Run representative native capture/manual checks:

```sh
cargo run --bin wer
cargo run --bin wer -- --screenshot /tmp/aligned-map.ppm composite 0 0 1
cargo run --release --bin wer -- --pov-script "pos:0,0; mouse:-60,100; snap:/tmp/aligned-pov.ppm"
```

Add a deterministic Split capture command/script form during implementation and
include it here once the final CLI syntax is chosen.

## 9. Risks and mitigations

1. **Big-bang shell rewrite.** `App` and `WebAppState` currently own many
   concerns. Move value contracts, input, controller, map, panel, and renderer
   in milestones with temporary re-exports/wrappers; require each milestone to
   compile and test on native and wasm.
2. **Storage/executor scope creep.** Native and browser backends have different
   lifecycle constraints, especially synchronous `Storage` versus IndexedDB.
   Share actions, state transitions, and typed effects without redesigning the
   backend contracts in this UI refactor.
3. **Travel applied twice.** Split can accidentally update from both panes.
   Centralize traveler movement and assert one travel computation and one
   `RegionMap::update` per controller tick.
4. **Two rendering loops.** Keeping the current browser loops would double
   updates or present stale Map state. Replace them before enabling Split.
5. **POV clear erases Map.** Existing POV rendering assumes a whole surface.
   Use pane-sized offscreen targets or rigorously scoped load/viewport passes;
   test neighbor-pane preservation.
6. **Aspect/picking drift.** CSS, canvas, renderer, and JS currently know
   different sizes. Make shared physical rectangles the only render/pick
   authority and property-test round trips.
7. **Grid aliasing.** One source-cell line can disappear under reduction.
   Define physical-pixel coverage and implement equivalent CPU/GPU thresholds.
8. **GPU/CPU overclaim.** CPU and GPU refinement cannot be byte-identical.
   Gate exact shared CPU bytes and authoritative source data; use semantic and
   visual GPU checks under ADR 0017.
9. **Raycast cost.** Testing every terrain triangle or organism every pointer
   event can be expensive. Use chunk bounds, grid DDA, existing visible
   organism lists, and dirty-key caching; measure POV hover cost separately.
10. **Raycast/render disagreement.** Analytic terrain or broad-phase volumes
    can select invisible content. Intersect resident drawn terrain and
    canonical icosphere triangles after the box/ellipsoid broad phase; exclude
    skirts, absent chunks, and stale frame transforms.
11. **Accessibility regression.** Global shortcut listeners currently consume
    keys too broadly. Scope shortcuts to the focused view, preserve normal
    toolbar/form keyboard behavior, provide visible focus, labels, pressed
    state, and narrow-screen access.
12. **Panel performance.** A richer shared model can cause DOM or texture churn.
    Key snapshots by semantic generations, cap telemetry cadence, diff stable
    DOM fields, hash native panel pixels, and avoid rebuilding unchanged data.
13. **WebGPU capability confusion.** Adapter presence is not a successfully
    initialized map/POV renderer. Publish capabilities only after initialization
    and handle loss with typed state transitions.
14. **Version-boundary mistakes.** Presentation pixels and input routing may
    intentionally change, but generation and records do not. Any discovered
    generator drift stops the refactor for separate diagnosis; do not re-bless
    determinism fixtures casually.

## 10. Final acceptance checklist

### Architecture

- [ ] A new ADR records shared viewer ownership and the one-world multi-view
      rule.
- [ ] `viewer-host` contains no environment API and compiles natively and for
      wasm.
- [ ] Platform shells are thin adapters/services/renderers rather than separate
      viewer implementations.
- [ ] There is one controller tick, map composer, atlas packer, inspection
      builder, panel model, and binding registry.
- [ ] Renderer presents all visible views in one surface frame and exposes no
      live readback.

### Input and functionality

- [ ] Native and web raw traces emit the same semantic actions.
- [ ] Web buttons enter the same ordered reducer as keys.
- [ ] One-shot repeats, blur/focus loss, wheel accumulation, and held axes have
      shared tested behavior.
- [ ] POV look requires the primary button on both platforms.
- [ ] All current native gameplay/view actions are representable and exposed in
      web where the required backend capability exists.
- [ ] Help/control metadata cannot drift from the binding registry.

### Map

- [ ] Native and web use the same CPU composer and GPU atlas preparation.
- [ ] Web shows organisms and the complete available overlay/channel set.
- [ ] Compose/refinement/zoom controls affect the renderer they report.
- [ ] Map cells remain square, the map stays inside its pane, and grid lines
      remain visible at supported reductions.
- [ ] Desktop browser body/canvas overflow and fixed-backing-store defects are
      covered by automated assertions.

### Information and picking

- [ ] One semantic model supplies native bitmap and web DOM panels.
- [ ] The panel is visible in Map, POV, and Split on both platforms.
- [ ] Desktop web information uses three columns or an equivalently measured
      horizontal layout, with a tested narrow collapse.
- [ ] Map and POV terrain/organism hover use shared data and report the same
      fields.
- [ ] POV ray results agree with resident terrain triangles and rendered
      organism primitives without GPU readback.

### Split view

- [ ] Map, POV, and fixed-ratio Split are available in native and supported web
      environments.
- [ ] Clicking a pane changes visible focus and routes keyboard/wheel input.
- [ ] Both panes follow one traveler and one post-update world state.
- [ ] Split performs one world update and one surface present per frame.
- [ ] POV loss cleanly returns to Map without losing world state.
- [ ] Adjustable ratio remains optional and cannot block the fixed-ratio exit
      criterion.

### Verification and versioning

- [ ] Focused input/map/panel/picking/layout/renderer tests pass.
- [ ] Browser desktop/narrow/DPR/fallback/Split sign-off passes.
- [ ] Full fmt, warning-denying clippy, workspace check/test, wasm check, Node
      wasm parity, static build, and web sign-off pass.
- [ ] Phase harnesses remain green.
- [ ] `WORLD_ALGORITHM_VERSION`, layer revisions, record format, and generation
      determinism fixtures are unchanged.
