import { commandById } from "./commands.js";
import { runStartupBenchmark } from "./benchmark.js";
import { exportSnapshot, openVault } from "./storage.js";

const fields = new Map(
  Array.from(document.querySelectorAll("[data-field]"), (node) => [node.dataset.field, node]),
);

let workerProbe;
let lastSnapshot;
// The wasm module namespace, kept for the async POV renderer bring-up.
let wasmMod;
// The canvas placement of the last drawn map image (letterboxed, square
// source), so cursor picking inverts the exact draw transform.
let mapViewport;

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

// The view magnification (native main.rs: mouse wheel, powers of two up to
// MAX_ZOOM). Presentation-only, exactly like the native `magnify()` — a
// nearest-neighbor center crop that reveals no data beyond the field
// resolution.
const MAX_ZOOM = 16;
let zoom = 1;
let scrollAccum = 0;
// The last composed frame, kept so wheel zoom redraws without recomposing.
let lastMapFrame;

// Blit the composed RGBA window onto the canvas, preserving the source's
// square aspect (letterboxed) so regions stay square like the native viewer.
// At zoom > 1 only the center 1/zoom block is drawn (the native center crop).
const drawCpuMap = (header, pixels) => {
  const canvas = document.getElementById("world-canvas");
  const ctx = canvas.getContext("2d");
  if (!ctx || header.kind !== "rgba8") return;
  lastMapFrame = { header, pixels };
  const source = new ImageData(
    new Uint8ClampedArray(pixels.buffer, pixels.byteOffset, pixels.byteLength),
    header.width,
    header.height,
  );
  const scratch = document.createElement("canvas");
  scratch.width = header.width;
  scratch.height = header.height;
  const scratchCtx = scratch.getContext("2d");
  scratchCtx.putImageData(source, 0, 0);
  const sw = header.width / zoom;
  const sh = header.height / zoom;
  const sx = (header.width - sw) / 2;
  const sy = (header.height - sh) / 2;
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
  ctx.drawImage(scratch, sx, sy, sw, sh, dx, dy, dw, dh);
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

const initWasm = async () => {
  try {
    const mod = await import("../generated/platform_web.js");
    await mod.default();
    wasmMod = mod;
    const app = new mod.WebApp(JSON.stringify({ tier: "auto", storage: false }));
    window.__werApp = app;
    const hash = mod.origin_feature_hash();
    const hex = `0x${hash.toString(16).padStart(16, "0")}`;
    write("wasm-status", "wasm loaded", "ok");
    write("origin-hash", `origin ${hex}`, "ok");
    appendDiagnostic(`origin_feature_hash=${hex}`);
    document.body.dataset.originFeatureHash = hex;
    updateSnapshot(JSON.parse(app.info_snapshot()));
    const started = performance.now();
    renderMap();
    appendDiagnostic(`map settle+compose ${(performance.now() - started).toFixed(0)}ms`);
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
  if (state.available) dispatchCommand("storage:enable");
};

const initBenchmark = () => {
  const result = runStartupBenchmark();
  appendDiagnostic(`benchmark:${result.ms.toFixed(3)}ms/${result.hardwareConcurrency} cores`);
  dispatchCommand("tier:benchmark", result);
};

const renderMap = () => {
  const app = window.__werApp;
  if (!app) return;
  const header = JSON.parse(app.render_cpu_map());
  const t0 = performance.now();
  const pixels = app.map_pixels();
  const t1 = performance.now();
  drawCpuMap(header, pixels);
  const t2 = performance.now();
  perf.composeMs = t1 - t0;
  perf.presentMs = t2 - t1;
  perf.uploadKb = pixels.byteLength / 1024;
};

// Mirror the wasm snapshot into the toolbar so toggles visibly register:
// buttons carry pressed state, selects show the mode the runtime is in.
// One command registry, one source of truth (phase-7-plan.md §3.3).
const syncControls = (snapshot) => {
  const pov = snapshot.view.pov;
  const pressed = {
    "toggle:compose": snapshot.renderer.compose,
    "toggle:refinement": snapshot.renderer.refinement,
    "mode:map": snapshot.view.mode === "map",
    "mode:pov": snapshot.view.mode === "pov",
    "pov:walk": pov.motion === "walk",
    "pov:toggle-baked": pov.shadow_ao,
    "pov:toggle-detail": pov.detail_normals,
    "pov:toggle-water": pov.water,
  };
  for (const [command, state] of Object.entries(pressed)) {
    const control = document.querySelector(`button[data-command="${command}"]`);
    if (control) control.setAttribute("aria-pressed", String(state));
  }
  const selectValues = {
    channel: snapshot.channel,
    worker: { inline: "inline", workers: "workers", "shared-memory": "shared" }[
      snapshot.executor.mode
    ],
    "pov:scale": pov.render_scale === 0.25 ? "quarter" : pov.render_scale === 0.5 ? "half" : "full",
  };
  for (const [command, value] of Object.entries(selectValues)) {
    const control = document.querySelector(`select[data-command="${command}"]`);
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
  write("webgpu-status", `${snapshot.renderer.mode} / refine ${snapshot.renderer.refinement}`);
  const pov = snapshot.view.pov;
  write(
    "view",
    snapshot.view.mode === "pov"
      ? `pov (${pov.motion}, scale ${pov.render_scale}${pov.water ? "" : ", water off"})`
      : `map${snapshot.view.pov_supported ? "" : " / pov unavailable"}`,
  );
  syncControls(snapshot);
  syncViewMode(snapshot);
};

const dispatchCommand = (id, value) => {
  const app = window.__werApp;
  if (!app) {
    appendDiagnostic(`command-dropped (wasm not ready): ${id}`);
    return;
  }
  const snapshot = JSON.parse(app.apply_command(JSON.stringify({ id, value })));
  updateSnapshot(snapshot);
  appendDiagnostic(`settle_hash=${snapshot.settle_hash}`);
  renderMap();
};

// Map-mode movement, mirroring the native shell (main.rs `apply_movement`):
// WASD/arrows move continuously while held at 500 u/s (Shift sprints x4) —
// the speed itself lives in the wasm facade; JS only reports held keys per
// animation frame. The rAF loop runs only while a movement key is down, so
// an idle viewer costs nothing.
const MOVE_KEYS = new Map([
  ["ArrowUp", [0, 1]],
  ["KeyW", [0, 1]],
  ["ArrowDown", [0, -1]],
  ["KeyS", [0, -1]],
  ["ArrowLeft", [-1, 0]],
  ["KeyA", [-1, 0]],
  ["ArrowRight", [1, 0]],
  ["KeyD", [1, 0]],
]);
const heldMoves = new Set();
let sprintHeld = false;
let moveFrame = 0;
let lastMoveTime = 0;

const movementFrame = (now) => {
  moveFrame = 0;
  const app = window.__werApp;
  const inMap = lastSnapshot?.view?.mode !== "pov";
  if (!app || !inMap || heldMoves.size === 0) return;
  const dt = Math.min(now - (lastMoveTime || now), 100);
  lastMoveTime = now;
  let mx = 0;
  let my = 0;
  for (const code of heldMoves) {
    const [dx, dy] = MOVE_KEYS.get(code);
    mx += dx;
    my += dy;
  }
  const input = { move_x: Math.sign(mx), move_y: Math.sign(my), sprint: sprintHeld };
  const t0 = performance.now();
  const snapshot = JSON.parse(app.update(dt, JSON.stringify(input)));
  perf.updateMs = performance.now() - t0;
  updateSnapshot(snapshot);
  renderMap();
  // The native once-per-second fps roll (main.rs `update_telemetry`).
  perf.frames += 1;
  if (now - perf.lastRoll >= 1000) {
    perf.fps = Math.round((perf.frames * 1000) / (now - (perf.lastRoll || now - 1000)));
    perf.frames = 0;
    perf.lastRoll = now;
  }
  moveFrame = requestAnimationFrame(movementFrame);
};

const startMovement = () => {
  if (moveFrame) return;
  lastMoveTime = 0;
  perf.frames = 0;
  perf.lastRoll = 0;
  moveFrame = requestAnimationFrame(movementFrame);
};

// ---- POV mode (phase-7-plan.md §9.9): host the shared 3D renderer on the
// POV canvas. The wasm facade owns camera/meshing/rendering; JS owns the
// canvas swap, the rAF loop, held keys, pointer-lock mouse look, and the
// wheel speed control — the same split as the native winit shell.
const POV_MOVE = new Map([
  ["KeyW", ["move_y", 1]],
  ["ArrowUp", ["move_y", 1]],
  ["KeyS", ["move_y", -1]],
  ["ArrowDown", ["move_y", -1]],
  ["KeyD", ["move_x", 1]],
  ["ArrowRight", ["move_x", 1]],
  ["KeyA", ["move_x", -1]],
  ["ArrowLeft", ["move_x", -1]],
  ["Space", ["move_z", 1]],
  ["ShiftLeft", ["move_z", -1]],
]);
let povActive = false;
let povEntering = false;
const povHeld = new Set();
let povLook = { dx: 0, dy: 0 };
let povWheel = 0;
let povFrameHandle = 0;
let povLastTime = 0;
let povFailures = 0;

const povCanvas = () => document.getElementById("pov-canvas");

const enterPov = async () => {
  if (povActive || povEntering || !wasmMod) return;
  povEntering = true;
  try {
    const canvas = povCanvas();
    await wasmMod.pov_init(canvas, canvas.width, canvas.height);
  } catch (error) {
    povEntering = false;
    appendDiagnostic(`pov init failed: ${String(error)}`);
    // The device-loss path: back to map mode with the CPU map (the
    // phase-7 "unsupported-feature paths return to map mode cleanly").
    dispatchCommand("renderer:device-lost");
    return;
  }
  povEntering = false;
  povActive = true;
  povFailures = 0;
  document.getElementById("world-canvas").hidden = true;
  povCanvas().hidden = false;
  povCanvas().focus();
  povLastTime = 0;
  perf.frames = 0;
  perf.lastRoll = 0;
  appendDiagnostic("pov: shared 3D renderer active (click canvas for mouse look)");
  povFrameHandle = requestAnimationFrame(povFrame);
};

const exitPov = () => {
  if (!povActive) return;
  povActive = false;
  cancelAnimationFrame(povFrameHandle);
  povFrameHandle = 0;
  povHeld.clear();
  if (document.pointerLockElement) document.exitPointerLock();
  povCanvas().hidden = true;
  document.getElementById("world-canvas").hidden = false;
  renderMap();
};

const povFrame = (now) => {
  if (!povActive) return;
  const app = window.__werApp;
  if (!app || lastSnapshot?.view?.mode !== "pov") {
    exitPov();
    return;
  }
  const dt = Math.min(now - (povLastTime || now), 100);
  povLastTime = now;
  const input = {
    time: now / 1000,
    look_dx: povLook.dx,
    look_dy: povLook.dy,
    wheel: povWheel,
    move_x: 0,
    move_y: 0,
    move_z: 0,
  };
  povLook = { dx: 0, dy: 0 };
  povWheel = 0;
  for (const code of povHeld) {
    const [axis, dir] = POV_MOVE.get(code);
    input[axis] += dir;
  }
  input.move_x = Math.sign(input.move_x);
  input.move_y = Math.sign(input.move_y);
  input.move_z = Math.sign(input.move_z);
  const t0 = performance.now();
  const status = JSON.parse(app.pov_frame(dt, JSON.stringify(input)));
  perf.updateMs = performance.now() - t0;
  // Debug handle: the last POV frame status, readable from the console.
  window.__povStatus = status;
  if (status.rendered) {
    povFailures = 0;
  } else {
    povFailures += 1;
    if (povFailures > 30) {
      appendDiagnostic("pov: renderer failing; returning to map mode");
      dispatchCommand("renderer:device-lost");
      return;
    }
  }
  perf.frames += 1;
  if (now - perf.lastRoll >= 1000) {
    perf.fps = Math.round((perf.frames * 1000) / (now - (perf.lastRoll || now - 1000)));
    perf.frames = 0;
    perf.lastRoll = now;
  }
  povFrameHandle = requestAnimationFrame(povFrame);
};

// Keep the canvas swap and frame loop in lockstep with the facade's view
// mode, whatever changed it (button, Tab key, device loss).
const syncViewMode = (snapshot) => {
  const wantPov = snapshot.view.mode === "pov";
  if (wantPov && !povActive) {
    enterPov();
  } else if (!wantPov && povActive) {
    exitPov();
  }
};

// Mouse look: pointer lock on click (the plan's browser control), with
// plain drag-look as the fallback, mirroring the native drag scheme.
document.getElementById("pov-canvas").addEventListener("click", () => {
  if (povActive && !document.pointerLockElement) {
    document.getElementById("pov-canvas").requestPointerLock();
  }
});
document.addEventListener("pointerlockchange", () => {
  if (document.pointerLockElement === povCanvas()) {
    appendDiagnostic("pov: pointer lock (Esc releases)");
    dispatchCommand("pov:pointer-lock");
  }
});
document.getElementById("pov-canvas").addEventListener("pointermove", (event) => {
  if (!povActive) return;
  if (document.pointerLockElement === povCanvas() || event.buttons & 1) {
    povLook.dx += event.movementX;
    povLook.dy += event.movementY;
  }
});
document.getElementById("pov-canvas").addEventListener(
  "wheel",
  (event) => {
    event.preventDefault();
    if (!povActive) return;
    povWheel += event.deltaMode === 1 ? -event.deltaY : -event.deltaY / 40;
  },
  { passive: false },
);

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
  write("fps", moveFrame || povActive ? `${perf.fps}` : "idle");
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

for (const control of document.querySelectorAll("button[data-command]")) {
  if (!commandById.has(control.dataset.command)) {
    appendDiagnostic(`unregistered-command:${control.dataset.command}`);
  }
  control.addEventListener("click", () => {
    appendDiagnostic(`command:${control.dataset.command}`);
    dispatchCommand(control.dataset.command);
  });
}

for (const control of document.querySelectorAll("select[data-command]")) {
  if (!commandById.has(control.dataset.command)) {
    appendDiagnostic(`unregistered-command:${control.dataset.command}`);
  }
  control.addEventListener("change", () => {
    const id = control.dataset.command;
    appendDiagnostic(`${id}:${control.value}`);
    // Worker modes are distinct command ids (they mirror native runtime
    // switches); the other selects pass their value with one id.
    if (id === "worker") {
      dispatchCommand(`worker:${control.value}`);
    } else {
      dispatchCommand(id, control.value);
    }
  });
}

const exportButton = document.querySelector('[data-command="storage:export"]');
if (exportButton) {
  exportButton.addEventListener("click", () => {
    if (!lastSnapshot) return;
    const href = exportSnapshot(lastSnapshot);
    appendDiagnostic(`export:${href.slice(0, 16)}`);
    URL.revokeObjectURL(href);
  });
}

window.addEventListener("keydown", (event) => {
  if (event.target instanceof HTMLInputElement || event.target instanceof HTMLSelectElement) {
    return;
  }
  const inPov = lastSnapshot?.view?.mode === "pov";
  sprintHeld = event.shiftKey;
  if (!inPov && MOVE_KEYS.has(event.code)) {
    event.preventDefault();
    heldMoves.add(event.code);
    startMovement();
    return;
  }
  if (inPov && POV_MOVE.has(event.code)) {
    event.preventDefault();
    povHeld.add(event.code);
    return;
  }
  for (const command of commandById.values()) {
    if (!command.key || event.key !== command.key) continue;
    // The native shell's POV key gate (3d-phase-1-plan.md §8.4), mirrored:
    // POV-group keys act only in POV mode, and Tab toggles the modes.
    if (command.group === "POV" && !inPov) continue;
    const id = command.id === "mode:pov" && inPov ? "mode:map" : command.id;
    event.preventDefault();
    dispatchCommand(id);
    appendDiagnostic(`key:${command.key}`);
    break;
  }
});

document.getElementById("world-canvas").addEventListener(
  "wheel",
  (event) => {
    event.preventDefault();
    if (lastSnapshot?.view?.mode === "pov") return;
    // Native accumulator semantics: line scrolls count notches directly,
    // pixel scrolls (touchpads) count ~40px per notch.
    scrollAccum += event.deltaMode === 1 ? -event.deltaY : -event.deltaY / 40;
    let next = zoom;
    while (scrollAccum >= 1) {
      next = Math.min(next * 2, MAX_ZOOM);
      scrollAccum -= 1;
    }
    while (scrollAccum <= -1) {
      next = Math.max(next / 2, 1);
      scrollAccum += 1;
    }
    if (next !== zoom) {
      zoom = next;
      write("zoom", `x${zoom}`);
      if (lastMapFrame) drawCpuMap(lastMapFrame.header, lastMapFrame.pixels);
    }
  },
  { passive: false },
);

window.addEventListener("keyup", (event) => {
  sprintHeld = event.shiftKey;
  heldMoves.delete(event.code);
  povHeld.delete(event.code);
});

// Held keys must not survive focus loss (alt-tab mid-move).
window.addEventListener("blur", () => {
  heldMoves.clear();
  povHeld.clear();
  sprintHeld = false;
});

document.getElementById("world-canvas").addEventListener("pointermove", (event) => {
  const canvas = event.currentTarget;
  if (!mapViewport || !lastSnapshot) {
    write("cursor", "none");
    return;
  }
  // Invert the draw transform: canvas point -> map pixel -> world position
  // (8 world units per composed pixel, player at the window center).
  const rect = canvas.getBoundingClientRect();
  const px = ((event.clientX - rect.left) / rect.width) * canvas.width;
  const py = ((event.clientY - rect.top) / rect.height) * canvas.height;
  // Inverting the crop makes picking zoom-invariant, like the native
  // `pixel_to_world`.
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
  const REGION_SIZE = 256;
  const res = mapViewport.resolution;
  const cell = REGION_SIZE / res;
  const [rx, ry] = lastSnapshot.region;
  const half = (mapViewport.width / res - 1) / 2;
  const west = (rx - half) * REGION_SIZE;
  const north = (ry + half + 1) * REGION_SIZE;
  const wx = west + mx * cell;
  const wy = north - my * cell;
  write("cursor", `${Math.round(wx)}, ${Math.round(wy)}`);
  inspectCursor(wx, wy);
});

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

drawBootCanvas();
const webgpuAvailable = probeWebGpu();
initWorkerProbe();
await initWasm();
if (webgpuAvailable) {
  dispatchCommand("renderer:webgpu");
}
await initStorage();
initBenchmark();
