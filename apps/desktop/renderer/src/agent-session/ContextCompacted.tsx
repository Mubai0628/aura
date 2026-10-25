import type { AgentContextClearedView, AgentContextCompactedView } from '@aura/contracts/generated';
import type { ReactNode } from 'react';

const tokens = (value: number) =>
  new Intl.NumberFormat('zh-CN', { notation: 'compact', maximumFractionDigits: 1 }).format(value);

/** How large the request was counted before and after, when it is known. */
const sizes = (before?: number | null, after?: number | null) =>
  before != null && after != null ? `（约 ${tokens(before)} → ${tokens(after)} tokens）` : '';

function ContextNote({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div aria-label={label} className="agent-context-compacted" role="note">
      <span aria-hidden="true" className="agent-context-compacted-line" />
      <span className="agent-context-compacted-text">{children}</span>
      <span aria-hidden="true" className="agent-context-compacted-line" />
    </div>
  );
}

/**
 * Where the
 * context was compacted, the turn says so — older history became a summary and the recent part
 * stayed as it was — so a meter that fell has its reason in the conversation.
 */
export function ContextCompacted({ compacted }: { compacted: AgentContextCompactedView }) {
  const cleared = compacted.cleared_tool_results ?? 0;
  return (
    <ContextNote label="上下文已压缩">
      {compacted.manual ? '已按你的要求压缩上下文' : '上下文接近上限，已自动压缩'}
      {sizes(compacted.input_tokens_before, compacted.input_tokens_after)}：
      {cleared > 0 ? `${cleared} 个较早的工具结果已清理，` : ''}较早的内容换成了摘要
      {compacted.retained_recent_tokens > 0
        ? `，最近约 ${tokens(compacted.retained_recent_tokens)} tokens 原样保留`
        : ''}
      。
    </ContextNote>
  );
}

/**
 * Older tool results gave their room back before a step, with nothing
 * summarized; the meter fell for that reason, and the turn says so.
 */
export function ContextCleared({ cleared }: { cleared: AgentContextClearedView }) {
  return (
    <ContextNote label="较早的工具结果已清理">
      上下文接近上限，已清理 {cleared.cleared_tool_results} 个较早的工具结果
      {sizes(cleared.input_tokens_before, cleared.input_tokens_after)}。
    </ContextNote>
  );
}

/**
 * The step went through the
 * provider's backup connection, because the one before it failed before answering.
 */
export function BackupConnectionNote({ name }: { name: string }) {
  return <ContextNote label="已改用备用连接">已改用备用连接「{name}」</ContextNote>;
}
