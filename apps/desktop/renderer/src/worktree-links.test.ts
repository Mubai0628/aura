import { describe, expect, it } from 'vitest';
import { listedWorkspaces, worktreeParents } from './worktree-links';

const workspace = (workspace_id: string) => ({ workspace_id });
const session = (workspace_id: string, state: string) => ({ workspace_id, state });

describe('worktree workspaces in lists', () => {
  const parents = worktreeParents({
    worktrees: [
      { workspace_id: 'wt-agent', parent_workspace_id: 'main' },
      { workspace_id: 'wt-user', parent_workspace_id: 'main' },
      { workspace_id: 'wt-orphan', parent_workspace_id: 'gone' },
    ],
  });

  it('never lists a sub-agent worktree and lists the user one right after its workspace', () => {
    const listed = listedWorkspaces(
      ['main', 'wt-agent', 'other', 'wt-user'].map(workspace),
      [
        session('main', 'active'),
        session('wt-agent', 'subagent'),
        session('wt-user', 'active'),
        session('other', 'active'),
      ],
      parents,
    );
    expect(listed.map((each) => each.workspace_id)).toEqual(['main', 'wt-user', 'other']);
  });

  it('drops a worktree whose conversation was archived and lists one without its workspace last', () => {
    const listed = listedWorkspaces(
      ['wt-orphan', 'wt-user', 'main'].map(workspace),
      [session('wt-user', 'archived'), session('wt-orphan', 'active')],
      parents,
    );
    expect(listed.map((each) => each.workspace_id)).toEqual(['main', 'wt-orphan']);
  });

  it('reads a projection without worktrees as before', () => {
    expect(worktreeParents({}).size).toBe(0);
    expect(worktreeParents(undefined).size).toBe(0);
    expect(
      listedWorkspaces(['a', 'b'].map(workspace), [], new Map()).map((each) => each.workspace_id),
    ).toEqual(['a', 'b']);
  });
});
