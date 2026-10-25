import { expect, test } from 'vitest';
import {
  arrange,
  byFolder,
  comparisonLabel,
  defaultBranchOf,
  fileKind,
  type ReviewFile,
} from './review-model';

// The review page's comparisons, its file kinds and its folder groups.

const file = (path: string): ReviewFile => ({ path, binary: false, untracked: false });
// Core names branches by their full refs.
const branch = (name: string, remote = false) => ({
  name: `refs/${remote ? 'remotes' : 'heads'}/${name}`,
  remote,
  commit: 'a'.repeat(40),
});

test('"全部改动" compares with main, else master, else the upstream', () => {
  expect(defaultBranchOf([branch('feature'), branch('main')], 'origin/feature')).toBe('main');
  expect(defaultBranchOf([branch('master'), branch('origin/main', true)], null)).toBe('master');
  expect(defaultBranchOf([branch('feature'), branch('origin/main', true)], null)).toBe(
    'origin/main',
  );
  expect(defaultBranchOf([branch('feature')], 'origin/feature')).toBe('origin/feature');
  expect(defaultBranchOf([branch('feature')], null)).toBeUndefined();
  expect(comparisonLabel({ kind: 'uncommitted' })).toBe('未提交的改动');
  expect(comparisonLabel({ kind: 'branch', branch: 'main' }, 'main')).toBe(
    '全部改动 · 与 main 比较',
  );
  expect(comparisonLabel({ kind: 'branch', branch: 'dev' }, 'main')).toBe('与 dev 比较');
});

test('tests and what builds write are told apart from the source', () => {
  for (const path of [
    'src/main.rs',
    'apps/desktop/renderer/src/App.tsx',
    'docs/user/README.md',
    'contest.ts',
  ])
    expect(fileKind(path), path).toBe('source');
  for (const path of [
    'tests/api.rs',
    'src/App.test.tsx',
    'src/__tests__/a.ts',
    'pkg/server_test.go',
    'test_utils.py',
    'e2e/login.spec.ts',
  ])
    expect(fileKind(path), path).toBe('test');
  for (const path of [
    'pnpm-lock.yaml',
    'Cargo.lock',
    'dist/app.js',
    'packages/contracts/src/generated/contracts.ts',
    'web/app.min.js',
    'src/__snapshots__/a.snap',
  ])
    expect(fileKind(path), path).toBe('build');
});

test('files keep their order; grouped by folder, or by kind with tests and builds apart', () => {
  const files = ['src/b.rs', 'README.md', 'src/a.rs', 'tests/x.rs', 'Cargo.lock'].map(file);
  expect(arrange(files, false)[0]!.files.map((item) => item.path)).toEqual([
    'Cargo.lock',
    'README.md',
    'src/a.rs',
    'src/b.rs',
    'tests/x.rs',
  ]);
  expect(
    arrange(files, true).map((group) => [group.kind, group.files.map((item) => item.path)]),
  ).toEqual([
    ['source', ['README.md', 'src/a.rs', 'src/b.rs']],
    ['test', ['tests/x.rs']],
    ['build', ['Cargo.lock']],
  ]);
  expect(byFolder(arrange(files, false)[0]!.files).map((group) => group.folder)).toEqual([
    '',
    'src',
    'tests',
  ]);
});
