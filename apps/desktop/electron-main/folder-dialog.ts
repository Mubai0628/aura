import { stat } from 'node:fs/promises';
import { dialog, type BrowserWindow, type FileFilter, type OpenDialogOptions } from 'electron';

export type SelectedFolder = { path: string | null };

const pending = new WeakMap<BrowserWindow, Promise<SelectedFolder>>();

/**
 * Whether a path Core listed is still a folder. Only a folder goes to the shell: a file put in
 * its place would be opened by its program.
 */
export async function isDirectory(path: string): Promise<boolean> {
  try {
    return (await stat(path)).isDirectory();
  } catch {
    return false;
  }
}

/**
 * The ordinary spelling of a folder Core names. Core holds canonical paths in Windows' verbatim
 * form (`\\?\C:\…`, `\\?\UNC\host\share\…`), which the Windows shell does not open.
 */
export function ordinaryPath(path: string): string {
  if (path.startsWith('\\\\?\\UNC\\')) return `\\\\${path.slice('\\\\?\\UNC\\'.length)}`;
  return path.startsWith('\\\\?\\') ? path.slice('\\\\?\\'.length) : path;
}

/**
 * Main's own dialogs for Settings (a Hook package, a Skill folder or ZIP file to import).
 * Nothing is read or trusted here: Core checks what was chosen, and trust is given only to
 * what the user confirms. One dialog per window; a second request while it is open gets the
 * same answer.
 */
async function choose(owner: BrowserWindow, options: OpenDialogOptions): Promise<SelectedFolder> {
  const existing = pending.get(owner);
  if (existing) return existing;
  const selection = (async (): Promise<SelectedFolder> => {
    const result = await dialog.showOpenDialog(owner, options);
    const path = result.filePaths[0];
    if (owner.isDestroyed() || result.canceled || path === undefined) return { path: null };
    if (result.filePaths.length !== 1) throw new Error('选择框返回了无效的选择。');
    return { path };
  })();
  pending.set(owner, selection);
  try {
    return await selection;
  } finally {
    pending.delete(owner);
  }
}

export function selectFolder(owner: BrowserWindow, title: string): Promise<SelectedFolder> {
  return choose(owner, {
    title,
    buttonLabel: '选择文件夹',
    properties: ['openDirectory', 'dontAddToRecent'],
  });
}

export function selectFile(
  owner: BrowserWindow,
  title: string,
  filters: FileFilter[],
): Promise<SelectedFolder> {
  return choose(owner, {
    title,
    buttonLabel: '选择文件',
    filters,
    properties: ['openFile', 'dontAddToRecent'],
  });
}
