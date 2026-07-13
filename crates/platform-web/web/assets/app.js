const fields = new Map(
  Array.from(document.querySelectorAll("[data-field]"), (node) => [node.dataset.field, node]),
);

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

const probeWebGpu = () => {
  if ("gpu" in navigator) {
    write("webgpu-status", "WebGPU available", "ok");
    appendDiagnostic("WebGPU: available");
  } else {
    write("webgpu-status", "WebGPU unavailable", "warn");
    appendDiagnostic("WebGPU: unavailable; CPU/static fallback active");
  }
};

const initWasm = async () => {
  try {
    const mod = await import("../generated/platform_web.js");
    await mod.default();
    const hash = mod.origin_feature_hash();
    const hex = `0x${hash.toString(16).padStart(16, "0")}`;
    write("wasm-status", "wasm loaded", "ok");
    write("origin-hash", `origin ${hex}`, "ok");
    appendDiagnostic(`origin_feature_hash=${hex}`);
    document.body.dataset.originFeatureHash = hex;
  } catch (error) {
    write("wasm-status", "wasm failed", "err");
    write("origin-hash", "origin hash unavailable", "err");
    appendDiagnostic(`wasm initialization failed: ${String(error)}`);
    throw error;
  }
};

for (const control of document.querySelectorAll("[data-command]")) {
  control.addEventListener("click", () => {
    appendDiagnostic(`command:${control.dataset.command}`);
  });
  control.addEventListener("change", () => {
    if (control.dataset.command === "tier") {
      write("tier", control.value);
      appendDiagnostic(`tier:${control.value}`);
    }
  });
}

drawBootCanvas();
probeWebGpu();
await initWasm();
