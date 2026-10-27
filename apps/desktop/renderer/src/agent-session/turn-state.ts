import type { AgentTurnView, AgentToolActivityEntry } from '@aura/contracts/generated';

export const terminalTurn = (state: string) =>
  ['completed', 'cancelled', 'failed', 'recovery_required'].includes(state);

export function turnStatus(
  item: AgentTurnView,
  calls: ReadonlyMap<string, AgentToolActivityEntry>,
  awaitingContext: boolean,
) {
  if (awaitingContext) return '等待处理上下文';
  const state = item.turn.state.state;
  if (state === 'completed') return '已完成';
  if (state === 'cancelled') return '已停止';
  if (state === 'failed' || state === 'recovery_required') return '任务中断';
  if (state === 'awaiting_approval') return '等待授权';
  if (state === 'awaiting_query') return '等待回答';
  if (state === 'pending') return '准备开始';
  const latest = item.assistant_messages.at(-1);
  if (latest?.partial && latest.context_compacting) return '正在压缩上下文';
  if (
    latest?.tool_call_ids.some((id) => {
      const call = calls.get(id)?.summary;
      return call && !call.outcome && call.status !== 'finalized';
    })
  )
    return '正在执行工具';
  return latest?.partial && latest.text_total_bytes > 0 ? '正在生成' : '正在思考';
}
