import type { Point } from '../canvas/layout';

const SNAP_DISTANCE = 8;

interface CardBounds extends Point {
  width: number;
  height: number;
}

export interface SnapAlignment {
  moving: CardBounds;
  targets: { x: number[]; y: number[] };
}

export interface SnapGuides {
  x?: number | undefined;
  y?: number | undefined;
}

const anchors = (start: number, size: number) => [start, start + size / 2, start + size];

export function snapAlignment(
  moving: CardBounds,
  stationary: readonly CardBounds[],
): SnapAlignment {
  return {
    moving,
    targets: {
      x: stationary.flatMap(({ x, width }) => anchors(x, width)),
      y: stationary.flatMap(({ y, height }) => anchors(y, height)),
    },
  };
}

function nearest(from: number[], targets: number[], tolerance: number) {
  let best: { correction: number; at: number } | undefined;
  for (const start of from) {
    for (const at of targets) {
      const correction = at - start;
      const distance = Math.abs(correction);
      if (distance <= tolerance && (!best || distance < Math.abs(best.correction))) {
        best = { correction, at };
      }
    }
  }
  return best;
}

/** The snap distance stays the same on screen as the canvas zoom changes. */
export function snapDisplacement(
  delta: Point,
  alignment: SnapAlignment | undefined,
  zoom: number,
): { delta: Point; guides?: SnapGuides } {
  if (!alignment) return { delta };
  const { moving, targets } = alignment;
  const x = nearest(anchors(moving.x + delta.x, moving.width), targets.x, SNAP_DISTANCE / zoom);
  const y = nearest(anchors(moving.y + delta.y, moving.height), targets.y, SNAP_DISTANCE / zoom);
  return {
    delta: { x: delta.x + (x?.correction ?? 0), y: delta.y + (y?.correction ?? 0) },
    guides: { x: x?.at, y: y?.at },
  };
}
