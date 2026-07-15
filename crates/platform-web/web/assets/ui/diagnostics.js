// Bounded platform diagnostics log. Shared UI module: the browser shell and
// the native overlay pages append to the same kind of `<pre>` host, and the
// log keeps a bounded tail so the DOM (and the panel layout) never grows with
// session length. Newest entries win.
export const MAX_DIAGNOSTIC_LINES = 100;

export const createDiagnosticsLog = (resolveNode) => (message) => {
  const node = resolveNode();
  if (!node) return;
  const lines = `${node.textContent}\n${message}`.trim().split("\n");
  node.textContent = lines.slice(-MAX_DIAGNOSTIC_LINES).join("\n");
  node.scrollTop = node.scrollHeight;
};
