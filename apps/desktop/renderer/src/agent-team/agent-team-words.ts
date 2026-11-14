import type { AgentTeamMemberView } from '@aura/contracts/generated';
import { approvalActionLabel } from '../agent-session/approval-action-label';

const TEAM_TOOLS: Record<string, string> = {
  'agents.spawn': '派子 Agent',
  'agents.status': '查看子 Agent',
  'agents.wait': '等子 Agent 报告',
  'agents.send': '给子 Agent 发消息',
  'agents.result': '读子 Agent 结果',
  'agents.stop': '停止子 Agent',
  'agents.merge': '合回工作副本',
  'agents.approve': '代批子 Agent 的请求',
  'agents.discard': '丢弃工作副本',
  'agent.report': '交报告',
};

/** A tool by what it does; one without words keeps its name. */
export const toolWords = (tool: string): string => TEAM_TOOLS[tool] ?? approvalActionLabel(tool);

/** Where a sub-agent is, as the page and the card name it. */
export type MemberPhase =
  'approval' | 'reviewing' | 'working' | 'question' | 'blocked' | 'failed' | 'ready' | 'stopped';

export function memberPhase(member: AgentTeamMemberView): MemberPhase {
  if (member.state === 'stopped') return 'stopped';
  if (member.state === 'running') {
    if (member.approvals.length) return 'approval';
    // A question the main agent answers first is not the user's yet.
    return member.reviewing?.length ? 'reviewing' : 'working';
  }
  switch (member.last_report) {
    case 'waiting':
      return 'question';
    case 'blocked':
      return 'blocked';
    case 'failed':
      return 'failed';
    default:
      return 'ready';
  }
}

export const PHASE_WORDS: Record<MemberPhase, string> = {
  approval: '等审批',
  reviewing: '主 Agent 审核中',
  working: '运行中',
  question: '有问题',
  blocked: '卡住',
  failed: '失败',
  ready: '已交报告',
  stopped: '已停止',
};

/** 审批、问题、卡住、失败: what the user may have to act on. */
export const needsUser = (phase: MemberPhase): boolean =>
  phase === 'approval' || phase === 'question' || phase === 'blocked' || phase === 'failed';

export const amount = (tokens: number): string => tokens.toLocaleString('zh-CN');
