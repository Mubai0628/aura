import type { GitWorkspaceReadRequestV1 } from '@aura/contracts/generated';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test } from 'vitest';
import { ReviewPanel } from './ReviewPanel';
import { resetReviewOptionsForTests } from './review-options';
import { resetReviewStoreForTests } from './review-store';
import { answer, installGit, session, status, workspace } from './review-fixture';

// The summary of what is uncommitted counts lines — "总结审查当前未提交内容改变
// 的行数汇总" — and a new file Git does not know yet is lines too: a 1068-line new file read
// "+0 −0". Up to ten new files are counted, each read on its own as the page already reads them;
// beyond that the summary says how many are not counted.

let git: ReturnType<typeof installGit>;
beforeEach(() => {
  resetReviewOptionsForTests();
  resetReviewStoreForTests();
  git = installGit();
});
afterEach(cleanup);

const newFiles = (count: number) =>
  Array.from({ length: count }, (_, index) => ({
    path: `new/${String(index).padStart(2, '0')}.md`,
    original_path: null,
    index_status: '?',
    worktree_status: '?',
    untracked: true,
    comparison_note: null,
    conflict: false,
    submodule: false,
  }));

test('a new file’s lines are in the summary', async () => {
  render(<ReviewPanel workspaceId={workspace} sessionId={session} />);
  // The four changed files add 16 lines and delete 11; notes/new.md adds 1.
  expect(await screen.findByText('5 个文件 · +17 −11')).toBeTruthy();
  // Counting reads one byte of the new file (Git counts it whole); showing it pages its diff.
  const counted = git.read.mock.calls
    .map(([request]) => request.query)
    .filter((query) => query.kind === 'untracked_diff' && query.input.max_bytes === 1);
  expect(counted).toEqual([
    { kind: 'untracked_diff', input: { path: 'notes/new.md', offset_bytes: 0, max_bytes: 1 } },
  ]);
});

test('beyond ten new files the summary says how many are not counted', async () => {
  git.read.mockImplementation(async (request: GitWorkspaceReadRequestV1) => {
    if (request.query.kind === 'status') {
      const many = status();
      many.entries = newFiles(13);
      many.total_entries = 13;
      return { ok: true, value: { kind: 'status', result: many } } as never;
    }
    if (request.query.kind === 'untracked_diff')
      return {
        ok: true,
        value: {
          kind: 'diff',
          result: {
            files: [
              {
                path: request.query.input.path,
                original_path: null,
                added: 2,
                deleted: 0,
                binary: false,
                comparison_note: null,
              },
            ],
            patch: '',
            offset_bytes: 0,
            next_offset: null,
            total_bytes: 0,
            truncated: false,
            warnings: [],
          },
        },
      } as never;
    return answer(request) as never;
  });
  render(<ReviewPanel workspaceId={workspace} sessionId={session} />);
  // 4 changed files (+16 −11) and 13 new ones, 10 of them counted at 2 lines each.
  expect(await screen.findByText('17 个文件 · +36 −11（另有 3 个新文件未计行数）')).toBeTruthy();
  expect(
    git.read.mock.calls.filter(
      ([request]) => request.query.kind === 'untracked_diff' && request.query.input.max_bytes === 1,
    ),
  ).toHaveLength(10);
});
