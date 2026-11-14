import type { AgentTeamLeadView, AgentTeamMemberView } from '@aura/contracts/generated';
import { PHASE_WORDS, amount, memberPhase, toolWords } from './agent-team-words';

/** One agent's facts, in the list's columns: 角色、模型、状态、Token、当前工具. */
function Facts({
  role,
  handle,
  model,
  state,
  phase,
  tokens,
  tool,
}: {
  role: string;
  handle?: string | undefined;
  model: string;
  state: string;
  phase: string;
  tokens: number;
  tool?: string | null | undefined;
}) {
  return (
    <>
      <span className="agent-team-role">
        <strong>{role}</strong>
        {handle ? <small>{handle}</small> : null}
      </span>
      <span className="agent-team-model">{model}</span>
      <span className="agent-team-state" data-phase={phase}>
        {state}
      </span>
      <span className="agent-team-tokens">{amount(tokens)}</span>
      <span className="agent-team-tool">{tool ? toolWords(tool) : ''}</span>
    </>
  );
}

/** The conversation's main agent, then each of its sub-agents; a row shows that agent's
 *  conversation in the main area, the main agent's row the conversation itself. */
export function AgentTeamList({
  lead,
  members,
  chosen,
  onPick,
}: {
  lead?: AgentTeamLeadView | undefined;
  members: readonly AgentTeamMemberView[];
  /** The sub-agent the main area shows; none is the main agent. */
  chosen?: string | undefined;
  onPick: (agent: string | undefined) => void;
}) {
  return (
    <div className="agent-team-table">
      <div aria-hidden="true" className="agent-team-head">
        <span>角色</span>
        <span>模型</span>
        <span>状态</span>
        <span>Token</span>
        <span>当前工具</span>
      </div>
      <ul aria-label="Agent 列表" className="agent-team-list">
        <li>
          <button
            aria-current={chosen === undefined ? 'true' : 'false'}
            className="agent-team-row is-lead"
            onClick={() => onPick(undefined)}
            type="button"
          >
            <Facts
              role="主 Agent"
              model={lead?.model ?? ''}
              state={lead?.working ? '运行中' : '空闲'}
              phase={lead?.working ? 'working' : 'idle'}
              tokens={lead?.tokens ?? 0}
              tool={lead?.tool}
            />
          </button>
        </li>
        {members.map((member) => {
          const phase = memberPhase(member);
          return (
            <li key={member.session_id}>
              <button
                aria-current={member.agent === chosen ? 'true' : 'false'}
                className="agent-team-row"
                onClick={() => onPick(member.agent)}
                type="button"
              >
                <Facts
                  role={member.name}
                  handle={member.agent}
                  model={member.model}
                  state={PHASE_WORDS[phase]}
                  phase={phase}
                  tokens={member.tokens}
                  tool={member.tool}
                />
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
