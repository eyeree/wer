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
const panelColumnIds = ["explorer", "inspection", "world", "ecology", "system"];

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
      layout.panel.columns.length === panelColumnIds.length &&
        layout.panel.columns.map((column) => column.id).join(",") === panelColumnIds.join(",") &&
        layout.panel.columns.every((column) => column.connected && column.box.width > 0),
      `${name}: shared document is not mounted in five stable top-level panels`,
    );
    assert(
      layout.panel.sectionHosts.hover === "inspection" &&
        layout.panel.sectionHosts.ecology === "ecology",
      `${name}: Inspection or Ecology was not promoted into its dedicated host`,
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
        layout.panel.gridTemplateColumns.trim().split(/\s+/).length ===
          panelColumnIds.length + layout.panel.resizers.length,
        `${name}: desktop panel grid did not resolve to five panels plus dividers`,
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
      assert(layout.panel.resizers.length === columns.length - 1, `${name}: wrong divider count`);
      for (const [index, resizer] of layout.panel.resizers.entries()) {
        assert(
          resizer.display !== "none" &&
            resizer.orientation === "vertical" &&
            resizer.minimum <= resizer.value &&
            resizer.value <= resizer.maximum &&
            resizer.box.width > 0 &&
            columns[index].box.right <= resizer.box.x + 0.001 &&
            resizer.box.right <= columns[index + 1].box.x + 0.001,
          `${name}: divider ${index} is not between its adjacent panels`,
        );
      }
      const rowResizer = layout.panel.rowResizer;
      assert(
        rowResizer !== null &&
          rowResizer.display !== "none" &&
          rowResizer.orientation === "horizontal" &&
          rowResizer.minimum <= rowResizer.value &&
          rowResizer.value <= rowResizer.maximum &&
          rowResizer.box.height > 0 &&
          layout.boxes.viewer.bottom <= rowResizer.box.y + 0.001 &&
          rowResizer.box.bottom <= layout.boxes.infoPanel.y + 0.001,
        `${name}: information-panel divider is not between viewer and dock`,
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
        layout.panel.resizers.length === columns.length - 1 &&
          layout.panel.resizers.every((resizer) => resizer.display === "none"),
        `${name}: narrow layout did not hide every panel divider`,
      );
      assert(
        layout.panel.rowResizer?.display === "none",
        `${name}: narrow layout did not hide the information-panel divider`,
      );
      for (let index = 0; index + 1 < columns.length; index += 1) {
        assert(
          columns[index].box.bottom <= columns[index + 1].box.y + 0.001,
          `${name}: narrow panel ${columns[index].id} overlaps ${columns[index + 1].id}`,
        );
      }
    }
  }

  const panelMetrics = () =>
    evaluate(`(() => {
      const panel = window.__viewerCharacterization().panel;
      const widths = panel.columns.map((column) => column.box.width);
      const total = widths.reduce((sum, width) => sum + width, 0);
      return {
        widths,
        ratios: widths.map((width) => width / total),
        shares: panel.shares,
        resizers: panel.resizers,
        generation: window.__mapStatus.resize_generation,
      };
    })()`);
  const beforePanelDrag = panelMetrics();
  const firstDivider = beforePanelDrag.resizers[0].box;
  const dividerPoint = {
    x: Math.round(firstDivider.x + firstDivider.width / 2),
    y: Math.round(firstDivider.y + firstDivider.height / 2),
  };
  agent(["mouse", "move", String(dividerPoint.x), String(dividerPoint.y)]);
  agent(["mouse", "down", "left"]);
  agent(["mouse", "move", String(dividerPoint.x + 24), String(dividerPoint.y)]);
  agent(["mouse", "up", "left"]);
  agent(["wait", "--fn", "window.__viewerCharacterization().panel.shares[0] > 1"]);
  const afterPanelDrag = panelMetrics();
  assert(
    afterPanelDrag.widths[0] > beforePanelDrag.widths[0] + 20 &&
      afterPanelDrag.widths[1] < beforePanelDrag.widths[1] - 20 &&
      closeEnough(
        afterPanelDrag.widths[0] + afterPanelDrag.widths[1],
        beforePanelDrag.widths[0] + beforePanelDrag.widths[1],
        0.01,
      ) &&
      afterPanelDrag.widths
        .slice(2)
        .every((width, index) => closeEnough(width, beforePanelDrag.widths[index + 2], 0.01)),
    `dragging a divider did not resize only its adjacent panels: ${JSON.stringify({ beforePanelDrag, afterPanelDrag })}`,
  );

  agent(["set", "viewport", "1100", "740", "1"]);
  agent([
    "wait",
    "--fn",
    `window.__mapStatus.resize_generation > ${afterPanelDrag.generation} && window.__mapStatus.resize_redraw_generation === window.__mapStatus.resize_generation`,
  ]);
  const afterDesktopResize = panelMetrics();
  assert(
    afterDesktopResize.ratios.every((ratio, index) =>
      closeEnough(ratio, afterPanelDrag.ratios[index], 0.001),
    ),
    `desktop resize changed the user-selected panel ratios: ${JSON.stringify({ afterPanelDrag, afterDesktopResize })}`,
  );

  agent(["set", "viewport", "700", "700", "1"]);
  agent([
    "wait",
    "--fn",
    `window.__mapStatus.resize_generation > ${afterDesktopResize.generation} && window.__viewerCharacterization().viewport.layoutContract === "stacked-scroll"`,
  ]);
  const narrowPanelShares = panelMetrics();
  assert(
    narrowPanelShares.resizers.every((resizer) => resizer.display === "none") &&
      narrowPanelShares.shares.every((share, index) =>
        closeEnough(share, afterPanelDrag.shares[index], 1e-9),
      ),
    `narrow layout discarded the user-selected panel shares: ${JSON.stringify(narrowPanelShares)}`,
  );

  agent(["set", "viewport", "1100", "740", "1"]);
  agent([
    "wait",
    "--fn",
    `window.__mapStatus.resize_generation > ${narrowPanelShares.generation} && window.__mapStatus.resize_redraw_generation === window.__mapStatus.resize_generation`,
  ]);
  const restoredPanelRatios = panelMetrics();
  assert(
    restoredPanelRatios.ratios.every((ratio, index) =>
      closeEnough(ratio, afterPanelDrag.ratios[index], 0.001),
    ),
    `returning from narrow layout did not restore panel ratios: ${JSON.stringify({ afterPanelDrag, restoredPanelRatios })}`,
  );

  const hoverBaseline = evaluate(`(() => {
    const view = window.__viewerCharacterization();
    const canvas = document.getElementById("world-canvas");
    const box = canvas.getBoundingClientRect();
    const map = view.canvases.mapViewport;
    return {
      point: {
        x: Math.round(box.left + ((map.dx + map.dw / 2) / canvas.width) * box.width),
        y: Math.round(box.top + ((map.dy + map.dh / 2) / canvas.height) * box.height),
      },
      invalidPoint: map.dx > 1
        ? {
            x: Math.round(box.left + ((map.dx / 2) / canvas.width) * box.width),
            y: Math.round(box.top + ((map.dy + map.dh / 2) / canvas.height) * box.height),
          }
        : {
            x: Math.round(box.left + ((map.dx + map.dw / 2) / canvas.width) * box.width),
            y: Math.round(box.top + ((map.dy / 2) / canvas.height) * box.height),
          },
      invalidWorld: JSON.parse(
        window.__werApp.map_world_at(
          map.dx > 1 ? map.dx / 2 : map.dx + map.dw / 2,
          map.dx > 1 ? map.dy + map.dh / 2 : map.dy / 2,
        ),
      ),
      scrollHeights: Object.fromEntries(
        view.panel.columns.map((column) => [column.id, column.scrollHeight]),
      ),
    };
  })()`);
  agent(["mouse", "move", String(hoverBaseline.point.x), String(hoverBaseline.point.y)]);
  agent([
    "wait",
    "--fn",
    `document.querySelector('[data-panel-field-row="hover.terrain.status"]')?.hidden === false && document.querySelector('[data-panel-field-row="ecology.roster-size"]')?.hidden === false`,
  ]);
  const hoverExpanded = evaluate(`(() => {
    const view = window.__viewerCharacterization();
    const inspection = view.panel.columns.find((column) => column.id === "inspection");
    const fieldValue = (id) => document.querySelector(
      '[data-panel-field="' + id + '"]',
    )?.textContent;
    const grid = document.querySelector('[data-panel-section="hover"] dl');
    const rows = Array.from(grid.children)
      .filter((row) => !row.hidden && row.dataset.span !== "wide")
      .map((row) => {
        const term = row.querySelector("dt").getBoundingClientRect();
        const description = row.querySelector("dd").getBoundingClientRect();
        return {
          id: row.dataset.panelFieldRow,
          termX: term.x,
          termTop: term.top,
          descriptionX: description.x,
          descriptionTop: description.top,
        };
      });
    return {
      scrollHeights: Object.fromEntries(
        view.panel.columns.map((column) => [column.id, column.scrollHeight]),
      ),
      inspectionPoint: {
        x: Math.round(inspection.box.x + inspection.box.width / 2),
        y: Math.round(inspection.box.y + 24),
      },
      gridTracks: getComputedStyle(grid).gridTemplateColumns.trim().split(/\\s+/),
      rows,
      signature: {
        kind: fieldValue("hover.kind"),
        world: fieldValue("hover.terrain.world"),
        status: fieldValue("hover.terrain.status"),
        ecology: fieldValue("ecology.roster-size"),
      },
    };
  })()`);
  assert(
    hoverBaseline.invalidWorld === null &&
      hoverExpanded.scrollHeights.explorer === hoverBaseline.scrollHeights.explorer &&
      hoverExpanded.scrollHeights.world === hoverBaseline.scrollHeights.world &&
      hoverExpanded.scrollHeights.inspection >= hoverBaseline.scrollHeights.inspection &&
      hoverExpanded.signature.kind !== "none",
    `hover content changed a non-hover panel scroll extent: ${JSON.stringify({ hoverBaseline, hoverExpanded })}`,
  );
  const labelOrigins = hoverExpanded.rows
    .map((row) => row.termX)
    .filter((origin, index, origins) =>
      origins.findIndex((candidate) => closeEnough(candidate, origin, 1)) === index,
    )
    .sort((left, right) => left - right);
  const laneCounts = labelOrigins.map(
    (origin) => hoverExpanded.rows.filter((row) => closeEnough(row.termX, origin, 1)).length,
  );
  assert(
    hoverExpanded.gridTracks.length === 4 &&
      hoverExpanded.rows.length > 1 &&
      labelOrigins.length === 2 &&
      Math.abs(laneCounts[0] - laneCounts[1]) <= 1 &&
      hoverExpanded.rows.every(
        (row) =>
          row.descriptionX > row.termX && closeEnough(row.termTop, row.descriptionTop, 1),
      ) &&
      hoverExpanded.rows.every(
        (row, index, rows) =>
          index % 2 === 1 ||
          index + 1 >= rows.length ||
          (rows[index + 1].termX > row.descriptionX &&
            closeEnough(rows[index + 1].termTop, row.termTop, 1)),
      ),
    `Inspection did not distribute visible fields evenly across two label/value columns: ${JSON.stringify({ tracks: hoverExpanded.gridTracks, rows: hoverExpanded.rows, laneCounts })}`,
  );
  agent([
    "mouse",
    "move",
    String(hoverBaseline.invalidPoint.x),
    String(hoverBaseline.invalidPoint.y),
  ]);
  agent([
    "mouse",
    "move",
    String(hoverExpanded.inspectionPoint.x),
    String(hoverExpanded.inspectionPoint.y),
  ]);
  // The pointer transition is physical; target the following scroll at the
  // panel so this harness does not depend on agent-browser's process-local
  // low-level wheel coordinates.
  agent([
    "scroll",
    "down",
    "600",
    "--selector",
    '[data-panel-column="inspection"]',
  ]);
  agent([
    "wait",
    "--fn",
    `(() => { const inspection = document.querySelector('[data-panel-column="inspection"]'); return (inspection.scrollHeight - inspection.clientHeight <= 1 || inspection.scrollTop > 0) && document.querySelector('[data-panel-field="hover.kind"]')?.textContent !== "none"; })()`,
  ]);
  const retainedHover = evaluate(`(() => {
    const inspection = document.querySelector('[data-panel-column="inspection"]');
    const fieldValue = (id) => document.querySelector(
      '[data-panel-field="' + id + '"]',
    )?.textContent;
    return {
      signature: {
        kind: fieldValue("hover.kind"),
        world: fieldValue("hover.terrain.world"),
        status: fieldValue("hover.terrain.status"),
        ecology: fieldValue("ecology.roster-size"),
      },
      terrainVisible:
        document.querySelector('[data-panel-field-row="hover.terrain.status"]')?.hidden === false,
      ecologyVisible:
        document.querySelector('[data-panel-field-row="ecology.roster-size"]')?.hidden === false,
      scrollTop: inspection.scrollTop,
      maxScroll: inspection.scrollHeight - inspection.clientHeight,
    };
  })()`);
  assert(
    JSON.stringify(retainedHover.signature) === JSON.stringify(hoverExpanded.signature) &&
      retainedHover.terrainVisible &&
      retainedHover.ecologyVisible &&
      (retainedHover.maxScroll <= 1 || retainedHover.scrollTop > 0),
    `moving into and scrolling Inspection cleared the last hover: ${JSON.stringify(retainedHover)}`,
  );

  const dockMetrics = () =>
    evaluate(`(() => {
      const view = window.__viewerCharacterization();
      const viewerHeight = view.boxes.viewer.height;
      const panelHeight = view.boxes.infoPanel.height;
      return {
        viewer: view.boxes.viewer,
        panel: view.boxes.infoPanel,
        resizer: view.panel.rowResizer,
        canvasHost: view.boxes.canvasHost,
        ratio: panelHeight / (viewerHeight + panelHeight),
        rowShares: view.panel.rowShares,
        columnShares: view.panel.shares,
        columnWidths: view.panel.columns.map((column) => column.box.width),
        mapBacking: view.canvases.map.backing,
        povBacking: view.canvases.pov.backing,
        generation: window.__mapStatus.resize_generation,
        redrawGeneration: window.__mapStatus.resize_redraw_generation,
        dpr: window.devicePixelRatio,
      };
    })()`);
  const beforeDockDrag = dockMetrics();
  const dockDividerPoint = {
    x: Math.round(beforeDockDrag.resizer.box.x + beforeDockDrag.resizer.box.width / 2),
    y: Math.round(beforeDockDrag.resizer.box.y + beforeDockDrag.resizer.box.height / 2),
  };
  agent(["mouse", "move", String(dockDividerPoint.x), String(dockDividerPoint.y)]);
  agent(["mouse", "down", "left"]);
  agent(["mouse", "move", String(dockDividerPoint.x), String(dockDividerPoint.y - 48)]);
  agent(["mouse", "up", "left"]);
  agent([
    "wait",
    "--fn",
    `window.__viewerCharacterization().panel.rowShares[0] < ${beforeDockDrag.rowShares[0]} && window.__mapStatus.resize_generation > ${beforeDockDrag.generation} && window.__mapStatus.resize_redraw_generation === window.__mapStatus.resize_generation`,
  ]);
  const afterDockDrag = dockMetrics();
  assert(
    afterDockDrag.viewer.height < beforeDockDrag.viewer.height - 40 &&
      afterDockDrag.panel.height > beforeDockDrag.panel.height + 40 &&
      closeEnough(
        afterDockDrag.viewer.height + afterDockDrag.panel.height,
        beforeDockDrag.viewer.height + beforeDockDrag.panel.height,
        1,
      ) &&
      afterDockDrag.canvasHost.height < beforeDockDrag.canvasHost.height - 40 &&
      afterDockDrag.mapBacking.height < beforeDockDrag.mapBacking.height - 40 * afterDockDrag.dpr &&
      afterDockDrag.mapBacking.height ===
        Math.round(afterDockDrag.canvasHost.height * afterDockDrag.dpr) &&
      afterDockDrag.mapBacking.width ===
        Math.round(afterDockDrag.canvasHost.width * afterDockDrag.dpr) &&
      JSON.stringify(afterDockDrag.mapBacking) === JSON.stringify(afterDockDrag.povBacking) &&
      afterDockDrag.columnShares.every((share, index) =>
        closeEnough(share, beforeDockDrag.columnShares[index], 1e-9),
      ) &&
      afterDockDrag.columnWidths.every((width, index) =>
        closeEnough(width, beforeDockDrag.columnWidths[index], 0.01),
      ),
    `dragging the information divider did not transfer space only from the viewer: ${JSON.stringify({ beforeDockDrag, afterDockDrag })}`,
  );

  agent(["set", "viewport", "1100", "840", "1"]);
  agent([
    "wait",
    "--fn",
    `window.__mapStatus.resize_generation > ${afterDockDrag.generation} && window.__mapStatus.resize_redraw_generation === window.__mapStatus.resize_generation && window.__viewerCharacterization().canvases.map.backing.height === Math.round(window.__viewerCharacterization().boxes.canvasHost.height * window.devicePixelRatio)`,
  ]);
  const afterDockWindowResize = dockMetrics();
  assert(
    closeEnough(afterDockWindowResize.ratio, afterDockDrag.ratio, 0.002) &&
      afterDockWindowResize.rowShares.every((share, index) =>
        closeEnough(share, afterDockDrag.rowShares[index], 1e-9),
      ) &&
      afterDockWindowResize.columnShares.every((share, index) =>
        closeEnough(share, afterDockDrag.columnShares[index], 1e-9),
      ) &&
      JSON.stringify(afterDockWindowResize.mapBacking) ===
        JSON.stringify(afterDockWindowResize.povBacking),
    `window resize did not preserve the viewer/information ratio: ${JSON.stringify({ afterDockDrag, afterDockWindowResize })}`,
  );

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

  // Exercise the production DOM adapter with real CDP pointer/keyboard input
  // plus a DOM WheelEvent (agent-browser 0.26 does not target canvas wheel
  // events). Direct facade calls above isolate reducer invariants; these events
  // prove canvas focus, DPR conversion, wheel, capture, and RAF scheduling.
  evaluate(`(() => {
    for (let index = 0; index < 8; index += 1) window.__werApp.action("zoom-out");
    window.dispatchEvent(new KeyboardEvent("keydown", { code: "F24" }));
    return true;
  })()`);
  agent(["wait", "--fn", "window.__viewerCharacterization().renderer.zoom === 1"]);
  const mapInputPoint = evaluate(`(() => {
    const view = window.__viewerCharacterization();
    const canvas = document.getElementById("world-canvas");
    const box = canvas.getBoundingClientRect();
    const map = view.canvases.mapViewport;
    return {
      x: Math.round(box.left + ((map.dx + map.dw / 2) / canvas.width) * box.width),
      y: Math.round(box.top + ((map.dy + map.dh / 2) / canvas.height) * box.height),
      physical: [map.dx + map.dw / 2, map.dy + map.dh / 2],
      center: JSON.parse(window.__werApp.map_world_at(map.dx + map.dw / 2, map.dy + map.dh / 2)),
      serial: window.__mapStatus.update_serial,
    };
  })()`);
  agent(["mouse", "move", String(mapInputPoint.x), String(mapInputPoint.y)]);
  agent(["mouse", "down", "left"]);
  agent(["mouse", "up", "left"]);
  evaluate(`document.getElementById("world-canvas").dispatchEvent(
    new WheelEvent("wheel", { clientX: ${mapInputPoint.x}, clientY: ${mapInputPoint.y}, deltaY: -80, deltaMode: WheelEvent.DOM_DELTA_PIXEL, bubbles: true, cancelable: true })
  )`);
  agent(["wait", "--fn", "window.__viewerCharacterization().renderer.zoom === 4"]);
  const zoom4 = evaluate(`(() => {
    const view = window.__viewerCharacterization();
    const map = view.canvases.mapViewport;
    return {
      center: JSON.parse(window.__werApp.map_world_at(map.dx + map.dw / 2, map.dy + map.dh / 2)),
      serial: window.__mapStatus.update_serial,
      zoom: view.renderer.zoom,
    };
  })()`);
  evaluate(`document.getElementById("world-canvas").dispatchEvent(
    new WheelEvent("wheel", { clientX: ${mapInputPoint.x}, clientY: ${mapInputPoint.y}, deltaY: -80, deltaMode: WheelEvent.DOM_DELTA_PIXEL, bubbles: true, cancelable: true })
  )`);
  agent(["wait", "--fn", "window.__viewerCharacterization().renderer.zoom === 16"]);
  const zoom16 = evaluate(`(() => {
    const view = window.__viewerCharacterization();
    const map = view.canvases.mapViewport;
    return {
      center: JSON.parse(window.__werApp.map_world_at(map.dx + map.dw / 2, map.dy + map.dh / 2)),
      serial: window.__mapStatus.update_serial,
      zoom: view.renderer.zoom,
    };
  })()`);
  assert(
    zoom4.zoom === 4 &&
      zoom16.zoom === 16 &&
      zoom4.serial > mapInputPoint.serial &&
      zoom16.serial > zoom4.serial &&
      closeEnough(zoom4.center[0], mapInputPoint.center[0], 1e-8) &&
      closeEnough(zoom4.center[1], mapInputPoint.center[1], 1e-8) &&
      closeEnough(zoom16.center[0], mapInputPoint.center[0], 1e-8) &&
      closeEnough(zoom16.center[1], mapInputPoint.center[1], 1e-8),
    `real Map wheel input did not preserve the 1/4/16 center: ${JSON.stringify({ mapInputPoint, zoom4, zoom16 })}`,
  );

  evaluate(`(() => {
    window.__werApp.renderer_available();
    return true;
  })()`);
  agent(["click", 'button[data-action="set-presentation"][data-value="split"]']);
  agent([
    "wait",
    "--fn",
    'window.__viewerCharacterization().renderer.viewMode === "split" && document.getElementById("pov-canvas").hidden === false',
  ]);
  const povInputPoint = evaluate(`(() => {
    const view = window.__viewerCharacterization();
    const canvas = document.getElementById("pov-canvas");
    const box = canvas.getBoundingClientRect();
    const pane = view.canvases.sharedLayout.pov_pane;
    return {
      x: Math.round(box.left + ((pane[0] + pane[2] / 2) / canvas.width) * box.width),
      y: Math.round(box.top + ((pane[1] + pane[3] / 2) / canvas.height) * box.height),
    };
  })()`);
  const beforeHover = evaluate(`({
    serial: window.__mapStatus.update_serial,
    orientation: window.__povStatus.orientation,
  })`);
  agent(["mouse", "move", String(povInputPoint.x), String(povInputPoint.y)]);
  agent(["mouse", "move", String(povInputPoint.x + 24), String(povInputPoint.y + 12)]);
  agent(["wait", "--fn", `window.__mapStatus.update_serial > ${beforeHover.serial}`]);
  const afterHover = evaluate(`({
    serial: window.__mapStatus.update_serial,
    orientation: window.__povStatus.orientation,
  })`);
  agent(["mouse", "down", "left"]);
  agent(["mouse", "up", "left"]);
  agent(["wait", "--fn", `window.__mapStatus.update_serial > ${afterHover.serial}`]);
  const afterClick = evaluate(`({
    serial: window.__mapStatus.update_serial,
    orientation: window.__povStatus.orientation,
    focused: window.__viewerCharacterization().renderer.focusedView,
  })`);
  agent(["mouse", "down", "left"]);
  agent(["mouse", "move", String(povInputPoint.x + 70), String(povInputPoint.y + 42)]);
  agent(["mouse", "up", "left"]);
  agent(["wait", "--fn", `window.__mapStatus.update_serial > ${afterClick.serial}`]);
  const afterDrag = evaluate(`({
    serial: window.__mapStatus.update_serial,
    orientation: window.__povStatus.orientation,
    focused: window.__viewerCharacterization().renderer.focusedView,
  })`);
  assert(
    JSON.stringify(beforeHover.orientation) === JSON.stringify(afterHover.orientation) &&
      JSON.stringify(afterHover.orientation) === JSON.stringify(afterClick.orientation) &&
      JSON.stringify(afterClick.orientation) !== JSON.stringify(afterDrag.orientation) &&
      afterClick.focused === "pov" &&
      afterDrag.focused === "pov",
    `real POV pointer input bypassed or missed the primary-hold gate: ${JSON.stringify({ beforeHover, afterHover, afterClick, afterDrag })}`,
  );
  agent(["press", "Tab"]);
  agent([
    "wait",
    "--fn",
    'window.__viewerCharacterization().renderer.viewMode === "split" && window.__viewerCharacterization().renderer.focusedView === "map"',
  ]);

  const helpUrl = new URL("help/", url).href;
  agent(["open", helpUrl]);
  agent(["wait", "--fn", 'document.body.dataset.helpReady === "true"']);
  const generatedHelp = evaluate(`(() => {
    const rows = Array.from(document.querySelectorAll("[data-generated-help] [data-help-action]"));
    const ids = rows.map((row) => row.dataset.helpAction);
    return {
      ready: document.body.dataset.helpReady,
      rows: rows.length,
      unique: new Set(ids).size,
      allComplete: rows.every(
        (row) =>
          row.children.length === 3 &&
          row.children[0].textContent.trim().length > 0 &&
          row.children[1].textContent.trim().length > 0 &&
          ["global", "focused view", "map", "pov"].includes(row.children[2].textContent.trim()),
      ),
      status: document.querySelector("[data-help-status]")?.textContent ?? "",
    };
  })()`);
  assert(
    generatedHelp.ready === "true" &&
      generatedHelp.rows > 0 &&
      generatedHelp.unique === generatedHelp.rows &&
      generatedHelp.allComplete &&
      generatedHelp.status.includes(`${generatedHelp.rows} actions`),
    `browser help did not render the shared action/binding registry: ${JSON.stringify(generatedHelp)}`,
  );
} finally {
  try {
    agent(["close"]);
  } catch {
    // Preserve the first assertion/automation failure.
  }
}

console.log(`browser layout assertions ok: ${cases.length} cases`);
