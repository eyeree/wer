// ---- Shared information document ------------------------------------------
// Rust owns sampling, labels, formatting, severity, and the native three-lane
// grouping. This module promotes the hover and ecology sections into dedicated
// top-level hosts, then mutates only the value/severity/visibility properties
// that actually changed. Normal frame telemetry is capped at 2 Hz; hover
// invalidation is intentionally immediate.
//
// Shared UI module: the shell supplies `fetchDocument(domUpdates)` (wasm
// facade on the web, IPC on the native overlay pages; `null` while the
// runtime is not ready) and a `diagnostic` sink. Everything DOM-side —
// stable field nodes, resizable columns, the viewer/panel row split — is
// identical on both platforms.
const PANEL_REFRESH_MS = 500;

export const createPanelDock = ({ fetchDocument, diagnostic, ready = () => true }) => {
  const panelRoot = document.querySelector("[data-panel-document]");
  const panelLayout = document.querySelector(".panel-columns");
  const appShell = document.querySelector(".app-shell");
  const viewerNode = document.querySelector(".viewer");
  const infoPanelNode = document.querySelector(".info-panel");
  const infoPanelResizerNode = document.querySelector("[data-info-panel-resizer]");
  const panelColumnNodes = Array.from(document.querySelectorAll("[data-panel-column]"));
  const panelColumnHosts = new Map(
    panelColumnNodes.map((node) => [node.dataset.panelColumn, node]),
  );
  const panelResizerNodes = Array.from(document.querySelectorAll("[data-panel-resizer]"));
  const PANEL_SECTION_HOSTS = new Map([
    ["hover", "inspection"],
    ["ecology", "ecology"],
  ]);
  const PANEL_DRAG_MIN_COLUMN_WIDTH = 136;
  const PANEL_KEYBOARD_STEP = 16;
  const INFO_PANEL_DRAG_MIN_VIEWER_HEIGHT = 180;
  const INFO_PANEL_DRAG_MIN_PANEL_HEIGHT = 120;
  const INFO_PANEL_KEYBOARD_STEP = 16;
  // Shares stay unitless: CSS resolves them against each new dock width, so a
  // user-selected proportion survives ordinary and narrow -> desktop resizes.
  const panelShares = panelColumnNodes.map((node) =>
    node.dataset.panelColumn === "inspection" ? 2 : 1,
  );
  // The same fractional contract applies vertically. Resizing the information
  // dock changes only these adjacent rows, leaving the overall shell height and
  // therefore the browser viewport contract unchanged.
  const infoPanelRowShares = [7, 3];
  let activePanelResize = null;
  let activeInfoPanelResize = null;
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

  const safeDomId = (kind, id) => `panel-${kind}-${id.replaceAll(/[^A-Za-z0-9_-]/g, "-")}`;

  const updatePanelResizerValue = (index) => {
    const resizer = panelResizerNodes[index];
    const leftWidth = panelColumnNodes[index]?.getBoundingClientRect().width ?? 0;
    const rightWidth = panelColumnNodes[index + 1]?.getBoundingClientRect().width ?? 0;
    const pairWidth = leftWidth + rightWidth;
    if (!resizer || !(pairWidth > 0)) return;
    const minimumLeft = Math.min(PANEL_DRAG_MIN_COLUMN_WIDTH, leftWidth);
    const minimumRight = Math.min(PANEL_DRAG_MIN_COLUMN_WIDTH, rightWidth);
    const percentage = Math.round((leftWidth / pairWidth) * 100);
    resizer.setAttribute("aria-valuemin", `${Math.floor((minimumLeft / pairWidth) * 100)}`);
    resizer.setAttribute(
      "aria-valuemax",
      `${Math.ceil(((pairWidth - minimumRight) / pairWidth) * 100)}`,
    );
    resizer.setAttribute("aria-valuenow", `${percentage}`);
    resizer.setAttribute(
      "aria-valuetext",
      `${panelColumnNodes[index].ariaLabel} ${percentage}%, ${panelColumnNodes[index + 1].ariaLabel} ${100 - percentage}%`,
    );
  };

  const updatePanelResizerValues = () => {
    for (let index = 0; index < panelResizerNodes.length; index += 1) {
      updatePanelResizerValue(index);
    }
  };

  const applyPanelShares = () => {
    if (!panelLayout) return;
    for (const [index, node] of panelColumnNodes.entries()) {
      panelLayout.style.setProperty(
        `--panel-${node.dataset.panelColumn}-share`,
        `${panelShares[index]}fr`,
      );
    }
    updatePanelResizerValues();
  };

  const resizePanelPair = (
    index,
    requestedLeftWidth,
    pairWidth,
    pairShares,
    minimumLeft,
    minimumRight,
  ) => {
    if (!(pairWidth > 0) || !(pairShares > 0)) return;
    const leftWidth = Math.max(
      minimumLeft,
      Math.min(pairWidth - minimumRight, requestedLeftWidth),
    );
    const leftShare = pairShares * (leftWidth / pairWidth);
    panelShares[index] = leftShare;
    panelShares[index + 1] = pairShares - leftShare;
    applyPanelShares();
  };

  const finishPanelResize = (event) => {
    const resize = activePanelResize;
    if (!resize || (event && event.pointerId !== resize.pointerId)) return;
    activePanelResize = null;
    delete resize.resizer.dataset.dragging;
    document.body.classList.remove("panel-resizing");
    if (resize.resizer.hasPointerCapture(resize.pointerId)) {
      resize.resizer.releasePointerCapture(resize.pointerId);
    }
  };

  const installPanelResizers = () => {
    if (!panelLayout || panelResizerNodes.length !== panelColumnNodes.length - 1) return;
    for (const resizer of panelResizerNodes) {
      const index = Number.parseInt(resizer.dataset.panelResizer ?? "", 10);
      if (!Number.isInteger(index) || index < 0 || index + 1 >= panelColumnNodes.length) {
        throw new Error(`invalid panel resizer index ${resizer.dataset.panelResizer}`);
      }
      resizer.addEventListener("pointerdown", (event) => {
        if (event.button !== 0 || activePanelResize || activeInfoPanelResize) return;
        const leftWidth = panelColumnNodes[index].getBoundingClientRect().width;
        const rightWidth = panelColumnNodes[index + 1].getBoundingClientRect().width;
        activePanelResize = {
          index,
          pointerId: event.pointerId,
          startX: event.clientX,
          startLeftWidth: leftWidth,
          pairWidth: leftWidth + rightWidth,
          pairShares: panelShares[index] + panelShares[index + 1],
          minimumLeft: Math.min(PANEL_DRAG_MIN_COLUMN_WIDTH, leftWidth),
          minimumRight: Math.min(PANEL_DRAG_MIN_COLUMN_WIDTH, rightWidth),
          resizer,
        };
        resizer.dataset.dragging = "true";
        document.body.classList.add("panel-resizing");
        resizer.setPointerCapture(event.pointerId);
        event.preventDefault();
      });
      resizer.addEventListener("pointermove", (event) => {
        const resize = activePanelResize;
        if (!resize || event.pointerId !== resize.pointerId || resize.resizer !== resizer) return;
        resizePanelPair(
          resize.index,
          resize.startLeftWidth + event.clientX - resize.startX,
          resize.pairWidth,
          resize.pairShares,
          resize.minimumLeft,
          resize.minimumRight,
        );
        event.preventDefault();
      });
      resizer.addEventListener("pointerup", finishPanelResize);
      resizer.addEventListener("pointercancel", finishPanelResize);
      resizer.addEventListener("lostpointercapture", finishPanelResize);
      resizer.addEventListener("keydown", (event) => {
        if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
        const leftWidth = panelColumnNodes[index].getBoundingClientRect().width;
        const rightWidth = panelColumnNodes[index + 1].getBoundingClientRect().width;
        const direction = event.key === "ArrowLeft" ? -1 : 1;
        resizePanelPair(
          index,
          leftWidth + direction * PANEL_KEYBOARD_STEP * (event.shiftKey ? 3 : 1),
          leftWidth + rightWidth,
          panelShares[index] + panelShares[index + 1],
          Math.min(PANEL_DRAG_MIN_COLUMN_WIDTH, leftWidth),
          Math.min(PANEL_DRAG_MIN_COLUMN_WIDTH, rightWidth),
        );
        event.preventDefault();
      });
    }
    applyPanelShares();
  };

  installPanelResizers();

  const updateInfoPanelResizerValue = () => {
    if (!infoPanelResizerNode || !viewerNode || !infoPanelNode) return;
    const viewerHeight = viewerNode.getBoundingClientRect().height;
    const panelHeight = infoPanelNode.getBoundingClientRect().height;
    const pairHeight = viewerHeight + panelHeight;
    if (!(pairHeight > 0)) return;
    const minimumViewer = Math.min(INFO_PANEL_DRAG_MIN_VIEWER_HEIGHT, viewerHeight);
    const minimumPanel = Math.min(INFO_PANEL_DRAG_MIN_PANEL_HEIGHT, panelHeight);
    const percentage = Math.round((viewerHeight / pairHeight) * 100);
    infoPanelResizerNode.setAttribute(
      "aria-valuemin",
      `${Math.floor((minimumViewer / pairHeight) * 100)}`,
    );
    infoPanelResizerNode.setAttribute(
      "aria-valuemax",
      `${Math.ceil(((pairHeight - minimumPanel) / pairHeight) * 100)}`,
    );
    infoPanelResizerNode.setAttribute("aria-valuenow", `${percentage}`);
    infoPanelResizerNode.setAttribute(
      "aria-valuetext",
      `Viewer ${percentage}%, information panel ${100 - percentage}%`,
    );
  };

  const applyInfoPanelRowShares = () => {
    if (!appShell) return;
    appShell.style.setProperty("--viewer-row-share", `${infoPanelRowShares[0]}fr`);
    appShell.style.setProperty("--info-panel-row-share", `${infoPanelRowShares[1]}fr`);
    updateInfoPanelResizerValue();
  };

  const resizeInfoPanelRows = (
    requestedViewerHeight,
    pairHeight,
    pairShares,
    minimumViewer,
    minimumPanel,
  ) => {
    if (!(pairHeight > 0) || !(pairShares > 0)) return;
    const viewerHeight = Math.max(
      minimumViewer,
      Math.min(pairHeight - minimumPanel, requestedViewerHeight),
    );
    infoPanelRowShares[0] = pairShares * (viewerHeight / pairHeight);
    infoPanelRowShares[1] = pairShares - infoPanelRowShares[0];
    applyInfoPanelRowShares();
  };

  const finishInfoPanelResize = (event) => {
    const resize = activeInfoPanelResize;
    if (!resize || (event && event.pointerId !== resize.pointerId)) return;
    activeInfoPanelResize = null;
    delete resize.resizer.dataset.dragging;
    document.body.classList.remove("info-panel-resizing");
    if (resize.resizer.hasPointerCapture(resize.pointerId)) {
      resize.resizer.releasePointerCapture(resize.pointerId);
    }
  };

  const installInfoPanelResizer = () => {
    if (!infoPanelResizerNode || !viewerNode || !infoPanelNode) return;
    infoPanelResizerNode.addEventListener("pointerdown", (event) => {
      if (event.button !== 0 || activeInfoPanelResize || activePanelResize) return;
      const viewerHeight = viewerNode.getBoundingClientRect().height;
      const panelHeight = infoPanelNode.getBoundingClientRect().height;
      activeInfoPanelResize = {
        pointerId: event.pointerId,
        startY: event.clientY,
        startViewerHeight: viewerHeight,
        pairHeight: viewerHeight + panelHeight,
        pairShares: infoPanelRowShares[0] + infoPanelRowShares[1],
        minimumViewer: Math.min(INFO_PANEL_DRAG_MIN_VIEWER_HEIGHT, viewerHeight),
        minimumPanel: Math.min(INFO_PANEL_DRAG_MIN_PANEL_HEIGHT, panelHeight),
        resizer: infoPanelResizerNode,
      };
      infoPanelResizerNode.dataset.dragging = "true";
      document.body.classList.add("info-panel-resizing");
      infoPanelResizerNode.setPointerCapture(event.pointerId);
      event.preventDefault();
    });
    infoPanelResizerNode.addEventListener("pointermove", (event) => {
      const resize = activeInfoPanelResize;
      if (!resize || event.pointerId !== resize.pointerId) return;
      resizeInfoPanelRows(
        resize.startViewerHeight + event.clientY - resize.startY,
        resize.pairHeight,
        resize.pairShares,
        resize.minimumViewer,
        resize.minimumPanel,
      );
      event.preventDefault();
    });
    infoPanelResizerNode.addEventListener("pointerup", finishInfoPanelResize);
    infoPanelResizerNode.addEventListener("pointercancel", finishInfoPanelResize);
    infoPanelResizerNode.addEventListener("lostpointercapture", finishInfoPanelResize);
    infoPanelResizerNode.addEventListener("keydown", (event) => {
      if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
      const viewerHeight = viewerNode.getBoundingClientRect().height;
      const panelHeight = infoPanelNode.getBoundingClientRect().height;
      const direction = event.key === "ArrowUp" ? -1 : 1;
      resizeInfoPanelRows(
        viewerHeight + direction * INFO_PANEL_KEYBOARD_STEP * (event.shiftKey ? 3 : 1),
        viewerHeight + panelHeight,
        infoPanelRowShares[0] + infoPanelRowShares[1],
        Math.min(INFO_PANEL_DRAG_MIN_VIEWER_HEIGHT, viewerHeight),
        Math.min(INFO_PANEL_DRAG_MIN_PANEL_HEIGHT, panelHeight),
      );
      event.preventDefault();
    });
    applyInfoPanelRowShares();
  };

  installInfoPanelResizer();
  window.addEventListener("resize", () => {
    window.requestAnimationFrame(() => {
      updatePanelResizerValues();
      updateInfoPanelResizerValue();
    });
  });

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
    const hostName = PANEL_SECTION_HOSTS.get(section.id) ?? section.column;
    const host = panelColumnHosts.get(hostName);
    if (!host) throw new Error(`panel document named unknown host ${hostName}`);

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

  const refreshPanel = () => {
    window.clearTimeout(panelRefreshTimer);
    panelRefreshTimer = 0;
    if (!ready() || document.hidden) return;
    panelLastRefresh = performance.now();
    try {
      const documentModel = fetchDocument(panelDomUpdates);
      if (!documentModel) return;
      applyPanelDocument(documentModel);
      panelRefreshes += 1;
    } catch (error) {
      diagnostic(`panel refresh failed: ${String(error)}`);
    }
  };

  const requestPanelRefresh = (immediate = false) => {
    if (!ready() || document.hidden) return;
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

  const panelStatus = () => ({
    connected: panelRoot?.isConnected ?? false,
    hidden: panelRoot?.hidden ?? true,
    schemaVersion: panelSchemaVersion,
    revision: panelDocumentRevision,
    refreshes: panelRefreshes,
    domUpdates: panelDomUpdates,
    sections: panelSections.size,
    fields: panelFields.size,
  });

  // The `panel` slice of the shared characterization probe: read-only DOM and
  // share-state geometry, never GPU pixels (ADR 0017).
  const panelCharacterization = () => {
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
    const columnNodes = Array.from(document.querySelectorAll("[data-panel-column]"));
    const resizerNodes = Array.from(document.querySelectorAll("[data-panel-resizer]"));
    const columnsStyle = document.querySelector(".panel-columns")
      ? getComputedStyle(document.querySelector(".panel-columns"))
      : null;
    return {
      connected: panelRoot?.isConnected ?? false,
      hidden: panelRoot?.hidden ?? true,
      busy: panelRoot?.getAttribute("aria-busy") ?? null,
      gridTemplateColumns: columnsStyle?.gridTemplateColumns ?? null,
      sections: panelSections.size,
      fields: panelFields.size,
      shares: [...panelShares],
      rowShares: [...infoPanelRowShares],
      rowResizer: infoPanelResizerNode
        ? {
            box: nodeRect(infoPanelResizerNode),
            display: getComputedStyle(infoPanelResizerNode).display,
            orientation: infoPanelResizerNode.getAttribute("aria-orientation"),
            minimum: Number.parseInt(
              infoPanelResizerNode.getAttribute("aria-valuemin") ?? "",
              10,
            ),
            maximum: Number.parseInt(
              infoPanelResizerNode.getAttribute("aria-valuemax") ?? "",
              10,
            ),
            value: Number.parseInt(infoPanelResizerNode.getAttribute("aria-valuenow") ?? "", 10),
          }
        : null,
      sectionHosts: Object.fromEntries(
        ["hover", "ecology"].map((id) => [
          id,
          document.querySelector(`[data-panel-section="${id}"]`)?.parentElement?.dataset
            .panelColumn ?? null,
        ]),
      ),
      columns: columnNodes.map((node) => ({
        id: node.dataset.panelColumn,
        connected: node.isConnected,
        box: nodeRect(node),
        overflowY: getComputedStyle(node).overflowY,
        scrollHeight: node.scrollHeight,
        clientHeight: node.clientHeight,
      })),
      resizers: resizerNodes.map((node) => ({
        index: Number.parseInt(node.dataset.panelResizer ?? "", 10),
        box: nodeRect(node),
        display: getComputedStyle(node).display,
        orientation: node.getAttribute("aria-orientation"),
        minimum: Number.parseInt(node.getAttribute("aria-valuemin") ?? "", 10),
        maximum: Number.parseInt(node.getAttribute("aria-valuemax") ?? "", 10),
        value: Number.parseInt(node.getAttribute("aria-valuenow") ?? "", 10),
      })),
      status: panelStatus(),
    };
  };

  return {
    refresh: refreshPanel,
    requestRefresh: requestPanelRefresh,
    status: panelStatus,
    characterization: panelCharacterization,
  };
};
