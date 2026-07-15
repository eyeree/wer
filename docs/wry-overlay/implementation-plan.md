# wry Overlay Implementation Plan — one DOM UI over native wgpu

Status: **M0–M2 complete, overlay is the default.** The `overlay` cargo
feature is in `platform-native`'s default set and `WER_OVERLAY` defaults to
on (`0` opts out; runtime failure falls back loudly to the bitmap panel).
CI installs WebKitGTK for the native job; the cargo-xwin Windows release
build compiles wry/WebView2. On Linux the shell prefers the X11 winit
backend and restricts GDK to X11 for overlay runs (child webviews cannot
attach to Wayland; GDK would otherwise pick Wayland independently of winit).
M1 extracted the shared UI runtime
(`assets/ui/{panel-dock,toolbar,keys,diagnostics}.js` behind the
`bridge-wasm.js`/`bridge-ipc.js` seam; `web-signoff --assert-layout` and
`--profile-alignment` green) and lifted the presentation/descriptor
serializers into `viewer_host::dto` so both shells emit identical JSON. M2
ships the native dock: `WER_OVERLAY=1 cargo run --bin wer --features overlay`
hosts two wry child webviews (control toolbar strip on top, information-panel
dock with the five resizable columns below) rendering the browser shell's
exact UI from `wer://`-served shared assets, fed by IPC pushes
(descriptors/presentation/panel-document); the wgpu deck fits between and the
bitmap panel + winit input remain the default and headless/benchmark path.
M2 note: the toolbar/panel strips are separate documents today, so the M2
dock geometry is fixed (toolbar 116 px, panel 3/10 of height) until M4's
content-rect handshake. M0 verdict below — **conditional go** (see
[`spike-notes.md`](spike-notes.md)). All measured M0 gates passed (IPC ~2–4 ms
RTT, no presentation-path perturbation, clean GTK-pump coexistence, 36–50 ms
webview startup, ~40 MiB + ~330 MiB helper-process memory). One hard platform
finding: **X11 cannot alpha-composite a child webview over the wgpu window**
(child windows replace parent pixels; no WebKit setting changes this), so the
full-window transparent overlay is a Windows/macOS shape only. On Linux/X11
the durable shape is the M2 dock model: the webview owns non-overlapping UI
rectangles, winit keeps input over the 3D panes, and both sources feed the
same `NormalizedInputEvent` path. M3 below is amended accordingly.
Owner docs: this file. Decision record: to be captured as a new ADR (see M6).
Related: ADR 0028 (shared viewer host), ADR 0017 (no live readback),
ADR 0002 (crate boundaries), `docs/plans/prototype/phase-6-plan.md`.

## 1. Goal and rationale

Give the native and web viewers **one UI implementation** — the DOM/JS panel
and chrome that already exists in `crates/platform-web/web/assets` — while the
native viewer keeps its pristine wgpu Map/POV path (single surface
acquire/submit/present, no readback, no compositor detour for 3D content).

Why this architecture and not a Rust-native UI toolkit (egui et al.):

- The web shell is the durable investment: the project's endgame is a
  native-vs-web 3D rendering performance comparison, after which native may be
  dropped entirely. UI built in the DOM survives either outcome; UI built in a
  native-only toolkit does not.
- Rich UI is already accruing on the web side (resizable subpanels, resizable
  info panel). CSS/DOM gives those interactions nearly free; any Rust toolkit
  means building them twice.
- If both shells run **identical UI and input code**, the perf A/B isolates
  exactly the variable that matters — browser wasm/WebGPU vs native wgpu —
  instead of also comparing two UI stacks.

Mechanism: a **wry child webview** (`WebViewBuilder::build_as_child`, wry
0.55.x) embedded in the existing winit window, layered over the wgpu surface,
loading the same UI assets the browser shell uses. `PanelDocument` JSON and
input events cross an IPC boundary instead of a wasm-bindgen boundary; the
semantic contract (ADR 0028: Rust owns sampling, labels, formatting, severity,
column placement; the shell renders) is unchanged.

Non-goals:

- No change to world generation, determinism, `WORLD_ALGORITHM_VERSION`, any
  `algorithm_revision`, or any golden fixture. This is presentation plumbing.
- No networking. The custom protocol and IPC are in-process webview APIs, not
  sockets.
- Multi-viewport 3D-in-panel rendering is **scoped and designed** here (M5)
  but is expected to land as its own follow-on plan + ADR, because it requires
  renderer API surgery independent of the overlay.

## 2. Architecture overview

```
+--------------------------------------------------------------+
| winit Window (platform-native, unchanged ApplicationHandler) |
|                                                              |
|  +--------------------------------------------------------+  |
|  | wry child webview (transparent, topmost)               |  |
|  |   shared UI assets (panel, chrome, input capture)      |  |
|  |   bridge-ipc.js  <—IPC—>  overlay module (Rust)        |  |
|  +--------------------------------------------------------+  |
|  +--------------------------------------------------------+  |
|  | wgpu surface (renderer, unchanged one-frame contract)  |  |
|  |   Map / POV / Split panes, focus decoration            |  |
|  +--------------------------------------------------------+  |
+--------------------------------------------------------------+
```

- **UI layer (webview):** the shared JS runtime renders the info panel and all
  future chrome. Regions where 3D content must show through are transparent.
  All input lands here first (once M3 lands) and is forwarded down.
- **Content layer (wgpu):** `Renderer::render_frame(MultiViewFrame)` exactly
  as today. The overlay never reads GPU pixels (ADR 0017 holds); alignment
  between DOM regions and wgpu viewports is a **rect handshake**, not
  composition.
- **Semantics (viewer-host):** untouched by this plan's boundary rules.
  `ViewerController::tick`, `InputMapper`, `resolve_view_layout`, and
  `build_panel_document` remain the single authorities. wry appears **only**
  in `platform-native` (a platform crate — allowed by ADR 0002/0028).

### Input ownership decision

> **Amended by M0** (spike-notes.md): on Linux/X11 the webview cannot cover
> the 3D panes (no child-window alpha compositing), so DOM-captures-all is a
> Windows/macOS shape. On Linux, DOM captures input over the UI rectangles it
> owns and winit keeps the 3D panes; both feed the same
> `NormalizedInputEvent`/`InputMapper` path, so viewer-host semantics are
> identical either way. The rationale below stands where full coverage is
> available.

**The DOM captures all input and forwards it down** (once the webview covers
the window, M3). Rationale:

- It is the path the web shell already ships: per-pane pointer listeners with
  `setPointerCapture` drags feeding absolute positions into
  `NormalizedInputEvent` (`crates/viewer-host/src/input.rs`), with look deltas
  derived Rust-side by the `InputMapper`. No Pointer Lock API anywhere (the
  web smoke test forbids it); nothing about the input model changes.
- Per-region click-through for child webviews is not portably available, so
  "winit keeps map/POV input" would require carving the webview into
  UI-only rectangles forever, blocking full-window chrome (drag from panel
  over map, drop targets, modal surfaces).
- Measured cost is one IPC hop (~0.1–1 ms for small messages) on top of the
  webview event pipeline the browser build already tolerates. M0 measures
  this; M3 makes it a permanent telemetry field.

The legacy pure-winit input path (`WinitInputAdapter`) is **retained behind
`--no-overlay`** as the benchmark-clean and recovery mode (see §4).

### IPC protocol v0

Two channels, both JSON, versioned with a `protocol` integer so the JS and
Rust sides can assert compatibility at startup:

| Direction | Transport | Messages |
|---|---|---|
| JS → Rust (high rate) | `window.ipc.postMessage` → `with_ipc_handler` | `input.key`, `input.pointer_move`, `input.pointer_button`, `input.pointer_cancel`, `input.wheel`, `input.host_focus`, `ui.action` (toolbar → `ViewerAction` id), `ui.content_rect` (M4), `ui.viewport_rects` (M5) |
| JS → Rust (request/response) | `fetch("wer://api/…")` via `with_custom_protocol` | `panel-document`, `layout`, `characterization`, `manifest` |
| Rust → JS (push) | `evaluate_script` calling `window.__werOverlay.push(msg)` | `layout-changed` (on `TickOutput.dirty`), `mode/focus-changed`, `recovery` notices |

Input message payloads mirror the `WebApp` wasm exports one-for-one
(`key_event`, `pointer_move`, `pointer_button`, `pointer_cancel`, `wheel`,
`surface_focus`, `host_focus`, `resize_surface` in
`crates/platform-web/src/lib.rs`), so the shared JS input-capture code is
identical on both shells and only the bridge differs.

Threading: the wry IPC handler fires on the platform UI thread. The handler
does no work itself — it decodes and pushes onto an `std::sync::mpsc` channel
owned by `App`; `App` drains the channel at the top of `frame()` (and in
`about_to_wait`) into `InputMapper::handle_event` before
`InputMapper::take_frame`. This keeps event→tick ordering identical to the
winit path and keeps `viewer-host` free of any thread awareness.

## 3. Shared UI layout: where the code lives

Today the UI runtime is `crates/platform-web/web/assets/app.js` (+ `app.css`,
`help.js`, etc.), copied into `target/web-dist` by `web-build` and importing
the wasm facade dynamically (`initWasm()` → `import("../generated/platform_web.js")`).

Refactor (M1) into an explicit backend seam, keeping everything under
`crates/platform-web/web/` so `web-build`/`web-serve`/`web-signoff` are
untouched:

```
crates/platform-web/web/assets/
  ui/            # shared, bridge-agnostic UI runtime (panel DOM builder,
                 # input capture, layout chrome, diagnostics, characterization)
  bridge-wasm.js # implements ViewerBridge over the WebApp wasm facade
  bridge-ipc.js  # implements ViewerBridge over wer:// + window.ipc  (M2)
  app.js         # thin entry: pick bridge, boot ui/
```

`ViewerBridge` is the JS mirror of the `WebApp` surface: `panelDocument()`,
`keyEvent(…)`, `pointerMove(…)`, `pointerButton(…)`, `pointerCancel(…)`,
`wheel(…)`, `viewAt(x,y)`, `resizeSurface(w,h)`, `hostFocus(b)`,
`surfaceFocus(b)`, plus a `push` subscription for Rust-initiated messages.
The wasm bridge is a rename-level wrapper of today's calls; the IPC bridge is
new. One deliberate asymmetry: on web the 3D content is DOM `<canvas>`
elements; on native it is *absence of DOM* (transparent regions). The shared
`ui/` code therefore renders **view-region elements** (`div[data-view-kind]`)
that are canvases on web and transparent hit-target divs on native, so the
per-pane listener/pointer-capture code (`canvasPoint`, `capturedPointerViews`)
is shared verbatim.

Native asset serving: the custom protocol serves `wer://ui/<path>`.
Production embeds the files at compile time via a small `build.rs` in
`platform-native` that generates an `include_bytes!` manifest from
`crates/platform-web/web/assets` (no new dependency); dev mode overrides with
`WER_UI_DIR=<path>` to serve from disk for live-reload editing.

## 4. Modes and fallbacks

| Mode | UI | Input | When |
|---|---|---|---|
| Overlay (default once M3 gates pass) | wry webview (DOM) | DOM → IPC → `InputMapper` | interactive native runs |
| `--no-overlay` / `WER_OVERLAY=0` | `font8x8` bitmap panel (`crates/platform-native/src/panel.rs`, `Hud`) | `WinitInputAdapter` | **benchmark mode** (pristine presentation path), Wayland, missing WebKitGTK/WebView2, recovery |
| Headless (`--screenshot`, `--pov-script`) | CPU bitmap panel via `Hud::compose` | scripted | unchanged; never creates a webview |

Rules:

- The webview is created only in interactive overlay mode. The headless
  harness and F12 CPU capture path keep the bitmap panel **permanently** —
  same precedent as the CPU map composer being the headless/test path
  (ADR 0017). The bitmap panel is therefore *retained*, not deleted, at the
  end of this plan; it stops being the interactive path.
- If webview creation fails at runtime (no X11, missing runtime, GTK init
  failure), the shell logs the reason, falls back to `--no-overlay` behavior,
  and records the fallback in the F12 dump `[overlay]` section.
- Perf-comparison integrity: all committed baseline numbers
  (`docs/plans/prototype/perf-baseline.md`) and `wer-scale --report` runs are
  taken in `--no-overlay` mode. The overlay may change the OS presentation
  path (compositor involvement, flip-model behavior); M0 measures whether it
  does, and the benchmark-mode rule makes the answer irrelevant to the ledger.

## 5. Platform constraints (accepted up front)

- **Linux: X11 only.** `build_as_child` does not work on Wayland; WSLg X11 is
  the stable path in this dev environment anyway. Wayland sessions get
  `--no-overlay` fallback. Requires `libwebkit2gtk-4.1-dev` at build time and
  `gtk::init()` before webview creation plus a GTK pump
  (`while gtk::events_pending() { gtk::main_iteration_do(false); }`) in
  `ApplicationHandler::about_to_wait`. The `gtk` dependency is gated
  `#[cfg(all(unix, not(target_os = "macos")))]` inside `platform-native`.
- **Windows:** WebView2 runtime required (present on Win10/11 by default).
- **macOS:** WKWebView native; transparency supported (App Store caveat noted
  in wry docs — irrelevant here).
- wry pinned in `[workspace.dependencies]` (`wry = "0.55"`), referenced with
  `wry.workspace = true` from `platform-native` only — the neutral crates and
  `viewer-host` never see it (ADR 0002/0028 boundary rule).

## 6. Milestones

Naming follows the repo convention (`overlay m1: …` commit prefixes). Every
milestone ends green on the full CI set: `cargo fmt --all -- --check`,
`RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets`,
`cargo test --workspace`, the wasm32 checks, and
`cargo run --bin web-signoff -- --assert-layout`.

### M0 — Feasibility spike (throwaway branch, findings committed as docs)

Prove the risky physics before touching shared code. A standalone example
(`crates/platform-native/examples/overlay_spike.rs` or a scratch branch):
winit window + wgpu clear loop + wry child webview with a static test page.

Measure and record in `docs/wry-overlay/spike-notes.md`:

1. **Compositing:** transparent webview over the wgpu surface on WSLg/X11 and
   Windows — flicker, z-order, resize behavior, damage artifacts.
2. **Input latency:** timestamped `pointermove` → IPC handler → next
   `frame()` delta, p50/p95, vs the winit path. Also verify DOM
   `setPointerCapture` drag semantics inside wry on both platforms.
3. **Presentation-path perturbation:** native frame timings (existing
   `pass-timing` feature) with and without an idle overlay attached, to size
   the benchmark-mode concern.
4. **GTK pump coexistence** with `ControlFlow::Wait` and the redraw-chain
   pacer: confirm no starvation and no busy-loop (may need
   `ControlFlow::wait_duration` while the overlay is live).
5. Webview startup time (cold/warm) and memory overhead.

Gate: written go/no-go with numbers. Everything after M0 assumes "go".

### M1 — Extract the shared UI runtime with a bridge seam (web only)

- Restructure `crates/platform-web/web/assets` into `ui/` + `bridge-wasm.js`
  + thin `app.js` as in §3. Pure refactor: byte-identical behavior goals, no
  Rust changes except (if needed) none.
- Introduce `div[data-view-kind]` view-region abstraction in `ui/` while the
  web shell continues to bind them to its two canvases.
- Define `ViewerBridge` (documented in `ui/README.md` alongside the IPC
  protocol table) and the `protocol` version constant shared by both bridges.

Gate: `cargo run --bin web-signoff -- --assert-layout` and
`-- --profile-alignment` pass unchanged; `smoke.mjs` still forbids Pointer
Lock; no `web-dist` functional regressions. This milestone is deliberately
shippable and worthwhile even if the overlay is later abandoned.

### M2 — Native overlay host: DOM panel in a strip webview

Scope the webview to the **panel dock strip only** (the region the bitmap
panel occupies today, to the right of the square map). Winit input keeps
working untouched because the webview never overlaps the Map/POV viewports.

- New `crates/platform-native/src/overlay.rs`: webview lifecycle (create on
  `resumed` after the renderer, destroy/recreate on window loss), GTK
  init/pump, custom protocol (`wer://ui/…` assets from the embedded manifest
  or `WER_UI_DIR`; `wer://api/panel-document` returning
  `BrowserHostState`-equivalent JSON built from the same
  `viewer_host::build_panel_document` output the `Hud` consumes), bounds
  tracking on `WindowEvent::Resized` (physical pixels; webview `set_bounds`
  with scale-factor-aware `Rect`).
- `bridge-ipc.js`: implement `panelDocument()` over `fetch("wer://api/…")`,
  reuse the web shell's 500 ms poll cadence (`PANEL_REFRESH_MS`) and
  changed-only `applyPanelDocument` diffing; push channel wired for
  `layout-changed` so hover refresh stays immediate (parity with the rAF-tail
  `requestPanelRefresh(frame.hover_changed)` behavior — native pushes a
  `hover-changed` nudge from `frame()` when `TickOutput.dirty` says so).
- When the overlay is live, `native_frame_layout` stops attaching
  `InformationSurface` uploads (the bitmap strip) to the frame; `Hud`
  rasterization is skipped for interactive frames but stays wired for F12/
  headless.
- `WER_OVERLAY=1` opts in; default remains the bitmap panel until M3 gates.

Gate: side-by-side visual parity of panel content native-vs-web (same
`PanelDocument` revision renders the same fields); panel poll cadence
verified against `panel_build_count` semantics (no rebuild when nothing
changed); fallback path (unset `DISPLAY`/Wayland/`WER_OVERLAY=0`) verified;
`native_panel_source_characterization` fixture test extended to cover the
"overlay active ⇒ no information upload" invariant.

### M3 — Full-window overlay and unified input

> **Amended by M0**: full-window transparent bounds are Windows/macOS only
> (verify on Windows before building on it). On Linux/X11, M3 instead means:
> the dock webview owns its rectangles and their input; winit input over the
> 3D panes stays live (the two sources already converge in `InputMapper`);
> layout geometry flows to the DOM so dock chrome and wgpu panes agree. The
> X11 SHAPE hole-punching experiment (spike-notes.md §consequences) decides
> whether Linux can later join the full-coverage shape.

- Expand webview bounds to the whole window. The shared `ui/` input capture
  (per-view-region pointer listeners, `setPointerCapture`, window key
  listeners, focus listeners) now runs on native, forwarding through
  `bridge-ipc.js` postMessage → `overlay.rs` → mpsc → `InputMapper`.
- `overlay.rs` translates messages exactly as `WebApp` does:
  `PhysicalKey::from_dom_code`, absolute physical positions tagged with
  `ViewKind`, `WheelDelta::{Lines,Pixels}`, `Modifiers`, `host_focus`/
  `surface_focus`. `InputContext` comes from the controller as today.
- View-region divs position themselves from the layout JSON
  (`wer://api/layout`, plus `layout-changed` pushes) mirroring
  `ResolvedViewLayout` (`map_pane`, `pov_pane`, `divider`, focus rects) so
  DOM hit-testing agrees with `hit_view` half-open semantics. The divider
  drag emits the same split-ratio `ViewerAction`s as the web shell.
- Focus decoration: remains a renderer `FocusDecoration` pass initially
  (visual parity with `--no-overlay`); revisit as DOM chrome in M4.
- winit input handlers become inert while the overlay owns input (guarded,
  not deleted — `--no-overlay` uses them), except `Resized`, `Focused`
  (forwarded as `surface_focus`), and `CloseRequested`.
- Telemetry: add `input_ipc_ms` (postMessage timestamp → drain time) to
  `PlatformTelemetry`, surfaced in the panel System column and `state.txt`
  `[frame]`.
- Flip the default: overlay on when available; `--no-overlay`/`WER_OVERLAY=0`
  and automatic fallback stay.

Gate: full interaction parity checklist vs `--no-overlay` — map pan/zoom,
channel cycling, mode/focus switching (Map/POV/Split), POV WASD +
primary-drag look, split divider drag, F-key overlays, F12 dump; input
latency numbers within the M0-established envelope; `wer --inline` still
works under the overlay; `native_input_characterization` fixtures updated to
describe both input sources.

### M4 — DOM-owned panel chrome: resizable panels on both shells

- Implement resizable subpanels and whole-panel resize **once**, in shared
  `ui/` code + CSS, so web and native get them in the same commit.
- Ownership split (keeps ADR 0028 intact): panel/subpanel sizes are
  presentation-local UI state living in the DOM (persisted per-shell later if
  wanted); anything semantic (mode, focus, split ratio) remains
  `ViewerAction`s into `viewer-host`. The one new downward message is
  `ui.content_rect` — the rect the DOM has reserved for 3D content — which
  the native shell feeds into `resolve_view_layout` as the `content`
  `PixelRect` (the web shell equivalently sizes its canvas host). This is the
  same "UI reserves space, viewer-host resolves panes inside it" shape both
  shells already have; only the source of the content rect moves.
- Resize drags produce at most one frame of rect lag between DOM chrome and
  wgpu viewports; acceptable, but instrument it (content-rect serial echoed
  in `__viewerCharacterization` and `state.txt`).

Gate: `web-signoff --assert-layout` extended with resizable-panel assertions
(new layout matrix entries), and the same interactions verified on native via
the M3 checklist plus dump-diffing; panel resize does not perturb settled
world state (`update_serial` unaffected by pure UI resizes).

### M5 — Multi-viewport punch-through (design here, land as follow-on)

The renderer is hard-wired to one Map pane + one POV pane
(`MultiViewFrame{map: Option<_>, pov: Option<_>}`, fixed
`[FramePassKind; 8]`, pairwise non-overlap in `FramePassPlan::new`).
Interactive 3D content inside DOM panels therefore needs renderer API
surgery, which should be its own plan + ADR. What this plan fixes now is the
**contract** so M1–M4 don't preclude it:

- `ui.viewport_rects`: the DOM reports a set of
  `{id, rect, kind: "pov-secondary" | …}` transparent regions each frame they
  change; rects are physical pixels in surface space.
- Renderer generalization sketch: `FramePlanRequest`/`MultiViewFrame` grow a
  `Vec`/slice of secondary 3D panes with new `FramePassKind` variants
  (offscreen + composite per pane, shadows optional), non-overlap validation
  extended over all color regions, all recorded in the same single
  acquire/submit/present. No readback, ever — the DOM never sees these
  pixels; it only frames them.
- Content stays CPU-authoritative for inspection (same rule as Map/POV
  picking today).

Gate (for the follow-on): `FramePassPlan` unit tests over N panes; a demo
panel with an embedded orbiting-camera viewport on both shells.

### M6 — Sign-off, benchmark integrity, and documentation

- **ADR 00xx — "DOM overlay UI over the native surface"**: records the
  decision, the input-ownership choice, X11-only + runtime-fallback
  constraints, the benchmark-mode rule, the retained bitmap-panel headless
  path, and the M5 renderer contract. Update `docs/adr/README.md`.
- F12 dump: new `[overlay]` section in `state.txt` (active/fallback reason,
  protocol version, ui asset source+hash, webview backend, input source,
  `input_ipc_ms` stats, content-rect serial).
- `wer://api/characterization`: native twin of
  `window.__viewerCharacterization` so one probe schema serves both shells;
  add an `overlay-signoff` mode to the native characterization tests (drive
  the IPC protocol headlessly against a mock bridge — no webview needed for
  logic-level tests; the compositing-level checks stay manual + spike-note
  documented).
- `perf-baseline.md`: annotate that all entries are `--no-overlay`; add one
  explicitly-labeled overlay-attached row so the overhead is tracked over
  time rather than assumed.
- Update `AGENTS.md` (commands, debugging notes: overlay env vars, WebKitGTK
  install, X11 requirement) and `README.md` (human-facing run instructions).

## 7. Risk register

| Risk | Likelihood | Mitigation |
|---|---|---|
| Transparent-over-GPU compositing artifacts (flicker, z-fights) on some platform | Medium | M0 spike gates the whole plan; known Tauri/winit flicker reports are v1-window-transparency-specific; strip-mode M2 works even if full-window transparency fails |
| GTK pump vs `ControlFlow::Wait` starvation/busy-loop | Medium | M0 item 4; switch to bounded `wait_duration` while overlay is live |
| Input event coalescing in webview hurts POV look feel | Low | Web shell already ships this input model; M0/M3 latency telemetry makes it measurable, `--no-overlay` preserves the raw path |
| Overlay perturbs presentation path and contaminates perf baselines | Medium | Benchmark-mode rule (§4): all ledger numbers are `--no-overlay`; overlay overhead tracked as its own labeled row |
| Wayland-only environments | Accepted | Runtime fallback to `--no-overlay`; documented constraint |
| WebKitGTK availability/quality in WSL2 | Medium | M0 verifies; `WER_UI_DIR` + Windows-native run as alternate dev path |
| Two runtimes in the native binary (webview + JS) inflate startup/memory | Low | Measured in M0; native's benchmark role uses `--no-overlay` anyway |
| Divergence between `bridge-wasm` and `bridge-ipc` semantics | Medium | Single shared `ui/` runtime + protocol version + the M6 shared characterization schema; parity checklist in M3 |
| wry/webview API churn | Low | wry pinned via workspace deps; surface area confined to `overlay.rs` |

## 8. Open questions (resolve during M0–M2)

1. Exact GTK crate/version to pair with wry 0.55 on Linux, and whether the
   pump needs `wait_duration` pacing (M0).
2. Whether `set_bounds` on a child webview tracks DPI scale changes cleanly on
   all three platforms, or needs explicit scale-factor handling next to the
   existing `Resized` path (M0/M2).
3. Panel dock geometry in M2: keep the current fixed-width right strip, or
   adopt the web shell's dock grid immediately (affects how much CSS `ui/`
   needs to parameterize per shell).
4. Where per-shell UI-local state (subpanel sizes) persists — DOM
   localStorage is unavailable in a custom-protocol page on some backends;
   may need a tiny `wer://api/ui-state` (M4).
5. Whether the focus border moves from renderer decoration to DOM chrome once
   the whole window is DOM-owned (M4).
