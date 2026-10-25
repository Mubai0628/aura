import { useId, useRef } from 'react';
import { ModalDialog } from '../ModalDialog';
import { AppIcon } from '../app-icons';
import '../model-dialogs.css';

export function RestartUpdateDialog({
  version,
  busy,
  valid,
  error,
  onCancel,
  onConfirm,
}: {
  version: string;
  busy: boolean;
  valid: boolean;
  error: string | undefined;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const titleId = useId();
  const cancel = useRef<HTMLButtonElement>(null);
  return (
    <ModalDialog
      className="confirmation-dialog update-restart-dialog"
      initialFocusRef={cancel}
      layer="confirmation"
      onClose={busy ? () => undefined : onCancel}
      titleId={titleId}
    >
      <header className="modal-header">
        <div className="confirmation-copy">
          <span className="confirmation-icon">
            <AppIcon name="warning" size={22} />
          </span>
          <h2 id={titleId}>重启并更新</h2>
          <p>将更新到 {version} 并重启 Aura。运行中的任务会被中断，请先保存未保存的内容。</p>
          {!valid ? <p role="alert">版本状态已变化，请关闭后重新确认。</p> : null}
          {error ? (
            <p className="update-error" role="alert">
              {error}
            </p>
          ) : null}
        </div>
        <button
          aria-label="关闭确认对话框"
          className="icon-button"
          disabled={busy}
          onClick={onCancel}
          type="button"
        >
          <AppIcon name="close" />
        </button>
      </header>
      <footer className="modal-actions confirmation-dialog-actions">
        <button disabled={busy} onClick={onCancel} ref={cancel} type="button">
          取消
        </button>
        <button
          className="danger-button"
          disabled={busy || !valid}
          onClick={onConfirm}
          type="button"
        >
          {busy ? '正在重启…' : '重启并更新'}
        </button>
      </footer>
    </ModalDialog>
  );
}
