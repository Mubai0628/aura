/** One square of a pixel bar: the part it shows, or room left. */
export type PixelCell<K extends string> = K | 'free';

/**
 * Splits `cells` squares among `parts` in proportion to their share of `total` (largest remainder
 * first). A part that has anything keeps at least one square while the bar has room; what the parts
 * do not fill is free.
 */
export function pixelCells<K extends string>(
  parts: readonly { key: K; value: number }[],
  total: number,
  cells: number,
): PixelCell<K>[] {
  const values = parts.map((part) => Math.max(0, part.value));
  const sum = values.reduce((a, b) => a + b, 0);
  const whole = Math.max(total, sum, 1);
  const exact = values.map((value) => (value / whole) * cells);
  const counts = exact.map((value) => Math.floor(value));
  values.forEach((value, index) => {
    if (value > 0 && counts[index] === 0) counts[index] = 1;
  });
  const filled = Math.min(cells, Math.max(Math.round((sum / whole) * cells), sum > 0 ? 1 : 0));
  let used = counts.reduce((a, b) => a + b, 0);
  const byRemainder = exact
    .map((value, index) => ({ index, rest: value - Math.floor(value) }))
    .sort((a, b) => b.rest - a.rest);
  for (const { index } of byRemainder) {
    if (used >= filled) break;
    counts[index] = (counts[index] ?? 0) + 1;
    used += 1;
  }
  while (used > cells) {
    const largest = counts.indexOf(Math.max(...counts));
    counts[largest] = (counts[largest] ?? 0) - 1;
    used -= 1;
  }
  const out: PixelCell<K>[] = [];
  parts.forEach((part, index) => {
    for (let n = 0; n < (counts[index] ?? 0); n += 1) out.push(part.key);
  });
  while (out.length < cells) out.push('free');
  return out;
}
