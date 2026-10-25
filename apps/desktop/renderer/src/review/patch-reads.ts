import type { GitDiffResultV1, GitReadInputV1 } from '@aura/contracts/generated';
import type { GitApiV1 } from '../../../shared/git-api';
import { userMessage } from '../agent-session/user-message';
import { parseDiff, type ParsedDiff } from './diff-parse';
import type { ReviewFile } from './review-model';

/** One request reads at most 64 KiB; a file shows about this much before asking for more. */
const PAGE = 65_536;
const SHOWN = 512 * 1024;

export interface PatchState {
  loading: boolean;
  text: string;
  diff?: ParsedDiff;
  /** Where the rest starts, while the file has more than is shown. */
  next?: number;
  error?: string;
  /** Read before Git was last checked: still shown, and read again when it is near. */
  stale?: boolean;
}

interface Job {
  key: string;
  run: () => Promise<void>;
}

/**
 * Each file's diff, read for the comparison on screen. Two reads run at a time; the one the
 * user asks for goes first; reads still waiting when the comparison changes are dropped.
 * The diffs are kept with the comparison while the app runs. After Git is
 * checked again, a diff already read stays on screen while it is read again.
 */
export class PatchReads {
  readonly #states = new Map<string, PatchState>();
  readonly #listeners = new Set<() => void>();
  /** Files queued or being read. */
  readonly #pending = new Set<string>();
  #queue: Job[] = [];
  #active = 0;
  #disposed = false;
  #version = 0;
  #session: string;

  constructor(
    private readonly api: GitApiV1,
    private readonly workspace: string,
    session: string,
    private readonly base: string | null,
    private readonly ignoreWhitespace: boolean,
  ) {
    this.#session = session;
  }

  get = (path: string): PatchState | undefined => this.#states.get(path);

  /** Changes whenever any file's diff does. */
  version = (): number => this.#version;

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => {
      this.#listeners.delete(listener);
    };
  };

  /** The conversation whose scope the reads use: any of the workspace's will do. */
  useSession(session: string): void {
    this.#session = session;
  }

  /**
   * Git was checked again: the files that left the comparison are forgotten, and every diff read
   * before is read again when its card is near, staying on screen meanwhile.
   */
  refresh(paths: ReadonlySet<string>): void {
    for (const [path, state] of this.#states) {
      if (!paths.has(path)) this.#states.delete(path);
      else if (!state.loading && !state.stale) this.#states.set(path, { ...state, stale: true });
    }
    this.#notify();
  }

  /** Reads a file's diff once — again after a failure or a check of Git; `first` puts it ahead. */
  request(file: ReviewFile, first = false): void {
    if (this.#disposed) return;
    const state = this.#states.get(file.path);
    if (this.#pending.has(file.path) || (state && !state.error && !state.stale)) {
      if (first) this.#promote(file.path);
      return;
    }
    if (state?.stale && state.diff) {
      this.#enqueue({ key: file.path, run: () => this.#read(file, 0, '', state) }, first);
      return;
    }
    this.#set(file.path, { loading: true, text: '' });
    this.#enqueue({ key: file.path, run: () => this.#read(file, 0, '') }, first);
  }

  /** The next part of a long diff. */
  more(file: ReviewFile): void {
    const state = this.#states.get(file.path);
    if (this.#disposed || !state || state.loading || state.next === undefined) return;
    const { next, text } = state;
    this.#set(file.path, { ...state, loading: true });
    this.#enqueue({ key: file.path, run: () => this.#read(file, next, text) }, true);
  }

  dispose(): void {
    this.#disposed = true;
    this.#queue = [];
  }

  /** `shown`: the diff on screen while the file is read again from its start. */
  async #read(file: ReviewFile, offset: number, before: string, shown?: PatchState): Promise<void> {
    let text = before;
    let next: number | undefined = offset;
    try {
      while (next !== undefined && text.length - before.length < SHOWN) {
        const page = await this.#page(file, next);
        if (this.#disposed) return;
        text += page.patch;
        next = page.next_offset ?? undefined;
      }
      if (shown && shown.text === text && shown.next === next) {
        // Nothing changed: the card keeps what it shows.
        this.#states.set(file.path, { ...shown, stale: false });
        return;
      }
      // A part may end inside a line; that line waits for the next part.
      const lines = next === undefined ? text : text.slice(0, text.lastIndexOf('\n') + 1);
      this.#set(file.path, {
        loading: false,
        text,
        diff: parseDiff(lines),
        ...(next === undefined ? {} : { next }),
      });
    } catch (error) {
      if (this.#disposed) return;
      if (shown) {
        this.#set(file.path, {
          ...shown,
          stale: false,
          error: userMessage(error, '没能读取最新的差异，显示的是上次读到的内容。'),
        });
        return;
      }
      this.#set(file.path, {
        loading: false,
        text: before,
        ...(before ? { diff: parseDiff(before), next: offset } : {}),
        error: userMessage(error, '这个文件的差异读取失败，请重试。'),
      });
    }
  }

  async #page(file: ReviewFile, offset: number): Promise<GitDiffResultV1> {
    const query: GitReadInputV1 = file.untracked
      ? {
          kind: 'untracked_diff',
          input: { path: file.path, offset_bytes: offset, max_bytes: PAGE },
        }
      : {
          kind: 'review_diff',
          input: {
            base: this.base,
            paths: file.original ? [file.path, file.original] : [file.path],
            ignore_whitespace: this.ignoreWhitespace,
            offset_bytes: offset,
            max_bytes: PAGE,
          },
        };
    const result = await this.api.read({
      workspace_id: this.workspace,
      session_id: this.#session,
      query,
    });
    if (!result.ok) throw result.error;
    if (result.value.kind !== 'diff' || result.value.result.offset_bytes !== offset)
      throw Error('差异响应与当前部分不匹配，请刷新后重试。');
    return result.value.result;
  }

  #enqueue(job: Job, first: boolean): void {
    this.#pending.add(job.key);
    if (first) this.#queue.unshift(job);
    else this.#queue.push(job);
    this.#drain();
  }

  #promote(key: string): void {
    const index = this.#queue.findIndex((job) => job.key === key);
    if (index > 0) this.#queue.unshift(...this.#queue.splice(index, 1));
  }

  #drain(): void {
    while (!this.#disposed && this.#active < 2 && this.#queue.length) {
      const job = this.#queue.shift()!;
      this.#active += 1;
      void job.run().finally(() => {
        this.#pending.delete(job.key);
        this.#active -= 1;
        this.#drain();
      });
    }
  }

  #set(path: string, state: PatchState): void {
    this.#states.set(path, state);
    this.#notify();
  }

  #notify(): void {
    this.#version += 1;
    for (const listener of this.#listeners) listener();
  }
}
