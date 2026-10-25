import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ReviewPanel } from './ReviewPanel';
import { resetReviewOptionsForTests } from './review-options';
import { resetReviewStoreForTests } from './review-store';
import { installGit, session, workspace } from './review-fixture';

// Searching the review — file names and changed lines, one match at a time,
// in order, the current one marked and brought into view.

beforeEach(() => {
  resetReviewOptionsForTests();
  resetReviewStoreForTests();
  installGit();
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

test('search finds file names and lines, steps through them in order and closes with Escape', async () => {
  const scrolled = vi.fn();
  Element.prototype.scrollIntoView = scrolled;
  const user = userEvent.setup();
  render(<ReviewPanel workspaceId={workspace} sessionId={session} />);
  await screen.findByText('5 个文件 · +17 −11');
  await waitFor(() => expect(document.querySelectorAll('.diff-row').length).toBeGreaterThan(7));
  await user.click(screen.getByRole('button', { name: '搜索改动' }));
  const box = screen.getByRole('searchbox', { name: '在改动中搜索' });
  await user.type(box, 'hello');
  // README.md's added line, then src/app.ts's removed and added lines.
  expect(screen.getByText('1/3')).toBeTruthy();
  const current = () => document.querySelector('.review-match.is-current')?.closest('article');
  expect(current()?.getAttribute('aria-label')).toBe('README.md 的改动');
  await user.keyboard('{Enter}');
  expect(screen.getByText('2/3')).toBeTruthy();
  expect(current()?.getAttribute('aria-label')).toBe('src/app.ts 的改动');
  await user.click(screen.getByRole('button', { name: '上一个结果' }));
  expect(screen.getByText('1/3')).toBeTruthy();
  await user.keyboard('{Shift>}{Enter}{/Shift}');
  expect(screen.getByText('3/3')).toBeTruthy();
  expect(scrolled).toHaveBeenCalled();
  await user.clear(box);
  await user.type(box, 'app.');
  // Two file names: src/app.ts and tests/app.test.ts.
  expect(screen.getByText('1/2')).toBeTruthy();
  await user.keyboard('{Escape}');
  expect(screen.queryByRole('searchbox', { name: '在改动中搜索' })).toBeNull();
  expect(document.querySelectorAll('.review-match')).toHaveLength(0);
});

test('a search with no match says so', async () => {
  const user = userEvent.setup();
  render(<ReviewPanel workspaceId={workspace} sessionId={session} />);
  await screen.findByText('5 个文件 · +17 −11');
  await user.click(screen.getByRole('button', { name: '搜索改动' }));
  await user.type(screen.getByRole('searchbox', { name: '在改动中搜索' }), 'nothing-here');
  expect(screen.getByText('没有结果')).toBeTruthy();
  expect((screen.getByRole('button', { name: '下一个结果' }) as HTMLButtonElement).disabled).toBe(
    true,
  );
});
