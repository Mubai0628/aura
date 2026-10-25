// Whether a pane divider is being dragged. Terminals resize their console once the drag ends:
// the inbox ConPTY keeps only one screen and repaints it after every resize, so resizing while
// the panel passes through narrow widths loses whatever no longer fits.
let dragging = false;
const ends = new Set<() => void>();

export const paneDragging = () => dragging;

export function setPaneDragging(active: boolean) {
  if (dragging === active) return;
  dragging = active;
  if (!active) for (const listener of [...ends]) listener();
}

export function onPaneDragEnd(listener: () => void) {
  ends.add(listener);
  return () => {
    ends.delete(listener);
  };
}
