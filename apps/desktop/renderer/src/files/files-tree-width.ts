/**
 * How wide the
 * 文件 page's tree is beside an open file. A per-viewer convenience, kept in this browser only.
 */
const KEY = 'aura.ui.v1.files-tree-width';

export const FILES_TREE_MIN_WIDTH = 150;
export const FILES_TREE_DEFAULT_WIDTH = 220;
/** The open file keeps at least this much of the page. */
export const FILES_VIEW_MIN_WIDTH = 240;
/** The line between the tree and the file (its grid column in files.css). */
const SPLITTER_WIDTH = 7;
/** The widest a tree can be when the page's own width is not known yet. */
const UNMEASURED_MAX_WIDTH = 560;

export function readFilesTreeWidth(): number {
  try {
    const stored = Number.parseInt(window.localStorage.getItem(KEY) ?? '', 10);
    return Number.isFinite(stored) && stored >= FILES_TREE_MIN_WIDTH
      ? stored
      : FILES_TREE_DEFAULT_WIDTH;
  } catch {
    return FILES_TREE_DEFAULT_WIDTH;
  }
}

export function writeFilesTreeWidth(width: number): void {
  try {
    window.localStorage.setItem(KEY, String(Math.round(width)));
  } catch {
    // A preference that cannot be kept is only forgotten.
  }
}

/** The widest the tree may be on a page this wide. */
export function filesTreeMaxWidth(pageWidth: number): number {
  return pageWidth > 0
    ? Math.max(FILES_TREE_MIN_WIDTH, Math.round(pageWidth - FILES_VIEW_MIN_WIDTH - SPLITTER_WIDTH))
    : UNMEASURED_MAX_WIDTH;
}
