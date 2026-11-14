/** A rectangle on screen, as `getBoundingClientRect` gives it. */
export interface Box {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

/** Where the open provider's models go, and how wide they are. */
export type SubmenuPlacement =
  | { side: 'left'; width: number }
  | { side: 'right'; width: number }
  | {
      side: 'above';
      width: number;
      /** How far right of the providers' right edge it moves to stay inside what clips it. */
      shift: number;
      /** The height left above the providers. */
      maxHeight: number;
    };

const WIDTH = 280;
const NARROWEST = 200;
const GAP = 6;
const MARGIN = 8;

/**
 * The models beside the providers on the side with room for them, narrowed to no less than
 * NARROWEST before they leave that side; with room on neither side, above the providers. All of it
 * inside `bounds`, the nearest box that clips the menu.
 */
export function placeSubmenu(menu: Box, bounds: Box): SubmenuPlacement {
  const left = menu.left - GAP - MARGIN - bounds.left;
  const right = bounds.right - MARGIN - GAP - menu.right;
  if (left >= WIDTH) return { side: 'left', width: WIDTH };
  if (right >= WIDTH) return { side: 'right', width: WIDTH };
  if (Math.max(left, right) >= NARROWEST)
    return left >= right ? { side: 'left', width: left } : { side: 'right', width: right };
  const width = Math.min(WIDTH, bounds.right - bounds.left - 2 * MARGIN);
  return {
    side: 'above',
    width,
    shift: Math.max(0, bounds.left + MARGIN - (menu.right - width)),
    maxHeight: menu.top - GAP - MARGIN - bounds.top,
  };
}

/**
 * How far right the providers, right-aligned at the model button from `left` to `right`, move to
 * start inside `bounds`, no further than its right margin allows.
 */
export function shiftInside(left: number, right: number, bounds: Box): number {
  const short = bounds.left + MARGIN - left;
  return short > 0 ? Math.max(0, Math.min(short, bounds.right - MARGIN - right)) : 0;
}

/** The nearest box around `element` that cuts off what overflows it, or the window. */
export function clippingBox(element: HTMLElement): Box {
  for (let node = element.parentElement; node; node = node.parentElement) {
    if (getComputedStyle(node).overflowX !== 'visible') return node.getBoundingClientRect();
  }
  return { left: 0, right: window.innerWidth, top: 0, bottom: window.innerHeight };
}
