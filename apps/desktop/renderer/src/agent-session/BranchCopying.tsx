import type { AgentBranchCopyingView } from '@aura/contracts/generated';
import './branch.css';

/**
 * A branch
 * opens with its newest turns while the older ones are still carried; the top of the conversation
 * says so, with the spinner and the light the thinking step has, until the last of them is there.
 */
export function BranchCopying({ copying }: { copying: AgentBranchCopyingView }) {
  if (copying.failed)
    return (
      <p className="agent-branch-copying" data-failed="true" role="alert">
        之前的会话消息没能克隆完成（已克隆 {copying.copied_turns} / {copying.total_turns}{' '}
        轮）。可以从原会话重新新建分支。
      </p>
    );
  return (
    <p className="agent-branch-copying" role="status">
      <span aria-hidden="true" className="agent-turn-spinner" />
      <span className="agent-branch-copying-label">之前的会话消息正在克隆</span>
      <span className="agent-branch-copying-count">
        {copying.copied_turns} / {copying.total_turns} 轮
      </span>
    </p>
  );
}
