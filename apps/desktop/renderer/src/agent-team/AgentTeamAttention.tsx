import type { AgentTeamMemberView } from '@aura/contracts/generated';
import { ApprovalPanel } from '../agent-session/ApprovalPanel';
import { PHASE_WORDS, memberPhase, needsUser } from './agent-team-words';
import { refreshAgentTeam } from './team-store';

/**
 * What the sub-agents need from the user, first on the page and only when there is something:
 * each approval to decide, then each question, block and failure with a way to its record.
 */
export function AgentTeamAttention({
  sessionId,
  members,
  onPick,
}: {
  sessionId: string;
  members: readonly AgentTeamMemberView[];
  onPick: (agent: string) => void;
}) {
  const reported = members.filter((member) => {
    const phase = memberPhase(member);
    return phase !== 'approval' && needsUser(phase);
  });
  const approvals = members.flatMap((member) => member.approvals.map((id) => ({ member, id })));
  if (!approvals.length && !reported.length) return null;
  return (
    <section aria-label="要你处理" className="agent-team-attention">
      <h4 className="agent-team-section-title">要你处理</h4>
      {approvals.map(({ member, id }) => (
        <ApprovalPanel
          key={`${member.session_id}:${id}`}
          sessionId={member.session_id}
          approvalId={id}
          from={`子 Agent · ${member.name}`}
          onChanged={() => refreshAgentTeam(sessionId)}
        />
      ))}
      {reported.map((member) => {
        const words = PHASE_WORDS[memberPhase(member)];
        return (
          <article
            aria-label={`${member.name} ${words}`}
            className="agent-team-notice"
            data-phase={memberPhase(member)}
            key={member.session_id}
          >
            <header>
              <strong>{member.name}</strong>
              <span className="agent-team-state" data-phase={memberPhase(member)}>
                {words}
              </span>
            </header>
            {member.question || member.summary ? <p>{member.question ?? member.summary}</p> : null}
            <button className="button is-small" onClick={() => onPick(member.agent)} type="button">
              查看
            </button>
          </article>
        );
      })}
    </section>
  );
}
