import type { GitBranchV1 } from '@aura/contracts/generated';

/**
 * What the review page compares, which kind of file each change is, and how
 * the file list is grouped.
 */

/** The working tree against HEAD, or against the commit where HEAD left a branch. */
export type Comparison = { kind: 'uncommitted' } | { kind: 'branch'; branch: string };

export function comparisonLabel(comparison: Comparison, defaultBranch?: string): string {
  if (comparison.kind === 'uncommitted') return '未提交的改动';
  return comparison.branch === defaultBranch
    ? `全部改动 · 与 ${comparison.branch} 比较`
    : `与 ${comparison.branch} 比较`;
}

/** A branch as people name it — `main`, `origin/main` — from Core's `refs/heads/main`. */
export const branchName = (ref: string) => ref.replace(/^refs\/(?:heads|remotes)\//u, '');

/** The branch "全部改动" compares with: main, else master, else the upstream's branch. */
export function defaultBranchOf(
  branches: GitBranchV1[],
  upstream: string | null | undefined,
): string | undefined {
  const local = branches.filter((branch) => !branch.remote).map((b) => branchName(b.name));
  for (const name of ['main', 'master']) if (local.includes(name)) return name;
  const remote = branches.filter((branch) => branch.remote).map((b) => branchName(b.name));
  for (const name of ['origin/main', 'origin/master']) if (remote.includes(name)) return name;
  return upstream ?? undefined;
}

export interface ReviewFile {
  path: string;
  original?: string | undefined;
  added?: number | null | undefined;
  deleted?: number | null | undefined;
  binary: boolean;
  /** Not yet known to Git: a new file listed by status, read on its own. */
  untracked: boolean;
  note?: string | null | undefined;
}

/** New files whose lines are counted for the totals (each is one read of its own). */
export const COUNTED_NEW_FILES = 10;

/**
 * What the changed files add up to — lines
 * added and deleted, new files' too once counted — and how many new files are left uncounted
 * because there are more than are counted.
 */
export function reviewTotals(files: readonly ReviewFile[]) {
  let added = 0;
  let deleted = 0;
  for (const file of files) {
    added += file.added ?? 0;
    deleted += file.deleted ?? 0;
  }
  const fresh = files.filter((file) => file.untracked).length;
  return {
    files: files.length,
    added,
    deleted,
    uncounted: Math.max(0, fresh - COUNTED_NEW_FILES),
  };
}

export type FileKind = 'source' | 'test' | 'build';

const TEST =
  /(^|\/)(tests?|__tests__|specs?|e2e|fixtures?)\/|[._-](test|spec)s?\.[a-z0-9]+$|_test\.[a-z0-9]+$|(^|\/)test_[^/]+$/iu;
const BUILD =
  /(^|\/)(dist|build|out|target|generated|coverage|\.next|vendor|node_modules)\/|(^|\/)(package-lock\.json|pnpm-lock\.yaml|yarn\.lock|cargo\.lock|poetry\.lock|go\.sum|composer\.lock|gemfile\.lock)$|\.(min\.(js|css)|map|snap|lock)$/iu;

/** Tests, and what builds or tools write, apart from the source changes. */
export function fileKind(path: string): FileKind {
  if (BUILD.test(path)) return 'build';
  if (TEST.test(path)) return 'test';
  return 'source';
}

export const KIND_LABELS: Record<FileKind, string> = {
  source: '源码与其他',
  test: '测试文件',
  build: '构建与生成文件',
};

export function folderOf(path: string): string {
  const slash = path.lastIndexOf('/');
  return slash < 0 ? '' : path.slice(0, slash);
}

export function nameOf(path: string): string {
  return path.slice(path.lastIndexOf('/') + 1);
}

/** Files in order, then grouped by their folder, the folders in the order they first appear. */
export function byFolder(files: ReviewFile[]): { folder: string; files: ReviewFile[] }[] {
  const groups = new Map<string, ReviewFile[]>();
  for (const file of files) {
    const folder = folderOf(file.path);
    groups.set(folder, [...(groups.get(folder) ?? []), file]);
  }
  return [...groups].map(([folder, members]) => ({ folder, files: members }));
}

/** A search match's id: the file's path, then `path` or the row's hunk and index. */
export const matchId = (path: string, at: string) => `${path}\n${at}`;

/** The order files show in: by path, and by kind first when tests and builds are apart. */
export function arrange(
  files: ReviewFile[],
  separate: boolean,
): { kind?: FileKind; files: ReviewFile[] }[] {
  const sorted = [...files].sort((left, right) => left.path.localeCompare(right.path));
  if (!separate) return [{ files: sorted }];
  return (['source', 'test', 'build'] as const)
    .map((kind) => ({ kind, files: sorted.filter((file) => fileKind(file.path) === kind) }))
    .filter((group) => group.files.length > 0);
}
