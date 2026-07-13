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
];

export const commandById = new Map(COMMANDS.map((command) => [command.id, command]));
