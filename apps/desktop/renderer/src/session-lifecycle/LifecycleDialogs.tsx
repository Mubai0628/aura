import { useId, useState } from 'react';
import { AppIcon } from '../app-icons';
import { ModalDialog } from '../ModalDialog';
import { userMessage } from '../agent-session/user-message';
import { deleteArchivedWorkspace } from './workspace-lifecycle';
import './session-lifecycle.css';

/**
 * The two irreversible actions. Each says what goes, what stays and that it cannot be
 * undone, and names its button after the action.
 */
function ConfirmDialog({
  title,
  body,
  action,
  busyAction,
  run,
  onClose,
}: {
  title: string;
  body: string;
  action: string;
  busyAction: string;
  run: () => Promise<void>;
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
      await run();
      onClose();
    } catch (reason) {
      setError(userMessage(reason, `${action}没有完成，请刷新后查看。`));
      setBusy(false);
    }
  };
  return (
    <ModalDialog className="sidebar-edit-dialog" titleId={titleId} onClose={close}>
      <header className="modal-header">
        <h2 id={titleId}>{title}</h2>
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
        <p>{body}</p>
        {error && <p role="alert">{error}</p>}
        <div className="sidebar-dialog-actions">
          <button type="button" onClick={close} disabled={busy}>
            取消
          </button>
          <button
            type="button"
            className="is-destructive"
            disabled={busy}
            onClick={() => void confirm()}
          >
            {busy ? busyAction : action}
          </button>
        </div>
      </div>
    </ModalDialog>
  );
}

export function DeleteSessionDialog({
  session,
  onClose,
}: {
  session: { id: string; title: string };
  onClose: () => void;
}) {
  return (
    <ConfirmDialog
      title={`删除会话「${session.title || '新会话'}」？`}
      body="会删除它的对话、过程与审计记录，无法恢复。工作区里的文件与记忆不受影响。"
      action="删除会话"
      busyAction="正在删除…"
      run={async () => {
        const result = await window.aura.v1.deleteSession({ session_id: session.id });
        if (!result.ok) throw result.error;
      }}
      onClose={onClose}
    />
  );
}

export function DeleteWorkspaceDialog({
  workspace,
  onClose,
}: {
  workspace: { id: string; name: string };
  onClose: () => void;
}) {
  return (
    <ConfirmDialog
      title={`删除工作区「${workspace.name}」？`}
      body="会删除 Aura 为它保存的全部会话（对话、过程与审计记录）和记忆索引，无法恢复。文件夹里的内容（包括 .memories）不受影响。"
      action="删除工作区"
      busyAction="正在删除…"
      run={() => deleteArchivedWorkspace(workspace.id)}
      onClose={onClose}
    />
  );
}
