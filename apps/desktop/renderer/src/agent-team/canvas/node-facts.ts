import type { AgentTeamLeadView, AgentTeamMemberView } from '@aura/contracts/generated';
import { PHASE_WORDS, memberPhase } from '../agent-team-words';

/** One agent as its card on the canvas shows it. */
export interface NodeFacts {
  name: string;
  handle?: string | undefined;
  model: string;
  phase: string;
  state: string;
  tokens: number;
  tool?: string | null | undefined;
  branch?: string | undefined;
  approvals: number;
  ended: boolean;
}

export const leadFacts = (lead: AgentTeamLeadView | undefined): NodeFacts => ({
  name: '主 Agent',
  model: lead?.model ?? '',
  phase: lead?.working ? 'working' : 'idle',
  state: lead?.working ? '运行中' : '空闲',
  tokens: lead?.tokens ?? 0,
  tool: lead?.tool,
  approvals: 0,
  ended: false,
});

export function memberFacts(member: AgentTeamMemberView): NodeFacts {
  const phase = memberPhase(member);
  return {
    name: member.name,
    handle: member.agent,
    model: member.model,
    phase,
    state: PHASE_WORDS[phase],
    tokens: member.tokens,
    tool: member.state === 'stopped' ? undefined : member.tool,
    branch: member.worktree?.state === 'active' ? member.worktree.branch : undefined,
    approvals: member.approvals.length,
    ended:
      member.state === 'stopped' || (member.worktree != null && member.worktree.state !== 'active'),
  };
}
