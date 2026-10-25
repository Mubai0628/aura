import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { expect, test } from 'vitest';
import { isGitParams, parseGitResult } from './git-validation.js';
const fixture = JSON.parse(
  readFileSync(
    resolve(import.meta.dirname, '../../../fixtures/contracts/git-review.v1.json'),
    'utf8',
  ),
);
test('desktop untracked query validates its generated Rust fixture and rejects caller authority or extra paths', () => {
  expect(isGitParams('git.workspace.read', fixture.request)).toBe(true);
  expect(parseGitResult('git.workspace.read', fixture.result)).toEqual(fixture.result);
  for (const input of [
    { path: 'new', max_bytes: 0 },
    { path: 'new', max_bytes: 65537 },
    { path: 'new', actor: 'model' },
    { paths: ['new'] },
  ]) {
    expect(
      isGitParams('git.workspace.read', {
        ...fixture.request,
        query: { kind: 'untracked_diff', input },
      }),
    ).toBe(false);
  }
});
