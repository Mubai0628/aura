import type { FileScope } from './file-tree-store';

/** How many files of one name are offered. */
const LIMIT = 20;

/**
 * The workspace files a bare name may mean.
 * A model names a file it made by its name alone (`fengjing_01.jpg`) as often as by its path: a
 * file of that name at the root is the one meant; otherwise every file of that name is.
 */
export async function filesNamed(scope: FileScope, name: string): Promise<string[]> {
  const where = { workspace_id: scope.workspace, session_id: scope.session };
  try {
    const here = await window.aura.v1.statWorkspaceFile({ ...where, target: { path: name } });
    if (here.ok) return [name];
    const found = await window.aura.v1.globWorkspacePaths({
      ...where,
      pattern: `**/${name}`,
      limit: LIMIT,
    });
    return found.ok ? found.value.paths : [];
  } catch {
    return [];
  }
}

/** Whether the workspace has a file at this path. */
export async function inWorkspace(scope: FileScope, path: string): Promise<boolean> {
  try {
    const found = await window.aura.v1.statWorkspaceFile({
      workspace_id: scope.workspace,
      session_id: scope.session,
      target: { path },
    });
    return found.ok;
  } catch {
    return false;
  }
}

/** A path with no folder: it may mean a file elsewhere in the workspace. */
export const isBareName = (path: string) => !path.includes('/');
