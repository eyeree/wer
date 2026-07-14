import { execFileSync } from "node:child_process";

const [url] = process.argv.slice(2);
if (!url) {
  throw new Error("usage: node assert-layout.mjs <url>");
}

const session = `wer-layout-assert-${process.pid}`;
const cases = [
  { width: 1280, height: 720, dpr: 1, contract: "bounded-desktop" },
  { width: 900, height: 700, dpr: 1, contract: "bounded-desktop" },
  { width: 700, height: 700, dpr: 1, contract: "stacked-scroll" },
  { width: 900, height: 700, dpr: 2, contract: "bounded-desktop" },
  { width: 901, height: 701, dpr: 1.25, contract: "bounded-desktop" },
];

const agent = (args, capture = false) =>
  execFileSync("agent-browser", ["--session", session, ...args], {
    cwd: process.cwd(),
    encoding: "utf8",
    stdio: capture ? ["ignore", "pipe", "pipe"] : ["ignore", "ignore", "inherit"],
  });

const evaluate = (expression) => {
  const raw = agent(["--json", "eval", expression], true);
  const response = JSON.parse(raw);
  if (!response.success) throw new Error(`agent-browser evaluation failed: ${raw}`);
  return response.data?.result;
};

const assert = (condition, message) => {
  if (!condition) throw new Error(message);
};

const closeEnough = (actual, expected, tolerance = 0.001) =>
  Math.abs(actual - expected) <= tolerance;

let centerWorld;
let previousResizeGeneration = 0;
try {
  for (const [index, requested] of cases.entries()) {
    agent([
      "set",
      "viewport",
      String(requested.width),
      String(requested.height),
      String(requested.dpr),
    ]);
    if (index === 0) agent(["open", url]);
    agent([
      "wait",
      "--fn",
      `document.body.dataset.originFeatureHash !== undefined && window.__mapStatus?.presented === true && window.__mapStatus.resize_generation > ${previousResizeGeneration} && window.__mapStatus.resize_redraw_generation === window.__mapStatus.resize_generation && window.__viewerCharacterization().canvases.map.backing.width === Math.round(window.__viewerCharacterization().boxes.canvasHost.width * window.devicePixelRatio) && window.__viewerCharacterization().canvases.map.backing.height === Math.round(window.__viewerCharacterization().boxes.canvasHost.height * window.devicePixelRatio)`,
    ]);

    const measured = evaluate(`(() => {
      const characterization = window.__viewerCharacterization();
      const map = characterization.canvases.mapViewport;
      const center = map
        ? JSON.parse(window.__werApp.map_world_at(map.dx + map.dw / 2, map.dy + map.dh / 2))
        : null;
      const outside = map
        ? JSON.parse(window.__werApp.map_world_at(map.dx - 0.01, map.dy + map.dh / 2))
        : null;
      return { characterization, center, outside, mapStatus: window.__mapStatus };
    })()`);
    const { characterization: layout, center, outside, mapStatus } = measured;
    previousResizeGeneration = mapStatus.resize_generation;
    const name = `${requested.width}x${requested.height}@${requested.dpr}`;
    assert(layout.viewport.layoutContract === requested.contract, `${name}: wrong layout contract`);
    assert(closeEnough(layout.viewport.dpr, requested.dpr), `${name}: DPR did not apply`);

    const host = layout.boxes.canvasHost;
    const backing = layout.canvases.map.backing;
    assert(
      backing.width === Math.round(host.width * requested.dpr) &&
        backing.height === Math.round(host.height * requested.dpr),
      `${name}: backing ${backing.width}x${backing.height} does not match CSS×DPR`,
    );
    assert(
      layout.canvases.pov.backing.width === backing.width &&
        layout.canvases.pov.backing.height === backing.height,
      `${name}: CPU and GPU stage backings diverged`,
    );

    const map = layout.canvases.mapViewport;
    assert(map && map.dw === map.dh, `${name}: physical map destination is not square`);
    assert(
      map.dx >= 0 && map.dy >= 0 && map.dx + map.dw <= backing.width && map.dy + map.dh <= backing.height,
      `${name}: fitted map escaped its backing surface`,
    );
    assert(center !== null && outside === null, `${name}: shared physical pick bounds failed`);
    if (centerWorld === undefined) centerWorld = center;
    assert(
      closeEnough(center[0], centerWorld[0], 1e-8) &&
        closeEnough(center[1], centerWorld[1], 1e-8),
      `${name}: resize/DPR changed the map center pick`,
    );
    assert(mapStatus.presented, `${name}: resized map was not presented`);
    assert(
      mapStatus.resize_redraw_generation === mapStatus.resize_generation,
      `${name}: resize did not independently redraw the new backing store`,
    );

    const document = layout.document;
    if (requested.contract === "bounded-desktop") {
      assert(
        document.scrollWidth === document.clientWidth &&
          document.scrollHeight === document.clientHeight,
        `${name}: desktop body overflowed the viewport`,
      );
      for (const [boxName, box] of Object.entries(layout.boxes)) {
        assert(
          box.right <= document.clientWidth + 0.001 && box.bottom <= document.clientHeight + 0.001,
          `${name}: ${boxName} escaped the bounded desktop viewport`,
        );
      }
    } else {
      assert(document.scrollWidth === document.clientWidth, `${name}: narrow layout overflowed horizontally`);
      assert(document.scrollHeight > document.clientHeight, `${name}: narrow stack did not expose page scroll`);
      assert(
        layout.boxes.infoPanel.bottom <= document.scrollHeight + 0.001,
        `${name}: narrow information panel is unreachable`,
      );
    }
  }
} finally {
  try {
    agent(["close"]);
  } catch {
    // Preserve the first assertion/automation failure.
  }
}

console.log(`browser layout assertions ok: ${cases.length} cases`);
