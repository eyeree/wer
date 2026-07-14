const rows = document.querySelector("[data-generated-help]");
const status = document.querySelector("[data-help-status]");

const installHelp = (descriptors) => {
  if (!rows) throw new Error("help page is missing its generated action host");
  rows.replaceChildren();
  for (const descriptor of descriptors) {
    const row = document.createElement("tr");
    row.dataset.helpAction = descriptor.id;

    const action = document.createElement("td");
    const label = document.createElement("strong");
    label.textContent = descriptor.label;
    const help = document.createElement("span");
    help.textContent = descriptor.help;
    action.append(label, document.createElement("br"), help);

    const bindings = document.createElement("td");
    bindings.textContent =
      descriptor.bindings.length > 0
        ? descriptor.bindings.map((binding) => binding.help).join(" ")
        : "No default physical binding.";

    const scope = document.createElement("td");
    scope.textContent = descriptor.scope.replaceAll("-", " ");
    row.append(action, bindings, scope);
    rows.append(row);
  }
};

try {
  const mod = await import("../generated/platform_web.js");
  await mod.default();
  const descriptors = JSON.parse(mod.viewer_action_descriptors());
  installHelp(descriptors);
  document.body.dataset.helpReady = "true";
  if (status) status.textContent = `${descriptors.length} actions from the shared registry.`;
} catch (error) {
  document.body.dataset.helpReady = "false";
  if (status) status.textContent = `Control metadata unavailable: ${String(error)}`;
  throw error;
}
