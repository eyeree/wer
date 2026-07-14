import { runStartupBenchmark } from "./benchmark.js";
import { openVault } from "./storage.js";

const fields = new Map(
  Array.from(document.querySelectorAll("[data-field]"), (node) => [node.dataset.field, node]),
);

let workerProbe;
let lastSnapshot;
const registeredActions = new Set();
// The wasm module namespace, kept for the async POV renderer bring-up.
let wasmMod;
// The canvas placement of the last drawn map image (letterboxed, square
// source), so cursor picking inverts the exact draw transform.
let mapViewport;
let mapScratch;
let mapScratchContext;
let mapImageData;

const write = (name, value, cls) => {
  const node = fields.get(name);
  if (!node) return;
  // Skipping unchanged text keeps the throttled panel refresh from causing
  // any DOM/layout work in the steady state.
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
  uploadKb: null,
};

// Newest entries win: the log keeps a bounded tail so the DOM (and the
// panel layout) never grows with session length.
const MAX_DIAGNOSTIC_LINES = 100;

const appendDiagnostic = (message) => {
  const node = fields.get("diagnostics");
  if (!node) return;
  const lines = `${node.textContent}\n${message}`.trim().split("\n");
  node.textContent = lines.slice(-MAX_DIAGNOSTIC_LINES).join("\n");
  node.scrollTop = node.scrollHeight;
};

const drawBootCanvas = () => {
  const canvas = document.getElementById("world-canvas");
  const ctx = canvas.getContext("2d");
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
  const ctx = canvas.getContext("2d");
  if (!ctx || header.kind !== "rgba8") return false;
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
  const sw = header.width;
  const sh = header.height;
  const sx = 0;
  const sy = 0;
  const scale = Math.min(canvas.width / sw, canvas.height / sh);
  const dw = sw * scale;
  const dh = sh * scale;
  const dx = (canvas.width - dw) / 2;
  const dy = (canvas.height - dh) / 2;
  mapViewport = {
    dx,
    dy,
    dw,
    dh,
    sx,
    sy,
    sw,
    sh,
    width: header.width,
    height: header.height,
    resolution: header.resolution,
  };
  ctx.imageSmoothingEnabled = false;
  ctx.fillStyle = "#0b0d0f";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.drawImage(mapScratch, sx, sy, sw, sh, dx, dy, dw, dh);
  return true;
};

// Report WebGPU availability. Returns whether the atlas/POV renderer path
// can be enabled once the wasm app exists — dispatching `renderer:webgpu`
// here would be dropped, since the probe runs before `initWasm` resolves.
const probeWebGpu = () => {
  if ("gpu" in navigator) {
    write("webgpu-status", "WebGPU available", "ok");
    appendDiagnostic("WebGPU: available");
    return true;
  }
  write("webgpu-status", "WebGPU unavailable", "warn");
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

const initWasm = async () => {
  try {
    const mod = await import("../generated/platform_web.js");
    await mod.default();
    wasmMod = mod;
    const app = new mod.WebApp(JSON.stringify({ tier: "auto", storage: false }));
    window.__werApp = app;
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
    write("wasm-status", "wasm loaded", "ok");
    write("origin-hash", `origin ${hex}`, "ok");
    appendDiagnostic(`origin_feature_hash=${hex}`);
    document.body.dataset.originFeatureHash = hex;
    const snapshot = JSON.parse(app.info_snapshot());
    updateSnapshot(snapshot);
    syncViewMode(snapshot, "cpu");
    // The first shared frame performs the first and only world update. Map
    // presentation is read-only and waits for that frame instead of secretly
    // settling the world from `map_pixels`.
    scheduleFrame();
  } catch (error) {
    write("wasm-status", "wasm failed", "err");
    write("origin-hash", "origin hash unavailable", "err");
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
  perf.uploadKb = pixels.byteLength / 1024;
  return presented;
};

// Mirror the wasm snapshot into the toolbar so toggles visibly register:
// buttons carry pressed state, selects show the mode the runtime is in.
// Shared action descriptors are the one source of truth (alignment plan §5.2).
const syncControls = (snapshot) => {
  const pov = snapshot.view.pov;
  const pressed = {
    "toggle-gpu-compose": snapshot.map.backend === "gpu-atlas",
    "toggle-refinement": snapshot.renderer.refinement,
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
    control.setAttribute("aria-pressed", String(snapshot.map.overlays[control.dataset.overlayKey]));
  }
  for (const control of document.querySelectorAll('button[data-action="set-presentation"]')) {
    control.setAttribute("aria-pressed", String(control.dataset.value === snapshot.view.mode));
  }
  const selectValues = {
    "set-map-channel": snapshot.channel,
    "set-resource-tier": snapshot.tier.runtime,
    "set-worker-backend": { inline: "inline", workers: "workers", "shared-memory": "shared-workers" }[
      snapshot.executor.mode
    ],
    "set-pov-render-scale": `${pov.render_scale}`,
  };
  for (const [action, value] of Object.entries(selectValues)) {
    const control = document.querySelector(`select[data-action="${action}"]`);
    if (control && value !== undefined) control.value = value;
  }
};

const updateSnapshot = (snapshot) => {
  lastSnapshot = snapshot;
  write("region", `${snapshot.region[0]}, ${snapshot.region[1]}`);
  write("channel", snapshot.channel);
  write("tier", `${snapshot.tier.name} / ${snapshot.tier.cache_ceiling_mb} MB`);
  write("executor", `${snapshot.executor.mode} / ${snapshot.executor.parallelism}`);
  write("storage", snapshot.storage.mode);
  zoom = snapshot.zoom;
  write("zoom", `x${zoom}`);
  write("webgpu-status", `${snapshot.renderer.mode} / refine ${snapshot.renderer.refinement}`);
  write("map-decor-status", snapshot.map.decor_status.replaceAll("-", " "));
  const pov = snapshot.view.pov;
  write(
    "view",
    snapshot.view.mode === "pov"
      ? `pov (${pov.motion}, scale ${pov.render_scale}${pov.water ? "" : ", water off"})`
      : `map${snapshot.view.pov_supported ? "" : " / pov unavailable"}`,
  );
  syncControls(snapshot);
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
let povActive = false;
let povEntering = false;
let povRendererReady = false;
let povFailures = 0;
let viewerFrameHandle = 0;
let lastViewerTime = 0;

const povCanvas = () => document.getElementById("pov-canvas");

const initializePovRenderer = async () => {
  if (povRendererReady || povEntering || !wasmMod) return;
  povEntering = true;
  try {
    const canvas = povCanvas();
    await wasmMod.viewer_renderer_init(canvas, canvas.width, canvas.height);
  } catch (error) {
    povEntering = false;
    appendDiagnostic(`pov init failed: ${String(error)}`);
    // The device-loss path: back to map mode with the CPU map (the
    // phase-7 "unsupported-feature paths return to map mode cleanly").
    window.__werApp?.renderer_lost();
    scheduleFrame();
    return;
  }
  povEntering = false;
  povRendererReady = true;
  // Only successful adapter/device initialization opens the shared POV gate.
  window.__werApp?.renderer_available();
  appendDiagnostic("pov: shared WebGPU renderer ready");
  scheduleFrame();
};

const enterPov = () => {
  if (povActive || !povRendererReady) return;
  povActive = true;
  povFailures = 0;
  document.getElementById("world-canvas").hidden = true;
  povCanvas().hidden = false;
  povCanvas().focus();
  lastViewerTime = 0;
  perf.frames = 0;
  perf.lastRoll = 0;
  appendDiagnostic("pov: shared 3D renderer active (hold primary button to look)");
};

const exitPov = () => {
  if (!povActive) return;
  povActive = false;
  povCanvas().hidden = true;
  document.getElementById("world-canvas").hidden = false;
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
  const snapshot = frame.snapshot;
  window.__povStatus = frame.pov;
  if (frame.pov.active) {
    if (frame.pov.rendered) {
      povFailures = 0;
    } else {
      povFailures += 1;
      if (povFailures > 30) {
        appendDiagnostic("pov: renderer failing; returning to map mode");
        povRendererReady = false;
        app.renderer_lost();
      }
    }
  }
  perf.updateMs = performance.now() - t0;
  updateSnapshot(snapshot);
  syncViewMode(snapshot, frame.map.path);
  let mapPresented = false;
  if (frame.map.active && frame.map.path === "cpu") {
    mapPresented = frame.map_dirty ? renderMap() : mapViewport !== undefined;
  } else if (frame.map.active && frame.map.path === "gpu-atlas") {
    mapPresented = frame.map.gpu_submitted;
  }
  window.__mapStatus = { ...frame.map, presented: mapPresented };
  if (frame.map.active && frame.map.path === "gpu-atlas") {
    perf.uploadKb = frame.map.upload_bytes / 1024;
  }
  handleEffects();
  perf.frames += 1;
  if (now - perf.lastRoll >= 1000) {
    perf.fps = Math.round((perf.frames * 1000) / (now - (perf.lastRoll || now - 1000)));
    perf.frames = 0;
    perf.lastRoll = now;
  }
  if (frame.needs_frame || app.needs_frame()) scheduleFrame();
};

const scheduleFrame = () => {
  if (!viewerFrameHandle) viewerFrameHandle = requestAnimationFrame(viewerFrame);
};

// Keep the canvas swap and frame loop in lockstep with the facade's view
// mode, whatever changed it (button, Tab key, device loss).
const syncViewMode = (snapshot, mapPath = "cpu") => {
  const wantPov = snapshot.view.mode === "pov";
  if (wantPov && !povActive) {
    enterPov();
  } else if (!wantPov && povActive) {
    exitPov();
  }
  if (!wantPov) {
    const gpuMap = mapPath === "gpu-atlas";
    document.getElementById("world-canvas").hidden = gpuMap;
    povCanvas().hidden = !gpuMap;
  }
};

// ---- Throttled info panel (the native painted panel, phase-7-plan.md §3.2:
// an HTML info panel replaces it in the browser). Stats refresh at 2 Hz —
// never per frame — so the panel cannot meaningfully affect performance.
const PANEL_REFRESH_MS = 500;

const megabytes = (bytes) => `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
const millis = (value) => (value === null ? "—" : `${value.toFixed(1)}ms`);

const updatePanelStats = (snapshot) => {
  const stats = snapshot.stats;
  if (!stats) return;
  write("player", `${snapshot.world_pos[0].toFixed(0)}, ${snapshot.world_pos[1].toFixed(0)}`);
  write("fps", viewerFrameHandle || povActive ? `${perf.fps}` : "idle");
  write("update-ms", millis(perf.updateMs));
  write("compose-ms", millis(perf.composeMs));
  write("present-ms", millis(perf.presentMs));
  write("upload-kb", perf.uploadKb === null ? "—" : `${perf.uploadKb.toFixed(0)} KB/f`);
  write("stat-regions", `${stats.active_regions}`);
  write(
    "stat-cache",
    `${megabytes(stats.cache_bytes + stats.macro_cache_bytes)} / ${snapshot.tier.cache_ceiling_mb} MB`,
  );
  write(
    "stat-pool",
    `${stats.pool_hits}h/${stats.pool_misses}m ${megabytes(stats.pool_bytes)}`,
  );
  write("stat-jobs", `${stats.dispatched} run, ${stats.cancelled} cancelled`);
  write("stat-deferred", `${stats.deferred_regens}`);
  write("stat-converged", `${stats.converged}`);
  write("stat-cost", `${stats.regen_cost}`);
  write("stat-rosters", `${stats.rosters_built} built, ${megabytes(stats.roster_cache_bytes)}`);
  write("stat-organisms", `${stats.organisms}`);
  write("stat-realized", `${stats.authoritative_realized}a/${stats.organisms_realized}v`);
  write("stat-resonance", `${stats.resonance.toFixed(2)} (${stats.resonance_nodes} nodes)`);
  write("stat-anchors", `${stats.anchors}`);
  write(
    "regen-by-layer",
    stats.regen_by_layer.map((layer) => `${layer.name} ${layer.total}`).join(" · "),
  );
  const domains = ["Pla", "Cli", "Geo", "Hyd", "Eco", "Mor", "Beh", "Aes"];
  write(
    "bias",
    snapshot.bias
      .map((value, i) => `${domains[i]} ${value >= 0 ? "+" : ""}${value.toFixed(2)}`)
      .join(" · "),
  );
};

setInterval(() => {
  const app = window.__werApp;
  if (!app || document.hidden) return;
  updatePanelStats(JSON.parse(app.info_snapshot()));
}, PANEL_REFRESH_MS);

document.querySelector(".toolbar")?.addEventListener("click", (event) => {
  const control = event.target.closest("button[data-action]");
  if (control) dispatchAction(control.dataset.action, control.dataset.value ?? "");
});

document.querySelector(".toolbar")?.addEventListener("change", (event) => {
  const control = event.target.closest("select[data-action]");
  if (control) dispatchAction(control.dataset.action, control.value);
});

window.addEventListener("keydown", (event) => {
  if (event.target instanceof HTMLInputElement || event.target instanceof HTMLSelectElement) {
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

for (const canvas of document.querySelectorAll("canvas[data-view-kind]")) {
  const routedView = () =>
    canvas === povCanvas() && lastSnapshot?.view.mode !== "pov" ? "map" : canvas.dataset.viewKind;
  canvas.addEventListener("focus", () => window.__werApp?.surface_focus(true));
  canvas.addEventListener("blur", () => window.__werApp?.surface_focus(false));
  canvas.addEventListener("pointerdown", (event) => {
    const app = window.__werApp;
    if (!app) return;
    canvas.focus();
    const [x, y] = canvasPoint(canvas, event);
    const handled = app.pointer_button(event.pointerId, event.button, true, x, y, routedView());
    if (event.button === 0) canvas.setPointerCapture(event.pointerId);
    if (handled) event.preventDefault();
    scheduleFrame();
  });
  canvas.addEventListener("pointermove", (event) => {
    const app = window.__werApp;
    if (!app) return;
    const [x, y] = canvasPoint(canvas, event);
    if (app.pointer_move(event.pointerId, x, y, routedView())) scheduleFrame();
  });
  canvas.addEventListener("pointerup", (event) => {
    const app = window.__werApp;
    if (!app) return;
    const [x, y] = canvasPoint(canvas, event);
    const handled = app.pointer_button(event.pointerId, event.button, false, x, y, routedView());
    if (canvas.hasPointerCapture(event.pointerId)) canvas.releasePointerCapture(event.pointerId);
    if (handled) event.preventDefault();
    scheduleFrame();
  });
  canvas.addEventListener("pointercancel", (event) => {
    if (canvas.hasPointerCapture(event.pointerId)) canvas.releasePointerCapture(event.pointerId);
    window.__werApp?.pointer_cancel(event.pointerId);
    scheduleFrame();
  });
  canvas.addEventListener("lostpointercapture", (event) => {
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
      if (app.wheel(delta, lines, routedView())) event.preventDefault();
      scheduleFrame();
    },
    { passive: false },
  );
}

const updateMapHover = (event) => {
  const canvas = event.currentTarget;
  if (!mapViewport || !lastSnapshot || lastSnapshot.view.mode !== "map" || canvas.hidden) {
    write("cursor", "none");
    return;
  }
  // Invert only the canvas letterbox here. Rust owns zoom and world
  // projection through the same MapComposer that produced these pixels.
  const rect = canvas.getBoundingClientRect();
  const px = ((event.clientX - rect.left) / rect.width) * canvas.width;
  const py = ((event.clientY - rect.top) / rect.height) * canvas.height;
  const mx = mapViewport.sx + ((px - mapViewport.dx) / mapViewport.dw) * mapViewport.sw;
  const my = mapViewport.sy + ((py - mapViewport.dy) / mapViewport.dh) * mapViewport.sh;
  if (
    mx < mapViewport.sx ||
    my < mapViewport.sy ||
    mx >= mapViewport.sx + mapViewport.sw ||
    my >= mapViewport.sy + mapViewport.sh
  ) {
    write("cursor", "none");
    return;
  }
  const world = JSON.parse(window.__werApp.map_world_at(mx, my));
  if (!world) {
    write("cursor", "none");
    return;
  }
  const [wx, wy] = world;
  const organism = JSON.parse(window.__werApp.map_organism_at(mx, my));
  write(
    "cursor",
    organism
      ? `${organism.id} @ ${Math.round(organism.world[0])}, ${Math.round(organism.world[1])}`
      : `${Math.round(wx)}, ${Math.round(wy)}`,
  );
  inspectCursor(wx, wy);
};

document.getElementById("world-canvas").addEventListener("pointermove", updateMapHover);
document.getElementById("pov-canvas").addEventListener("pointermove", updateMapHover);

// The native CURSOR readout, throttled: hovering samples the settled cell
// through the cache-read-only `inspect` export at most every 100ms.
let lastInspect = 0;
const inspectCursor = (wx, wy) => {
  const app = window.__werApp;
  const now = performance.now();
  if (!app || now - lastInspect < 100) return;
  lastInspect = now;
  const cell = JSON.parse(app.inspect(wx, wy));
  const value = (v, digits = 1) => (v === null ? "—" : v.toFixed(digits));
  write(
    "cursor-status",
    `${cell.status}  region ${cell.region[0]}, ${cell.region[1]}`,
  );
  write(
    "cursor-stability",
    cell.stability === null ? "—" : `${cell.stability.toFixed(2)} rev ${cell.revision}`,
  );
  write(
    "cursor-etm",
    `${value(cell.elevation, 0)} / ${value(cell.temperature)} / ${value(cell.moisture, 2)}`,
  );
  write(
    "cursor-rrw",
    `${value(cell.hardness, 2)} / ${value(cell.river, 2)} / ${value(cell.wetness, 2)}`,
  );
  write(
    "cursor-sfv",
    `${value(cell.soil_depth, 2)} / ${value(cell.fertility, 2)} / ${value(cell.vegetation, 2)}`,
  );
  write("cursor-biome", cell.biome === null ? "—" : cell.biome);
};

// Milestone 0 characterization probe. Browser automation calls this stable,
// read-only surface instead of reconstructing layout selectors or parsing
// screenshots. The intentionally broken pre-alignment geometry is committed
// as evidence, then replaced by behavioral assertions in the viewport
// milestone. GPU pixels are never read back (ADR 0017).
window.__viewerCharacterization = () => {
  const round = (value) => Math.round(value * 1000) / 1000;
  const rect = (selector) => {
    const node = document.querySelector(selector);
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
  const snapshot = window.__werApp ? JSON.parse(window.__werApp.info_snapshot()) : null;
  return {
    viewport: {
      width: window.innerWidth,
      height: window.innerHeight,
      dpr: window.devicePixelRatio,
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
    canvases: {
      map: canvas("#world-canvas"),
      pov: canvas("#pov-canvas"),
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
    renderer: snapshot
      ? {
          mode: snapshot.renderer.mode,
          compose: snapshot.renderer.compose,
          refinement: snapshot.renderer.refinement,
          viewMode: snapshot.view.mode,
          povSupported: snapshot.view.pov_supported,
          domStatus: document.querySelector('[data-field="webgpu-status"]')?.textContent ?? null,
          povStatus: window.__povStatus ?? null,
        }
      : null,
  };
};

drawBootCanvas();
const webgpuAvailable = probeWebGpu();
initWorkerProbe();
await initWasm();
if (webgpuAvailable) {
  await initializePovRenderer();
}
await initStorage();
initBenchmark();
