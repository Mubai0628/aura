import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ReviewPanel } from './ReviewPanel';
import { resetReviewOptionsForTests } from './review-options';
import { resetReviewStoreForTests } from './review-store';
import { installGit, session, workspace } from './review-fixture';

// A file's diff is read when its card comes near the screen. The cards scroll inside the
// review's own list, so "near" is measured against that list — measured against the window, a
// card below the list's visible part was never near and stayed "正在读取差异…".

const roots: (Element | Document | null | undefined)[] = [];
beforeEach(() => {
  resetReviewOptionsForTests();
  resetReviewStoreForTests();
  installGit();
  roots.length = 0;
  vi.stubGlobal(
    'IntersectionObserver',
    class {
      constructor(_callback: IntersectionObserverCallback, options?: IntersectionObserverInit) {
        roots.push(options?.root);
      }
      observe() {}
      disconnect() {}
      unobserve() {}
      takeRecords() {
        return [];
      }
    },
  );
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test('each card is watched within the review’s own scrolling list', async () => {
  render(<ReviewPanel workspaceId={workspace} sessionId={session} />);
  await screen.findByText('5 个文件 · +17 −11');
  const list = document.querySelector('.review-cards');
  expect(roots.length).toBeGreaterThan(0);
  expect(roots.every((root) => root === list)).toBe(true);
});
