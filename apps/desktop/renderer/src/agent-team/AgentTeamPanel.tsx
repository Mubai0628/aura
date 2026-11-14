import { useState } from 'react';
import { AppIcon } from '../app-icons';
import { AgentTeamAttention } from './AgentTeamAttention';
import { AgentTeamList } from './AgentTeamList';
import { AgentTeamCanvas } from './canvas/AgentTeamCanvas';
import { readTeamView, writeTeamView, type TeamView } from './canvas/places';
import { focusAgent, useFocusedAgent } from './team-focus';
import { useAgentTeam, useAgentTeamUnread } from './team-store';
import './agent-team.css';

/**
 * The 多 Agent page of the right sidebar: what needs the user, and the conversation's main agent
 * and its sub-agents, on a canvas or in a list. Picking one shows its conversation in the main
 * area. It follows the conversation while it is shown and can fill the main area until something
 * is picked.
 */
export function AgentTeamPanel({
  sessionId,
  visible,
  expanded = false,
  onToggleExpanded,
}: {
  sessionId: string;
  visible: boolean;
  expanded?: boolean;
  onToggleExpanded?: (() => void) | undefined;
}) {
  const view = useAgentTeam(sessionId, visible);
  const unread = useAgentTeamUnread(sessionId);
  const focused = useFocusedAgent(sessionId);
  const [shown, setShown] = useState<TeamView>(readTeamView);
  const members = view?.members ?? [];
  const chosen = members.some((member) => member.agent === focused) ? focused : undefined;
  // Filling the main area, the page goes back to the sidebar so the pick shows there.
  const pick = (agent: string | undefined) => {
    focusAgent(sessionId, agent);
    if (expanded) onToggleExpanded?.();
  };
  const show = (next: TeamView) => {
    setShown(next);
    writeTeamView(next);
  };
  const attention = <AgentTeamAttention sessionId={sessionId} members={members} onPick={pick} />;
  return (
    <section
      aria-label="多 Agent"
      className="agent-team-page"
      data-expanded={expanded}
      data-view={shown}
    >
      <div className="agent-team-toolbar">
        <div aria-label="多 Agent 视图" className="agent-team-views" role="group">
          <button
            aria-label="画布"
            aria-pressed={shown === 'canvas'}
            onClick={() => show('canvas')}
            title="画布"
            type="button"
          >
            <AppIcon name="agents" size={16} />
          </button>
          <button
            aria-label="列表"
            aria-pressed={shown === 'list'}
            onClick={() => show('list')}
            title="列表"
            type="button"
          >
            <AppIcon name="list" size={16} />
          </button>
        </div>
        {onToggleExpanded ? (
          <button
            aria-label={expanded ? '还原多 Agent 页' : '放大多 Agent 页'}
            aria-pressed={expanded}
            className="icon-button"
            onClick={onToggleExpanded}
            title={expanded ? '还原多 Agent 页' : '放大多 Agent 页'}
            type="button"
          >
            <AppIcon name={expanded ? 'shrink' : 'expand'} size={15} />
          </button>
        ) : null}
      </div>
      {unread ? (
        <p className="agent-team-problem" role="status">
          子 Agent 列表暂时读不到（Aura Core 没有回应），这里显示的可能不是最新状态；会自动重试。
        </p>
      ) : null}
      {shown === 'canvas' ? (
        <div className="agent-team-stage">
          <AgentTeamCanvas sessionId={sessionId} view={view} chosen={chosen} onPick={pick} />
          {attention}
        </div>
      ) : (
        <>
          {attention}
          <AgentTeamList lead={view?.lead} members={members} chosen={chosen} onPick={pick} />
          {view && !members.length ? <p className="agent-team-empty">没有子 Agent</p> : null}
        </>
      )}
    </section>
  );
}
