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
];

export const commandById = new Map(COMMANDS.map((command) => [command.id, command]));
