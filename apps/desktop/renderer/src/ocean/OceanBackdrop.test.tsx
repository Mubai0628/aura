import { cleanup, render } from '@testing-library/react';
import type { BackgroundMotion } from '@aura/contracts/generated';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { resetUiSettingsForTests } from '../ui-settings-store';
import { OceanBackdrop } from './OceanBackdrop';

// The ocean moves only while it can be seen, holds still for less motion, and stops when
// the empty conversation goes.

/** A 2D context that accepts every call: jsdom has no canvas. */
function fakeContext() {
  return new Proxy({}, { get: () => () => undefined, set: () => true }) as CanvasRenderingContext2D;
}

let frames: number;
const getContext = HTMLCanvasElement.prototype.getContext;
beforeEach(() => {
  frames = 0;
  resetUiSettingsForTests();
  Object.defineProperty(window, 'aura', { configurable: true, value: undefined });
  HTMLCanvasElement.prototype.getContext = (() => fakeContext()) as unknown as typeof getContext;
  vi.stubGlobal(
    'requestAnimationFrame',
    vi.fn(() => ++frames),
  );
  vi.stubGlobal('cancelAnimationFrame', vi.fn());
});
afterEach(() => {
  cleanup();
  HTMLCanvasElement.prototype.getContext = getContext;
  vi.unstubAllGlobals();
});

/** Stubs the system's motion setting; the function returned changes it as Windows would. */
function prefersLessMotion(less: boolean) {
  const listeners = new Set<(event: MediaQueryListEvent) => void>();
  const reduce = {
    matches: less,
    addEventListener: (_: 'change', listener: (event: MediaQueryListEvent) => void) =>
      listeners.add(listener),
    removeEventListener: (_: 'change', listener: (event: MediaQueryListEvent) => void) =>
      listeners.delete(listener),
  };
  vi.stubGlobal('matchMedia', (query: string) =>
    query.includes('reduce') ? reduce : { ...reduce, matches: false },
  );
  return (next: boolean) => {
    reduce.matches = next;
    for (const listener of [...listeners]) listener({ matches: next } as MediaQueryListEvent);
  };
}

/** 设置 → 环境 → 背景动画, as Core keeps it. */
function chooseMotion(background_motion: BackgroundMotion) {
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        getUiSettings: vi.fn().mockResolvedValue({
          ok: true,
          value: {
            revision: 2,
            settings: {
              show_debug_information: false,
              default_terminal: 'power_shell',
              background_motion,
            },
            issue: null,
          },
        }),
      },
    },
  });
}

test('set to 开启, the sea moves even while less motion is asked for', async () => {
  chooseMotion('on');
  prefersLessMotion(true);
  render(<OceanBackdrop />);
  await vi.waitFor(() => expect(requestAnimationFrame).toHaveBeenCalledTimes(1));
});

test('set to 关闭, the sea holds still whatever the system asks', async () => {
  chooseMotion('off');
  const setLessMotion = prefersLessMotion(false);
  render(<OceanBackdrop />);
  await vi.waitFor(() => expect(cancelAnimationFrame).toHaveBeenCalled());
  const started = vi.mocked(requestAnimationFrame).mock.calls.length;
  setLessMotion(true);
  setLessMotion(false);
  expect(requestAnimationFrame).toHaveBeenCalledTimes(started);
});

test('the sea is decoration screen readers skip, and it starts moving', () => {
  prefersLessMotion(false);
  const { container, unmount } = render(<OceanBackdrop />);
  const backdrop = container.querySelector('.ocean-backdrop');
  expect(backdrop?.getAttribute('aria-hidden')).toBe('true');
  expect(backdrop?.querySelector('canvas')).toBeTruthy();
  expect(requestAnimationFrame).toHaveBeenCalledTimes(1);
  unmount();
  expect(cancelAnimationFrame).toHaveBeenCalledWith(1);
});

test('for less motion it draws one still picture and never animates', () => {
  prefersLessMotion(true);
  render(<OceanBackdrop />);
  expect(requestAnimationFrame).not.toHaveBeenCalled();
});

test('asking for less motion later stops the sea, and no longer asking starts it again', () => {
  const setLessMotion = prefersLessMotion(false);
  render(<OceanBackdrop />);
  setLessMotion(true);
  expect(cancelAnimationFrame).toHaveBeenCalledWith(1);
  setLessMotion(false);
  expect(requestAnimationFrame).toHaveBeenCalledTimes(2);
});

test('a sea opened while less motion was asked for starts once it no longer is', () => {
  const setLessMotion = prefersLessMotion(true);
  const { unmount } = render(<OceanBackdrop />);
  setLessMotion(false);
  expect(requestAnimationFrame).toHaveBeenCalledTimes(1);
  unmount();
  setLessMotion(true);
  setLessMotion(false);
  expect(requestAnimationFrame).toHaveBeenCalledTimes(1);
});

test('a hidden window stops the sea and a shown one starts it again', () => {
  prefersLessMotion(false);
  render(<OceanBackdrop />);
  const hidden = vi.spyOn(document, 'hidden', 'get').mockReturnValue(true);
  document.dispatchEvent(new Event('visibilitychange'));
  expect(cancelAnimationFrame).toHaveBeenCalled();
  hidden.mockReturnValue(false);
  document.dispatchEvent(new Event('visibilitychange'));
  expect(requestAnimationFrame).toHaveBeenCalledTimes(2);
});

test('without a canvas to draw on nothing runs', () => {
  HTMLCanvasElement.prototype.getContext = (() => null) as typeof getContext;
  prefersLessMotion(false);
  render(<OceanBackdrop />);
  expect(requestAnimationFrame).not.toHaveBeenCalled();
});
