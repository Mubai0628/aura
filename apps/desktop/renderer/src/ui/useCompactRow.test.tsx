import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { useCompactRow } from './useCompactRow';

// A text button squeezed until its words no longer fit on one line shows its
// icon alone, at a fixed size; the words come back once the row is wide enough for them again.

function Row({
  words = '工作区读写',
  extra = false,
  first = false,
}: {
  words?: string;
  extra?: boolean;
  first?: boolean;
}) {
  const ref = useCompactRow<HTMLDivElement>();
  return (
    <div data-testid="row" ref={ref}>
      <button data-adaptive title={`权限：${words}`} type="button">
        <svg aria-hidden="true" />
        <span data-adaptive-label>{words}</span>
      </button>
      {first ? (
        <button aria-label="思考程度：高" data-adaptive data-adaptive-first type="button">
          <svg aria-hidden="true" />
          <span data-adaptive-label>高</span>
        </button>
      ) : null}
      {extra ? <button aria-label="停止当前任务" type="button" /> : null}
    </div>
  );
}

let resized: () => void = () => {};
beforeEach(() => {
  vi.stubGlobal(
    'ResizeObserver',
    class {
      constructor(callback: () => void) {
        resized = callback;
      }
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

/**
 * jsdom lays nothing out: the row is `width` wide, and `needed` wide while it is measured at its
 * parts' own widths with every word shown.
 */
function size(row: HTMLElement, width: number, needed: number) {
  Object.defineProperty(row, 'getBoundingClientRect', {
    configurable: true,
    value: () => ({ width: row.dataset.measuring === 'true' ? needed : width }) as DOMRect,
  });
}

function measure(row: HTMLElement, width: number, needed: number) {
  size(row, width, needed);
  act(() => resized());
}

test('words that no longer fit leave the icon; they come back with room to spare', () => {
  render(<Row />);
  const row = screen.getByTestId('row');
  measure(row, 300, 220);
  expect(row.dataset.compact).toBeUndefined();
  measure(row, 200, 220);
  expect(row.dataset.compact).toBe('true');
  // The words are still the button's name.
  expect(screen.getByRole('button', { name: '工作区读写' })).toBeTruthy();
  measure(row, 224, 220);
  expect(row.dataset.compact).toBe('true');
  measure(row, 240, 220);
  expect(row.dataset.compact).toBeUndefined();
  // Measuring leaves nothing behind.
  expect(row.dataset.measuring).toBeUndefined();
});

// Words that needed more
// room for a moment must not keep the row at icons once shorter words fit the same width.
test('shorter words at the same width come back', () => {
  const { rerender } = render(<Row words="正在加载模型…" />);
  const row = screen.getByTestId('row');
  measure(row, 200, 260);
  expect(row.dataset.compact).toBe('true');
  size(row, 200, 150);
  rerender(<Row words="Flash" />);
  expect(row.dataset.compact).toBeUndefined();
  expect(screen.getByRole('button', { name: 'Flash' })).toBeTruthy();
});

test('a part that changes by itself is measured again', async () => {
  render(<Row words="正在切换…" />);
  const row = screen.getByTestId('row');
  const label = screen.getByText('正在切换…');
  measure(row, 200, 260);
  expect(row.dataset.compact).toBe('true');
  size(row, 200, 150);
  // Changed outside this row's render (the picker re-renders alone): the change is observed.
  await act(async () => {
    label.textContent = 'Flash';
    await Promise.resolve();
  });
  expect(row.dataset.compact).toBeUndefined();
});

test('a button shown only for a while does not keep the icons', () => {
  const { rerender } = render(<Row extra />);
  const row = screen.getByTestId('row');
  measure(row, 200, 230);
  expect(row.dataset.compact).toBe('true');
  size(row, 200, 190);
  rerender(<Row />);
  expect(row.dataset.compact).toBeUndefined();
});

// The thinking level beside the model is the first to give way, so it does not take the
// permission's and the model's words away at a width that held them before it was there.
test('a part that gives way first leaves its icon while the other words still fit', () => {
  render(<Row first />);
  const row = screen.getByTestId('row');
  // `needed` with every word; `trimmed` with the first part's words away.
  const at = (width: number, needed: number, trimmed: number) => {
    Object.defineProperty(row, 'getBoundingClientRect', {
      configurable: true,
      value: () =>
        ({
          width:
            row.dataset.measuring !== 'true'
              ? width
              : row.dataset.compact === 'first'
                ? trimmed
                : needed,
        }) as DOMRect,
    });
    act(() => resized());
    return row.dataset.compact;
  };
  expect(at(300, 260, 230)).toBeUndefined();
  expect(at(240, 260, 230)).toBe('first');
  expect(screen.getByRole('button', { name: '工作区读写' })).toBeTruthy();
  expect(at(220, 260, 230)).toBe('true');
  // Back one step at a time, each with room to spare.
  expect(at(235, 260, 230)).toBe('true');
  expect(at(239, 260, 230)).toBe('first');
  expect(at(265, 260, 230)).toBe('first');
  expect(at(269, 260, 230)).toBeUndefined();
  expect(row.dataset.measuring).toBeUndefined();
});

test('the same words at the same width are not measured again', () => {
  const { rerender } = render(<Row />);
  const row = screen.getByTestId('row');
  measure(row, 200, 220);
  expect(row.dataset.compact).toBe('true');
  const spy = vi.spyOn(row, 'getBoundingClientRect');
  rerender(<Row />);
  act(() => resized());
  // Each time one look at the row's width, none at the width its words need.
  expect(spy).toHaveBeenCalledTimes(2);
  expect(row.dataset.compact).toBe('true');
});
