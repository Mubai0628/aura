import type { FsListEntry } from '@aura/contracts/generated';
import { useSyncExternalStore } from 'react';
import { userMessage } from '../agent-session/user-message';

/** The conversation and workspace whose files a 文件 tab reads. */
export interface FileScope {
  session: string;
  workspace: string;
  /** The workspace root on disk, to read absolute paths inside it. */
  root: string;
  /**
   * The file a message of the conversation carried under this name, as a 文件 tab entry —
   * a file outside the workspace the model names by its name alone.
   */
  attachmentNamed?: ((name: string) => string | undefined) | undefined;
  /**
   * The file inside a zip a message of the conversation carried that a link names, as a
   * 文件 tab entry; none when the conversation carried no zip.
   */
  zipEntryNamed?: ((link: string) => Promise<string | undefined>) | undefined;
}

export interface Folder {
  entries?: FsListEntry[];
  /** Where the folder's next page starts, when it holds more than the pages read so far. */
  next?: string;
  /** The next page is being read. */
  loadingMore?: boolean;
  /** Why the last read of a next page failed; the pages read stay. */
  moreError?: string;
  loading: boolean;
  error?: string;
}

/** A path being brought into view, or why that did not work. */
export interface Locating {
  path: string;
  /** A folder picked from the filter opens too; a file is only marked. */
  folder: boolean;
  /** The filter text it was picked from, to pick again. */
  from?: string;
  error?: string;
}

export interface TreeState {
  open: boolean;
  expanded: ReadonlySet<string>;
  folders: ReadonlyMap<string, Folder>;
  /** The path 在文件树中显示 or the filter last asked for: its row is marked and scrolled to. */
  located?: string;
  /** Which locate marked it: asking for the same path again scrolls to its row again. */
  locatedAt?: number;
  /** A locate under way, or the reason the last one did not reach its path. */
  locating?: Locating;
}

export const ROOT = '.';
/** A new workspace's file index takes a moment: its listing is asked again, a few times. */
const INDEX_RETRIES = 8;
const INDEX_RETRY_MS = 1000;
/** How often a locate looks again at a folder whose listing waits for the index. */
const LOCATE_WAIT_MS = 250;
const LOCATE_WAITS = ((INDEX_RETRIES + 2) * INDEX_RETRY_MS) / LOCATE_WAIT_MS;
const INITIAL: TreeState = { open: true, expanded: new Set(), folders: new Map() };

const keyOf = (scope: FileScope) => `${scope.workspace}\n${scope.session}`;
const parentOf = (path: string) =>
  path.includes('/') ? path.slice(0, path.lastIndexOf('/')) : ROOT;

/**
 * One collator for every listing: `localeCompare` with options builds one per comparison, which
 * made a 10,000-entry folder take about 0.4 s to sort on the page's thread.
 */
const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });
const order = (left: FsListEntry, right: FsListEntry) =>
  Number(right.kind === 'directory') - Number(left.kind === 'directory') ||
  collator.compare(left.name, right.name);

function listingFailure(error: unknown): string {
  const code =
    typeof error === 'object' && error !== null ? (error as { code?: unknown }).code : undefined;
  if (code === 'workspace.catalog_unavailable')
    return '文件列表暂时读不出来：工作区还在建立文件索引，请稍后重试。';
  if (code === 'file.not_found') return '文件列表暂时读不出来：这个文件夹已不存在。';
  return userMessage(error, '文件列表暂时读不出来，请重试。');
}

function locateFailure(path: string, error: unknown): string {
  const code =
    typeof error === 'object' && error !== null ? (error as { code?: unknown }).code : undefined;
  if (code === 'file.not_found') return `没有找到 ${path}：它可能已被删除或移走。`;
  if (error instanceof Error) return error.message;
  return userMessage(error, `没有定位到 ${path}，请重试。`);
}

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * One tree per conversation, shared by its 文件 tabs — whether it shows, which folders are
 * open and what each lists — so another tab, or the same tab later, finds it as it was left.
 */
class FileTreeStore {
  readonly #trees = new Map<string, TreeState>();
  readonly #listeners = new Set<() => void>();
  /** The latest listing asked for each folder; an answer to an older one is dropped. */
  readonly #generations = new Map<string, number>();
  /** First-page reads under way, by folder: a locate waits for one instead of asking again. */
  readonly #reads = new Map<string, Promise<void>>();
  /** The newest locate of each tree; an older one stops where it is. */
  readonly #locates = new Map<string, number>();
  /** Locates numbered across every tree, so a number names one locate. */
  #asked = 0;

  get = (scope: FileScope): TreeState => this.#trees.get(keyOf(scope)) ?? INITIAL;

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => {
      this.#listeners.delete(listener);
    };
  };

  setOpen(scope: FileScope, open: boolean): void {
    this.#put(scope, { ...this.get(scope), open });
  }

  /** Lists a folder once; a folder already listed, or being listed, is left alone. */
  ensure(scope: FileScope, path: string): void {
    const folder = this.get(scope).folders.get(path);
    if (!folder || (folder.error && !folder.loading)) void this.#read(scope, path);
  }

  /**
   * Lists a folder; what it listed stays shown meanwhile. Only the latest listing asked for a
   * folder is kept: one that answers after a newer one was asked is dropped, so an older answer
   * never replaces a newer one.
   */
  async load(
    scope: FileScope,
    path: string,
    retries = INDEX_RETRIES,
    generation?: number,
  ): Promise<void> {
    const key = `${keyOf(scope)}\n${path}`;
    const asked = generation ?? (this.#generations.get(key) ?? 0) + 1;
    if (generation === undefined) this.#generations.set(key, asked);
    const latest = () => this.#generations.get(key) === asked;
    if (!latest()) return;
    this.#folder(scope, path, { ...this.get(scope).folders.get(path), loading: true });
    let folder: Folder;
    try {
      const result = await window.aura.v1.listWorkspaceFiles({
        session_id: scope.session,
        workspace_id: scope.workspace,
        target: { path },
      });
      if (!latest()) return;
      if (!result.ok && result.error.code === 'workspace.catalog_unavailable' && retries > 0) {
        setTimeout(() => void this.load(scope, path, retries - 1, asked), INDEX_RETRY_MS);
        return;
      }
      // An open folder that is gone (the model deleted or moved it) leaves the tree with
      // everything opened below it, instead of staying behind as an error row.
      if (!result.ok && result.error.code === 'file.not_found' && path !== ROOT) {
        this.#forget(scope, path);
        return;
      }
      folder = result.ok
        ? {
            loading: false,
            entries: [...result.value.entries].sort(order),
            ...(result.value.next_after ? { next: result.value.next_after } : {}),
          }
        : { loading: false, error: listingFailure(result.error) };
    } catch (reason) {
      if (!latest()) return;
      folder = { loading: false, error: listingFailure(reason) };
    }
    this.#folder(scope, path, folder);
  }

  /**
   * Reads a folder's next page and adds it to the entries listed. A listing of the folder asked
   * for meanwhile (a refresh) starts it again from its first page, and this page is dropped. An
   * entry a locate added ahead of its page is not listed twice.
   */
  async more(scope: FileScope, path: string): Promise<void> {
    const folder = this.get(scope).folders.get(path);
    if (!folder?.entries || !folder.next || folder.loadingMore || folder.loading) return;
    const key = `${keyOf(scope)}\n${path}`;
    const asked = this.#generations.get(key);
    const latest = () => this.#generations.get(key) === asked;
    const asking: Folder = { ...folder, loadingMore: true };
    delete asking.moreError;
    this.#folder(scope, path, asking);
    let page: Folder;
    try {
      const result = await window.aura.v1.listWorkspaceFiles({
        session_id: scope.session,
        workspace_id: scope.workspace,
        target: { path },
        after: folder.next,
      });
      if (!latest()) return;
      const current = this.get(scope).folders.get(path) ?? folder;
      if (!result.ok) {
        page = { ...current, loadingMore: false, moreError: listingFailure(result.error) };
      } else {
        const listed = new Set((current.entries ?? []).map((entry) => entry.path));
        const added = result.value.entries.filter((entry) => !listed.has(entry.path));
        page = {
          loading: false,
          entries: [...(current.entries ?? []), ...added].sort(order),
          ...(result.value.next_after ? { next: result.value.next_after } : {}),
        };
      }
    } catch (reason) {
      if (!latest()) return;
      const current = this.get(scope).folders.get(path) ?? folder;
      page = { ...current, loadingMore: false, moreError: listingFailure(reason) };
    }
    this.#folder(scope, path, page);
  }

  toggle(scope: FileScope, path: string): void {
    const current = this.get(scope);
    const expanded = new Set(current.expanded);
    if (expanded.has(path)) expanded.delete(path);
    else expanded.add(path);
    this.#put(scope, { ...current, expanded });
    if (expanded.has(path)) this.ensure(scope, path);
  }

  /**
   * Opens a folder and every folder above it (the folder of the file a tab shows), each listed;
   * nothing is marked or scrolled to. Asked again with them open, the tree is left as it is.
   */
  reveal(scope: FileScope, path: string): void {
    const parts = path.split('/');
    const current = this.get(scope);
    const expanded = new Set(current.expanded);
    for (let index = 1; index <= parts.length; index += 1)
      expanded.add(parts.slice(0, index).join('/'));
    if (expanded.size > current.expanded.size) this.#put(scope, { ...current, expanded });
    for (const folder of [ROOT, ...expanded]) this.ensure(scope, folder);
  }

  /** A folder picked from the filter (`from`) opens, and its row is brought in and marked. */
  locateFolder(scope: FileScope, path: string, from?: string): void {
    void this.#show(scope, path, true, from);
  }

  /** 在文件树中显示 — the tree shows, the file's folders open, and its row is brought in and marked. */
  locate(scope: FileScope, path: string): void {
    void this.#show(scope, path, false);
  }

  /**
   * Brings `path` into the tree: the folders above it open, each with its first page, and every
   * row on the way present — a row whose page is not listed yet is read on its own and added,
   * the folder's paging left as it was — then marked for the list to scroll to. A folder opens
   * too. Only what lies on the path is read, however many entries come before it in its folders.
   */
  async #show(scope: FileScope, path: string, folder: boolean, from?: string): Promise<void> {
    const key = keyOf(scope);
    const asked = (this.#asked += 1);
    this.#locates.set(key, asked);
    const current = () => this.#locates.get(key) === asked;
    const parts = path.split('/');
    const above = parts.slice(0, -1).map((_, index) => parts.slice(0, index + 1).join('/'));
    const opened = [...above, ...(folder ? [path] : [])];
    const state = this.get(scope);
    const expanded = new Set([...state.expanded, ...opened]);
    const locating: Locating = { path, folder, ...(from === undefined ? {} : { from }) };
    this.#put(scope, { ...state, open: true, expanded, located: path, locatedAt: asked, locating });
    // Every folder it opens is asked for at once: a locate a newer one replaces still leaves
    // the folders it opened listed, not waiting for a read nobody started.
    for (const opening of [ROOT, ...opened]) this.ensure(scope, opening);
    try {
      const chain = [ROOT, ...above];
      for (const [index, parent] of chain.entries()) {
        await this.#loaded(scope, parent, current);
        if (!current()) return;
        const child = chain[index + 1] ?? path;
        const listed = this.get(scope).folders.get(parent)?.entries;
        if (!listed?.some((entry) => entry.path === child)) {
          const entry = await this.#entry(scope, child);
          if (!current()) return;
          this.#pin(scope, parent, entry);
        }
      }
      if (folder) await this.#loaded(scope, path, current);
      if (current()) this.#settle(scope);
    } catch (reason) {
      if (!current()) return;
      const now = this.get(scope);
      this.#put(scope, { ...now, locating: { ...locating, error: locateFailure(path, reason) } });
    }
  }

  /** Tries the last locate again. */
  retryLocate(scope: FileScope): void {
    const locating = this.get(scope).locating;
    if (locating) void this.#show(scope, locating.path, locating.folder, locating.from);
  }

  /**
   * Stops a locate under way, or puts away the reason one failed. Its path is no longer marked,
   * so the tree does not jump to the row when a later page brings it in.
   */
  dismissLocate(scope: FileScope): void {
    this.#locates.set(keyOf(scope), (this.#asked += 1));
    const now = this.get(scope);
    if (!now.locating) return;
    const next = { ...now };
    delete next.locating;
    if (next.located === now.locating.path) {
      delete next.located;
      delete next.locatedAt;
    }
    this.#put(scope, next);
  }

  /** Lists again the root and every open folder (the refresh button); a locate stops. */
  refresh(scope: FileScope): void {
    this.dismissLocate(scope);
    for (const folder of [ROOT, ...this.get(scope).expanded]) void this.load(scope, folder);
  }

  resetForTests(): void {
    this.#trees.clear();
    this.#generations.clear();
    this.#reads.clear();
    this.#locates.clear();
    this.#asked = 0;
  }

  /** Starts a folder's first page, kept until it is answered so a locate can wait for it. */
  #read(scope: FileScope, path: string): Promise<void> {
    const key = `${keyOf(scope)}\n${path}`;
    const read = this.load(scope, path).finally(() => {
      if (this.#reads.get(key) === read) this.#reads.delete(key);
    });
    this.#reads.set(key, read);
    return read;
  }

  /** A folder's first page, waited for: a read under way is shared, a failure is thrown. */
  async #loaded(scope: FileScope, path: string, current: () => boolean): Promise<void> {
    const key = `${keyOf(scope)}\n${path}`;
    for (let round = 0; round < LOCATE_WAITS && current(); round += 1) {
      const folder = this.get(scope).folders.get(path);
      if (folder?.entries) return;
      if (folder?.error && !folder.loading) throw new Error(folder.error);
      const reading = this.#reads.get(key);
      if (reading) await reading;
      else if (!folder?.loading) await this.#read(scope, path);
      // A listing that waits for the index to finish answers on its own later.
      else await wait(LOCATE_WAIT_MS);
    }
    if (current()) throw new Error('文件列表暂时读不出来，请重试。');
  }

  /** One path's row, read on its own. Below an ignored folder it is ignored too. */
  async #entry(scope: FileScope, path: string): Promise<FsListEntry> {
    const result = await window.aura.v1.statWorkspaceFile({
      session_id: scope.session,
      workspace_id: scope.workspace,
      target: { path },
    });
    if (!result.ok) throw result.error;
    const parent = parentOf(path);
    const holder = this.get(scope)
      .folders.get(parentOf(parent))
      ?.entries?.find((entry) => entry.path === parent);
    return {
      path: result.value.path,
      name: path.slice(path.lastIndexOf('/') + 1),
      kind: result.value.kind,
      byte_length: result.value.byte_length ?? null,
      ignored: result.value.ignored || holder?.ignored === true,
      generated: result.value.generated,
      reparse_tag: result.value.reparse_tag ?? null,
    };
  }

  /** Adds a row ahead of its page; the folder's next page still starts where it did. */
  #pin(scope: FileScope, parent: string, entry: FsListEntry): void {
    const folder = this.get(scope).folders.get(parent);
    if (!folder?.entries || folder.entries.some((item) => item.path === entry.path)) return;
    this.#folder(scope, parent, { ...folder, entries: [...folder.entries, entry].sort(order) });
  }

  #settle(scope: FileScope): void {
    const now = this.get(scope);
    if (!now.locating) return;
    const next = { ...now };
    delete next.locating;
    this.#put(scope, next);
  }

  #forget(scope: FileScope, path: string): void {
    const current = this.get(scope);
    const gone = (item: string) => item === path || item.startsWith(`${path}/`);
    const expanded = new Set([...current.expanded].filter((item) => !gone(item)));
    const folders = new Map([...current.folders].filter(([item]) => !gone(item)));
    this.#put(scope, { ...current, expanded, folders });
  }

  #folder(scope: FileScope, path: string, folder: Folder): void {
    const current = this.get(scope);
    const folders = new Map(current.folders);
    folders.set(path, folder);
    this.#put(scope, { ...current, folders });
  }

  #put(scope: FileScope, state: TreeState): void {
    this.#trees.set(keyOf(scope), state);
    for (const listener of this.#listeners) listener();
  }
}

export const fileTrees = new FileTreeStore();

export function useFileTree(scope: FileScope): TreeState {
  return useSyncExternalStore(fileTrees.subscribe, () => fileTrees.get(scope));
}

export function resetFileTreesForTests(): void {
  fileTrees.resetForTests();
}
