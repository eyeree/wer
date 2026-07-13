import { commandById } from "./commands.js";
import { runStartupBenchmark } from "./benchmark.js";
import { exportSnapshot, openVault } from "./storage.js";

const fields = new Map(
  Array.from(document.querySelectorAll("[data-field]"), (node) => [node.dataset.field, node]),
);

let workerProbe;
let lastSnapshot;
// The canvas placement of the last drawn map image (letterboxed, square
// source), so cursor picking inverts the exact draw transform.
let mapViewport;

const write = (name, value, cls) => {
  const node = fields.get(name);
  if (!node) return;
  node.textContent = value;
  if (cls) node.className = cls;
};

const appendDiagnostic = (message) => {
  const node = fields.get("diagnostics");
  if (!node) return;
  node.textContent = `${node.textContent}\n${message}`.trim();
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

// Blit the composed RGBA window onto the canvas, preserving the source's
// square aspect (letterboxed) so regions stay square like the native viewer.
const drawCpuMap = (header, pixels) => {
  const canvas = document.getElementById("world-canvas");
  const ctx = canvas.getContext("2d");
  if (!ctx || header.kind !== "rgba8") return;
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
  const scale = Math.min(canvas.width / header.width, canvas.height / header.height);
  const dw = header.width * scale;
  const dh = header.height * scale;
  const dx = (canvas.width - dw) / 2;
  const dy = (canvas.height - dh) / 2;
  mapViewport = {
    dx,
    dy,
    dw,
    dh,
    width: header.width,
    height: header.height,
    resolution: header.resolution,
  };
  ctx.imageSmoothingEnabled = false;
  ctx.fillStyle = "#0b0d0f";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.drawImage(scratch, dx, dy, dw, dh);
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
  drawCpuMap(JSON.parse(app.render_cpu_map()), app.map_pixels());
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
    "pov:toggle-baked": pov.baked_light,
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
  appendDiagnostic(`settle_hash=${snapshot.settle_hash}`);
};

const dispatchCommand = (id, value) => {
  const app = window.__werApp;
  if (!app) {
    appendDiagnostic(`command-dropped (wasm not ready): ${id}`);
    return;
  }
  const snapshot = JSON.parse(app.apply_command(JSON.stringify({ id, value })));
  updateSnapshot(snapshot);
  renderMap();
};

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
  const mx = ((px - mapViewport.dx) / mapViewport.dw) * mapViewport.width;
  const my = ((py - mapViewport.dy) / mapViewport.dh) * mapViewport.height;
  if (mx < 0 || my < 0 || mx >= mapViewport.width || my >= mapViewport.height) {
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
  write("cursor", `${Math.round(west + mx * cell)}, ${Math.round(north - my * cell)}`);
});

drawBootCanvas();
const webgpuAvailable = probeWebGpu();
initWorkerProbe();
await initWasm();
if (webgpuAvailable) {
  dispatchCommand("renderer:webgpu");
}
await initStorage();
initBenchmark();
