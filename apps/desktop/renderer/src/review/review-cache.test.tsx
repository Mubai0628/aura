import type { GitWorkspaceReadRequestV1 } from '@aura/contracts/generated';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test } from 'vitest';
import { ReviewPanel } from './ReviewPanel';
import { resetReviewOptionsForTests } from './review-options';
import { resetReviewStoreForTests } from './review-store';
import {
  answer,
  installGit,
  listings,
  patches,
  session,
  status,
  workspace,
} from './review-fixture';

// "每次我打开审查页面都要重新读取git信息，改动变化也都要重新读取……这里是没加
// 缓存吗" — the page shows what it read last at once and checks Git quietly behind it.

let git: ReturnType<typeof installGit>;
const readme = patches['README.md']!;
beforeEach(() => {
  resetReviewOptionsForTests();
  resetReviewStoreForTests();
  git = installGit();
});
afterEach(() => {
  cleanup();
  patches['README.md'] = readme;
});

const card = (path: string) => screen.getByRole('article', { name: `${path} 的改动` });

/** Reads (those `only` picks, or all) wait until the test lets them answer, with what Core
 *  would answer then; the others answer at once. */
function holdReads(only: (request: GitWorkspaceReadRequestV1) => boolean = () => true) {
  const held: (() => void)[] = [];
  git.read.mockImplementation((request: GitWorkspaceReadRequestV1) =>
    only(request)
      ? (new Promise((resolve) => held.push(() => resolve(answer(request)))) as never)
      : (Promise.resolve(answer(request)) as never),
  );
  return () => {
    for (const release of held.splice(0)) release();
  };
}

const readsReadme = (request: GitWorkspaceReadRequestV1) =>
  request.query.kind === 'review_diff' && request.query.input.paths?.[0] === 'README.md';

async function openedOnce() {
  const shown = render(<ReviewPanel workspaceId={workspace} sessionId={session} />);
  await screen.findByText('5 个文件 · +17 −11');
  await waitFor(() => expect(card('README.md').textContent).toContain('说 hello'));
  shown.unmount();
}

test('opened again, the page shows what it read at once and checks Git behind it', async () => {
  await openedOnce();
  const before = git.read.mock.calls.length;
  const release = holdReads();
  render(<ReviewPanel workspaceId={workspace} sessionId={session} />);
  // At once: no "正在读取 Git…", the files and the diff read last.
  expect(screen.getByText(/5 个文件 · \+17 −11/u)).toBeTruthy();
  expect(screen.queryByText('正在读取 Git…')).toBeNull();
  expect(card('README.md').textContent).toContain('说 hello');
  expect(screen.getByRole('status').textContent).toBe('正在检查改动…');
  await waitFor(() => expect(git.read.mock.calls.length).toBe(before + 2));
  release();
  await waitFor(() => expect(screen.queryByText('正在检查改动…')).toBeNull());
});

test('a diff on screen stays while it is read again, then shows what changed', async () => {
  await openedOnce();
  patches['README.md'] = readme.replace('+说 hello', '+说 你好');
  const before = git.read.mock.calls.length;
  // The check of Git and README's diff wait; the other files answer at once.
  const release = holdReads(
    (request) =>
      request.query.kind === 'status' ||
      readsReadme(request) ||
      (request.query.kind === 'review_diff' && !request.query.input.paths?.length),
  );
  render(<ReviewPanel workspaceId={workspace} sessionId={session} />);
  expect(card('README.md').textContent).toContain('说 hello');
  await waitFor(() => expect(git.read.mock.calls.length).toBe(before + 2));
  release();
  // README is read again; its old lines stay meanwhile.
  await waitFor(() =>
    expect(git.read.mock.calls.filter(([request]) => readsReadme(request))).toHaveLength(2),
  );
  expect(card('README.md').textContent).toContain('说 hello');
  expect(card('README.md').textContent).not.toContain('正在读取差异');
  release();
  await waitFor(() => expect(card('README.md').textContent).toContain('说 你好'));
});

test('a file that left the comparison leaves the page after the check', async () => {
  await openedOnce();
  git.read.mockImplementation(async (request: GitWorkspaceReadRequestV1) =>
    request.query.kind === 'status'
      ? { ok: true, value: { kind: 'status', result: { ...status(), entries: [] } } }
      : answer(request),
  );
  render(<ReviewPanel workspaceId={workspace} sessionId={session} />);
  expect(card('notes/new.md')).toBeTruthy();
  await waitFor(() =>
    expect(screen.queryByRole('article', { name: 'notes/new.md 的改动' })).toBeNull(),
  );
});

test('the file list asks Git for line counts alone', async () => {
  render(<ReviewPanel workspaceId={workspace} sessionId={session} />);
  await screen.findByText('5 个文件 · +17 −11');
  expect(listings(git.read).every((input) => input?.stats_only === true)).toBe(true);
});

test('the comparison and the closed files stay as they were left in the workspace', async () => {
  const user = userEvent.setup();
  const first = render(<ReviewPanel workspaceId={workspace} sessionId={session} />);
  await screen.findByText('5 个文件 · +17 −11');
  await user.click(screen.getByRole('button', { name: /未提交的改动/u }));
  await user.click(screen.getByRole('menuitemradio', { name: '全部改动（与 main 比较）' }));
  await screen.findByRole('button', { name: /全部改动 · 与 main 比较/u });
  await user.click(within(card('README.md')).getByRole('button', { name: 'README.md' }));
  first.unmount();
  render(<ReviewPanel workspaceId={workspace} sessionId={session} />);
  expect(screen.getByRole('button', { name: /全部改动 · 与 main 比较/u })).toBeTruthy();
  expect(card('README.md').getAttribute('data-collapsed')).toBe('true');
});
