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
  loading: boolean;
  error?: string;
}

export interface TreeState {
  open: boolean;
  expanded: ReadonlySet<string>;
  folders: ReadonlyMap<string, Folder>;
  /** The file 在文件树中显示 last asked for: its row is marked and scrolled into view. */
  located?: string;
}

export const ROOT = '.';
/** A new workspace's file index takes a moment: its listing is asked again, a few times. */
const INDEX_RETRIES = 8;
const INDEX_RETRY_MS = 1000;
const INITIAL: TreeState = { open: true, expanded: new Set(), folders: new Map() };

const keyOf = (scope: FileScope) => `${scope.workspace}\n${scope.session}`;

const order = (left: FsListEntry, right: FsListEntry) =>
  Number(right.kind === 'directory') - Number(left.kind === 'directory') ||
  left.name.localeCompare(right.name, undefined, { numeric: true, sensitivity: 'base' });

function listingFailure(error: unknown): string {
  const code =
    typeof error === 'object' && error !== null ? (error as { code?: unknown }).code : undefined;
  if (code === 'workspace.catalog_unavailable')
    return '文件列表暂时读不出来：工作区还在建立文件索引，请稍后重试。';
  if (code === 'file.not_found') return '文件列表暂时读不出来：这个文件夹已不存在。';
  return userMessage(error, '文件列表暂时读不出来，请重试。');
}

/**
 * One tree per conversation, shared by its 文件 tabs — whether it shows, which folders are
 * open and what each lists — so another tab, or the same tab later, finds it as it was left.
 */
class FileTreeStore {
  readonly #trees = new Map<string, TreeState>();
  readonly #listeners = new Set<() => void>();

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
    if (!folder || (folder.error && !folder.loading)) void this.load(scope, path);
  }

  async load(scope: FileScope, path: string, retries = INDEX_RETRIES): Promise<void> {
    this.#folder(scope, path, { ...this.get(scope).folders.get(path), loading: true });
    let folder: Folder;
    try {
      const result = await window.aura.v1.listWorkspaceFiles({
        session_id: scope.session,
        workspace_id: scope.workspace,
        target: { path },
      });
      if (!result.ok && result.error.code === 'workspace.catalog_unavailable' && retries > 0) {
        setTimeout(() => void this.load(scope, path, retries - 1), INDEX_RETRY_MS);
        return;
      }
      // An open folder that is gone (the model deleted or moved it) leaves the tree with
      // everything opened below it, instead of staying behind as an error row.
      if (!result.ok && result.error.code === 'file.not_found' && path !== ROOT) {
        this.#forget(scope, path);
        return;
      }
      folder = result.ok
        ? { loading: false, entries: [...result.value.entries].sort(order) }
        : { loading: false, error: listingFailure(result.error) };
    } catch (reason) {
      folder = { loading: false, error: listingFailure(reason) };
    }
    this.#folder(scope, path, folder);
  }

  toggle(scope: FileScope, path: string): void {
    const current = this.get(scope);
    const expanded = new Set(current.expanded);
    if (expanded.has(path)) expanded.delete(path);
    else expanded.add(path);
    this.#put(scope, { ...current, expanded });
    if (expanded.has(path)) this.ensure(scope, path);
  }

  /** Opens a folder and every folder above it (a folder picked from the filter). */
  reveal(scope: FileScope, path: string): void {
    const parts = path.split('/');
    const current = this.get(scope);
    const expanded = new Set(current.expanded);
    for (let index = 1; index <= parts.length; index += 1)
      expanded.add(parts.slice(0, index).join('/'));
    this.#put(scope, { ...current, expanded });
    for (const folder of [ROOT, ...expanded]) this.ensure(scope, folder);
  }

  /** 在文件树中显示 — the tree shows, the file's folders open, and its row is marked. */
  locate(scope: FileScope, path: string): void {
    const parts = path.split('/');
    const current = this.get(scope);
    const expanded = new Set(current.expanded);
    for (let index = 1; index < parts.length; index += 1)
      expanded.add(parts.slice(0, index).join('/'));
    this.#put(scope, { ...current, open: true, expanded, located: path });
    for (const folder of [ROOT, ...expanded]) this.ensure(scope, folder);
  }

  /** Lists again the root and every open folder (the refresh button). */
  refresh(scope: FileScope): void {
    for (const folder of [ROOT, ...this.get(scope).expanded]) void this.load(scope, folder);
  }

  resetForTests(): void {
    this.#trees.clear();
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
