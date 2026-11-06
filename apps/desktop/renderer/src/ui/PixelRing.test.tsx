import { render } from '@testing-library/react';
import { expect, test } from 'vitest';

import { PixelRing } from './PixelRing';

// The ring is 32 one-pixel squares; a share lights that many of them clockwise from the top.

const squares = (d: string | null) => (d ?? '').split('z').filter(Boolean);

function parts(share: number) {
  const { container } = render(<PixelRing share={share} />);
  const lit = container.querySelector('.pixel-ring-lit')!.getAttribute('d');
  const rest = container.querySelector('.pixel-ring-rest')!.getAttribute('d');
  return { lit: squares(lit), rest: squares(rest) };
}

test('a share lights that many squares and leaves the rest', () => {
  expect(parts(0).lit).toHaveLength(0);
  expect(parts(0).rest).toHaveLength(32);
  expect(parts(0.5).lit).toHaveLength(16);
  expect(parts(0.5).rest).toHaveLength(16);
  expect(parts(1).lit).toHaveLength(32);
  expect(parts(1.7).lit).toHaveLength(32);
  expect(parts(-1).lit).toHaveLength(0);
  // Between two squares, the nearer one: 15.68 squares light 16, 15.04 light 15.
  expect(parts(0.49).lit).toHaveLength(16);
  expect(parts(0.47).lit).toHaveLength(15);
});

test('the first lit squares are the top of the ring, going clockwise', () => {
  const { lit } = parts(2 / 32);
  // The two squares just right of twelve o'clock on the top row.
  expect(lit).toEqual(['M8 2h1v1h-1', 'M9 2h1v1h-1']);
  const quarter = parts(0.25).lit;
  // A quarter ends at three o'clock: the right column's upper squares.
  expect(quarter.at(-1)).toBe('M13 7h1v1h-1');
});

test('every square sits on whole pixels inside the 16px box', () => {
  for (const square of parts(1).lit) {
    const [, x, y] = /^M(\d+) (\d+)h1v1h-1$/u.exec(square)!;
    expect(Number(x)).toBeGreaterThanOrEqual(2);
    expect(Number(x)).toBeLessThanOrEqual(13);
    expect(Number(y)).toBeGreaterThanOrEqual(2);
    expect(Number(y)).toBeLessThanOrEqual(13);
  }
});
