// The wasm side of the shared-UI bridge seam. Shared ui/ modules talk to the
// viewer runtime only through this callback bundle; the native overlay pages
// substitute bridge-ipc.js with the same shape (wry IPC + wer:// protocol)
// while the UI code stays identical. Returns `null` from queries while the
// runtime is not ready so the UI can stay quiet instead of guessing.
export const createWasmBridge = ({ perf, requestFrame, diagnostic }) => {
  const finiteTelemetry = (value) => (Number.isFinite(value) ? value : 0);

  const dispatch = (id, value = "") => {
    const app = window.__werApp;
    if (!app) {
      diagnostic(`action-dropped (wasm not ready): ${id}`);
      return;
    }
    try {
      app.action(id, value === "" ? undefined : `${value}`);
      diagnostic(`action:${id}${value === "" ? "" : `=${value}`}`);
      requestFrame();
    } catch (error) {
      diagnostic(`action-rejected:${id}:${String(error)}`);
    }
  };

  const keyEvent = (code, pressed, repeat, modifiers) => {
    const app = window.__werApp;
    if (!app) return null;
    return app.key_event(
      code,
      pressed,
      repeat,
      modifiers.shift,
      modifiers.control,
      modifiers.alt,
      modifiers.superKey,
    );
  };

  const fetchPanelDocument = (domUpdates) => {
    const app = window.__werApp;
    if (!app) return null;
    return JSON.parse(
      app.panel_document(
        perf.fps,
        finiteTelemetry(perf.updateMs),
        finiteTelemetry(perf.composeMs),
        finiteTelemetry(perf.presentMs),
        finiteTelemetry(perf.uploadKib),
        domUpdates,
      ),
    );
  };

  return { dispatch, keyEvent, fetchPanelDocument, ready: () => !!window.__werApp };
};
