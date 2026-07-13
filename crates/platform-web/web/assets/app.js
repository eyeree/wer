import { commandById } from "./commands.js";
import { exportSnapshot, openVault } from "./storage.js";

const fields = new Map(
  Array.from(document.querySelectorAll("[data-field]"), (node) => [node.dataset.field, node]),
);

let workerProbe;
let lastSnapshot;

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

const drawCpuMap = (map) => {
  const canvas = document.getElementById("world-canvas");
  const ctx = canvas.getContext("2d");
  if (!ctx || map.kind !== "rgba8") return;
  const source = new ImageData(new Uint8ClampedArray(map.pixels), map.width, map.height);
  const scratch = document.createElement("canvas");
  scratch.width = map.width;
  scratch.height = map.height;
  const scratchCtx = scratch.getContext("2d");
  scratchCtx.putImageData(source, 0, 0);
  ctx.imageSmoothingEnabled = false;
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.drawImage(scratch, 0, 0, canvas.width, canvas.height);
};

const probeWebGpu = () => {
  if ("gpu" in navigator) {
    write("webgpu-status", "WebGPU available", "ok");
    appendDiagnostic("WebGPU: available");
    queueMicrotask(() => dispatchCommand("renderer:webgpu"));
  } else {
    write("webgpu-status", "WebGPU unavailable", "warn");
    appendDiagnostic("WebGPU: unavailable; CPU/static fallback active");
  }
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
    renderMap();
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

const renderMap = () => {
  const app = window.__werApp;
  if (!app) return;
  drawCpuMap(JSON.parse(app.render_cpu_map()));
};

const updateSnapshot = (snapshot) => {
  lastSnapshot = snapshot;
  write("region", `${snapshot.region[0]}, ${snapshot.region[1]}`);
  write("tier", snapshot.tier);
  write("executor", `${snapshot.executor.mode} / ${snapshot.executor.parallelism}`);
  write("storage", snapshot.storage.mode);
  write("webgpu-status", `${snapshot.renderer.mode} / refine ${snapshot.renderer.refinement}`);
  appendDiagnostic(`settle_hash=${snapshot.settle_hash}`);
};

const dispatchCommand = (id, value) => {
  const app = window.__werApp;
  if (!app) return;
  const snapshot = JSON.parse(app.apply_command(JSON.stringify({ id, value })));
  updateSnapshot(snapshot);
  renderMap();
};

for (const control of document.querySelectorAll("[data-command]")) {
  if (!commandById.has(control.dataset.command)) {
    appendDiagnostic(`unregistered-command:${control.dataset.command}`);
  }
  control.addEventListener("click", () => {
    appendDiagnostic(`command:${control.dataset.command}`);
    dispatchCommand(control.dataset.command);
  });
  control.addEventListener("change", () => {
    if (control.dataset.command === "tier") {
      write("tier", control.value);
      appendDiagnostic(`tier:${control.value}`);
      dispatchCommand("tier", control.value);
    } else if (control.dataset.command === "worker") {
      appendDiagnostic(`worker:${control.value}`);
      dispatchCommand(`worker:${control.value}`);
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
  for (const command of commandById.values()) {
    if (command.key && event.key === command.key) {
      event.preventDefault();
      dispatchCommand(command.id);
      appendDiagnostic(`key:${command.key}`);
      break;
    }
  }
});

document.getElementById("world-canvas").addEventListener("pointermove", (event) => {
  const canvas = event.currentTarget;
  const rect = canvas.getBoundingClientRect();
  const nx = (event.clientX - rect.left) / rect.width - 0.5;
  const ny = (event.clientY - rect.top) / rect.height - 0.5;
  write("cursor", `${Math.round(nx * 768)}, ${Math.round(ny * 512)}`);
});

drawBootCanvas();
probeWebGpu();
initWorkerProbe();
await initWasm();
await initStorage();
