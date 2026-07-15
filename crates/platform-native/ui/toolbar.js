// Native overlay control-panel entry (wry-overlay plan M2): identical shared
// ui/toolbar.js wiring, IPC bridge instead of the wasm facade. Descriptor
// tables and presentation state arrive as pushes from the shell after the
// ready announcement.
import { createIpcBridge } from "../assets/bridge-ipc.js";
import { createToolbar } from "../assets/ui/toolbar.js";
import { installKeyForwarding } from "../assets/ui/keys.js";

const bridge = createIpcBridge();
const toolbar = createToolbar({ dispatch: bridge.dispatch });

bridge.onPush((message) => {
  if (message.kind === "descriptors") {
    toolbar.installMapControls(message.map);
    const registered = new Set(message.actions.map((descriptor) => descriptor.id));
    toolbar.disableUnregisteredControls(registered, () => {});
  } else if (message.kind === "presentation") {
    toolbar.syncControls(message.presentation);
  }
});

installKeyForwarding({ keyEvent: bridge.keyEvent, requestFrame: () => {} });
bridge.announceReady("toolbar");
