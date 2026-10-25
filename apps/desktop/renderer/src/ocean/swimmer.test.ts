import { expect, test } from 'vitest';
import { Swimmer, tri } from './swimmer';

// The sharks of the video swim curves, their bodies bending as they turn.

const orbit = () => ({ phase: 0, rx: 300, ry: 200, speed: 0.4, pace: 60, wobble: 0 });

function angle(a: { x: number; y: number }, b: { x: number; y: number }) {
  return Math.atan2(b.y - a.y, b.x - a.x);
}

test('the body follows the path the head has traced, so a turning shark bends', () => {
  const shark = new Swimmer('white', 1000, orbit());
  shark.place(500, 400);
  const straight = shark.spine();
  const head = straight[0]!;
  const tail = straight.at(-1)!;
  // Laid out straight behind the head at first: off the line behind it only by the body's sway.
  const offLine = (p: { x: number; y: number }) =>
    Math.abs(-Math.sin(shark.heading) * (p.x - head.x) + Math.cos(shark.heading) * (p.y - head.y));
  for (const point of straight) expect(offLine(point)).toBeLessThan(shark.halfWidth * 0.7);
  expect(Math.hypot(head.x - tail.x, head.y - tail.y)).toBeGreaterThan(shark.length);

  for (let i = 0; i < 120; i++) shark.update(1 / 30, 500, 400);
  const turned = shark.spine();
  expect(turned[0]!.x).toBeCloseTo(shark.x, 0);
  expect(turned[0]!.y).toBeCloseTo(shark.y, 0);
  const front = angle(turned[2]!, turned[0]!);
  const back = angle(turned.at(-1)!, turned.at(-3)!);
  const bend = Math.abs(((front - back + Math.PI * 3) % (Math.PI * 2)) - Math.PI);
  expect(bend).toBeGreaterThan(0.15);
});

test('fins reach past the body, and a hammerhead’s head is wide', () => {
  const white = new Swimmer('white', 1000, orbit());
  const hammer = new Swimmer('hammer', 1000, orbit());
  const fins = white.reach(0.27);
  expect(fins.all).toBeGreaterThan(fins.body * 1.5);
  expect(white.reach(0.5).all).toBe(white.reach(0.5).body);
  expect(hammer.reach(0.035).all).toBeGreaterThan(white.reach(0.035).all * 2);
  expect(white.reach(1.05).body).toBe(0);
  expect(white.reach(1.05).all).toBeGreaterThan(0);
  expect(tri(0.5, 0.5)).toBe(1);
  expect(tri(-0.1, 0.5)).toBe(0);
});

test('a shark startled by the pointer swims away and calms down', () => {
  const shark = new Swimmer('blue', 1000, orbit());
  shark.place(500, 400);
  const pointer = { x: shark.x + 20, y: shark.y };
  shark.update(1 / 30, 500, 400, pointer);
  expect(shark.flee).toBeGreaterThan(0.9);
  const before = Math.hypot(pointer.x - shark.x, pointer.y - shark.y);
  for (let i = 0; i < 60; i++) shark.update(1 / 30, 500, 400);
  expect(Math.hypot(pointer.x - shark.x, pointer.y - shark.y)).toBeGreaterThan(before);
  for (let i = 0; i < 120; i++) shark.update(1 / 30, 500, 400);
  expect(shark.flee).toBe(0);
});
