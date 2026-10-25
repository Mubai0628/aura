import { expect, test } from 'vitest';
import { SPLASH_INTRO_MS } from '../startup-splash';
import { NAME, createRainLake } from './rain-lake';
import { seeded } from './rain';
import { nameIn, recordingCanvas, waterIn, type Written } from './test-canvas';

// The lake in characters,
// rain stirring it, and the name over the water.

/** Plays the lake at 30 frames a second up to `until` seconds; returns what the last frame wrote. */
function play(until: number, leaveAtEnd = 0, seed = 5) {
  const { canvas, written } = recordingCanvas();
  const lake = createRainLake(canvas, seeded(seed))!;
  for (let frame = 0; frame / 30 < until; frame++) lake.frame(frame / 30);
  const from = written.length;
  lake.frame(until, leaveAtEnd);
  return written.slice(from);
}

/** Every frame at 30 a second up to `until`, and every rectangle filled on the way. */
function frames(until: number, seed = 5) {
  const { canvas, written, filled } = recordingCanvas();
  const lake = createRainLake(canvas, seeded(seed))!;
  const each: Written[][] = [];
  for (let frame = 0; frame / 30 <= until; frame++) {
    const from = written.length;
    lake.frame(frame / 30);
    each.push(written.slice(from));
  }
  return { each, filled };
}

/** The name's letters on the reel at `x`. */
const onReel = (frame: readonly Written[], x: number) =>
  nameIn(frame).filter((w) => Math.round(w.x) === x);
/** A reel that has stopped shows its own letter alone, upright, on the line. */
const stoppedOn = (frame: readonly Written[], x: number, letter: string) => {
  const shown = onReel(frame, x);
  return shown.length === 1 && shown[0]!.text === letter && shown[0]!.scaleY === 1;
};

test('nothing is filled behind the name: the rain shows around the letters', () => {
  expect(frames(SPLASH_INTRO_MS / 1000 + 1).filled).toEqual([]);
});

test('the letters roll in one after another, left to right, each reel stopping on its letter', () => {
  const { each } = frames(SPLASH_INTRO_MS / 1000);
  // Before the first reel turns, there is no name.
  expect(nameIn(each[0]!)).toEqual([]);
  // Once the intro has played the name is whole: each letter alone, upright, on one line.
  const whole = nameIn(each.at(-1)!).sort((a, b) => a.x - b.x);
  expect(whole.map((w) => w.text).join('')).toBe(NAME);
  expect(new Set(whole.map((w) => w.y)).size).toBe(1);
  expect(whole.every((w) => w.scaleY === 1 && w.alpha === 1)).toBe(true);

  const reels = whole.map((w) => Math.round(w.x));
  const shown = reels.map((x) => each.findIndex((frame) => onReel(frame, x).length > 0));
  const stopped = reels.map((x, i) => each.findIndex((frame) => stoppedOn(frame, x, NAME[i]!)));
  for (let i = 1; i < reels.length; i++) {
    expect(shown[i]!, 'each reel starts after the one on its left').toBeGreaterThan(shown[i - 1]!);
    expect(stopped[i]!, 'and stops after it').toBeGreaterThan(stopped[i - 1]!);
  }
  // A reel turns: while it rolls, other letters pass over and under the line, turned away from
  // the viewer, and the first stops only after rolling for a while.
  expect(stopped[0]! - shown[0]!).toBeGreaterThanOrEqual(8);
  const rolling = onReel(each[shown[0]! + 3]!, reels[0]!);
  expect(rolling.length).toBeGreaterThan(1);
  expect(new Set(rolling.map((w) => Math.round(w.y))).size).toBeGreaterThan(1);
  expect(rolling.some((w) => w.scaleY < 0.9)).toBe(true);
  expect(rolling.some((w) => w.text !== NAME[0])).toBe(true);
});

/** Calm water shows only dots: anything else on the water is a ring. */
const stirred = (water: readonly { text: string }[]) =>
  water.filter((w) => '·-~=+*#%'.includes(w.text));

test('rain falls as streaks and stirs the water into rings', () => {
  const frame = waterIn(play(3));
  expect(frame.length).toBeGreaterThan(1000);
  expect(frame.some((w) => w.text === '|' || w.text === "'")).toBe(true);
  expect(stirred(frame).length).toBeGreaterThan(150);
  // A fresh ring crests.
  expect(frame.some((w) => '=+*#%'.includes(w.text))).toBe(true);
  // Before the first drop lands the lake is calm.
  expect(stirred(waterIn(play(0.2)))).toEqual([]);
});

test('a leaving screen dissolves the water cell by cell and fades the name', () => {
  const staying = play(3);
  const leaving = play(3, 0.8);
  expect(waterIn(leaving).length).toBeLessThan(waterIn(staying).length * 0.35);
  for (const letter of nameIn(leaving)) expect(letter.alpha).toBeCloseTo(0.2, 5);
});

test('for less motion it is one picture: rings already spreading and the whole name', () => {
  const { canvas, written } = recordingCanvas();
  createRainLake(canvas, seeded(5))!.still();
  expect(
    nameIn(written)
      .map((w) => w.text)
      .join(''),
  ).toBe(NAME);
  expect(stirred(waterIn(written)).length).toBeGreaterThan(100);
  // No noise: the water has already settled.
  expect(waterIn(written).some((w) => '01<>/\\{}[]'.includes(w.text))).toBe(false);
});

test('without a canvas to draw on there is no lake', () => {
  const canvas = document.createElement('canvas');
  canvas.getContext = (() => null) as unknown as typeof canvas.getContext;
  expect(createRainLake(canvas)).toBeUndefined();
});
