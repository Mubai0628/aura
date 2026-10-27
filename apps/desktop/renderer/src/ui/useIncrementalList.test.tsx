import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';

import { ListSentinel } from './SkeletonRows';
import { LIST_PAGE, useIncrementalList } from './useIncrementalList';

// A long list draws its first
// page at once and the next as its end comes into view.

let reached: (() => void)[] = [];
beforeEach(() => {
  reached = [];
  vi.stubGlobal(
    'IntersectionObserver',
    class {
      constructor(callback: (entries: { isIntersecting: boolean }[]) => void) {
        reached.push(() => callback([{ isIntersecting: true }]));
      }
      observe() {}
      disconnect() {}
    },
  );
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function List({ items, keep }: { items: readonly string[]; keep?: number }) {
  const pages = useIncrementalList(items, undefined, keep);
  return (
    <ul aria-label="list">
      {pages.shown.map((item) => (
        <li key={item}>{item}</li>
      ))}
      {pages.more ? <ListSentinel sentinel={pages.sentinel} /> : null}
      {pages.kept ? <li key={pages.kept}>{pages.kept}</li> : null}
    </ul>
  );
}

const names = (count: number, prefix = 'skill') =>
  Array.from({ length: count }, (_, index) => `${prefix}-${index}`);

test('the first page is drawn at once and the next when the end comes into view', () => {
  const items = names(100);
  render(<List items={items} />);
  expect(screen.getAllByText(/^skill-/u)).toHaveLength(LIST_PAGE);
  act(() => reached.at(-1)!());
  expect(screen.getAllByText(/^skill-/u)).toHaveLength(LIST_PAGE * 2);
  act(() => reached.at(-1)!());
  expect(screen.getAllByText(/^skill-/u)).toHaveLength(100);
});

test('another list, such as a search, starts again from its first page', () => {
  const view = render(<List items={names(100)} />);
  act(() => reached.at(-1)!());
  view.rerender(<List items={names(90, 'tool')} />);
  expect(screen.getAllByText(/^tool-/u)).toHaveLength(LIST_PAGE);
});

test('a short list is drawn whole with nothing waiting', () => {
  render(<List items={names(5)} />);
  expect(screen.getAllByText(/^skill-/u)).toHaveLength(5);
  expect(reached).toHaveLength(0);
});

test('without IntersectionObserver every row is drawn', () => {
  vi.stubGlobal('IntersectionObserver', undefined);
  render(<List items={names(100)} />);
  expect(screen.getAllByText(/^skill-/u)).toHaveLength(100);
});

test('a row to keep beyond the pages drawn is drawn on its own, not with every row before it', () => {
  render(<List items={names(200)} keep={99} />);
  expect(screen.getAllByText(/^skill-/u)).toHaveLength(LIST_PAGE + 1);
  expect(screen.getByText('skill-99')).toBeTruthy();
  act(() => reached.at(-1)!());
  expect(screen.getAllByText(/^skill-/u)).toHaveLength(LIST_PAGE * 2 + 1);
  act(() => reached.at(-1)!());
  // The pages reached it: it is drawn in its place, once.
  expect(screen.getAllByText(/^skill-/u)).toHaveLength(LIST_PAGE * 3);
  expect(screen.getAllByText('skill-99')).toHaveLength(1);
});

test('the same list given again, as after acting on one of its rows, keeps the rows drawn', () => {
  const view = render(<List items={names(100)} />);
  act(() => reached.at(-1)!());
  expect(screen.getAllByText(/^skill-/u)).toHaveLength(LIST_PAGE * 2);
  view.rerender(<List items={names(100)} />);
  expect(screen.getAllByText(/^skill-/u)).toHaveLength(LIST_PAGE * 2);
});
