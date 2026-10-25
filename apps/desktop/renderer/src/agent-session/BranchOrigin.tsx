import type { AgentBranchOrigin } from '@aura/contracts/generated';
import { useState } from 'react';
import { ToolIcon } from '../tool-icon';
import './branch.css';

const when = (at: string) =>
  new Date(at).toLocaleString('zh-CN', {
    month: 'numeric',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });

/**
 * The line between what a branch carried and its own turns. It names the
 * conversation and turn it came from, and opens that conversation while it still exists.
 */
export function BranchOrigin({
  origin,
  onOpenSource,
}: {
  origin: AgentBranchOrigin;
  /** Opens the source; `false` when it no longer exists. */
  onOpenSource?: ((sessionId: string) => boolean) | undefined;
}) {
  const [gone, setGone] = useState(false);
  return (
    <div aria-label="分支起点" className="agent-branch-origin" role="note">
      <span className="agent-branch-line" aria-hidden="true" />
      <span className="agent-branch-text">
        <ToolIcon name="git-branch" size={13} />
        从「{origin.source_title.trim() || '新会话'}」第 {origin.turn_number} 轮分支 ·{' '}
        {when(origin.created_at)}
        {onOpenSource ? (
          gone ? (
            <span className="agent-branch-gone">原会话已不存在</span>
          ) : (
            <button
              className="agent-branch-source"
              onClick={() => setGone(!onOpenSource(origin.source_session_id))}
              type="button"
            >
              打开原会话
            </button>
          )
        ) : null}
      </span>
      <span className="agent-branch-line" aria-hidden="true" />
    </div>
  );
}
