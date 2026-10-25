import { expect, test } from 'vitest';
import { RING_SECONDS, Rain, seeded, surface, type Drop } from './rain';

// Drops keep falling while Core
// starts, and each stirs a ring that spreads over the water and fades.

const view = { w: 1000, h: 600 };
const drop: Drop = { x: 500, y: 300, at: 0, size: 1 };
/** What the drop adds to the water at (x, y) at `t`: its rings alone. */
const ring = (x: number, y: number, t: number) =>
  surface(x, y, t, [drop], view) - surface(x, y, t, [], view);
/** How far from the drop, along (dx, dy), the water it stirred reaches at `t`. */
function reach(t: number, dx: number, dy: number) {
  let farthest = 0;
  for (let d = 1; d < 600; d++) {
    const x = drop.x + dx * d;
    const y = drop.y + dy * d;
    if (x < 0 || x > view.w || y < 0 || y > view.h) break;
    if (Math.abs(ring(x, y, t)) > 0.01) farthest = d;
  }
  return farthest;
}

test('a drop stirs a ring that spreads out and fades', () => {
  expect(reach(0.5, 1, 0)).toBeGreaterThan(0);
  expect(reach(1.5, 1, 0)).toBeGreaterThan(reach(0.5, 1, 0) * 2);
  const strongest = (t: number) =>
    Math.max(...Array.from({ length: 480 }, (_, d) => Math.abs(ring(drop.x + d, drop.y, t))));
  expect(strongest(3)).toBeLessThan(strongest(0.5) * 0.25);
});

test('seen at a slant, a ring reaches farther across the lake than up it', () => {
  expect(reach(1, 1, 0)).toBeGreaterThan(reach(1, 0, -1) * 1.5);
});

test('the rain keeps falling while Core starts, each ring fades in time, and leaving stops it', () => {
  const rain = new Rain(seeded(7));
  let most = 0;
  for (let frame = 0; frame <= 30 * 12; frame++) {
    const t = frame / 30;
    rain.update(t, view, false);
    for (const ring of rain.drops) expect(t - ring.at).toBeLessThanOrEqual(RING_SECONDS);
    if (t > 4) expect(rain.drops.length).toBeGreaterThan(10);
    most = Math.max(most, rain.drops.length);
  }
  expect(most).toBeLessThan(45);

  const falling = rain.falling.length;
  for (let frame = 1; frame <= 30; frame++) rain.update(12 + frame / 30, view, true);
  expect(rain.falling.length).toBe(falling);
  expect(rain.drops.every((ring) => ring.at <= 12)).toBe(true);
});

test('the same start rains the same way', () => {
  const one = new Rain(seeded(3));
  const two = new Rain(seeded(3));
  for (let frame = 0; frame < 60; frame++) {
    one.update(frame / 30, view, false);
    two.update(frame / 30, view, false);
  }
  expect(one.drops).toEqual(two.drops);
  expect(one.drops.length).toBeGreaterThan(5);
});
