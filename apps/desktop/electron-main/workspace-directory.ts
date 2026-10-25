import type { WorkspaceDirectorySelection } from '@aura/contracts/generated';
import { dialog, type BrowserWindow } from 'electron';
import { basename } from 'node:path';

const pending = new WeakMap<BrowserWindow, Promise<WorkspaceDirectorySelection>>();

/** Native selection only; Core still validates the root when creating a workspace. */
export async function selectWorkspaceDirectory(
  owner: BrowserWindow,
): Promise<WorkspaceDirectorySelection> {
  const existing = pending.get(owner);
  if (existing) return existing;
  const selection = (async (): Promise<WorkspaceDirectorySelection> => {
    const result = await dialog.showOpenDialog(owner, {
      title: '选择工作区文件夹',
      buttonLabel: '选择文件夹',
      properties: ['openDirectory', 'dontAddToRecent'],
    });
    if (owner.isDestroyed() || result.canceled || result.filePaths.length === 0)
      return { status: 'cancelled' };
    const selectedRoot = result.filePaths[0];
    if (result.filePaths.length !== 1 || !selectedRoot)
      throw new Error('文件夹选择器返回了无效的选择。');
    return {
      status: 'selected',
      selected_root: selectedRoot,
      suggested_display_name: basename(selectedRoot) || selectedRoot,
    };
  })();
  pending.set(owner, selection);
  try {
    return await selection;
  } finally {
    pending.delete(owner);
  }
}
