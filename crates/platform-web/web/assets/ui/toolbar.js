// Shared control-panel (toolbar) wiring. The DOM supplies containers and
// styling only; action ids, channel/overlay descriptors, labels, groups, and
// order have one authority in viewer_host — the shell passes a `dispatch`
// callback (wasm facade on the web, IPC on the native overlay) and this
// module never talks to a runtime directly.
export const createToolbar = ({ dispatch }) => {
  const toolbar = document.querySelector(".toolbar");

  toolbar?.addEventListener("click", (event) => {
    const control = event.target.closest("button[data-action]");
    if (control) dispatch(control.dataset.action, control.dataset.value ?? "");
  });

  toolbar?.addEventListener("change", (event) => {
    const control = event.target.closest("select[data-action]");
    if (control) dispatch(control.dataset.action, control.value);
  });

  // Build map controls from the Rust descriptor registry. The DOM supplies
  // containers and styling only; channel/overlay ids, labels, groups, and order
  // have one authority in viewer_host::map.
  const installMapControls = ({ channels, overlays }) => {
    const channelSelect = document.querySelector('[data-generated="map-channels"]');
    if (channelSelect) {
      channelSelect.replaceChildren();
      const groups = new Map();
      for (const descriptor of channels) {
        let group = groups.get(descriptor.group);
        if (!group) {
          group = document.createElement("optgroup");
          group.label = descriptor.group_label;
          groups.set(descriptor.group, group);
          channelSelect.append(group);
        }
        const option = document.createElement("option");
        option.value = descriptor.id;
        option.textContent = descriptor.label;
        group.append(option);
      }
    }

    const overlayHost = document.querySelector('[data-generated="map-overlays"]');
    if (overlayHost) {
      overlayHost.replaceChildren();
      const groups = new Map();
      for (const descriptor of overlays) {
        let group = groups.get(descriptor.group);
        if (!group) {
          group = document.createElement("span");
          group.className = "map-control-group";
          group.setAttribute("aria-label", descriptor.group_label);
          groups.set(descriptor.group, group);
          overlayHost.append(group);
        }
        const button = document.createElement("button");
        button.type = "button";
        button.dataset.action = "toggle-overlay";
        button.dataset.value = descriptor.id;
        button.dataset.overlayKey = descriptor.id.replaceAll("-", "_");
        button.setAttribute("aria-pressed", "false");
        button.textContent = descriptor.label;
        group.append(button);
      }
    }
  };

  // Mirror the small serde-built presentation DTO into the toolbar so toggles
  // visibly register: buttons carry pressed state, selects show the mode the
  // runtime is in. Shared action descriptors are the one source of truth
  // (alignment plan §5.2).
  const syncControls = (presentation) => {
    const pov = presentation.view.pov;
    const pressed = {
      "toggle-gpu-compose": presentation.map.backend === "gpu-atlas",
      "toggle-refinement": presentation.map.refinement,
      "toggle-walk": pov.motion === "walk",
      "toggle-pov-shadow-ao": pov.shadow_ao,
      "toggle-pov-detail-normals": pov.detail_normals,
      "toggle-pov-water": pov.water,
    };
    for (const [action, state] of Object.entries(pressed)) {
      const control = document.querySelector(`button[data-action="${action}"]`);
      if (control) control.setAttribute("aria-pressed", String(state));
    }
    for (const control of document.querySelectorAll('button[data-action="toggle-overlay"]')) {
      control.setAttribute(
        "aria-pressed",
        String(presentation.map.overlays[control.dataset.overlayKey]),
      );
    }
    for (const control of document.querySelectorAll('button[data-action="set-presentation"]')) {
      control.setAttribute(
        "aria-pressed",
        String(control.dataset.value === presentation.view.mode),
      );
    }
    const selectValues = {
      "set-map-channel": presentation.map.channel,
      "set-resource-tier": presentation.tier.runtime,
      "set-worker-backend": {
        inline: "inline",
        workers: "workers",
        "shared-memory": "shared-workers",
      }[presentation.executor.mode],
      "set-pov-render-scale": `${pov.render_scale}`,
    };
    for (const [action, value] of Object.entries(selectValues)) {
      const control = document.querySelector(`select[data-action="${action}"]`);
      if (control && value !== undefined) control.value = value;
    }
  };

  // Controls whose action id is not in the shared registry cannot dispatch;
  // disable them and let the shell log the mismatch.
  const disableUnregisteredControls = (registeredActions, onUnregistered) => {
    for (const control of document.querySelectorAll("[data-action]")) {
      if (!registeredActions.has(control.dataset.action)) {
        onUnregistered(control.dataset.action);
        control.disabled = true;
      }
    }
  };

  return { installMapControls, syncControls, disableUnregisteredControls };
};
