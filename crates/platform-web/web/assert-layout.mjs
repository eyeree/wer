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
  { width: 901, height: 701, dpr: 1, contract: "bounded-desktop" },
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
      `document.body.dataset.originFeatureHash !== undefined && window.__mapStatus?.presented === true && window.__mapStatus.resize_generation > ${previousResizeGeneration} && window.__mapStatus.resize_redraw_generation === window.__mapStatus.resize_generation && window.__viewerCharacterization().panel.busy === "false" && window.__viewerCharacterization().panel.sections > 0 && window.__viewerCharacterization().canvases.map.backing.width === Math.round(window.__viewerCharacterization().boxes.canvasHost.width * window.devicePixelRatio) && window.__viewerCharacterization().canvases.map.backing.height === Math.round(window.__viewerCharacterization().boxes.canvasHost.height * window.devicePixelRatio)`,
    ]);

    const measured = evaluate(`(() => {
      const characterization = window.__viewerCharacterization();
      const panelRoot = document.querySelector("[data-panel-document]");
      const panelField = document.querySelector("[data-panel-field]");
      window.__layoutPanelRoot ||= panelRoot;
      window.__layoutPanelField ||= panelField;
      const map = characterization.canvases.mapViewport;
      const center = map
        ? JSON.parse(window.__werApp.map_world_at(map.dx + map.dw / 2, map.dy + map.dh / 2))
        : null;
      const outside = map
        ? JSON.parse(window.__werApp.map_world_at(map.dx - 0.01, map.dy + map.dh / 2))
        : null;
      return {
        characterization,
        center,
        outside,
        mapStatus: window.__mapStatus,
        panelIdentity: {
          rootStable: window.__layoutPanelRoot === panelRoot,
          fieldStable: window.__layoutPanelField === panelField,
          connected: panelRoot?.isConnected ?? false,
          hidden: panelRoot?.hidden ?? true,
        },
        clippedPanelText: Array.from(
          document.querySelectorAll("[data-panel-field-row]:not([hidden]) dt, [data-panel-field-row]:not([hidden]) dd"),
        ).filter((node) => node.scrollWidth > node.clientWidth + 1).map((node) => node.textContent),
      };
    })()`);
    const { characterization: layout, center, outside, mapStatus, panelIdentity, clippedPanelText } = measured;
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

    assert(
      panelIdentity.rootStable && panelIdentity.fieldStable && panelIdentity.connected && !panelIdentity.hidden,
      `${name}: information panel or a stable field node was rebuilt/unmounted`,
    );
    assert(
      clippedPanelText.length === 0,
      `${name}: panel labels/values clipped horizontally: ${JSON.stringify(clippedPanelText)}`,
    );
    assert(
      layout.panel.columns.length === 3 &&
        layout.panel.columns.map((column) => column.id).join(",") === "explorer,world,system" &&
        layout.panel.columns.every((column) => column.connected && column.box.width > 0),
      `${name}: shared document is not mounted in exactly three stable columns`,
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
      const columns = layout.panel.columns;
      assert(
        layout.panel.gridTemplateColumns.trim().split(/\s+/).length === 3,
        `${name}: desktop panel grid did not resolve to three columns`,
      );
      assert(
        columns.every(
          (column) =>
            column.overflowY === "auto" &&
            column.box.bottom <= layout.boxes.infoPanel.bottom + 0.001 &&
            column.clientHeight <= layout.boxes.infoPanel.height,
        ),
        `${name}: desktop panel columns are not independently bounded and scrollable`,
      );
      assert(
        columns[0].box.right <= columns[1].box.x + 0.001 &&
          columns[1].box.right <= columns[2].box.x + 0.001,
        `${name}: desktop panel columns overlap`,
      );
    } else {
      assert(document.scrollWidth === document.clientWidth, `${name}: narrow layout overflowed horizontally`);
      assert(document.scrollHeight > document.clientHeight, `${name}: narrow stack did not expose page scroll`);
      assert(
        layout.boxes.infoPanel.bottom <= document.scrollHeight + 0.001,
        `${name}: narrow information panel is unreachable`,
      );
      const columns = layout.panel.columns;
      assert(
        layout.panel.gridTemplateColumns.trim().split(/\s+/).length === 1 &&
          columns.every((column) => column.overflowY === "visible"),
        `${name}: narrow panel did not become a one-column page-scroll stack`,
      );
      assert(
        columns[0].box.bottom <= columns[1].box.y + 0.001 &&
          columns[1].box.bottom <= columns[2].box.y + 0.001,
        `${name}: narrow panel columns overlap vertically`,
      );
    }
  }

  const cadence = evaluate(`(() => {
    // First synchronize the injected DOM count with the readout. Its own text
    // change is intentionally excluded from that counter, so this settles.
    window.__refreshPanelForTest();
    window.__refreshPanelForTest();
    const field = document.querySelector("[data-panel-field]");
    const mapCounters = () => ({
      updateSerial: window.__mapStatus.update_serial,
      resize: window.__mapStatus.resize_generation,
      redraw: window.__mapStatus.resize_redraw_generation,
      presented: window.__mapStatus.presented,
      uploadBytes: window.__mapStatus.upload_bytes,
    });
    const before = {
      builds: window.__werApp.panel_build_count(),
      panel: window.__panelStatus(),
      map: mapCounters(),
    };
    window.__refreshPanelForTest();
    window.__refreshPanelForTest();
    window.__refreshPanelForTest();
    const after = {
      builds: window.__werApp.panel_build_count(),
      panel: window.__panelStatus(),
      map: mapCounters(),
      fieldStable: field === document.querySelector("[data-panel-field]"),
    };
    return { before, after };
  })()`);
  assert(
    cadence.after.builds === cadence.before.builds,
    "unchanged panel refreshes rebuilt the shared Rust document",
  );
  assert(
    cadence.after.panel.domUpdates === cadence.before.panel.domUpdates &&
      cadence.after.fieldStable,
    "unchanged panel refreshes mutated or replaced DOM fields",
  );
  assert(
    JSON.stringify(cadence.after.map) === JSON.stringify(cadence.before.map),
    "panel refresh changed map update/resize/redraw/presentation counters",
  );

  const modeMount = evaluate(`(async () => {
    const panel = document.querySelector("[data-panel-document]");
    const field = document.querySelector("[data-panel-field]");
    const results = [];
    const telemetry = () => ({
      compose: document.querySelector('[data-panel-field="performance.compose"]')?.textContent,
      present: document.querySelector('[data-panel-field="performance.present"]')?.textContent,
      upload: document.querySelector('[data-panel-field="performance.upload"]')?.textContent,
    });
    // Force one observable CPU upload before the mode transition. This makes
    // the stale-value regression deterministic rather than relying on boot
    // timing or the final idle Map frame.
    window.__werApp.action("zoom-in");
    // A direct wasm action deliberately does not own the RAF scheduler. Send
    // one unbound DOM key to exercise the production adapter's scheduling
    // path, then sample immediately after that single dirty frame.
    window.dispatchEvent(new KeyboardEvent("keydown", { code: "F24" }));
    await new Promise((resolve) => requestAnimationFrame(resolve));
    window.__refreshPanelForTest();
    const cpuTelemetry = telemetry();
    // The headless browser has no adapter. Inject only the shared capability
    // notification so the controller can exercise POV/Split state while the
    // renderer continues taking its tested fallback path.
    window.__werApp.renderer_available();
    for (const mode of ["map", "pov", "split", "map"]) {
      document.querySelector('button[data-action="set-presentation"][data-value="' + mode + '"]')?.click();
      await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      window.__refreshPanelForTest();
      results.push({ requested: mode, actual: document.querySelector('[data-panel-field="view.mode"]')?.textContent, panel: panel === document.querySelector("[data-panel-document]"), field: field === document.querySelector("[data-panel-field]"), connected: panel.isConnected, hidden: panel.hidden, telemetry: telemetry() });
    }
    return { cpuTelemetry, results };
  })()`);
  assert(
    modeMount.results.every(
      (entry) => entry.actual === entry.requested && entry.panel && entry.field && entry.connected && !entry.hidden,
    ),
    `Map/POV/Split transitions did not enter their requested mode or preserve the shared panel: ${JSON.stringify(modeMount)}`,
  );
  const povMount = modeMount.results.find((entry) => entry.requested === "pov");
  assert(
    modeMount.cpuTelemetry.upload !== "0 KiB/f" &&
      povMount?.telemetry.compose === "0.00 ms" &&
      povMount?.telemetry.present === "0.00 ms" &&
      povMount?.telemetry.upload === "0 KiB/f",
    `POV performance telemetry retained stale CPU-map values: ${JSON.stringify(modeMount)}`,
  );

  const splitAcceptance = evaluate(`(async () => {
    const app = window.__werApp;
    const panel = document.querySelector("[data-panel-document]");
    const stableField = document.querySelector("[data-panel-field]");
    const frame = (dt = 0) => JSON.parse(app.frame(dt, performance.now() / 1000));
    const nextPresentedCpuMap = async (minimumSerial) => {
      window.dispatchEvent(new KeyboardEvent("keydown", { code: "F24" }));
      for (let attempt = 0; attempt < 30; attempt += 1) {
        await new Promise((resolve) => requestAnimationFrame(resolve));
        if (
          window.__mapStatus?.update_serial >= minimumSerial &&
          window.__mapStatus?.path === "cpu" &&
          window.__mapStatus?.presented === true
        ) {
          return true;
        }
      }
      return false;
    };

    // Headless Chrome has no adapter. This explicit production hook opens
    // only the typed capability gate; frame status must remain truthful that
    // no renderer slot was attempted or presented.
    app.renderer_available();
    app.action("set-presentation", "split");
    const split = frame();
    const layout = split.layout;
    const map = layout.map_pane;
    const pov = layout.pov_pane;
    const mapPoint = [map[0] + map[2] / 2, map[1] + map[3] / 2];
    const povPoint = [pov[0] + pov[2] / 2, pov[1] + pov[3] / 2];
    const hitRouting = {
      map: app.view_at(mapPoint[0], mapPoint[1]),
      beforeSeam: app.view_at(pov[0] - 0.001, povPoint[1]),
      seam: app.view_at(pov[0], povPoint[1]),
      outside: app.view_at(layout.content[0] + layout.content[2], povPoint[1]),
    };

    app.surface_focus(true);
    const povPress = app.pointer_button(91, 0, true, povPoint[0], povPoint[1], hitRouting.seam);
    const povRelease = app.pointer_button(91, 0, false, povPoint[0], povPoint[1], hitRouting.seam);
    const clicked = frame();

    app.surface_focus(false);
    const toolbarTabHandled = app.key_event("Tab", true, false, false, false, false, false);
    const toolbarTab = frame();
    app.key_event("Tab", false, false, false, false, false, false);

    app.surface_focus(true);
    const surfaceTabHandled = app.key_event("Tab", true, false, false, false, false, false);
    const surfaceTab = frame();
    app.key_event("Tab", false, false, false, false, false, false);

    const cameraBeforeTravel = surfaceTab.pov.camera;
    const movementHandled = app.key_event("KeyW", true, false, false, false, false, false);
    const traveled = frame(100);
    app.key_event("KeyW", false, false, false, false, false, false);

    const cameraBeforeLoss = traveled.pov.camera;
    app.renderer_lost();
    const lost = frame();

    // Direct facade frames intentionally do not mutate canvases or DOM. Mark
    // the production Map path dirty, then let the ordinary RAF adapter draw
    // and bind the post-loss panel once.
    app.action("zoom-in");
    app.action("zoom-out");
    const cpuPresented = await nextPresentedCpuMap(lost.update_serial + 1);
    window.__refreshPanelForTest();
    const characterization = window.__viewerCharacterization();
    const traveler = [
      document.querySelector('[data-panel-field="traveler.x"]')?.textContent,
      document.querySelector('[data-panel-field="traveler.y"]')?.textContent,
    ];

    return {
      split,
      layout,
      hitRouting,
      focus: {
        povPress,
        povRelease,
        clicked,
        toolbarTabHandled,
        toolbarTab,
        surfaceTabHandled,
        surfaceTab,
      },
      travel: {
        movementHandled,
        cameraBeforeTravel,
        traveled,
      },
      fallback: {
        cameraBeforeLoss,
        lost,
        cpuPresented,
        mapStatus: window.__mapStatus,
        rendererStatus: window.__rendererFrameStatus,
        characterization,
        panelMode: document.querySelector('[data-panel-field="view.mode"]')?.textContent,
        panelFocus: document.querySelector('[data-panel-field="view.focus"]')?.textContent,
        traveler,
        warning: document.querySelector('[data-panel-field="warnings.renderer-device-loss"]')?.textContent,
        panelStable:
          panel === document.querySelector("[data-panel-document]") &&
          stableField === document.querySelector("[data-panel-field]") &&
          panel.isConnected &&
          !panel.hidden,
      },
    };
  })()`);

  const { split, layout, hitRouting, focus, travel, fallback } = splitAcceptance;
  const [contentX, contentY, contentWidth, contentHeight] = layout.content;
  const [mapX, mapY, mapWidth, mapHeight] = layout.map_pane;
  const [povX, povY, povWidth, povHeight] = layout.pov_pane;
  const rectIsContained = ([x, y, width, height]) =>
    x >= contentX &&
    y >= contentY &&
    x + width <= contentX + contentWidth &&
    y + height <= contentY + contentHeight;
  assert(
    split.presentation.view.mode === "split" &&
      split.presentation.view.focused === "map" &&
      split.map.active === true &&
      split.pov.active === true,
    `Split did not expose both panes from one shared state: ${JSON.stringify(splitAcceptance)}`,
  );
  assert(
    closeEnough(layout.split_ratio, 0.5, 1e-9) &&
      mapX === contentX &&
      mapY === contentY &&
      mapHeight === contentHeight &&
      povX === mapX + mapWidth &&
      povY === contentY &&
      povHeight === contentHeight &&
      mapWidth + povWidth === contentWidth &&
      mapWidth === Math.round(contentWidth * 0.5) &&
      rectIsContained(layout.map_pane) &&
      rectIsContained(layout.pov_pane) &&
      rectIsContained(layout.map_content) &&
      layout.map_content[2] === layout.map_content[3],
    `Split geometry is not an exact contained 50/50 partition: ${JSON.stringify(layout)}`,
  );
  assert(
    hitRouting.map === "map" &&
      hitRouting.beforeSeam === "map" &&
      hitRouting.seam === "pov" &&
      hitRouting.outside === undefined,
    `shared half-open Split hit routing failed: ${JSON.stringify(hitRouting)}`,
  );
  assert(
    split.renderer_frame.attempted === false &&
      split.renderer_frame.presented === false &&
      split.map.path === "gpu-cpu" &&
      split.map.drawn === false,
    `headless Split renderer status was not truthful: ${JSON.stringify(split.renderer_frame)}`,
  );

  assert(
    focus.povPress &&
      focus.povRelease &&
      focus.clicked.presentation.view.focused === "pov" &&
      focus.clicked.layout.focused === "pov" &&
      JSON.stringify(focus.clicked.layout.focus_border) ===
        JSON.stringify(focus.clicked.layout.pov_pane),
    `POV pane click did not focus before scoped input: ${JSON.stringify(focus)}`,
  );
  assert(
    focus.clicked.update_serial === split.update_serial + 1 &&
      focus.toolbarTabHandled === false &&
      focus.toolbarTab.update_serial === focus.clicked.update_serial + 1 &&
      focus.toolbarTab.presentation.view.focused === "pov" &&
      focus.surfaceTabHandled === true &&
      focus.surfaceTab.update_serial === focus.toolbarTab.update_serial + 1 &&
      focus.surfaceTab.presentation.view.focused === "map" &&
      JSON.stringify(focus.surfaceTab.layout.focus_border) ===
        JSON.stringify(focus.surfaceTab.layout.map_pane),
    `Tab did not respect surface focus or swap only Split focus: ${JSON.stringify(focus)}`,
  );

  assert(
    travel.movementHandled === true &&
      travel.traveled.update_serial === focus.surfaceTab.update_serial + 1 &&
      closeEnough(travel.traveled.travel, 50, 1e-6) &&
      closeEnough(
        travel.traveled.pov.camera[1] - travel.cameraBeforeTravel[1],
        travel.traveled.travel,
        1e-6,
      ) &&
      travel.traveled.presentation.view.mode === "split" &&
      travel.traveled.map.active === true &&
      travel.traveled.pov.active === true,
    `one Split frame did not produce one serial/travel/camera update: ${JSON.stringify(travel)}`,
  );

  assert(
    fallback.lost.update_serial === travel.traveled.update_serial + 1 &&
      fallback.lost.travel === 0 &&
      fallback.lost.presentation.view.mode === "map" &&
      fallback.lost.presentation.view.focused === "map" &&
      fallback.lost.presentation.view.pov_supported === false &&
      fallback.lost.map.active === true &&
      fallback.lost.map.path === "cpu" &&
      fallback.lost.pov.active === false &&
      fallback.lost.renderer_frame.attempted === false &&
      fallback.lost.renderer_frame.presented === false &&
      JSON.stringify(fallback.lost.pov.camera) === JSON.stringify(fallback.cameraBeforeLoss),
    `renderer loss did not atomically preserve world and reduce to Map focus: ${JSON.stringify(fallback)}`,
  );
  assert(
    fallback.cpuPresented &&
      fallback.mapStatus.path === "cpu" &&
      fallback.mapStatus.presented === true &&
      fallback.rendererStatus.attempted === false &&
      fallback.rendererStatus.presented === false &&
      fallback.characterization.renderer.viewMode === "map" &&
      fallback.characterization.renderer.focusedView === "map" &&
      fallback.panelMode === "map" &&
      fallback.panelFocus === "map" &&
      fallback.panelStable &&
      closeEnough(Number(fallback.traveler[0]), fallback.cameraBeforeLoss[0], 0.001) &&
      closeEnough(Number(fallback.traveler[1]), fallback.cameraBeforeLoss[1], 0.001) &&
      fallback.warning?.includes("device lost"),
    `post-loss CPU Map/panel state did not survive: ${JSON.stringify(fallback)}`,
  );
} finally {
  try {
    agent(["close"]);
  } catch {
    // Preserve the first assertion/automation failure.
  }
}

console.log(`browser layout assertions ok: ${cases.length} cases`);
