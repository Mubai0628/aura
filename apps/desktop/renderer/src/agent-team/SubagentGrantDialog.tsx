import type { AgentTeamMemberAllowances } from '@aura/contracts/generated';
import { useState } from 'react';
import { SessionGrantDialog } from '../security/SessionGrantDialog';
import { useSecurityControl } from '../security-control-store';
import { closeSubagentGrants, type SubagentGrantsTarget } from './subagent-grants';
import { refreshAgentTeam, showAgentTeam, useAgentTeam } from './team-store';
import { revokeFailure } from './failure-words';

type Allowance = keyof AgentTeamMemberAllowances;

const ALLOWANCES: readonly { allowance: Allowance; label: string }[] = [
  { allowance: 'deletions', label: '命令删除它工作副本里的文件' },
  { allowance: 'outside_writes', label: '命令写工作区以外' },
  { allowance: 'downloads', label: '下载' },
];

/**
 * A sub-agent's grants: what its commands may do without asking, given in the settings or on one
 * of its questions, and the grants of its own conversation; each can be taken back here.
 */
export function SubagentGrantDialog({ target }: { target: SubagentGrantsTarget }) {
  const security = useSecurityControl(target.workspaceId, target.sessionId);
  const member = useAgentTeam(target.parentSessionId, true)?.members.find(
    (candidate) => candidate.agent === target.agent,
  );
  const [pending, setPending] = useState<Allowance>();
  const [problem, setProblem] = useState<string>();
  const given = ALLOWANCES.filter(({ allowance }) => member?.allowed?.[allowance] === true);
  const revoke = async (allowance: Allowance) => {
    if (pending !== undefined) return;
    setPending(allowance);
    setProblem(undefined);
    try {
      const result = await window.aura.v1.revokeAgentTeamAllowance({
        session_id: target.parentSessionId,
        agent: target.agent,
        allowance,
      });
      if (result.ok) showAgentTeam(target.parentSessionId, result.value);
      else setProblem(revokeFailure(result.error));
    } catch (reason) {
      setProblem(revokeFailure(reason));
    } finally {
      setPending(undefined);
      refreshAgentTeam(target.parentSessionId);
    }
  };
  return (
    <SessionGrantDialog
      onClose={closeSubagentGrants}
      security={security}
      sessionId={target.sessionId}
      title={`${target.name} 的授权`}
      titleId="subagent-grants-title"
    >
      <section aria-label="不用问的事" className="session-grant-native">
        <h3>不用问的事</h3>
        {member === undefined ? (
          <p role="status">正在读取…</p>
        ) : given.length === 0 ? (
          <p>没有。</p>
        ) : (
          <ul className="compact-list">
            {given.map(({ allowance, label }) => (
              <li key={allowance}>
                <span>{label}</span>
                <button
                  disabled={pending !== undefined}
                  onClick={() => void revoke(allowance)}
                  type="button"
                >
                  {pending === allowance ? '正在撤销…' : '撤销'}
                </button>
              </li>
            ))}
          </ul>
        )}
        {problem ? <p role="alert">{problem}</p> : null}
      </section>
    </SessionGrantDialog>
  );
}
