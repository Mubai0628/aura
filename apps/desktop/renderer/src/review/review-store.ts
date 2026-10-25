import type {
  GitRuntimeStatusV1,
  GitStatusResultV1,
  GitWorkspaceReadRequestV1,
} from '@aura/contracts/generated';
import { userMessage } from '../agent-session/user-message';
import { PatchReads } from './patch-reads';
import { COUNTED_NEW_FILES, type Comparison, type ReviewFile } from './review-model';

/** Untracked files listed by one status read. */
const UNTRACKED = 2000;
/** Comparisons kept while the app runs; the one used longest ago goes first. */
const KEPT = 8;

/** What the review page last read of one workspace's comparison. */
export interface ReviewSnapshot {
  runtime?: GitRuntimeStatusV1;
  status?: GitStatusResultV1;
  files: ReviewFile[];
  /** More untracked files exist than one status read lists. */
  partial: boolean;
  reads?: PatchReads;
  error?: string;
  /** Git is being read: the first time, or quietly behind what is shown. */
  checking: boolean;
}

interface Entry {
  snapshot: ReviewSnapshot;
  /** Who asked last: the next check reads in that conversation's scope. */
  scope: Scope;
  running: boolean;
  /** Asked again while a check ran: one more follows it. */
  again: boolean;
  used: number;
}

interface Scope {
  workspace: string;
  session: string;
  base: string | null;
  ignoreWhitespace: boolean;
}

const entries = new Map<string, Entry>();
const listeners = new Set<() => void>();
let clock = 0;

export const reviewKey = (workspace: string, base: string | null, ignoreWhitespace: boolean) =>
  JSON.stringify([workspace, base, ignoreWhitespace]);

export function subscribeReviews(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export const reviewSnapshot = (key: string): ReviewSnapshot | undefined =>
  entries.get(key)?.snapshot;

function publish(entry: Entry, snapshot: ReviewSnapshot): void {
  entry.snapshot = snapshot;
  for (const listener of listeners) listener();
}

/**
 * What the review page read is kept
 * per workspace and comparison while the app runs. Opening the page again shows it at once and
 * checks Git quietly behind it; a check asked for while one runs follows it once.
 */
export function checkReview(scope: Scope): void {
  const key = reviewKey(scope.workspace, scope.base, scope.ignoreWhitespace);
  let entry = entries.get(key);
  if (!entry) {
    entry = {
      snapshot: { files: [], partial: false, checking: true },
      scope,
      running: false,
      again: false,
      used: 0,
    };
    entries.set(key, entry);
    forgetOldest();
  }
  entry.scope = scope;
  entry.used = ++clock;
  if (entry.running) {
    entry.again = true;
    return;
  }
  entry.running = true;
  if (!entry.snapshot.checking) publish(entry, { ...entry.snapshot, checking: true });
  void run(key, entry);
}

async function run(key: string, entry: Entry): Promise<void> {
  const scope = entry.scope;
  try {
    const read = await readGit(scope);
    if (entries.get(key) !== entry) return;
    // A new file keeps the lines counted last until it is counted again, so totals do not jump.
    const counted = new Map(
      entry.snapshot.files
        .filter((file) => file.untracked && file.added != null)
        .map((file) => [file.path, file]),
    );
    read.files = read.files.map((file) => {
      const last = file.untracked ? counted.get(file.path) : undefined;
      return last
        ? { ...file, added: last.added, deleted: last.deleted, binary: last.binary }
        : file;
    });
    const reads =
      entry.snapshot.reads ??
      new PatchReads(
        window.aura.v1.git,
        scope.workspace,
        scope.session,
        scope.base,
        scope.ignoreWhitespace,
      );
    reads.useSession(scope.session);
    reads.refresh(new Set(read.files.map((file) => file.path)));
    publish(entry, { ...read, reads, checking: false });
    await countNewFiles(key, entry, scope);
  } catch (error) {
    if (entries.get(key) !== entry) return;
    // A failed check keeps what was shown, and says so.
    publish(entry, {
      ...entry.snapshot,
      checking: false,
      error: userMessage(error, '无法读取仓库。'),
    });
  } finally {
    entry.running = false;
    if (entry.again && entries.get(key) === entry) {
      entry.again = false;
      checkReview(entry.scope);
    }
  }
}

/**
 * The changed files come from one `review_diff` read of their line counts alone (each file's diff
 * is read on its own, as it is shown); untracked files come from the status, which also names the
 * branch and the branches to compare with. Both are asked for at once.
 */
async function readGit(scope: Scope): Promise<Omit<ReviewSnapshot, 'checking' | 'reads'>> {
  const api = window.aura.v1.git;
  const query = async (value: GitWorkspaceReadRequestV1['query']) => {
    const result = await api.read({
      workspace_id: scope.workspace,
      session_id: scope.session,
      query: value,
    });
    if (!result.ok) throw result.error;
    return result.value;
  };
  const runtime = await api.runtime();
  if (!runtime.ok) throw runtime.error;
  if (runtime.value.state !== 'available')
    return { runtime: runtime.value, files: [], partial: false };
  const [status, list] = await Promise.all([
    query({
      kind: 'status',
      input: { include_untracked: true, include_branches: true, offset: 0, limit: UNTRACKED },
    }),
    query({
      kind: 'review_diff',
      input: {
        base: scope.base,
        paths: [],
        ignore_whitespace: scope.ignoreWhitespace,
        stats_only: true,
        offset_bytes: 0,
        max_bytes: 1,
      },
    }),
  ]);
  if (status.kind !== 'status') throw Error('Git 状态响应不匹配，请刷新后重试。');
  if (list.kind !== 'diff') throw Error('Git 差异响应不匹配，请刷新后重试。');
  const tracked: ReviewFile[] = list.result.files.map((file) => ({
    path: file.path,
    original: file.original_path ?? undefined,
    added: file.added,
    deleted: file.deleted,
    binary: file.binary,
    untracked: false,
    note: file.comparison_note,
  }));
  const known = new Set(tracked.map((file) => file.path));
  const untracked: ReviewFile[] = status.result.entries
    .filter((entry) => entry.untracked && !known.has(entry.path))
    .map((entry) => ({
      path: entry.path,
      binary: false,
      untracked: true,
      note: entry.comparison_note,
    }));
  return {
    runtime: runtime.value,
    status: status.result,
    files: [...tracked, ...untracked],
    partial: status.result.next_offset != null || status.result.truncated,
  };
}

/**
 * A new file is lines too — a 1068-line one read "+0 −0". The first
 * `COUNTED_NEW_FILES` new files are counted one by one with the read the page already uses for a
 * new file's lines (`untracked_diff`, its first byte only: Git counts the whole file), and the
 * totals follow as each answers. A file that cannot be counted stays as it was.
 */
async function countNewFiles(key: string, entry: Entry, scope: Scope): Promise<void> {
  const fresh = entry.snapshot.files.filter((file) => file.untracked).slice(0, COUNTED_NEW_FILES);
  for (const file of fresh) {
    if (entries.get(key) !== entry) return;
    let counted;
    try {
      const result = await window.aura.v1.git.read({
        workspace_id: scope.workspace,
        session_id: scope.session,
        query: {
          kind: 'untracked_diff',
          input: { path: file.path, offset_bytes: 0, max_bytes: 1 },
        },
      });
      if (result.ok && result.value.kind === 'diff') counted = result.value.result.files[0];
    } catch {
      // Counted again at the next check.
    }
    if (!counted || entries.get(key) !== entry) continue;
    const shown = entry.snapshot;
    publish(entry, {
      ...shown,
      files: shown.files.map((item) =>
        item.untracked && item.path === file.path
          ? { ...item, added: counted.added, deleted: counted.deleted, binary: counted.binary }
          : item,
      ),
    });
  }
}

function forgetOldest(): void {
  while (entries.size > KEPT) {
    let oldest: [string, Entry] | undefined;
    for (const item of entries) if (!oldest || item[1].used < oldest[1].used) oldest = item;
    if (!oldest) return;
    oldest[1].snapshot.reads?.dispose();
    entries.delete(oldest[0]);
  }
}

/** How the page looked for one workspace: the comparison, closed files, cards shown, scroll. */
export interface ReviewView {
  comparison: Comparison;
  collapsed: ReadonlySet<string>;
  limit: number;
  scrollTop: number;
}

const views = new Map<string, ReviewView>();

export const reviewView = (workspace: string): ReviewView | undefined => views.get(workspace);

export function keepReviewView(workspace: string, view: ReviewView): void {
  views.set(workspace, view);
}

export function resetReviewStoreForTests(): void {
  for (const entry of entries.values()) entry.snapshot.reads?.dispose();
  entries.clear();
  views.clear();
  clock = 0;
}
