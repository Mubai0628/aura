import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';

import { SessionList } from './SessionList';
import { LIST_PAGE } from './ui/useIncrementalList';

// A workspace with many conversations
// draws its first page at once and the rest as the end comes into view; the open one is always
// drawn.

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

const workspace = {
  id: 'workspace',
  name: 'Project',
  root: 'C:\\Project',
  sessions: Array.from({ length: 100 }, (_, index) => ({
    id: `session-${index}`,
    state: 'idle',
    title: `会话 ${index}`,
  })),
};

function list(selectedSessionId: string) {
  return render(
    <SessionList
      activeScreen="workspace"
      activity={new Map()}
      onSelectSession={() => undefined}
      selectedSessionId={selectedSessionId}
      workspace={workspace}
    />,
  );
}

const rows = () => screen.getAllByRole('button', { name: /^会话 \d+$/u });

test('the first page of conversations is drawn at once and the next as the end comes into view', () => {
  list('session-0');
  expect(rows()).toHaveLength(LIST_PAGE);
  act(() => reached.at(-1)!());
  expect(rows()).toHaveLength(LIST_PAGE * 2);
});

test('the open conversation is drawn even beyond the first page', () => {
  list('session-70');
  expect(rows()).toHaveLength(71);
  expect(screen.getByRole('button', { name: '会话 70' }).getAttribute('aria-current')).toBe('page');
  act(() => reached.at(-1)!());
  // The next page follows the open conversation: 71 + 40 covers the remaining rows.
  expect(rows()).toHaveLength(100);
});
