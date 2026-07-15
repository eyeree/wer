// Native overlay information-panel entry (wry-overlay plan M2): identical
// shared ui/panel-dock.js binder, fed by pushed PanelDocuments instead of a
// polled wasm facade. The dock's fetchDocument contract stays synchronous:
// pushes stage the latest document and force an immediate refresh.
import { createIpcBridge } from "../assets/bridge-ipc.js";
import { createPanelDock } from "../assets/ui/panel-dock.js";
import { createDiagnosticsLog } from "../assets/ui/diagnostics.js";
import { installKeyForwarding } from "../assets/ui/keys.js";

const diagnostic = createDiagnosticsLog(() =>
  document.querySelector('[data-platform-field="diagnostics"]'),
);
const bridge = createIpcBridge();

let latestDocument = null;
const dock = createPanelDock({
  fetchDocument: () => latestDocument,
  diagnostic,
  ready: () => latestDocument !== null,
});

// Native twins of the browser probes so automation can characterize either
// shell with one schema (plan M6 grows this into wer://api/characterization).
window.__panelStatus = dock.status;
window.__refreshPanelForTest = dock.refresh;
window.__panelCharacterization = dock.characterization;

bridge.onPush((message) => {
  if (message.kind === "panel-document") {
    latestDocument = message.document;
    dock.requestRefresh(true);
  } else if (message.kind === "diagnostic") {
    diagnostic(message.message);
  }
});

installKeyForwarding({ keyEvent: bridge.keyEvent, requestFrame: () => {} });
bridge.announceReady("panel");
