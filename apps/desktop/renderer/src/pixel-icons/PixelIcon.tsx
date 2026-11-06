import type { SVGProps } from 'react';
import { pixelIconSize, pixelPath, type PixelArt } from './pixel-art';

export interface PixelIconProps extends Omit<SVGProps<SVGSVGElement>, 'children'> {
  art: PixelArt;
  /** The same picture on the 12-pixel grid, for sizes of 12 or less. */
  small?: PixelArt | undefined;
  size: number;
}

const paths = new WeakMap<PixelArt, string>();

function pathOf(art: PixelArt): string {
  let path = paths.get(art);
  if (path === undefined) {
    path = pixelPath(art);
    paths.set(art, path);
  }
  return path;
}

/** A pixel picture in the text colour at the nearest size that keeps its squares whole. */
export function PixelIcon({ art, small, size, ...props }: PixelIconProps) {
  const { grid, drawn } = pixelIconSize(size, small !== undefined);
  return (
    <svg
      aria-hidden="true"
      fill="currentColor"
      height={drawn}
      shapeRendering="crispEdges"
      viewBox={`0 0 ${grid} ${grid}`}
      width={drawn}
      {...props}
    >
      <path d={pathOf(grid === 12 && small !== undefined ? small : art)} />
    </svg>
  );
}
