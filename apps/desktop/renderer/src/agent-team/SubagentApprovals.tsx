import { ApprovalPanel } from '../agent-session/ApprovalPanel';
import { refreshAgentTeam, useAgentTeam } from './team-store';

/**
 * The approvals the sub-agents of a conversation wait for, decided in that conversation. The
 * list follows the conversation while it has a turn under way or a sub-agent still works: a
 * sub-agent starts only from a turn of the conversation.
 */
export function SubagentApprovals({
  sessionId,
  activeTurn,
}: {
  sessionId: string;
  activeTurn?: string | undefined;
}) {
  const members = useAgentTeam(sessionId, activeTurn !== undefined)?.members ?? [];
  return (
    <>
      {members.flatMap((member) =>
        member.approvals.map((id) => (
          <ApprovalPanel
            key={`${member.session_id}:${id}`}
            sessionId={member.session_id}
            approvalId={id}
            from={`子 Agent · ${member.name}`}
            onChanged={() => refreshAgentTeam(sessionId)}
          />
        )),
      )}
    </>
  );
}
