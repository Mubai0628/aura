import { expect, test } from 'vitest';
import { pixelCells } from './pixel-cells';

const count = (cells: string[], key: string) => cells.filter((cell) => cell === key).length;

test('parts take squares in proportion and the rest of the bar is free', () => {
  const cells = pixelCells(
    [
      { key: 'a', value: 25 },
      { key: 'b', value: 25 },
    ],
    100,
    32,
  );
  expect(cells).toHaveLength(32);
  expect(count(cells, 'a')).toBe(8);
  expect(count(cells, 'b')).toBe(8);
  expect(count(cells, 'free')).toBe(16);
  expect(cells.slice(0, 8).every((cell) => cell === 'a')).toBe(true);
});

test('a small part keeps one square and a full bar has no free one', () => {
  const cells = pixelCells(
    [
      { key: 'big', value: 999 },
      { key: 'tiny', value: 1 },
    ],
    1_000,
    20,
  );
  expect(count(cells, 'tiny')).toBe(1);
  expect(count(cells, 'free')).toBe(0);
  expect(cells).toHaveLength(20);
});

test('more than the whole never overflows the bar, and nothing shows nothing', () => {
  expect(pixelCells([{ key: 'x', value: 500 }], 100, 10)).toEqual(Array(10).fill('x'));
  expect(pixelCells([{ key: 'x', value: 0 }], 100, 4)).toEqual(Array(4).fill('free'));
});
