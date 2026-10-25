import { act, cleanup, render } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import {
  SPLASH_GIVE_UP_MS,
  SPLASH_INTRO_MS,
  SPLASH_LEAVE_MS,
  SPLASH_SLOW_MS,
  useStartupSplash,
  type StartupPhase,
} from './startup-splash';

let started = 0;
const now = () => Date.now() - started;

function Host({ phase }: { phase: StartupPhase }) {
  useStartupSplash(phase, now);
  return null;
}

const splash = () => document.getElementById('aura-splash');
const caption = () => document.getElementById('aura-splash-caption')?.textContent;

beforeEach(() => {
  vi.useFakeTimers();
  started = Date.now();
  document.body.insertAdjacentHTML(
    'afterbegin',
    '<div id="aura-splash"><div id="aura-splash-caption">正在启动</div></div>',
  );
});
afterEach(() => {
  cleanup();
  splash()?.remove();
  vi.useRealTimers();
});

test('a fast start still plays the whole intro, then leaves without lingering', () => {
  vi.advanceTimersByTime(300);
  render(<Host phase="ready" />);
  vi.advanceTimersByTime(SPLASH_INTRO_MS - 300 - 1);
  expect(splash()?.classList.contains('is-leaving')).toBe(false);
  vi.advanceTimersByTime(1);
  expect(splash()?.classList.contains('is-leaving')).toBe(true);
  vi.advanceTimersByTime(SPLASH_LEAVE_MS);
  expect(splash()).toBeNull();
});

// The intro is drawn by the first script, so it ends where the startup screen says.
test('a fast start waits for the intro the startup screen began late', () => {
  splash()!.dataset.introEnds = '2400';
  vi.advanceTimersByTime(300);
  render(<Host phase="ready" />);
  vi.advanceTimersByTime(2400 - 300 - 1);
  expect(splash()?.classList.contains('is-leaving')).toBe(false);
  vi.advanceTimersByTime(1);
  expect(splash()?.classList.contains('is-leaving')).toBe(true);
});

test('Core ready after the intro: the screen leaves at once', () => {
  vi.advanceTimersByTime(3000);
  render(<Host phase="ready" />);
  act(() => undefined);
  vi.advanceTimersByTime(0);
  expect(splash()?.classList.contains('is-leaving')).toBe(true);
});

test('a failed start leaves at once, so the reason is on screen', () => {
  vi.advanceTimersByTime(100);
  render(<Host phase="failed" />);
  expect(splash()?.classList.contains('is-leaving')).toBe(true);
});

test('a slow start says so, keeps looping, and hands over after the long wait', () => {
  render(<Host phase="waiting" />);
  vi.advanceTimersByTime(SPLASH_SLOW_MS);
  expect(caption()).toBe('启动比平常慢一些，仍在等待 Aura Core');
  expect(splash()?.classList.contains('is-leaving')).toBe(false);
  vi.advanceTimersByTime(SPLASH_GIVE_UP_MS - SPLASH_SLOW_MS);
  expect(splash()?.classList.contains('is-leaving')).toBe(true);
});

test('waiting then ready: the long-wait timer does not fire later', () => {
  const view = render(<Host phase="waiting" />);
  vi.advanceTimersByTime(2000);
  view.rerender(<Host phase="ready" />);
  vi.advanceTimersByTime(0);
  expect(splash()?.classList.contains('is-leaving')).toBe(true);
  vi.advanceTimersByTime(SPLASH_LEAVE_MS);
  expect(splash()).toBeNull();
  // Nothing scheduled for the old phase touches the page afterwards.
  vi.advanceTimersByTime(SPLASH_GIVE_UP_MS);
  expect(splash()).toBeNull();
});

test('without the startup screen (tests, a reload) nothing happens', () => {
  splash()?.remove();
  expect(() => render(<Host phase="ready" />)).not.toThrow();
});
