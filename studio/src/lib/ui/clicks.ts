// Presses that have already done their job and must not land on what's underneath.

const PRESS = ["mousedown", "click", "dblclick"] as const;

/**
 * Call from a capture-phase pointerdown on `window` that closes a popover: that
 * press closes the popover and does nothing else. Nothing under it is selected,
 * sought or started, and the same goes for the click that ends the press.
 */
export function swallowPress(e: PointerEvent) {
  e.stopPropagation();
  const stop = (ev: Event) => {
    ev.stopPropagation();
    ev.preventDefault();
  };
  const end = () => {
    for (const kind of PRESS) window.removeEventListener(kind, stop, true);
    window.removeEventListener("pointerup", ended, true);
    window.removeEventListener("pointercancel", ended, true);
  };
  // The click comes right after pointerup, in the same task.
  const ended = () => setTimeout(end);
  for (const kind of PRESS) window.addEventListener(kind, stop, true);
  window.addEventListener("pointerup", ended, true);
  window.addEventListener("pointercancel", ended, true);
}

/**
 * Call when a click opens something that replaces what was clicked: the second
 * click of a double-click, at the same spot, would otherwise land on the new view
 * (on the editor's video it plays, and its double-click goes fullscreen).
 */
export function ignoreDoubleClick(first: MouseEvent, ms = 500) {
  const near = (e: MouseEvent) => Math.abs(e.clientX - first.clientX) < 8 && Math.abs(e.clientY - first.clientY) < 8;
  const stop = (e: Event) => {
    if (!near(e as MouseEvent)) return;
    e.stopPropagation();
    e.preventDefault();
  };
  const kinds = ["pointerdown", ...PRESS] as const;
  for (const kind of kinds) window.addEventListener(kind, stop, true);
  setTimeout(() => {
    for (const kind of kinds) window.removeEventListener(kind, stop, true);
  }, ms);
}
