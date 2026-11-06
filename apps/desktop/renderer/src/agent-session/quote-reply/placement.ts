/** A rectangle on screen, in the window's pixels. */
export interface Box {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/** The space kept between the selection and what sits beside it. */
const GAP = 6;
/** The least space kept from the sides of what can be seen. */
const EDGE = 8;

/** `value` within `low`..`high`; `low` wins when the range is empty, so the start stays in sight. */
const clamp = (value: number, low: number, high: number) => Math.max(low, Math.min(value, high));

/**
 * Where something of `size` sits beside the selection `anchor` inside `area`, the part of the
 * conversation that can be seen: above the selection when there is room, else below it, else over
 * the top of its visible part. Horizontally it is centred on the selection or starts at its left
 * edge, and it stays inside the area. A selection out of sight places nothing, unless `keep` holds
 * it at the nearest edge, as for a card being typed in.
 */
export function placeBeside(
  anchor: Box,
  size: { width: number; height: number },
  area: Box,
  align: 'center' | 'start',
  keep = false,
): { left: number; top: number } | undefined {
  let top: number;
  if (anchor.bottom <= area.top || anchor.top >= area.bottom) {
    if (!keep) return undefined;
    top = anchor.bottom <= area.top ? area.top + GAP : area.bottom - GAP - size.height;
  } else if (anchor.top - GAP - size.height >= area.top) top = anchor.top - GAP - size.height;
  else if (anchor.bottom + GAP + size.height <= area.bottom) top = anchor.bottom + GAP;
  else top = Math.max(anchor.top, area.top) + GAP;
  const left = align === 'center' ? (anchor.left + anchor.right - size.width) / 2 : anchor.left;
  return {
    left: clamp(left, area.left + EDGE, area.right - size.width - EDGE),
    top: clamp(top, area.top, area.bottom - size.height),
  };
}

/**
 * What can be seen of the conversation: its scrolling view without the composer's dock, which
 * floats over the view's bottom and says its height in `--conversation-dock-clearance`. Without a
 * view to measure, the window.
 */
export function visibleArea(viewport: HTMLElement | null): Box {
  const whole = { left: 0, top: 0, right: window.innerWidth, bottom: window.innerHeight };
  if (!viewport) return whole;
  const view = viewport.getBoundingClientRect();
  if (!view.width || !view.height) return whole;
  const dock = Number.parseFloat(
    getComputedStyle(viewport).getPropertyValue('--conversation-dock-clearance'),
  );
  return {
    left: view.left,
    top: view.top,
    right: view.right,
    bottom: Math.max(view.top, view.bottom - (Number.isFinite(dock) ? dock : 0)),
  };
}

/** Where a selection is on screen; nothing once its text has left the page. */
export function rangeBox(range: Range): Box | undefined {
  if (!range.startContainer.isConnected || !range.endContainer.isConnected) return undefined;
  const start = range.startContainer;
  // Layout-free environments have no range geometry; the start's element stands in for it.
  const box =
    typeof range.getBoundingClientRect === 'function'
      ? range.getBoundingClientRect()
      : (start instanceof Element ? start : start.parentElement)?.getBoundingClientRect();
  return box && { left: box.left, top: box.top, right: box.right, bottom: box.bottom };
}
