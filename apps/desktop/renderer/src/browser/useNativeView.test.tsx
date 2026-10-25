import { act, cleanup, render } from '@testing-library/react';
import { useRef } from 'react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

import type { BrowserUserCommand } from '../../../shared/browser-api';
import { useNativeView } from './useNativeView';

const S = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
let command: ReturnType<typeof vi.fn<(value: BrowserUserCommand) => Promise<unknown>>>;

beforeEach(() => {
  vi.useFakeTimers();
  command = vi.fn(async () => ({ ok: true }));
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { browser: { command } } },
  });
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

function Area({ onArea }: { onArea: (element: HTMLDivElement) => void }) {
  const area = useRef<HTMLDivElement>(null);
  useNativeView(area, S, 'tab-1', true);
  return (
    <div className="panel" data-testid="panel">
      <div
        ref={(element) => {
          area.current = element;
          if (element) onArea(element);
        }}
      />
    </div>
  );
}

const shows = () =>
  command.mock.calls
    .map(([value]) => value)
    .filter(
      (value): value is Extract<BrowserUserCommand, { kind: 'show' }> => value.kind === 'show',
    )
    .map((value) => value.bounds);

// The page area can move without
// changing size — a divider dragged while the panel kept its width — and the page follows it.
it('follows its area when it moves without changing size', async () => {
  let rect = new DOMRect(1262, 130, 653, 830);
  render(
    <Area
      onArea={(element) => {
        element.getBoundingClientRect = () => rect;
      }}
    />,
  );
  await act(async () => vi.advanceTimersByTimeAsync(50));
  expect(shows().at(-1)).toEqual({ x: 1262, y: 130, width: 653, height: 830 });

  rect = new DOMRect(1445, 130, 653, 830);
  await act(async () => {
    window.dispatchEvent(new MouseEvent('pointermove', { buttons: 1 }));
    await vi.advanceTimersByTimeAsync(50);
  });
  expect(shows().at(-1)?.x).toBe(1445);
});

// What the panel does not show, the page does not cover: a native view is drawn over the
// window, so an area wider than its panel would cover the conversation beside it.
it('never lays the page outside what its panel shows', async () => {
  render(
    <Area
      onArea={(element) => {
        element.getBoundingClientRect = () => new DOMRect(1445, 130, 653, 830);
        const panel = element.parentElement!;
        panel.style.overflow = 'hidden';
        panel.getBoundingClientRect = () => new DOMRect(1445, 60, 470, 920);
      }}
    />,
  );
  await act(async () => vi.advanceTimersByTimeAsync(50));
  expect(shows().at(-1)).toEqual({ x: 1445, y: 130, width: 470, height: 830 });
});
