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

function Area({
  onArea,
  onPicture,
}: {
  onArea: (element: HTMLDivElement) => void;
  onPicture?: (picture: string | undefined) => void;
}) {
  const area = useRef<HTMLDivElement>(null);
  const picture = useNativeView(area, S, 'tab-1', true);
  onPicture?.(picture);
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

// A menu or dialog of Aura's drawn over the page would hide under the native view, so the page
// steps aside for it; a picture of the page fills the area meanwhile instead of an empty space.
it('steps aside for a menu over it and leaves a picture of itself in its place', async () => {
  command.mockImplementation(async (value) =>
    value.kind === 'picture'
      ? { ok: true, picture: `data:image/jpeg;base64,${value.kind}` }
      : { ok: true },
  );
  const pictures: (string | undefined)[] = [];
  render(
    <Area
      onArea={(element) => {
        element.getBoundingClientRect = () => new DOMRect(1262, 130, 653, 830);
      }}
      onPicture={(picture) => pictures.push(picture)}
    />,
  );
  await act(async () => vi.advanceTimersByTimeAsync(50));
  const kinds = () => command.mock.calls.map(([value]) => value.kind);
  expect(kinds()).toEqual(['show']);

  const menu = document.createElement('div');
  menu.setAttribute('role', 'menu');
  menu.getBoundingClientRect = () => new DOMRect(1300, 200, 160, 120);
  await act(async () => {
    document.body.append(menu);
    await vi.advanceTimersByTimeAsync(200);
  });
  expect(kinds()).toEqual(['show', 'picture', 'hide']);
  expect(pictures.at(-1)).toBe('data:image/jpeg;base64,picture');

  // The page goes on running underneath; a menu left open gets a newer picture.
  await act(async () => vi.advanceTimersByTimeAsync(2_100));
  expect(kinds().filter((kind) => kind === 'picture')).toHaveLength(2);

  menu.remove();
  await act(async () => vi.advanceTimersByTimeAsync(50));
  expect(kinds().at(-1)).toBe('show');
  expect(pictures.at(-1)).toBeUndefined();
  await act(async () => vi.advanceTimersByTimeAsync(5_000));
  expect(kinds().filter((kind) => kind === 'picture')).toHaveLength(2);
});

// A menu that closes before the picture is ready leaves the page where it is.
it('stays shown when the menu is gone before its picture arrives', async () => {
  let deliver: (value: unknown) => void = () => undefined;
  command.mockImplementation((value) =>
    value.kind === 'picture'
      ? new Promise((resolve) => {
          deliver = resolve;
        })
      : Promise.resolve({ ok: true }),
  );
  render(
    <Area
      onArea={(element) => {
        element.getBoundingClientRect = () => new DOMRect(1262, 130, 653, 830);
      }}
    />,
  );
  await act(async () => vi.advanceTimersByTimeAsync(50));
  const menu = document.createElement('div');
  menu.setAttribute('role', 'menu');
  menu.getBoundingClientRect = () => new DOMRect(1300, 200, 160, 120);
  await act(async () => {
    document.body.append(menu);
    await vi.advanceTimersByTimeAsync(50);
    menu.remove();
    await vi.advanceTimersByTimeAsync(50);
  });
  await act(async () => {
    deliver({ ok: true, picture: 'data:image/jpeg;base64,late' });
    await vi.advanceTimersByTimeAsync(200);
  });
  const kinds = command.mock.calls.map(([value]) => value.kind);
  expect(kinds).not.toContain('hide');
  expect(kinds.at(-1)).toBe('show');
});
