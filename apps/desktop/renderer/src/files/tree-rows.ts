import type { FsListEntry } from '@aura/contracts/generated';
import { ROOT, type Folder, type TreeState } from './file-tree-store';

/**
 * One line of the tree as it is laid out: an entry, or a folder's note (still reading, could not
 * be read, empty, or a next page to load). Every line has the same height, so a line's place is
 * its index, and only the lines near the view are drawn.
 */
export type TreeRow = { key: string; path: string; depth: number } & (
  | { kind: 'entry'; entry: FsListEntry; expanded: boolean }
  | { kind: 'reading' }
  | { kind: 'error'; error: string }
  | { kind: 'empty' }
  | { kind: 'more'; folder: Folder }
);

export const entryKey = (path: string) => `e:${path}`;

/** The open folders' lines, in the order the tree shows them. */
export function treeRows(tree: TreeState): TreeRow[] {
  const rows: TreeRow[] = [];
  const add = (path: string, depth: number) => {
    const folder = tree.folders.get(path);
    if (folder?.error && !folder.loading) {
      rows.push({ kind: 'error', key: `error:${path}`, path, depth, error: folder.error });
      return;
    }
    if (!folder?.entries) {
      rows.push({ kind: 'reading', key: `reading:${path}`, path, depth });
      return;
    }
    if (folder.entries.length === 0)
      rows.push({ kind: 'empty', key: `empty:${path}`, path, depth });
    for (const entry of folder.entries) {
      const expanded = entry.kind === 'directory' && tree.expanded.has(entry.path);
      rows.push({
        kind: 'entry',
        key: entryKey(entry.path),
        path: entry.path,
        depth,
        entry,
        expanded,
      });
      if (expanded) add(entry.path, depth + 1);
    }
    if (folder.next) rows.push({ kind: 'more', key: `more:${path}`, path, depth, folder });
  };
  add(ROOT, 0);
  return rows;
}
