import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { createRef } from 'react';
import { HistorySentinel } from './HistorySentinel';

/** A controllable IntersectionObserver: nothing is near the sentinel until the
 *  test says so. */
const observers: { callback: (entries: unknown[]) => void; element: Element }[] = [];

beforeEach(() => {
  observers.length = 0;
  class FakeObserver {
    constructor(private readonly callback: (entries: unknown[]) => void) {
      this.callback = callback;
    }
    observe(element: Element): void {
      observers.push({ callback: this.callback, element });
    }
    disconnect(): void {
      /* nothing to release in the fake */
    }
  }
  Object.defineProperty(globalThis, 'IntersectionObserver', {
    configurable: true,
    value: FakeObserver,
  });
});

afterEach(() => {
  cleanup();
  observers.length = 0;
});

/** The sentinel inside a scrolling viewport, as the conversation renders it. */
function renderSentinel(cursor: string, onLoad: () => Promise<boolean | undefined>) {
  const viewportRef = createRef<HTMLDivElement>();
  const view = render(
    <div ref={viewportRef} data-testid="viewport">
      <HistorySentinel cursor={cursor} label="更早的消息" onLoad={onLoad} root={viewportRef} />
    </div>,
  );
  return { ...view, viewportRef };
}

/** Move the viewport near the sentinel (true) or away from it (false). */
async function approach(intersecting: boolean) {
  const observer = observers.at(-1);
  expect(observer).toBeDefined();
  await act(async () => {
    observer!.callback([{ isIntersecting: intersecting, target: observer!.element }]);
  });
}

test('reaching the top reads the next page without any button', async () => {
  const load = vi.fn().mockResolvedValue(true);
  renderSentinel('cursor-1', load);
  expect(screen.queryByRole('button')).toBeNull();
  await approach(true);
  expect(load).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole('button')).toBeNull();
});

test('a page that arrived does not chain-load while the reader stays put', async () => {
  const load = vi.fn().mockResolvedValue(true);
  const view = renderSentinel('cursor-1', load);
  await approach(true);
  expect(load).toHaveBeenCalledTimes(1);
  // The page landed and the cursor moved, but the reader never left the top:
  // nothing more is read until they scroll away and come back.
  view.rerender(
    <HistorySentinel cursor="cursor-2" label="更早的消息" onLoad={load} root={view.viewportRef} />,
  );
  await act(async () => undefined);
  expect(load).toHaveBeenCalledTimes(1);
  // Scrolling away re-arms the sentinel, so the next page is read on approach.
  await approach(false);
  await approach(true);
  expect(load).toHaveBeenCalledTimes(2);
});

test('a page that brought nothing stops and offers a retry instead of looping', async () => {
  const load = vi.fn().mockResolvedValue(false);
  renderSentinel('cursor-1', load);
  await approach(true);
  expect(load).toHaveBeenCalledTimes(1);
  const retry = await screen.findByRole('button', { name: '重试加载更早的消息' });
  // Staying at the top does not keep asking a source that answers with nothing.
  await approach(true);
  expect(load).toHaveBeenCalledTimes(1);
  load.mockResolvedValue(true);
  await act(async () => {
    retry.click();
  });
  expect(load).toHaveBeenCalledTimes(2);
  await waitFor(() => expect(screen.queryByRole('button')).toBeNull());
});

test('a request that was never made leaves the sentinel armed', async () => {
  const load = vi.fn().mockResolvedValue(undefined);
  renderSentinel('cursor-1', load);
  await approach(true);
  expect(load).toHaveBeenCalledTimes(1);
  // Nothing was asked for, so there is nothing to retry …
  expect(screen.queryByRole('button')).toBeNull();
  // … and the next approach asks again.
  await approach(false);
  await approach(true);
  expect(load).toHaveBeenCalledTimes(2);
});

test('the retry belongs to the page that failed, not to a later one', async () => {
  const load = vi.fn().mockResolvedValue(false);
  const view = renderSentinel('cursor-1', load);
  await approach(true);
  await screen.findByRole('button', { name: '重试加载更早的消息' });
  // A real page arrives from the projection before the reader retries: the
  // failure belonged to the older cursor and is cleared with it.
  view.rerender(
    <HistorySentinel cursor="cursor-2" label="更早的消息" onLoad={load} root={view.viewportRef} />,
  );
  await waitFor(() => expect(screen.queryByRole('button')).toBeNull());
});

test('a rejected request reports a retry instead of an unhandled failure', async () => {
  const load = vi.fn().mockRejectedValue(new Error('transport lost'));
  renderSentinel('cursor-1', load);
  await approach(true);
  await waitFor(() => expect(screen.getByRole('button')).toBeDefined());
  expect(load).toHaveBeenCalledTimes(1);
});
