import type { AgentToolRemoteDecision } from '@aura/contracts/generated';
import { PLATFORM_LABELS } from '../plugin-settings/plugin-labels';

const DECIDED: Record<AgentToolRemoteDecision['decision'], string> = {
  allow_once: '批准（仅此一次）',
  allow_session: '批准（本会话）',
  allow_for_current_turn: '批准（本轮）',
  allow_workspace: '批准（本工作区）',
  deny: '拒绝',
};

/** Who decided a call's approval on a chat platform, as the call's row says it. */
export function remoteDecisionText({ sender, decision }: AgentToolRemoteDecision): string {
  const decided = DECIDED[decision];
  const from = sender.from;
  if (!from) return `经插件 ${sender.plugin} ${decided}`;
  const platform = (PLATFORM_LABELS as Record<string, string>)[from.platform] ?? from.platform;
  return `经插件 ${sender.plugin} 由 ${platform} 用户 ${from.user_name || from.user_id} ${decided}`;
}
