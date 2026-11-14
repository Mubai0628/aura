import { memberPhase, needsUser } from './agent-team-words';
import { useAgentTeam } from './team-store';

/** The 近期活动 card's part for the conversation's sub-agents: how many work, how many need the
 *  user, and the way to the 多 Agent page. Absent while it has none. */
export function AgentTeamActivity({
  sessionId,
  onOpen,
}: {
  sessionId: string;
  onOpen: () => void;
}) {
  const members = useAgentTeam(sessionId, false)?.members ?? [];
  if (!members.length) return null;
  const phases = members.map(memberPhase);
  const working = phases.filter((phase) => phase === 'working' || phase === 'approval').length;
  const asking = phases.filter(needsUser).length;
  return (
    <section aria-label="子 Agent" className="activity-section">
      <h4 className="activity-section-title">子 Agent</h4>
      <div className="activity-row">
        <span className="activity-value" data-running={working > 0 || undefined}>
          {working ? `${working} 个在运行` : '没有在运行的'} · 共 {members.length} 个
          {asking ? ` · ${asking} 个要你处理` : ''}
        </span>
        <button className="activity-link" onClick={onOpen} type="button">
          查看
        </button>
      </div>
    </section>
  );
}
