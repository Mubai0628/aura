import type { AgentGraphEdge, AgentGraphView } from '@aura/contracts/generated';

export function peerEdgeActivity(view: AgentGraphView, edge: AgentGraphEdge) {
  const activity = view.peer_activity?.find(
    ({ edge: link }) =>
      link.source_session_id === edge.source_session_id &&
      link.target_session_id === edge.target_session_id,
  );
  if (!activity) return { text: '可协作', active: false };
  const text = {
    queued: '已排队',
    started: '执行中',
    completed: '已完成',
    failed: '执行失败',
    cancelled: '已取消',
    unknown: '结果未知，先对账',
  }[activity.state];
  return { text, active: activity.state === 'started' };
}
