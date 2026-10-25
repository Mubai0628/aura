import { cleanup, render } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
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

function prefersLessMotion(less: boolean) {
  vi.stubGlobal('matchMedia', (query: string) => ({ matches: less && query.includes('reduce') }));
}

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
