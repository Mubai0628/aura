import type {
  GitDiffResultV1,
  GitFileStatV1,
  GitStatusResultV1,
  GitWorkspaceReadRequestV1,
} from '@aura/contracts/generated';
import { vi } from 'vitest';

/** Tests: a repository on `feature`, with uncommitted changes, read through the Git bridge. */
export const workspace = 'workspace-one';
export const session = 'session-one';

export const stat = (path: string, added: number, deleted: number): GitFileStatV1 => ({
  path,
  original_path: null,
  added,
  deleted,
  binary: false,
  comparison_note: null,
});

export const files = [
  stat('src/app.ts', 2, 1),
  stat('README.md', 1, 0),
  stat('tests/app.test.ts', 3, 0),
  stat('pnpm-lock.yaml', 10, 10),
];

export const patches: Record<string, string> = {
  'src/app.ts': [
    'diff --git a/src/app.ts b/src/app.ts',
    '--- a/src/app.ts',
    '+++ b/src/app.ts',
    '@@ -1,3 +1,4 @@',
    ' const name = "aura";',
    '-console.log("hello");',
    '+console.log("hello, world");',
    '+export {};',
    ' run();',
    '',
  ].join('\n'),
  'README.md': [
    'diff --git a/README.md b/README.md',
    '--- a/README.md',
    '+++ b/README.md',
    '@@ -1 +1,2 @@',
    ' # Demo',
    '+说 hello',
    '',
  ].join('\n'),
  'notes/new.md': [
    'diff --git a/notes/new.md b/notes/new.md',
    'new file mode 100644',
    '--- /dev/null',
    '+++ b/notes/new.md',
    '@@ -0,0 +1 @@',
    '+新的笔记',
    '',
  ].join('\n'),
};

export const status = (): GitStatusResultV1 => ({
  branch: 'feature',
  head: 'a'.repeat(40),
  detached: false,
  upstream: 'origin/feature',
  ahead: 2,
  behind: 1,
  // Core lists branches by their full ref names.
  branches: [
    { name: 'refs/heads/feature', remote: false, commit: 'a'.repeat(40) },
    { name: 'refs/heads/main', remote: false, commit: 'b'.repeat(40) },
    { name: 'refs/remotes/origin/HEAD', remote: true, commit: 'b'.repeat(40) },
    { name: 'refs/remotes/origin/main', remote: true, commit: 'b'.repeat(40) },
  ],
  conflicts: [],
  entries: [
    {
      path: 'notes/new.md',
      original_path: null,
      index_status: '?',
      worktree_status: '?',
      untracked: true,
      comparison_note: null,
      conflict: false,
      submodule: false,
    },
  ],
  next_offset: null,
  operation: null,
  submodule_contents_checked: false,
  total_entries: 1,
  truncated: false,
});

export const diff = (
  patch: string,
  listed = files,
  offset = 0,
  next: number | null = null,
): GitDiffResultV1 => ({
  files: listed,
  patch,
  offset_bytes: offset,
  next_offset: next,
  total_bytes: patch.length,
  truncated: next !== null,
  warnings: [],
});

/** The answer to one read, as Core would give it. */
export function answer(request: GitWorkspaceReadRequestV1) {
  const query = request.query;
  if (query.kind === 'status') return { ok: true, value: { kind: 'status', result: status() } };
  if (query.kind === 'review_diff') {
    const paths = query.input.paths ?? [];
    if (!paths.length) return { ok: true, value: { kind: 'diff', result: diff('') } };
    const patch = patches[paths[0]!] ?? '';
    return {
      ok: true,
      value: {
        kind: 'diff',
        result: diff(
          patch,
          files.filter((file) => file.path === paths[0]),
        ),
      },
    };
  }
  if (query.kind === 'untracked_diff') {
    // Like Core: one new file, its lines counted by Git (`--numstat`), and its diff paged.
    const patch = patches[query.input.path] ?? '';
    const added = patch
      .split('\n')
      .filter((line) => line.startsWith('+') && !line.startsWith('+++'));
    return {
      ok: true,
      value: {
        kind: 'diff',
        result: diff(patch, [stat(query.input.path, added.length, 0)]),
      },
    };
  }
  throw Error(`unexpected query ${query.kind}`);
}

export function installGit() {
  const read = vi.fn(async (request: GitWorkspaceReadRequestV1) => answer(request));
  const runtime = vi.fn(async () => ({
    ok: true,
    value: {
      state: 'available',
      executable: 'git.exe',
      version: '2.45.0',
      message: '',
      next_step: '',
      installation_url: 'https://git-scm.com/downloads/win',
    },
  }));
  const openInstallation = vi.fn(async () => ({ ok: true, value: { opened: true } }));
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { git: { runtime, read, openInstallation } } },
  });
  return { read, runtime, openInstallation };
}

/** The `review_diff` reads that list the changed files (no paths), in order. */
export const listings = (read: ReturnType<typeof installGit>['read']) =>
  read.mock.calls
    .map(([request]) => request.query)
    .filter((query) => query.kind === 'review_diff' && !query.input.paths?.length)
    .map((query) => (query.kind === 'review_diff' ? query.input : undefined));
