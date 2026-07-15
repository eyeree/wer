// Shared window-level keyboard forwarding. The adapter forwards primitive
// DOM facts only (KeyboardEvent.code, repeat, modifiers); binding selection,
// held state, and repeat suppression live in viewer_host::InputMapper behind
// the shell's `keyEvent` callback. Interactive controls and resizers keep
// their own keys.
const interactiveTarget = (event) =>
  event.target instanceof Element &&
  event.target.closest(
    "button,input,select,textarea,[contenteditable='true'],[role='separator']",
  );

export const installKeyForwarding = ({ keyEvent, requestFrame }) => {
  window.addEventListener("keydown", (event) => {
    if (event.defaultPrevented || interactiveTarget(event)) return;
    const handled = keyEvent(event.code, true, event.repeat, {
      shift: event.shiftKey,
      control: event.ctrlKey,
      alt: event.altKey,
      superKey: event.metaKey,
    });
    if (handled === null) return;
    if (handled) event.preventDefault();
    requestFrame();
  });

  window.addEventListener("keyup", (event) => {
    if (event.defaultPrevented || interactiveTarget(event)) return;
    const handled = keyEvent(event.code, false, false, {
      shift: event.shiftKey,
      control: event.ctrlKey,
      alt: event.altKey,
      superKey: event.metaKey,
    });
    if (handled === null) return;
    if (handled) event.preventDefault();
    requestFrame();
  });
};
