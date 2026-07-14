import { runStartupBenchmark } from "./benchmark.js";
import { openVault } from "./storage.js";

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

// Newest entries win: the log keeps a bounded tail so the DOM (and the
// panel layout) never grows with session length.
const MAX_DIAGNOSTIC_LINES = 100;

const appendDiagnostic = (message) => {
  const node = platformFields.get("diagnostics");
  if (!node) return;
  const lines = `${node.textContent}\n${message}`.trim().split("\n");
  node.textContent = lines.slice(-MAX_DIAGNOSTIC_LINES).join("\n");
  node.scrollTop = node.scrollHeight;
};

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

// Build map controls from the Rust descriptor registry. The DOM supplies
// containers and styling only; channel/overlay ids, labels, groups, and order
// have one authority in viewer_host::map.
const installMapControls = ({ channels, overlays }) => {
  const channelSelect = document.querySelector('[data-generated="map-channels"]');
  if (channelSelect) {
    channelSelect.replaceChildren();
    const groups = new Map();
    for (const descriptor of channels) {
      let group = groups.get(descriptor.group);
      if (!group) {
        group = document.createElement("optgroup");
        group.label = descriptor.group_label;
        groups.set(descriptor.group, group);
        channelSelect.append(group);
      }
      const option = document.createElement("option");
      option.value = descriptor.id;
      option.textContent = descriptor.label;
      group.append(option);
    }
  }

  const overlayHost = document.querySelector('[data-generated="map-overlays"]');
  if (overlayHost) {
    overlayHost.replaceChildren();
    const groups = new Map();
    for (const descriptor of overlays) {
      let group = groups.get(descriptor.group);
      if (!group) {
        group = document.createElement("span");
        group.className = "map-control-group";
        group.setAttribute("aria-label", descriptor.group_label);
        groups.set(descriptor.group, group);
        overlayHost.append(group);
      }
      const button = document.createElement("button");
      button.type = "button";
      button.dataset.action = "toggle-overlay";
      button.dataset.value = descriptor.id;
      button.dataset.overlayKey = descriptor.id.replaceAll("-", "_");
      button.setAttribute("aria-pressed", "false");
      button.textContent = descriptor.label;
      group.append(button);
    }
  }
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
    for (const control of document.querySelectorAll("[data-action]")) {
      if (!registeredActions.has(control.dataset.action)) {
        appendDiagnostic(`unregistered-action:${control.dataset.action}`);
        control.disabled = true;
      }
    }
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

// Mirror the small serde-built presentation DTO into the toolbar so toggles visibly register:
// buttons carry pressed state, selects show the mode the runtime is in.
// Shared action descriptors are the one source of truth (alignment plan §5.2).
const syncControls = (presentation) => {
  const pov = presentation.view.pov;
  const pressed = {
    "toggle-gpu-compose": presentation.map.backend === "gpu-atlas",
    "toggle-refinement": presentation.map.refinement,
    "toggle-walk": pov.motion === "walk",
    "toggle-pov-shadow-ao": pov.shadow_ao,
    "toggle-pov-detail-normals": pov.detail_normals,
    "toggle-pov-water": pov.water,
  };
  for (const [action, state] of Object.entries(pressed)) {
    const control = document.querySelector(`button[data-action="${action}"]`);
    if (control) control.setAttribute("aria-pressed", String(state));
  }
  for (const control of document.querySelectorAll('button[data-action="toggle-overlay"]')) {
    control.setAttribute(
      "aria-pressed",
      String(presentation.map.overlays[control.dataset.overlayKey]),
    );
  }
  for (const control of document.querySelectorAll('button[data-action="set-presentation"]')) {
    control.setAttribute("aria-pressed", String(control.dataset.value === presentation.view.mode));
  }
  const selectValues = {
    "set-map-channel": presentation.map.channel,
    "set-resource-tier": presentation.tier.runtime,
    "set-worker-backend": { inline: "inline", workers: "workers", "shared-memory": "shared-workers" }[
      presentation.executor.mode
    ],
    "set-pov-render-scale": `${pov.render_scale}`,
  };
  for (const [action, value] of Object.entries(selectValues)) {
    const control = document.querySelector(`select[data-action="${action}"]`);
    if (control && value !== undefined) control.value = value;
  }
};

const updatePresentation = (presentation) => {
  lastPresentation = presentation;
  if (presentation.view.mode === "pov") clearPanelHover();
  zoom = presentation.map.zoom;
  writeStatus(
    "webgpu-status",
    `${presentation.renderer.mode} / refine ${presentation.map.refinement}`,
  );
  writeStatus("map-decor-status", presentation.decor_status.replaceAll("-", " "));
  syncControls(presentation);
};

const dispatchAction = (id, value = "") => {
  const app = window.__werApp;
  if (!app) {
    appendDiagnostic(`action-dropped (wasm not ready): ${id}`);
    return;
  }
  try {
    app.action(id, value === "" ? undefined : `${value}`);
    appendDiagnostic(`action:${id}${value === "" ? "" : `=${value}`}`);
    scheduleFrame();
  } catch (error) {
    appendDiagnostic(`action-rejected:${id}:${String(error)}`);
  }
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

// ---- Shared information document -------------------------------------------------
// Rust owns sampling, labels, formatting, severity, and column placement. The
// browser creates one accessible node per stable id, then mutates only the
// value/severity/visibility properties that actually changed. Normal frame
// telemetry is capped at 2 Hz; hover invalidation is intentionally immediate.
const PANEL_REFRESH_MS = 500;
const panelRoot = document.querySelector("[data-panel-document]");
const panelColumnHosts = new Map(
  Array.from(document.querySelectorAll("[data-panel-column]"), (node) => [
    node.dataset.panelColumn,
    node,
  ]),
);
const panelSections = new Map();
const panelFields = new Map();
let panelDomUpdates = 0;
let panelRefreshes = 0;
let panelLastRefresh = Number.NEGATIVE_INFINITY;
let panelRefreshTimer = 0;
let panelSchemaVersion = null;
let panelDocumentRevision = null;

const recordPanelMutation = (fieldId = null) => {
  // The counter readout is instrumentation, not panel content. Counting its
  // own text update would create a permanent N -> N+1 feedback rebuild at
  // every refresh, so this one observer-effect mutation is deliberately
  // excluded. Structure and every semantic field mutation remain counted.
  if (fieldId === "performance.dom-updates") return;
  panelDomUpdates += 1;
};

const safeDomId = (kind, id) =>
  `panel-${kind}-${id.replaceAll(/[^A-Za-z0-9_-]/g, "-")}`;

const newPanelField = (field, sectionId, attached) => {
  const row = document.createElement("div");
  row.className = "panel-field";
  row.dataset.panelFieldRow = field.id;
  row.dataset.severity = field.severity;
  row.dataset.span = field.span;
  row.hidden = !field.visible;

  const label = document.createElement("dt");
  label.textContent = field.label;
  const value = document.createElement("dd");
  value.dataset.panelField = field.id;
  value.textContent = field.value;
  row.append(label, value);

  const state = {
    row,
    value,
    sectionId,
    label: field.label,
    span: field.span,
    severity: field.severity,
    visible: field.visible,
  };
  panelFields.set(field.id, state);
  if (attached) recordPanelMutation();
  return state;
};

const newPanelSection = (section) => {
  const host = panelColumnHosts.get(section.column);
  if (!host) throw new Error(`panel document named unknown column ${section.column}`);

  const node = document.createElement("section");
  node.className = "panel-section";
  node.dataset.panelSection = section.id;
  const heading = document.createElement("h2");
  heading.id = safeDomId("section", section.id);
  heading.textContent = section.title;
  node.setAttribute("aria-labelledby", heading.id);
  if (section.id === "warnings") {
    node.setAttribute("aria-live", "polite");
    node.setAttribute("aria-atomic", "false");
  }
  const values = document.createElement("dl");
  node.append(heading, values);

  const state = {
    node,
    values,
    column: section.column,
    title: section.title,
    span: section.span,
  };
  panelSections.set(section.id, state);
  for (const field of section.fields) {
    if (panelFields.has(field.id)) throw new Error(`duplicate panel field id ${field.id}`);
    values.append(newPanelField(field, section.id, false).row);
  }
  host.append(node);
  recordPanelMutation();
  return state;
};

const ensurePanelStructure = (section) => {
  let state = panelSections.get(section.id);
  if (!state) return newPanelSection(section);
  if (
    state.column !== section.column ||
    state.title !== section.title ||
    state.span !== section.span
  ) {
    throw new Error(`panel section schema changed for ${section.id}`);
  }
  for (const field of section.fields) {
    const existing = panelFields.get(field.id);
    if (!existing) {
      state.values.append(newPanelField(field, section.id, true).row);
    } else if (
      existing.sectionId !== section.id ||
      existing.label !== field.label ||
      existing.span !== field.span
    ) {
      throw new Error(`panel field schema changed for ${field.id}`);
    }
  }
  return state;
};

const applyPanelDocument = (documentModel) => {
  if (!panelRoot || !Number.isInteger(documentModel.schema_version)) {
    throw new Error("invalid shared panel document");
  }
  if (panelSchemaVersion !== null && panelSchemaVersion !== documentModel.schema_version) {
    throw new Error(
      `panel schema changed from ${panelSchemaVersion} to ${documentModel.schema_version}`,
    );
  }
  panelSchemaVersion = documentModel.schema_version;

  const sectionIds = new Set();
  const fieldIds = new Set();
  for (const section of documentModel.sections) {
    if (sectionIds.has(section.id)) throw new Error(`duplicate panel section id ${section.id}`);
    sectionIds.add(section.id);
    ensurePanelStructure(section);
    for (const field of section.fields) {
      if (fieldIds.has(field.id)) throw new Error(`duplicate panel field id ${field.id}`);
      fieldIds.add(field.id);
      const state = panelFields.get(field.id);
      if (state.value.textContent !== field.value) {
        state.value.textContent = field.value;
        recordPanelMutation(field.id);
      }
      if (state.severity !== field.severity) {
        state.row.dataset.severity = field.severity;
        state.severity = field.severity;
        recordPanelMutation(field.id);
      }
      if (state.visible !== field.visible) {
        state.row.hidden = !field.visible;
        state.visible = field.visible;
        recordPanelMutation(field.id);
      }
    }
  }

  // Warning ids may disappear from a later document. Keep their nodes mounted
  // and hidden so an id always resolves to the same DOM object if it returns.
  for (const [id, state] of panelFields) {
    if (!fieldIds.has(id) && state.visible) {
      state.row.hidden = true;
      state.visible = false;
      recordPanelMutation(id);
    }
  }
  if (panelRoot.getAttribute("aria-busy") !== "false") {
    panelRoot.setAttribute("aria-busy", "false");
    recordPanelMutation();
  }
  panelDocumentRevision = documentModel.revision;
};

const finiteTelemetry = (value) => (Number.isFinite(value) ? value : 0);

const refreshPanel = () => {
  const app = window.__werApp;
  window.clearTimeout(panelRefreshTimer);
  panelRefreshTimer = 0;
  if (!app || document.hidden) return;
  panelLastRefresh = performance.now();
  try {
    const documentModel = JSON.parse(
      app.panel_document(
        perf.fps,
        finiteTelemetry(perf.updateMs),
        finiteTelemetry(perf.composeMs),
        finiteTelemetry(perf.presentMs),
        finiteTelemetry(perf.uploadKib),
        panelDomUpdates,
      ),
    );
    applyPanelDocument(documentModel);
    panelRefreshes += 1;
  } catch (error) {
    appendDiagnostic(`panel refresh failed: ${String(error)}`);
  }
};

const requestPanelRefresh = (immediate = false) => {
  if (!window.__werApp || document.hidden) return;
  const now = performance.now();
  const delay = immediate ? 0 : Math.max(0, panelLastRefresh + PANEL_REFRESH_MS - now);
  if (delay === 0) {
    refreshPanel();
    return;
  }
  if (!panelRefreshTimer) {
    panelRefreshTimer = window.setTimeout(refreshPanel, delay);
  }
};

document.addEventListener("visibilitychange", () => {
  if (!document.hidden) requestPanelRefresh();
});

window.__panelStatus = () => ({
  connected: panelRoot?.isConnected ?? false,
  hidden: panelRoot?.hidden ?? true,
  schemaVersion: panelSchemaVersion,
  revision: panelDocumentRevision,
  refreshes: panelRefreshes,
  domUpdates: panelDomUpdates,
  sections: panelSections.size,
  fields: panelFields.size,
});

// Deterministic browser-acceptance hook: it exercises the same production
// serializer and changed-only binder without introducing a timer race.
window.__refreshPanelForTest = refreshPanel;

document.querySelector(".toolbar")?.addEventListener("click", (event) => {
  const control = event.target.closest("button[data-action]");
  if (control) dispatchAction(control.dataset.action, control.dataset.value ?? "");
});

document.querySelector(".toolbar")?.addEventListener("change", (event) => {
  const control = event.target.closest("select[data-action]");
  if (control) dispatchAction(control.dataset.action, control.value);
});

window.addEventListener("keydown", (event) => {
  if (
    event.target instanceof Element &&
    event.target.closest("button,input,select,textarea,[contenteditable='true']")
  ) {
    return;
  }
  const app = window.__werApp;
  if (!app) return;
  const handled = app.key_event(
    event.code,
    true,
    event.repeat,
    event.shiftKey,
    event.ctrlKey,
    event.altKey,
    event.metaKey,
  );
  if (handled) event.preventDefault();
  scheduleFrame();
});

window.addEventListener("keyup", (event) => {
  const app = window.__werApp;
  if (!app) return;
  const handled = app.key_event(
    event.code,
    false,
    false,
    event.shiftKey,
    event.ctrlKey,
    event.altKey,
    event.metaKey,
  );
  if (handled) event.preventDefault();
  scheduleFrame();
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
  canvas.addEventListener("pointerleave", (event) => {
    if (canvas.hasPointerCapture(event.pointerId)) return;
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

let hoverEngaged = false;
let lastHoverSample = Number.NEGATIVE_INFINITY;

const clearPanelHover = () => {
  const app = window.__werApp;
  if (!app || !hoverEngaged) return;
  app.clear_hover();
  hoverEngaged = false;
  requestPanelRefresh(true);
};

const updateMapHover = (event) => {
  const canvas = event.currentTarget;
  const app = window.__werApp;
  const mode = lastPresentation?.view.mode;
  if (!app || !mapViewport || (mode !== "map" && mode !== "split") || canvas.hidden) {
    clearPanelHover();
    return;
  }
  const [physicalX, physicalY] = canvasPoint(canvas, event);
  const world = JSON.parse(app.map_world_at(physicalX, physicalY));
  if (!world) {
    clearPanelHover();
    return;
  }
  const now = performance.now();
  if (now - lastHoverSample < 100) return;
  lastHoverSample = now;
  const [wx, wy] = world;
  app.map_hover(wx, wy);
  hoverEngaged = true;
  requestPanelRefresh(true);
};

document.getElementById("world-canvas").addEventListener("pointermove", updateMapHover);
document.getElementById("pov-canvas").addEventListener("pointermove", updateMapHover);
document.getElementById("world-canvas").addEventListener("pointerleave", clearPanelHover);
document.getElementById("pov-canvas").addEventListener("pointerleave", clearPanelHover);

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
  const columnNodes = Array.from(document.querySelectorAll("[data-panel-column]"));
  const columnsStyle = document.querySelector(".panel-columns")
    ? getComputedStyle(document.querySelector(".panel-columns"))
    : null;
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
      infoPanel: rect(".info-panel"),
    },
    panel: {
      connected: panelRoot?.isConnected ?? false,
      hidden: panelRoot?.hidden ?? true,
      busy: panelRoot?.getAttribute("aria-busy") ?? null,
      gridTemplateColumns: columnsStyle?.gridTemplateColumns ?? null,
      sections: panelSections.size,
      fields: panelFields.size,
      columns: columnNodes.map((node) => ({
        id: node.dataset.panelColumn,
        connected: node.isConnected,
        box: nodeRect(node),
        overflowY: getComputedStyle(node).overflowY,
        scrollHeight: node.scrollHeight,
        clientHeight: node.clientHeight,
      })),
      status: window.__panelStatus(),
    },
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
