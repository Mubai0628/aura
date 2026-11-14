import type { AgentTeamMemberView, PermissionMode } from '@aura/contracts/generated';
import { useState } from 'react';
import { ConfirmationDialog } from '../ConfirmationDialog';
import { AppIcon } from '../app-icons';
import { FULL_ACCESS_DELETES } from '../security/full-access-notice';
import { PERMISSION_MODE_NAMES } from '../security/permission-mode-names';
import type { MenuEntry } from '../ui/useContextMenu';
import { modeFailure, worktreeOpenFailure } from './failure-words';
import { openSubagentGrants } from './subagent-grants';
import { refreshAgentTeam } from './team-store';

const MODES = ['read_only', 'workspace_read_write', 'full_access'] as const;

/**
 * What the user does to one sub-agent from its card on the canvas or its record: its worktree in
 * the file manager, its mode, which the user may switch while it works (完全访问 confirmed first),
 * and its grants. `problem` says why the last of these did not go through.
 */
export function useMemberMenu(parentSessionId: string) {
  const [confirm, setConfirm] = useState<AgentTeamMemberView>();
  const [problem, setProblem] = useState<string>();

  const setMode = async (member: AgentTeamMemberView, mode: PermissionMode) => {
    setProblem(undefined);
    try {
      const api = window.aura.v1;
      const state = await api.getSessionPermissionState({ session_id: member.session_id });
      if (!state.ok) return setProblem(modeFailure(state.error));
      const result = await api.setSessionPermissionMode({
        session_id: member.session_id,
        expected_revision: state.value.revision,
        mode,
      });
      if (!result.ok) setProblem(modeFailure(result.error));
    } catch (reason) {
      setProblem(modeFailure(reason));
    } finally {
      refreshAgentTeam(parentSessionId);
    }
  };
  const openFolder = async (member: AgentTeamMemberView) => {
    setProblem(undefined);
    try {
      const result = await window.aura.v1.openAgentTeamWorktree({
        session_id: parentSessionId,
        agent: member.agent,
      });
      if (!result.ok) setProblem(worktreeOpenFailure(result.error));
      else if (!result.value.completed) setProblem(result.value.message);
    } catch (reason) {
      setProblem(worktreeOpenFailure(reason));
    }
  };

  const entries = (member: AgentTeamMemberView): MenuEntry[] => {
    const list: MenuEntry[] = [];
    if (member.worktree?.state === 'active')
      list.push(
        {
          label: '在文件管理器中查看工作区',
          icon: <AppIcon name="folder" size={14} />,
          run: () => void openFolder(member),
        },
        'separator',
      );
    for (const mode of MODES)
      list.push({
        label: PERMISSION_MODE_NAMES[mode],
        icon: member.mode === mode ? <AppIcon name="check" size={14} /> : undefined,
        disabled: member.state === 'stopped',
        run: () => {
          if (member.mode === mode) return;
          if (mode === 'full_access') setConfirm(member);
          else void setMode(member, mode);
        },
      });
    const workspace = member.workspace_id;
    list.push('separator', {
      label: '授权管理',
      icon: <AppIcon name="shield" size={14} />,
      disabled: workspace == null,
      run: () => {
        if (workspace != null)
          openSubagentGrants({
            parentSessionId,
            agent: member.agent,
            name: member.name,
            sessionId: member.session_id,
            workspaceId: workspace,
          });
      },
    });
    return list;
  };

  const dialog =
    confirm === undefined ? null : (
      <ConfirmationDialog
        busy={false}
        title={`${confirm.name} 改为完全访问`}
        titleId="agent-member-full-access-title"
        confirmLabel="启用完全访问"
        description={`它可联网，并在已授权范围内修改或删除文件。${FULL_ACCESS_DELETES}`}
        onCancel={() => setConfirm(undefined)}
        onConfirm={() => {
          void setMode(confirm, 'full_access');
          setConfirm(undefined);
        }}
      />
    );
  return { entries, dialog, problem, clearProblem: () => setProblem(undefined) };
}
