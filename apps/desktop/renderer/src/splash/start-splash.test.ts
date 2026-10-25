import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { SPLASH_INTRO_MS, SPLASH_LEAVE_MS } from '../startup-splash';
import { startSplashLake } from './start-splash';
import { nameIn, recordingCanvas, type Written } from './test-canvas';

// The rain starts with the first script, follows the screen as it leaves, and stops once
// the screen is gone; a hidden window draws nothing, and less motion is one still picture.

let frames: ((now: number) => void)[];
let written: Written[];
let clock: number;

function mount() {
  const splash = document.createElement('div');
  splash.id = 'aura-splash';
  const made = recordingCanvas();
  made.canvas.id = 'aura-splash-lake';
  splash.append(made.canvas);
  document.body.append(splash);
  written = made.written;
  return splash;
}

function prefersLessMotion(less: boolean) {
  vi.stubGlobal('matchMedia', (query: string) => ({ matches: less && query.includes('reduce') }));
}

/** Runs the next frame the lake asked for, `ms` after the last one. */
function next(ms = 40) {
  clock += ms;
  const frame = frames.shift();
  expect(frame).toBeDefined();
  frame!(clock);
}

beforeEach(() => {
  frames = [];
  clock = 2000;
  vi.stubGlobal(
    'requestAnimationFrame',
    vi.fn((callback: (now: number) => void) => frames.push(callback)),
  );
  vi.stubGlobal('cancelAnimationFrame', vi.fn());
});
afterEach(() => {
  document.getElementById('aura-splash')?.remove();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

test('the rain starts at once, follows the screen as it fades, and stops once it is gone', () => {
  prefersLessMotion(false);
  const splash = mount();
  startSplashLake(document, () => clock);
  expect(requestAnimationFrame).toHaveBeenCalledTimes(1);
  next();
  written.length = 0;
  next(SPLASH_INTRO_MS);
  expect(written.length).toBeGreaterThan(1000);
  expect(nameIn(written).map((letter) => letter.text)).toHaveLength(4);
  expect(nameIn(written).every((letter) => letter.alpha === 1)).toBe(true);

  splash.classList.add('is-leaving');
  next();
  written.length = 0;
  next(SPLASH_LEAVE_MS / 2);
  for (const letter of nameIn(written)) expect(letter.alpha).toBeCloseTo(0.5, 1);

  splash.remove();
  next();
  expect(frames).toEqual([]);
});

// In development the first
// script runs only once the whole page has loaded, seconds after the window opened; the reels
// still turn from the start, and the app knows when they will have stopped.
test('the intro plays from the first frame, however late the script started', () => {
  prefersLessMotion(false);
  const splash = mount();
  startSplashLake(document, () => clock);
  next();
  expect(splash.dataset.introEnds).toBe(String(2040 + SPLASH_INTRO_MS));
  expect(nameIn(written)).toEqual([]);
  written.length = 0;
  next(500);
  const rolling = nameIn(written).map((letter) => letter.text);
  expect(rolling.length).toBeGreaterThan(0);
  expect(rolling.join('')).not.toBe('AURA');
  next();
  expect(splash.dataset.introEnds, 'set once').toBe(String(2040 + SPLASH_INTRO_MS));
});

test('a hidden window draws nothing until it is shown again', () => {
  prefersLessMotion(false);
  const hidden = vi.spyOn(document, 'hidden', 'get').mockReturnValue(true);
  mount();
  startSplashLake(document, () => clock);
  expect(requestAnimationFrame).not.toHaveBeenCalled();
  hidden.mockReturnValue(false);
  document.dispatchEvent(new Event('visibilitychange'));
  expect(requestAnimationFrame).toHaveBeenCalledTimes(1);
});

test('for less motion it draws one still picture and never animates', () => {
  prefersLessMotion(true);
  mount();
  startSplashLake(document, () => clock);
  expect(requestAnimationFrame).not.toHaveBeenCalled();
  expect(
    nameIn(written)
      .map((letter) => letter.text)
      .join(''),
  ).toBe('AURA');
});

test('a page without the startup screen starts nothing', () => {
  prefersLessMotion(false);
  startSplashLake(document, () => clock);
  expect(requestAnimationFrame).not.toHaveBeenCalled();
});
