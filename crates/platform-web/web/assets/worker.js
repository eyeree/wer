self.onmessage = (event) => {
  const message = event.data ?? {};
  if (message.kind === "ping") {
    self.postMessage({ kind: "pong", mode: message.mode ?? "workers" });
    return;
  }
  if (message.kind === "cancel") {
    self.postMessage({ kind: "cancelled", generation: message.generation ?? 0 });
    return;
  }
  self.postMessage({ kind: "ignored" });
};
