import { access, readFile } from "node:fs/promises";
import { join } from "node:path";

const dist = process.argv[2] ?? "target/web-dist";
const required = [
  "index.html",
  "help/index.html",
  "docs/world-model.html",
  "assets/app.css",
  "assets/app.js",
  "assets/benchmark.js",
  "assets/storage.js",
  "assets/worker.js",
  "assets/manifest.json",
  "assert-layout.mjs",
  "baselines/native-web-alignment-m0-layout.json",
  "generated/platform_web.js",
  "generated/platform_web_bg.wasm",
];

for (const path of required) {
  await access(join(dist, path));
}

const html = await readFile(join(dist, "index.html"), "utf8");
for (const url of ["./assets/app.css", "./assets/app.js", "./docs/world-model.html", "./help/"]) {
  if (!html.includes(url)) {
    throw new Error(`index.html does not contain relative URL ${url}`);
  }
}

if (html.includes("data-field=")) {
  throw new Error("index.html retains the pre-M6 untyped panel field registry");
}
const panelColumns = Array.from(
  html.matchAll(/data-panel-column="([^"]+)"/g),
  (match) => match[1],
);
if (
  panelColumns.length !== 3 ||
  panelColumns.some((column, index) => column !== ["explorer", "world", "system"][index])
) {
  throw new Error("index.html must mount exactly the explorer/world/system panel columns");
}
if (!html.includes("data-panel-document") || !html.includes("data-platform-field=\"diagnostics\"")) {
  throw new Error("index.html does not separate the shared panel from platform diagnostics");
}

const app = await readFile(join(dist, "assets/app.js"), "utf8");
if (/https?:\/\//.test(app)) {
  throw new Error("app.js contains an external network URL");
}
if (!app.includes("origin_feature_hash")) {
  throw new Error("app.js does not call the origin feature hash parity export");
}
if (!app.includes("new mod.WebApp")) {
  throw new Error("app.js does not construct the WebApp facade");
}
if (!app.includes("app.frame(dt, now / 1000)")) {
  throw new Error("app.js does not drive the single shared frame facade");
}
if (app.includes("app.pov_frame(") || app.includes("app.update(")) {
  throw new Error("app.js contains a second mode-specific logical frame path");
}
if (!app.includes("render_cpu_map")) {
  throw new Error("app.js does not render the CPU map buffer");
}
if (!app.includes("new ResizeObserver") || !app.includes("window.devicePixelRatio")) {
  throw new Error("app.js does not derive physical canvas size from CSS and DPR");
}
if (!app.includes("app.resize_surface(width, height)")) {
  throw new Error("app.js does not send the physical content rectangle through shared layout");
}
if (!app.includes("app.panel_document(") || !app.includes("applyPanelDocument")) {
  throw new Error("app.js does not bind the shared typed panel document");
}
if (!app.includes("PANEL_REFRESH_MS = 500") || !app.includes("requestPanelRefresh")) {
  throw new Error("app.js does not cap normal panel refreshes at 500ms");
}
if (
  !app.includes("window.__refreshPanelForTest = refreshPanel") ||
  !app.includes('fieldId === "performance.dom-updates"')
) {
  throw new Error("app.js cannot prove a settled, observer-effect-free panel cadence");
}
for (const property of ["field.value", "field.severity", "field.visible"]) {
  if (!app.includes(property)) {
    throw new Error(`app.js does not incrementally bind shared ${property}`);
  }
}
if (!app.includes("panelFields = new Map()") || !app.includes("dataset.panelField")) {
  throw new Error("app.js does not preserve stable DOM nodes by panel field id");
}
if (!app.includes("app.map_hover(wx, wy)") || !app.includes("app.clear_hover()")) {
  throw new Error("app.js does not route hover inspection through the shared sampler");
}
for (const legacy of [
  "app.info_snapshot(",
  "frame.snapshot",
  "updatePanelStats",
  "app.inspect(",
  "app.map_organism_at(",
  "const megabytes",
  "const millis",
]) {
  if (app.includes(legacy)) {
    throw new Error(`app.js retains legacy panel path ${legacy}`);
  }
}
if (app.includes("panelRoot.replaceChildren") || app.includes("panelRoot.innerHTML")) {
  throw new Error("app.js rebuilds the shared panel instead of updating stable nodes");
}
if (/canvas[^>]+(?:width|height)="\d+"/.test(html)) {
  throw new Error("index.html keeps a fixed canvas backing-size authority");
}
if (app.includes("Math.min(canvas.width /")) {
  throw new Error("app.js independently reconstructs the shared map fit");
}
if (!app.includes("app.map_descriptors()") || !app.includes("installMapControls")) {
  throw new Error("app.js does not build map controls from shared descriptors");
}
for (const generated of ["map-channels", "map-overlays"]) {
  if (!html.includes(`data-generated="${generated}"`)) {
    throw new Error(`index.html does not expose generated ${generated} controls`);
  }
}
if (app.includes("MAP_CHANNELS") || app.includes("paint_region") || app.includes("compose_map")) {
  throw new Error("browser assets contain a second map implementation or channel registry");
}
if (!app.includes("gpu_submitted") || !app.includes("presented: mapPresented")) {
  throw new Error("browser map status does not distinguish GPU submission from presentation");
}
if (!app.includes("renderer_available")) {
  throw new Error("app.js does not report WebGPU renderer availability");
}
if (!app.includes("new Worker")) {
  throw new Error("app.js does not initialize the worker probe");
}
if (!app.includes("openVault")) {
  throw new Error("app.js does not initialize browser storage");
}
if (!app.includes("storage_status(state.mode, state.failures)")) {
  throw new Error("app.js does not inject browser storage capability into the shared panel");
}
if (!app.includes("runStartupBenchmark")) {
  throw new Error("app.js does not run startup benchmark");
}
if (!html.includes('data-action="set-presentation" data-value="pov"')) {
  throw new Error("index.html does not expose POV mode control");
}
if (!html.includes('data-action="set-presentation" data-value="split"')) {
  throw new Error("index.html does not expose Split mode control");
}
if (!html.includes("<summary>Exploration</summary>")) {
  throw new Error("index.html does not expose the grouped Exploration controls");
}
for (const control of [
  "capture-anchor",
  "cycle-capture-category",
  "toggle-capture-polarity",
  "drop-anchor",
  "toggle-transition-mode",
  "clear-anchors",
]) {
  if (!html.includes(`data-action="${control}"`)) {
    throw new Error(`index.html does not expose exploration control ${control}`);
  }
}
for (const control of ["toggle-walk", "toggle-pov-shadow-ao", "toggle-pov-detail-normals", "toggle-pov-water", "set-pov-render-scale"]) {
  if (!html.includes(`data-action="${control}"`)) {
    throw new Error(`index.html does not expose POV control ${control}`);
  }
}
if (app.includes("MOVE_KEYS") || app.includes("POV_MOVE")) {
  throw new Error("app.js contains a second hand-written binding table");
}
if (app.includes("requestPointerLock") || app.includes("pointerlockchange")) {
  throw new Error("app.js enables pointer-lock free look");
}
if (!app.includes("event.code") || !app.includes("event.repeat")) {
  throw new Error("app.js does not forward KeyboardEvent.code/repeat");
}
if (!app.includes("setPointerCapture") || !app.includes("pointercancel")) {
  throw new Error("app.js does not transport primary drag cancellation");
}

const css = await readFile(join(dist, "assets/app.css"), "utf8");
if (!css.includes("grid-template-columns: repeat(3, minmax(0, 1fr))")) {
  throw new Error("app.css does not define the three-column desktop information dock");
}
if (!css.includes(".panel-column") || !css.includes("overflow-y: auto")) {
  throw new Error("app.css does not give each desktop panel column bounded scrolling");
}
if (!css.includes("@media (max-width: 760px)") || !css.includes("stacked-scroll")) {
  // `stacked-scroll` remains the M0/M1 named narrow contract in app.js.
  if (!app.includes('layoutContract: window.innerWidth <= 760 ? "stacked-scroll"')) {
    throw new Error("browser assets do not retain the explicit narrow scrolling contract");
  }
}

const docs = await readFile(join(dist, "docs/world-model.html"), "utf8");
for (const heading of ["World Model", "Possibility", "Terrain"]) {
  if (!docs.includes(heading)) {
    throw new Error(`generated world-model docs missing expected text ${heading}`);
  }
}

const help = await readFile(join(dist, "help/index.html"), "utf8");
for (const match of html.matchAll(/data-action="([^"]+)"/g)) {
  if (!help.includes(`data-help-action="${match[1]}"`)) {
    throw new Error(`help page missing action ${match[1]}`);
  }
}

JSON.parse(await readFile(join(dist, "assets/manifest.json"), "utf8"));
const layout = JSON.parse(
  await readFile(join(dist, "baselines/native-web-alignment-m0-layout.json"), "utf8"),
);
if (layout.schema !== "native-web-alignment-layout-characterization-v1") {
  throw new Error("layout characterization has an unknown schema");
}
if (layout.gpuPixelsCaptured !== false) {
  throw new Error("layout characterization must not contain GPU pixel captures");
}
const expectedLayoutCases = ["1280x720@1", "900x700@1", "700x700@1"];
if (
  layout.cases.length !== expectedLayoutCases.length ||
  layout.cases.some((entry, index) => entry.name !== expectedLayoutCases[index])
) {
  throw new Error("layout characterization does not contain the required viewport matrix");
}
for (const entry of layout.cases) {
  const measured = entry.measured;
  if (
    !measured?.viewport ||
    !measured?.document ||
    !measured?.canvases?.map?.css ||
    !measured?.canvases?.map?.backing ||
    !measured?.boxes?.canvasHost ||
    !measured?.boxes?.infoPanel ||
    !measured?.renderer?.mode
  ) {
    throw new Error(`layout characterization case ${entry.name} is incomplete`);
  }
}
const generatedJs = await readFile(join(dist, "generated/platform_web.js"), "utf8");
if (generatedJs.includes("crates/platform-web/web")) {
  throw new Error("generated wasm glue contains a source-tree asset path");
}
console.log(`web smoke ok: ${dist}`);
