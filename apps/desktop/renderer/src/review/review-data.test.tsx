import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test } from 'vitest';
import type { GitWorkspaceReadRequestV1 } from '@aura/contracts/generated';
import { ReviewPanel } from './ReviewPanel';
import { resetReviewOptionsForTests } from './review-options';
import { resetReviewStoreForTests } from './review-store';
import { answer, installGit, session, workspace } from './review-fixture';

// One Git read takes about a second on Windows, so the review asks for the
// status and the changed files at once instead of one after the other.

let git: ReturnType<typeof installGit>;
beforeEach(() => {
  resetReviewOptionsForTests();
  resetReviewStoreForTests();
  git = installGit();
});
afterEach(cleanup);

test('the status and the changed files are read at the same time', async () => {
  const held: ((value: unknown) => void)[] = [];
  git.read.mockImplementation(
    (request: GitWorkspaceReadRequestV1) =>
      new Promise((resolve) => held.push(() => resolve(answer(request)))) as never,
  );
  render(<ReviewPanel workspaceId={workspace} sessionId={session} />);
  await waitFor(() => expect(git.read).toHaveBeenCalledTimes(2));
  expect(git.read.mock.calls.map(([request]) => request.query.kind).sort()).toEqual([
    'review_diff',
    'status',
  ]);
  for (const release of held.splice(0)) release(undefined);
  // The list shows at once; the new file's lines follow once it has been counted.
  expect(await screen.findByText('5 个文件 · +16 −11')).toBeTruthy();
  await waitFor(() => expect(held.length).toBeGreaterThan(0));
  for (const release of held.splice(0)) release(undefined);
  expect(await screen.findByText('5 个文件 · +17 −11')).toBeTruthy();
});
