/** A picture on a square pixel grid: the row it starts on, then its rows downwards, `#` lit. */
export type PixelArt = readonly [top: number, rows: readonly string[]];

/** A picture from its rows as drawn, one per line between the template's first and last line. */
export function art(top: number, picture: string): PixelArt {
  return [top, picture.replace(/^\n|\n$/gu, '').split('\n')];
}

/** The picture as one path: each run of lit pixels in a row is a rectangle one pixel high. */
export function pixelPath([top, rows]: PixelArt): string {
  let path = '';
  rows.forEach((row, y) => {
    for (const run of row.matchAll(/#+/gu))
      path += `M${run.index} ${top + y}h${run[0].length}v1h-${run[0].length}z`;
  });
  return path;
}

/** The size a picture is drawn at, so that every square of its grid is whole screen pixels. */
export function pixelIconSize(size: number, hasSmall: boolean): { grid: 12 | 16; drawn: number } {
  if (size <= 12 && hasSmall) return { grid: 12, drawn: 12 };
  return { grid: 16, drawn: size >= 24 ? 32 : 16 };
}
