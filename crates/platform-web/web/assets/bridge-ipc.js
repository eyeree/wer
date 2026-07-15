// The native side of the shared-UI bridge seam (wry-overlay plan M2). Same
// callback-bundle shape as bridge-wasm.js, but the runtime lives across a wry
// IPC boundary: upward messages go through window.ipc.postMessage, downward
// state arrives as pushes evaluated by the native shell into
// window.__werOverlayPush. Queries the wasm bridge answers synchronously are
// answered here from the latest pushed state, so shared ui/ modules stay
// synchronous and identical on both shells.
export const createIpcBridge = () => {
  const post = (message) => window.ipc?.postMessage(JSON.stringify(message));
  const handlers = new Set();
  window.__werOverlayPush = (message) => {
    for (const handler of handlers) handler(message);
  };

  return {
    dispatch: (id, value = "") => post({ t: "action", id, value: `${value}` }),
    // Fire-and-forget: `undefined` (not `null`) tells ui/keys.js the runtime
    // is reachable but the handled verdict is unknown, so no preventDefault.
    keyEvent: (code, pressed, repeat, modifiers) => {
      post({
        t: "key",
        code,
        pressed,
        repeat,
        shift: modifiers.shift,
        control: modifiers.control,
        alt: modifiers.alt,
        super_key: modifiers.superKey,
      });
      return undefined;
    },
    onPush: (handler) => handlers.add(handler),
    announceReady: (pane) => post({ t: "ready", pane }),
  };
};
