/** The ring's 32 squares on a 12 × 12 grid, centred in 16 × 16. */
const ROWS = [
  '....####....',
  '..##....##..',
  '.#........#.',
  '.#........#.',
  '#..........#',
  '#..........#',
  '#..........#',
  '#..........#',
  '.#........#.',
  '.#........#.',
  '..##....##..',
  '....####....',
];

/** The squares clockwise from twelve o'clock, so a share lights the first ones. */
const CELLS = ROWS.flatMap((row, y) =>
  [...row].flatMap((mark, x) => (mark === '#' ? [{ x: x + 2, y: y + 2 }] : [])),
)
  .map((cell) => {
    const turn = Math.atan2(cell.x + 0.5 - 8, 8 - (cell.y + 0.5));
    return { ...cell, turn: turn < 0 ? turn + 2 * Math.PI : turn };
  })
  .sort((a, b) => a.turn - b.turn);

const path = (cells: typeof CELLS) => cells.map((cell) => `M${cell.x} ${cell.y}h1v1h-1z`).join('');

/**
 * A ring of squares with a share of them lit, clockwise from the top: the lit ones take the text
 * colour, the rest the strong line colour. Drawn at 16px so every square is one pixel.
 */
export function PixelRing({ share, className }: { share: number; className?: string }) {
  const lit = Math.round(Math.min(1, Math.max(0, share)) * CELLS.length);
  return (
    <svg
      aria-hidden="true"
      className={className ? `pixel-ring ${className}` : 'pixel-ring'}
      height="16"
      shapeRendering="crispEdges"
      viewBox="0 0 16 16"
      width="16"
    >
      <path className="pixel-ring-rest" d={path(CELLS.slice(lit))} />
      <path className="pixel-ring-lit" d={path(CELLS.slice(0, lit))} />
    </svg>
  );
}
