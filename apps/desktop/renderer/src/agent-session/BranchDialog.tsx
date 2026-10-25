import type { AgentSessionBranchResult } from '@aura/contracts/generated';
import { useId, useState } from 'react';
import { AppIcon } from '../app-icons';
import { ModalDialog } from '../ModalDialog';
import { branchFrom } from './branch';
import { userMessage } from './user-message';
import './branch.css';

/**
 * Before a branch is made the user is told what it carries and
 * what it does not. A refusal is shown here and can be tried again; nothing half made is left.
 */
export function BranchDialog({
  sessionId,
  title,
  turnTaskId,
  onBranched,
  onClose,
}: {
  sessionId: string;
  title: string;
  turnTaskId: string;
  onBranched: (made: AgentSessionBranchResult) => void;
  onClose: () => void;
}) {
  const titleId = useId();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const close = () => {
    if (!busy) onClose();
  };
  const confirm = async () => {
    setBusy(true);
    setError(undefined);
    try {
      onBranched(await branchFrom(sessionId, turnTaskId));
    } catch (reason) {
      setError(userMessage(reason, '没能新建分支，没有留下半个分支，可以重试。'));
      setBusy(false);
    }
  };
  const name = title.trim() || '新会话';
  return (
    <ModalDialog
      className="sidebar-edit-dialog agent-branch-dialog"
      titleId={titleId}
      onClose={close}
    >
      <header className="modal-header">
        <h2 id={titleId}>从这一轮新建分支？</h2>
        <button
          type="button"
          className="icon-button"
          aria-label="关闭"
          disabled={busy}
          onClick={close}
        >
          <AppIcon name="close" />
        </button>
      </header>
      <div className="sidebar-dialog-body">
        <ul className="agent-branch-facts">
          <li>会新建会话“分支：{name}”，带着从开头到这一轮为止的全部对话；之后两边各走各的。</li>
          <li>两个会话用同一个工作区里的文件，文件不会分支。</li>
          <li>
            原会话里单独授权过、已经发给模型的内容会一起带过去，只限这些内容；以后在分支里新读取工作区外的文件、网页等，仍会先问你。
          </li>
          <li>
            权限模式与原会话相同，不带“本会话允许”的批准；计划与目标模式不带过去，分支从普通模式开始。
          </li>
        </ul>
        {/* While the conversation is copied, the dialog says so and what comes next. */}
        {busy ? (
          <p className="agent-branch-progress" role="status">
            <span aria-hidden="true" className="agent-branch-spinner" />
            正在打开分支：先带上最近的几轮，较早的消息会在分支里继续复制。
          </p>
        ) : null}
        {error ? <p role="alert">{error}</p> : null}
        <div className="sidebar-dialog-actions">
          <button type="button" onClick={close} disabled={busy}>
            取消
          </button>
          <button type="button" disabled={busy} onClick={() => void confirm()}>
            {busy ? '正在新建分支…' : error ? '重试' : '新建分支'}
          </button>
        </div>
      </div>
    </ModalDialog>
  );
}
