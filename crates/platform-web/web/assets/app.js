import { runStartupBenchmark } from "./benchmark.js";
import { openVault } from "./storage.js";
import { createWasmBridge } from "./bridge-wasm.js";
import { createDiagnosticsLog } from "./ui/diagnostics.js";
import { createPanelDock } from "./ui/panel-dock.js";
import { createToolbar } from "./ui/toolbar.js";
import { installKeyForwarding } from "./ui/keys.js";

const statusFields = new Map(
  Array.from(document.querySelectorAll("[data-status-field]"), (node) => [
    node.dataset.statusField,
    node,
  ]),
);
const platformFields = new Map(
  Array.from(document.querySelectorAll("[data-platform-field]"), (node) => [
    node.dataset.platformField,
    node,
  ]),
);

let workerProbe;
let lastPresentation;
const registeredActions = new Set();
// The wasm module namespace, kept for the async POV renderer bring-up.
let wasmMod;
// Exact physical fitted rectangle returned by viewer_host::layout. JS uses it
// only to blit/characterize; Rust owns inverse projection and picking.
let mapViewport;
let sharedLayout;
let mapScratch;
let mapScratchContext;
let mapImageData;
let mapCanvasContext;
let surfaceResizeDirty = false;
let surfaceBacking = { width: 1, height: 1, dpr: 1 };
let surfaceResizeGeneration = 0;
let resizeRedrawGeneration = 0;

const writeStatus = (name, value, cls) => {
  const node = statusFields.get(name);
  if (!node) return;
  if (node.textContent !== `${value}`) node.textContent = value;
  if (cls && node.className !== cls) node.className = cls;
};

// Presentation-side timings (the native panel's fps/update/compose/present/
// upload numbers). Written wherever the work happens; the panel reads them
// on its own low-rate schedule so measurement never adds per-frame DOM work.
const perf = {
  fps: 0,
  frames: 0,
  lastRoll: 0,
  updateMs: null,
  composeMs: null,
  presentMs: null,
  uploadKib: null,
};

const appendDiagnostic = createDiagnosticsLog(() => platformFields.get("diagnostics"));

// The shared-UI bridge seam: ui/ modules reach the runtime only through this
// bundle, so the native overlay pages can swap in bridge-ipc.js unchanged.
const bridge = createWasmBridge({
  perf,
  requestFrame: () => scheduleFrame(),
  diagnostic: appendDiagnostic,
});
const dispatchAction = bridge.dispatch;

const panelDock = createPanelDock({
  fetchDocument: bridge.fetchPanelDocument,
  diagnostic: appendDiagnostic,
  ready: bridge.ready,
});
const requestPanelRefresh = panelDock.requestRefresh;
const refreshPanel = panelDock.refresh;

const toolbar = createToolbar({ dispatch: dispatchAction });
const { installMapControls, syncControls } = toolbar;

const drawBootCanvas = () => {
  const canvas = document.getElementById("world-canvas");
  mapCanvasContext ??= canvas.getContext("2d");
  const ctx = mapCanvasContext;
  if (!ctx) return;
  const w = canvas.width;
  const h = canvas.height;
  const gradient = ctx.createLinearGradient(0, 0, w, h);
  gradient.addColorStop(0, "#16362f");
  gradient.addColorStop(0.55, "#243b44");
  gradient.addColorStop(1, "#14171c");
  ctx.fillStyle = gradient;
  ctx.fillRect(0, 0, w, h);
  ctx.strokeStyle = "rgba(255,255,255,0.14)";
  for (let x = 0; x < w; x += 48) {
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, h);
    ctx.stroke();
  }
  for (let y = 0; y < h; y += 48) {
    ctx.beginPath();
    ctx.moveTo(0, y);
    ctx.lineTo(w, y);
    ctx.stroke();
  }
  ctx.fillStyle = "#eef1f3";
  ctx.font = "16px system-ui, sans-serif";
  ctx.fillText("Browser runtime boot", 18, 30);
};

// The view magnification is composed by the canonical Rust presenter. JS only
// reports it and blits the already transformed source pixels.
let zoom = 1;

// Blit the canonical RGBA window onto the canvas, preserving its square
// aspect. Zoom and every overlay have already been transformed together.
const drawCpuMap = (header, pixels) => {
  const canvas = document.getElementById("world-canvas");
  mapCanvasContext ??= canvas.getContext("2d");
  const ctx = mapCanvasContext;
  if (!ctx || header.kind !== "rgba8" || !mapViewport) return false;
  if (!mapScratch || mapScratch.width !== header.width || mapScratch.height !== header.height) {
    mapScratch = document.createElement("canvas");
    mapScratch.width = header.width;
    mapScratch.height = header.height;
    mapScratchContext = mapScratch.getContext("2d");
    mapImageData = mapScratchContext?.createImageData(header.width, header.height);
  }
  if (!mapScratchContext || !mapImageData) return false;
  mapImageData.data.set(pixels);
  mapScratchContext.putImageData(mapImageData, 0, 0);
  Object.assign(mapViewport, {
    sx: 0,
    sy: 0,
    sw: header.width,
    sh: header.height,
    width: header.width,
    height: header.height,
    resolution: header.resolution,
  });
  ctx.imageSmoothingEnabled = false;
  ctx.fillStyle = "#0b0d0f";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.drawImage(
    mapScratch,
    0,
    0,
    header.width,
    header.height,
    mapViewport.dx,
    mapViewport.dy,
    mapViewport.dw,
    mapViewport.dh,
  );
  return true;
};

const applySharedLayout = (layout) => {
  sharedLayout = layout ?? undefined;
  const rect = layout?.map_content;
  if (!rect) {
    mapViewport = undefined;
    return;
  }
  const [dx, dy, dw, dh] = rect;
  mapViewport = { ...mapViewport, dx, dy, dw, dh };
};

// Report WebGPU availability. Returns whether the atlas/POV renderer path
// can be enabled once the wasm app exists — dispatching `renderer:webgpu`
// here would be dropped, since the probe runs before `initWasm` resolves.
const probeWebGpu = () => {
  if ("gpu" in navigator) {
    writeStatus("webgpu-status", "WebGPU available", "ok");
    appendDiagnostic("WebGPU: available");
    return true;
  }
  writeStatus("webgpu-status", "WebGPU unavailable", "warn");
  appendDiagnostic("WebGPU: unavailable; CPU/static fallback active");
  return false;
};

// The CSS box is platform input; backing sizes and every shared rectangle are
// physical pixels. ResizeObserver handles layout changes, while the resolution
// media query catches DPR-only transitions between displays.
const resizeViewerSurface = () => {
  const host = document.querySelector(".canvas-host");
  if (!host) return false;
  const css = host.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  const width = Math.max(1, Math.round(css.width * dpr));
  const height = Math.max(1, Math.round(css.height * dpr));
  if (
    width === surfaceBacking.width &&
    height === surfaceBacking.height &&
    dpr === surfaceBacking.dpr
  ) {
    return false;
  }
  surfaceBacking = { width, height, dpr };
  surfaceResizeGeneration += 1;
  for (const canvas of document.querySelectorAll("canvas[data-view-kind]")) {
    if (canvas.width !== width) canvas.width = width;
    if (canvas.height !== height) canvas.height = height;
  }
  mapViewport = undefined;
  sharedLayout = undefined;
  surfaceResizeDirty = true;
  const app = window.__werApp;
  if (app) applySharedLayout(JSON.parse(app.resize_surface(width, height)));
  scheduleFrame();
  return true;
};

const resizeObserver = new ResizeObserver(() => resizeViewerSurface());
resizeObserver.observe(document.querySelector(".canvas-host"));
window.addEventListener("resize", resizeViewerSurface);

let dprQuery;
const watchDevicePixelRatio = () => {
  dprQuery?.removeEventListener("change", watchDevicePixelRatio);
  resizeViewerSurface();
  dprQuery = window.matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
  dprQuery.addEventListener("change", watchDevicePixelRatio);
};

const initWasm = async () => {
  try {
    const mod = await import("../generated/platform_web.js");
    await mod.default();
    wasmMod = mod;
    const requestedTier = new URL(window.location.href).searchParams.get("tier") ?? "auto";
    if (!["auto", "low", "mid", "high"].includes(requestedTier)) {
      throw new Error(`tier query must be auto, low, mid, or high; received ${requestedTier}`);
    }
    const app = new mod.WebApp(JSON.stringify({ tier: requestedTier, storage: false }));
    window.__werApp = app;
    applySharedLayout(
      JSON.parse(app.resize_surface(surfaceBacking.width, surfaceBacking.height)),
    );
    for (const descriptor of JSON.parse(app.action_descriptors())) {
      registeredActions.add(descriptor.id);
    }
    installMapControls(JSON.parse(app.map_descriptors()));
    toolbar.disableUnregisteredControls(registeredActions, (id) =>
      appendDiagnostic(`unregistered-action:${id}`),
    );
    const hash = mod.origin_feature_hash();
    const hex = `0x${hash.toString(16).padStart(16, "0")}`;
    writeStatus("wasm-status", "wasm loaded", "ok");
    writeStatus("origin-hash", `origin ${hex}`, "ok");
    appendDiagnostic(`origin_feature_hash=${hex}`);
    document.body.dataset.originFeatureHash = hex;
    // The first shared frame performs the first and only world update. Map
    // presentation is read-only and waits for that frame instead of secretly
    // settling the world from `map_pixels`.
    scheduleFrame();
  } catch (error) {
    writeStatus("wasm-status", "wasm failed", "err");
    writeStatus("origin-hash", "origin hash unavailable", "err");
    appendDiagnostic(`wasm initialization failed: ${String(error)}`);
    throw error;
  }
};

const initWorkerProbe = () => {
  if (!("Worker" in window)) {
    appendDiagnostic("Worker: unavailable; inline executor active");
    return;
  }
  workerProbe = new Worker("./assets/worker.js", { type: "module" });
  workerProbe.onmessage = (event) => appendDiagnostic(`worker:${event.data.kind}`);
  workerProbe.postMessage({ kind: "ping", mode: "workers" });
};

const initStorage = async () => {
  const state = await openVault();
  appendDiagnostic(`storage:${state.mode}`);
  window.__werApp?.storage_status(state.mode, state.failures);
  requestPanelRefresh(true);
  if (state.available) dispatchAction("set-storage-enabled", "true");
};

const initBenchmark = () => {
  dispatchAction("request-tier-benchmark");
};

const renderMap = () => {
  const app = window.__werApp;
  if (!app) return false;
  const header = JSON.parse(app.render_cpu_map());
  const t0 = performance.now();
  const pixels = app.map_pixels();
  const t1 = performance.now();
  const presented = drawCpuMap(header, pixels);
  const t2 = performance.now();
  perf.composeMs = t1 - t0;
  perf.presentMs = t2 - t1;
  perf.uploadKib = pixels.byteLength / 1024;
  return presented;
};

const inspectionSource = (presentation) =>
  presentation.view.mode === "pov" ||
  (presentation.view.mode === "split" && presentation.view.focused === "pov")
    ? "pov"
    : "map";

const updatePresentation = (presentation) => {
  const sourceChanged =
    lastPresentation && inspectionSource(lastPresentation) !== inspectionSource(presentation);
  lastPresentation = presentation;
  if (sourceChanged) requestPanelRefresh(true);
  zoom = presentation.map.zoom;
  writeStatus(
    "webgpu-status",
    `${presentation.renderer.mode} / refine ${presentation.map.refinement}`,
  );
  writeStatus("map-decor-status", presentation.decor_status.replaceAll("-", " "));
  syncControls(presentation);
};

// The browser adapter forwards primitive DOM facts only. Binding selection,
// held state, repeat suppression, wheel accumulation, and drag-look gating all
// live in viewer_host::InputMapper on the wasm side.
let gpuStageActive = false;
let rendererEntering = false;
let viewerRendererReady = false;
let rendererFailures = 0;
let viewerFrameHandle = 0;
let lastViewerTime = 0;

const gpuStage = () => document.getElementById("pov-canvas");

const initializeViewerRenderer = async () => {
  if (viewerRendererReady || rendererEntering || !wasmMod) return;
  rendererEntering = true;
  try {
    const canvas = gpuStage();
    await wasmMod.viewer_renderer_init(canvas, canvas.width, canvas.height);
    // ResizeObserver may have updated the backing while async adapter/device
    // creation was in flight. Replay the current dimensions after the slot is
    // installed so renderer resources and the already-updated shared layout
    // cannot start their first frame with different physical sizes.
    const app = window.__werApp;
    if (app) applySharedLayout(JSON.parse(app.resize_surface(canvas.width, canvas.height)));
  } catch (error) {
    rendererEntering = false;
    appendDiagnostic(`viewer renderer init failed: ${String(error)}`);
    // Initialization failure is distinct from losing a device that had
    // rendered successfully; both return to the CPU map without losing state.
    window.__werApp?.renderer_unavailable();
    scheduleFrame();
    return;
  }
  rendererEntering = false;
  viewerRendererReady = true;
  // Only successful adapter/device initialization opens the shared POV gate.
  window.__werApp?.renderer_available();
  appendDiagnostic("viewer: shared WebGPU renderer ready");
  scheduleFrame();
};

const setGpuStageActive = (active) => {
  if (gpuStageActive === active) return;
  gpuStageActive = active;
  rendererFailures = 0;
  lastViewerTime = 0;
  perf.frames = 0;
  perf.lastRoll = 0;
  appendDiagnostic(`viewer: ${active ? "shared GPU stage active" : "CPU Map stage active"}`);
};

const handleEffects = () => {
  const app = window.__werApp;
  if (!app) return;
  for (const effect of JSON.parse(app.take_effects())) {
    if (effect.kind === "benchmark") {
      const result = runStartupBenchmark();
      app.benchmark_result(result.ms);
      appendDiagnostic(`benchmark:${result.ms.toFixed(3)}ms/${result.hardwareConcurrency} cores`);
    } else {
      appendDiagnostic(`service-pending:${effect.kind}`);
    }
  }
};

const viewerFrame = (now) => {
  viewerFrameHandle = 0;
  const app = window.__werApp;
  if (!app) return;
  const dt = Math.min(now - (lastViewerTime || now), 100);
  lastViewerTime = now;
  const t0 = performance.now();
  const frame = JSON.parse(app.frame(dt, now / 1000));
  const presentation = frame.presentation;
  applySharedLayout(frame.layout);
  window.__rendererFrameStatus = frame.renderer_frame;
  window.__povStatus = { ...frame.pov, surface_presented: frame.renderer_frame.presented };
  if (frame.renderer_frame.attempted) {
    if (frame.renderer_frame.presented) {
      rendererFailures = 0;
    } else {
      rendererFailures += 1;
      if (rendererFailures > 30) {
        appendDiagnostic("viewer: renderer failing; returning to CPU Map mode");
        viewerRendererReady = false;
        app.renderer_lost();
      }
    }
  }
  perf.updateMs = performance.now() - t0;
  updatePresentation(presentation);
  syncViewMode(presentation, frame.map.path);
  // CPU composition/presentation timings are measured around drawCpuMap.
  // GPU Map and POV are submitted inside the wasm frame call, so these
  // unavailable path-specific values are explicitly zero instead of leaking
  // the last CPU frame into the shared information model.
  perf.composeMs = 0;
  perf.presentMs = 0;
  perf.uploadKib = 0;
  let mapPresented = false;
  const redrawsResize = surfaceResizeDirty;
  if (frame.map.active && frame.map.path === "cpu") {
    mapPresented = frame.map_dirty || surfaceResizeDirty ? renderMap() : mapViewport !== undefined;
  } else if (frame.map.active) {
    mapPresented = frame.map.drawn;
  }
  if (mapPresented) {
    if (redrawsResize) resizeRedrawGeneration = surfaceResizeGeneration;
    surfaceResizeDirty = false;
  }
  window.__mapStatus = {
    ...frame.map,
    dirty: frame.map_dirty,
    presented: mapPresented,
    update_serial: frame.update_serial,
    resize_generation: surfaceResizeGeneration,
    resize_redraw_generation: resizeRedrawGeneration,
  };
  if (frame.map.active && frame.map.path !== "cpu") {
    perf.uploadKib = frame.map.upload_bytes / 1024;
  }
  handleEffects();
  perf.frames += 1;
  if (now - perf.lastRoll >= 1000) {
    perf.fps = Math.round((perf.frames * 1000) / (now - (perf.lastRoll || now - 1000)));
    perf.frames = 0;
    perf.lastRoll = now;
  }
  requestPanelRefresh(frame.hover_changed);
  if (frame.needs_frame || app.needs_frame()) scheduleFrame();
};

const scheduleFrame = () => {
  if (!viewerFrameHandle) viewerFrameHandle = requestAnimationFrame(viewerFrame);
};

// Keep the canvas swap and frame loop in lockstep with the facade's view
// mode, whatever changed it (button, Tab key, device loss).
const syncViewMode = (presentation, mapPath = "cpu") => {
  const mode = presentation.view.mode;
  const focused = presentation.view.focused;
  const cpuCanvas = document.getElementById("world-canvas");
  const stage = gpuStage();
  const surfaceHadFocus = document.activeElement === cpuCanvas || document.activeElement === stage;
  const useGpuStage =
    mode === "pov" || mode === "split" || (mode === "map" && mapPath === "gpu-atlas");

  cpuCanvas.hidden = useGpuStage;
  stage.hidden = !useGpuStage;
  setGpuStageActive(useGpuStage);

  const focusedLabel = focused === "pov" ? "POV" : "Map";
  const host = document.querySelector(".canvas-host");
  host.dataset.presentationMode = mode;
  host.dataset.focusedView = focused;
  stage.dataset.presentationMode = mode;
  stage.dataset.focusedView = focused;
  stage.setAttribute(
    "aria-label",
    mode === "split"
      ? `Split world view; ${focusedLabel} pane focused`
      : mode === "pov"
        ? "Point-of-view world view"
        : "GPU world map",
  );
  cpuCanvas.setAttribute("aria-label", "World map; Map pane focused");

  const visibleSurface = useGpuStage ? stage : cpuCanvas;
  if (surfaceHadFocus && document.activeElement !== visibleSurface) {
    visibleSurface.focus({ preventScroll: true });
  }
};

window.__panelStatus = panelDock.status;

// Deterministic browser-acceptance hook: it exercises the same production
// serializer and changed-only binder without introducing a timer race.
window.__refreshPanelForTest = refreshPanel;

installKeyForwarding({
  keyEvent: bridge.keyEvent,
  requestFrame: () => scheduleFrame(),
});

// Held keys must not survive focus loss (alt-tab mid-move).
window.addEventListener("blur", () => {
  window.__werApp?.host_focus(false);
  scheduleFrame();
});
window.addEventListener("focus", () => window.__werApp?.host_focus(true));

const canvasPoint = (canvas, event) => {
  const rect = canvas.getBoundingClientRect();
  return [
    ((event.clientX - rect.left) / rect.width) * canvas.width,
    ((event.clientY - rect.top) / rect.height) * canvas.height,
  ];
};

const expectedCaptureLosses = new Set();
const capturedPointerViews = new Map();

for (const canvas of document.querySelectorAll("canvas[data-view-kind]")) {
  const hitView = (app, x, y) => app.view_at(x, y) ?? null;
  canvas.addEventListener("focus", () => window.__werApp?.surface_focus(true));
  canvas.addEventListener("blur", () => window.__werApp?.surface_focus(false));
  canvas.addEventListener("pointerdown", (event) => {
    const app = window.__werApp;
    if (!app) return;
    expectedCaptureLosses.delete(event.pointerId);
    canvas.focus();
    const [x, y] = canvasPoint(canvas, event);
    const view = hitView(app, x, y);
    if (!view) return;
    const handled = app.pointer_button(event.pointerId, event.button, true, x, y, view);
    if (event.button === 0 && handled) {
      capturedPointerViews.set(event.pointerId, view);
      canvas.setPointerCapture(event.pointerId);
    }
    if (handled) event.preventDefault();
    scheduleFrame();
  });
  canvas.addEventListener("pointermove", (event) => {
    const app = window.__werApp;
    if (!app) return;
    const [x, y] = canvasPoint(canvas, event);
    const view = capturedPointerViews.get(event.pointerId) ?? hitView(app, x, y);
    if (view && app.pointer_move(event.pointerId, x, y, view)) scheduleFrame();
  });
  canvas.addEventListener("pointerup", (event) => {
    const app = window.__werApp;
    if (!app) return;
    const [x, y] = canvasPoint(canvas, event);
    // Captured POV owns motion across the seam, but release records the pane
    // physically under the pointer. The captured view remains the fallback
    // outside both panes so drag state always clears.
    const view = hitView(app, x, y) ?? capturedPointerViews.get(event.pointerId);
    const handled = view
      ? app.pointer_button(event.pointerId, event.button, false, x, y, view)
      : false;
    capturedPointerViews.delete(event.pointerId);
    if (canvas.hasPointerCapture(event.pointerId)) {
      expectedCaptureLosses.add(event.pointerId);
      canvas.releasePointerCapture(event.pointerId);
    }
    if (handled) event.preventDefault();
    scheduleFrame();
  });
  canvas.addEventListener("pointercancel", (event) => {
    capturedPointerViews.delete(event.pointerId);
    if (canvas.hasPointerCapture(event.pointerId)) {
      expectedCaptureLosses.add(event.pointerId);
      canvas.releasePointerCapture(event.pointerId);
    }
    window.__werApp?.pointer_cancel(event.pointerId);
    scheduleFrame();
  });
  canvas.addEventListener("lostpointercapture", (event) => {
    capturedPointerViews.delete(event.pointerId);
    if (expectedCaptureLosses.delete(event.pointerId)) return;
    window.__werApp?.pointer_cancel(event.pointerId);
    scheduleFrame();
  });
  canvas.addEventListener(
    "wheel",
    (event) => {
      const app = window.__werApp;
      if (!app) return;
      const lines = event.deltaMode === WheelEvent.DOM_DELTA_LINE;
      const delta =
        event.deltaMode === WheelEvent.DOM_DELTA_PAGE
          ? -event.deltaY * canvas.height
          : -event.deltaY;
      const [x, y] = canvasPoint(canvas, event);
      const view = hitView(app, x, y);
      if (view && app.wheel(delta, lines, view)) event.preventDefault();
      scheduleFrame();
    },
    { passive: false },
  );
}

let lastHoverSample = Number.NEGATIVE_INFINITY;

const updateMapHover = (event) => {
  const canvas = event.currentTarget;
  const app = window.__werApp;
  const mode = lastPresentation?.view.mode;
  // Inspection is an observation, not transient pointer chrome. An inactive
  // surface or a letterbox/other-pane point supplies no new observation, so
  // retain the last valid sample while the user moves into the dock to read it.
  if (!app || !mapViewport || (mode !== "map" && mode !== "split") || canvas.hidden) {
    return;
  }
  const [physicalX, physicalY] = canvasPoint(canvas, event);
  const world = JSON.parse(app.map_world_at(physicalX, physicalY));
  if (!world) {
    return;
  }
  const now = performance.now();
  if (now - lastHoverSample < 100) return;
  lastHoverSample = now;
  const [wx, wy] = world;
  app.map_hover(wx, wy);
  requestPanelRefresh(true);
};

document.getElementById("world-canvas").addEventListener("pointermove", updateMapHover);
document.getElementById("pov-canvas").addEventListener("pointermove", updateMapHover);
// Keep the last valid Map sample while the pointer crosses the status bar into
// a scrollable panel (including the page-scrolling narrow layout). Surface
// pointer state still ends outside the shell; a captured drag keeps its
// established outside-surface semantics.
document.querySelector(".app-shell")?.addEventListener("pointerleave", (event) => {
  if (!capturedPointerViews.has(event.pointerId)) {
    window.__werApp?.pointer_cancel(event.pointerId);
  }
  scheduleFrame();
});

// Milestone 0 characterization probe. Browser automation calls this stable,
// read-only surface instead of reconstructing layout selectors or parsing
// screenshots. The intentionally broken pre-alignment geometry is committed
// as evidence, then replaced by behavioral assertions in the viewport
// milestone. GPU pixels are never read back (ADR 0017).
window.__viewerCharacterization = () => {
  const round = (value) => Math.round(value * 1000) / 1000;
  const nodeRect = (node) => {
    if (!node) return null;
    const box = node.getBoundingClientRect();
    return {
      x: round(box.x),
      y: round(box.y),
      width: round(box.width),
      height: round(box.height),
      right: round(box.right),
      bottom: round(box.bottom),
    };
  };
  const rect = (selector) => nodeRect(document.querySelector(selector));
  const canvas = (selector) => {
    const node = document.querySelector(selector);
    return node
      ? {
          hidden: node.hidden,
          css: rect(selector),
          backing: { width: node.width, height: node.height },
        }
      : null;
  };
  const documentElement = document.documentElement;
  return {
    viewport: {
      width: window.innerWidth,
      height: window.innerHeight,
      dpr: window.devicePixelRatio,
      layoutContract: window.innerWidth <= 760 ? "stacked-scroll" : "bounded-desktop",
    },
    document: {
      clientWidth: documentElement.clientWidth,
      clientHeight: documentElement.clientHeight,
      scrollWidth: documentElement.scrollWidth,
      scrollHeight: documentElement.scrollHeight,
    },
    boxes: {
      appShell: rect(".app-shell"),
      viewer: rect(".viewer"),
      toolbar: rect(".toolbar"),
      canvasHost: rect(".canvas-host"),
      statusBar: rect(".status-bar"),
      infoPanelResizer: rect("[data-info-panel-resizer]"),
      infoPanel: rect(".info-panel"),
    },
    panel: panelDock.characterization(),
    canvases: {
      map: canvas("#world-canvas"),
      pov: canvas("#pov-canvas"),
      observedBacking: surfaceBacking,
      sharedLayout: sharedLayout ?? null,
      mapViewport:
        mapViewport === undefined
          ? null
          : Object.fromEntries(
              Object.entries(mapViewport).map(([key, value]) => [
                key,
                typeof value === "number" ? round(value) : value,
              ]),
            ),
    },
    renderer: lastPresentation
      ? {
          mode: lastPresentation.renderer.mode,
          compose: lastPresentation.map.backend,
          zoom: lastPresentation.map.zoom,
          refinement: lastPresentation.map.refinement,
          viewMode: lastPresentation.view.mode,
          focusedView: lastPresentation.view.focused,
          povSupported: lastPresentation.view.pov_supported,
          domStatus:
            document.querySelector('[data-status-field="webgpu-status"]')?.textContent ?? null,
          povStatus: window.__povStatus ?? null,
          frameStatus: window.__rendererFrameStatus ?? null,
        }
      : null,
    performance: {
      tier: lastPresentation?.tier.runtime ?? null,
      fps: perf.fps,
      updateMs: perf.updateMs,
      composeMs: perf.composeMs,
      presentMs: perf.presentMs,
      uploadKib: perf.uploadKib,
    },
  };
};

watchDevicePixelRatio();
drawBootCanvas();
const webgpuAvailable = probeWebGpu();
initWorkerProbe();
await initWasm();
if (webgpuAvailable) {
  await initializeViewerRenderer();
}
await initStorage();
initBenchmark();
