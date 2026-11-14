import { useId, useRef } from 'react';
import { AppIcon } from '../app-icons';
import { ModalDialog } from '../ModalDialog';
import type { RelocationQuestion } from './useWorkspaceRelocation';

/** Whether the folder the user picked is the same project as the Workspace's old folder. */
export function RelocateQuestionDialog({
  busy,
  error,
  question,
  onCancel,
  onConfirm,
}: {
  busy: boolean;
  error: string | undefined;
  question: RelocationQuestion;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const titleId = useId();
  const cancelRef = useRef<HTMLButtonElement>(null);
  return (
    <ModalDialog
      className="sidebar-edit-dialog workspace-relocate-dialog"
      initialFocusRef={cancelRef}
      onClose={onCancel}
      titleId={titleId}
    >
      <header className="modal-header">
        <h2 id={titleId}>这是同一个项目吗？</h2>
        <button
          type="button"
          className="icon-button"
          aria-label="关闭"
          disabled={busy}
          onClick={onCancel}
        >
          <AppIcon name="close" />
        </button>
      </header>
      <div className="sidebar-dialog-body">
        <span className="sidebar-field-label">原位置</span>
        <p className="workspace-directory-value">{question.from}</p>
        <span className="sidebar-field-label">新位置</span>
        <p className="workspace-directory-value">{question.to}</p>
        {error && <p role="alert">{error}</p>}
        <div className="sidebar-dialog-actions">
          <button type="button" ref={cancelRef} disabled={busy} onClick={onCancel}>
            取消
          </button>
          <button type="button" disabled={busy} onClick={onConfirm}>
            {busy ? '正在切换…' : '是，切换'}
          </button>
        </div>
      </div>
    </ModalDialog>
  );
}
