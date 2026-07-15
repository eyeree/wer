# M0 spike notes — wry child webview over the native wgpu surface

Date: 2026-07-14. Environment: WSL2 (Ubuntu 24.04) under WSLg, X11
(`WAYLAND_DISPLAY=` unset for all runs — winit otherwise picks Wayland, where
child webviews are unsupported), llvmpipe software Vulkan, wry 0.55.1,
WebKitGTK 2.52.3 (`libwebkit2gtk-4.1`), debug build.

Spike binary: `crates/platform-native/examples/overlay_spike.rs`
(`cargo run -p platform-native --features overlay --example
overlay_spike`; knobs `SPIKE_OVERLAY=0|1`, `SPIKE_SECONDS`, `SPIKE_WINDOW`).
It drives the real `Renderer` with an animated CPU Map pane under a
transparent full-window wry child webview whose test page forwards input over
IPC. This answers the five M0 questions from
[`implementation-plan.md`](implementation-plan.md) §6.

## Verdict: conditional go — the dock model works, X11 transparency does not

Everything measurable is green (latency, pacing, pump coexistence, memory,
startup). The one hard failure is **alpha compositing of the child webview
over the wgpu parent window on X11**: transparent DOM regions render opaque
black. The plan's M2 (webview bounded to the panel dock, never overlapping
the 3D panes) is fully validated; M3's "full-window transparent overlay"
is **not available on Linux/X11** and the plan is amended accordingly.

## Measurements

20–35 s automated runs, no user interaction (`moves 0`; pings/RTT flow
automatically). Representative summaries:

| metric | overlay=0 (baseline) | overlay=1 |
|---|---|---|
| avg fps (uncapped X11 present) | 283.4 | 295.1 / 297.3 (two runs) |
| frame dt ms p50 / p95 | 3.49 / 4.06 | 3.31–3.33 / 3.78–3.80 |
| `render_frame` ms p50 / p95 | 1.60 / 1.98 | 1.48–1.50 / 1.85–1.86 |
| JS→Rust→JS IPC RTT ms p50 / p95 | — | 2.00–3.00 / 4.00 |
| IPC receive→frame-drain ms p50 / p95 | — | 0.39–0.40 / 0.55–0.61 |
| GTK pump max drain (bounded at 100) | 0 | 19–22 iterations |
| webview creation ms | — | 36–50 |
| self RSS delta | — | +40–42 MiB |
| WebKit helper processes RSS | 0 | ~330–350 MiB total |

Notes:

1. **No presentation-path perturbation detected** (M0 item 3): the overlay
   runs were, if anything, marginally *faster* than baseline — within noise.
   On X11-without-vsync at ~290 fps a compositing detour would be visible;
   it isn't. The benchmark-mode rule in the plan stays (cheap insurance),
   but it is not load-bearing on this evidence.
2. **IPC latency confirms the architecture's input budget** (M0 item 2): a
   2–4 ms JS→Rust→JS round trip (WebKit quantizes `performance.now()` to
   1 ms, so these are coarse upper bounds) and sub-millisecond queue latency
   from IPC receive to frame drain. One-way input forwarding costs roughly
   1–2 ms — imperceptible against a 16.7 ms frame, and consistent with the
   estimate that motivated DOM-owned input.
3. **GTK pump coexists cleanly with the redraw-chain pacer** (M0 item 4):
   bounded pump in `about_to_wait` drained at most ~22 iterations, frame
   pacing unaffected, no starvation in either direction. An 8 ms
   `WaitUntil` backstop keeps the webview serviced when winit goes quiet.
4. **Startup and memory** (M0 item 5): webview creation is 36–50 ms
   (negligible). Memory is the real cost: ~40 MiB in-process plus ~330 MiB
   of WebKit helper processes (WebKitNetworkProcess/WebKitWebProcess;
   sum of `VmRSS`, so shared pages are double-counted — treat as an upper
   bound). Acceptable for an interactive viewer; another reason benchmark
   runs use `--no-overlay`.
5. The webview received and executed the page (ready message, pings, DOM
   panel with live stats rendered correctly, blue drag box present). UA:
   `AppleWebKit/605.1.15 … Version/60.5 Safari/605.1.15`.

## The compositing failure (M0 item 1)

Observed via full-desktop screenshots of the WSLg window:

- `SPIKE_OVERLAY=0`: the animated Map renders and captures perfectly.
- `SPIKE_OVERLAY=1`: the webview's own DOM (panel strip) renders perfectly,
  but everywhere the page is CSS-transparent the window shows **opaque
  black** — the wgpu content underneath never shows through.
- Workarounds tried, no change: `WEBKIT_DISABLE_COMPOSITING_MODE=1`,
  `WEBKIT_DISABLE_DMABUF_RENDERER=1`, both together.

This matches the X11 protocol reality rather than a WebKit bug: X11 does not
alpha-blend a child window against its parent — a child window's pixels
replace the parent's within its bounds, and ARGB visuals only blend for
top-level windows under a compositing manager. So "transparent child webview
over the wgpu surface" cannot work on X11 regardless of WebKit settings.
Expectations elsewhere (unverified in this spike): Windows (WebView2 visual
hosting/DirectComposition) and macOS (CALayer) both composite child-view
alpha and should support the full-window overlay.

Also observed: the WSLg window manager ignores creation-time window position
hints and cascades windows (the spike now issues a post-map
`set_outer_position`, which helps but is not always honored either).
Irrelevant to the architecture; relevant to anyone automating screenshots.

## Consequences for the plan

1. **M2 (dock-bounded webview) is the durable Linux shape, not a stepping
   stone.** The webview owns non-overlapping UI rectangles (panel dock,
   toolbars); the wgpu panes are never covered by DOM. Everything measured
   here — IPC, pump, memory, panel rendering — validates it.
2. **M3 splits by platform.** Full-window DOM with transparent holes is a
   Windows/macOS shape (to be verified on Windows before relying on it).
   On Linux/X11, input over the 3D panes stays with winit
   (`WinitInputAdapter`), and DOM input covers the UI rectangles — both
   already feed the same `NormalizedInputEvent`/`InputMapper` path, so
   viewer-host semantics stay identical; what's lost on Linux is only
   window-spanning DOM chrome (drag from panel across the map, full-window
   modals).
3. **3D-in-panel punch-through (M5) needs a Linux answer.** Candidates, in
   order of attractiveness: (a) X11 SHAPE — wry exposes the underlying
   `webkit2gtk::WebView` widget on Linux, and
   `gtk_widget_shape_combine_region` can cut rectangular holes (bounding +
   input shape) into the child window; a small follow-up spike should test
   whether WSLg's Xwayland honors SHAPE on child windows. (b) Multiple
   dock-bounded webviews tiled around embedded viewports (state sharing
   across pages makes this a last resort). (c) Accept edge-adjacent 3D
   subpanels whose rects can be carved off the single webview rectangle.
4. **Wayland remains out entirely** (`build_as_child` is X11-only), matching
   the plan's accepted constraint; X11 is this project's stable WSLg path
   anyway.

## Reproduction

```sh
sudo apt install -y pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev

# Baseline / overlay A/B (force X11; Wayland cannot host child webviews).
WAYLAND_DISPLAY= SPIKE_OVERLAY=0 SPIKE_SECONDS=20 \
  cargo run -p platform-native --features overlay --example overlay_spike
WAYLAND_DISPLAY= SPIKE_OVERLAY=1 SPIKE_SECONDS=30 \
  cargo run -p platform-native --features overlay --example overlay_spike
```

Interactive checks (run without `SPIKE_SECONDS` and use the window): the
panel strip is DOM (translucent over the window clear color), the blue box
exercises `setPointerCapture` drags, all pointer/key events over the window
land in the page and are forwarded over IPC (the summary's "winit input
events" stays ~0), and the summary prints RTT/queue-latency percentiles on
close.
