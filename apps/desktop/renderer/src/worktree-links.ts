import type { WorkspaceSessionProjection } from '@aura/contracts/generated';

/** Each worktree's workspace, with the workspace it was made from. */
export function worktreeParents(
  projection: Pick<WorkspaceSessionProjection, 'worktrees'> | undefined,
): ReadonlyMap<string, string> {
  return new Map(
    (projection?.worktrees ?? []).map((link) => [link.workspace_id, link.parent_workspace_id]),
  );
}

/**
 * The workspaces a list shows, in its order. A sub-agent's worktree is never listed; a worktree
 * the user started a conversation in is listed while that conversation is, right after the
 * workspace it was made from.
 */
export function listedWorkspaces<T extends { workspace_id: string }>(
  workspaces: readonly T[],
  sessions: readonly { workspace_id: string; state: string }[],
  parents: ReadonlyMap<string, string>,
): T[] {
  const userConversation = (id: string) =>
    sessions.some(
      (session) =>
        session.workspace_id === id && session.state !== 'archived' && session.state !== 'subagent',
    );
  const worktrees = workspaces.filter(
    (workspace) => parents.has(workspace.workspace_id) && userConversation(workspace.workspace_id),
  );
  const listed: T[] = [];
  for (const workspace of workspaces) {
    if (parents.has(workspace.workspace_id)) continue;
    listed.push(workspace);
    listed.push(
      ...worktrees.filter((each) => parents.get(each.workspace_id) === workspace.workspace_id),
    );
  }
  // One made from a workspace that is not listed comes last.
  listed.push(...worktrees.filter((each) => !listed.includes(each)));
  return listed;
}
