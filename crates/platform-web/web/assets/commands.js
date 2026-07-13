export const COMMANDS = [
  {
    id: "channel:composite",
    key: "V",
    label: "Composite channel",
    group: "Map",
  },
  {
    id: "toggle:compose",
    key: ",",
    label: "Toggle GPU compose",
    group: "Renderer",
  },
  {
    id: "toggle:refinement",
    key: ".",
    label: "Toggle refinement",
    group: "Renderer",
  },
  {
    id: "tier",
    key: "",
    label: "Resource tier override",
    group: "Runtime",
  },
  {
    id: "renderer:webgpu",
    key: "",
    label: "Use WebGPU atlas",
    group: "Renderer",
  },
  {
    id: "renderer:cpu",
    key: "",
    label: "Use CPU map fallback",
    group: "Renderer",
  },
  {
    id: "renderer:device-lost",
    key: "",
    label: "Handle WebGPU device loss",
    group: "Renderer",
  },
  {
    id: "worker",
    key: "",
    label: "Worker mode override",
    group: "Runtime",
  },
  {
    id: "worker:inline",
    key: "",
    label: "Inline execution",
    group: "Runtime",
  },
  {
    id: "worker:workers",
    key: "",
    label: "Worker pool",
    group: "Runtime",
  },
  {
    id: "worker:shared",
    key: "",
    label: "Shared-memory worker pool",
    group: "Runtime",
  },
  {
    id: "worker:cancel-storm",
    key: "",
    label: "Cancel superseded worker jobs",
    group: "Runtime",
  },
  {
    id: "storage:enable",
    key: "",
    label: "Enable IndexedDB storage",
    group: "Storage",
  },
  {
    id: "storage:disable",
    key: "",
    label: "Disable browser storage",
    group: "Storage",
  },
  {
    id: "storage:save",
    key: "",
    label: "Save session",
    group: "Storage",
  },
  {
    id: "storage:reload",
    key: "",
    label: "Reload session",
    group: "Storage",
  },
  {
    id: "storage:export",
    key: "",
    label: "Export atlas bundle",
    group: "Storage",
  },
  {
    id: "storage:import",
    key: "",
    label: "Import atlas bundle",
    group: "Storage",
  },
  {
    id: "storage:reset",
    key: "",
    label: "Reset local vault",
    group: "Storage",
  },
];

export const commandById = new Map(COMMANDS.map((command) => [command.id, command]));
