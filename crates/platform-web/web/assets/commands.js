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
];

export const commandById = new Map(COMMANDS.map((command) => [command.id, command]));
