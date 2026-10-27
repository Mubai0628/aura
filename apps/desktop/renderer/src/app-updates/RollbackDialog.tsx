import { useId, useRef, useState } from 'react';
import { ModalDialog } from '../ModalDialog';
import { AppIcon } from '../app-icons';
import '../model-dialogs.css';

export function RollbackDialog({
  target,
  busy,
  valid,
  needsData,
  error,
  onCancel,
  onConfirm,
}: {
  target: string;
  busy: boolean;
  valid: boolean;
  /** The update changed the data, so going back always puts the copy from before it back. */
  needsData: boolean;
  error: string | undefined;
  onCancel: () => void;
  onConfirm: (restoreData: boolean) => Promise<boolean>;
}) {
  const titleId = useId();
  const warningId = useId();
  const cancel = useRef<HTMLButtonElement>(null);
  const [chosen, setRestoreData] = useState(false);
  const restoreData = needsData || chosen;
  const confirm = async () => {
    if (busy || !valid) return;
    await onConfirm(restoreData);
  };
  return (
    <ModalDialog
      className="confirmation-dialog update-rollback-dialog"
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
          <h2 id={titleId}>回到上一版本</h2>
          <p>将回到 {target} 并重启 Aura。运行中的任务会被中断，请先保存未保存的内容。</p>
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
      <div className="update-rollback-options">
        <label className="update-restore-choice">
          <input
            aria-describedby={restoreData ? warningId : undefined}
            checked={restoreData}
            disabled={busy || needsData}
            onChange={(event) => setRestoreData(event.target.checked)}
            type="checkbox"
          />
          同时恢复更新前的数据
        </label>
        {restoreData ? (
          <p className="update-error" id={warningId}>
            {needsData
              ? `这次更新升级了数据，${target} 读不了升级后的数据，所以会一起恢复更新前的备份；`
              : '将恢复更新到当前版本之前的备份，'}
            更新之后的新会话和改动会丢失。
          </p>
        ) : null}
        {!valid ? <p role="alert">版本状态已变化，请关闭后重新确认。</p> : null}
        {error ? (
          <p className="update-error" role="alert">
            {error}
          </p>
        ) : null}
      </div>
      <footer className="modal-actions confirmation-dialog-actions">
        <button disabled={busy} onClick={onCancel} ref={cancel} type="button">
          取消
        </button>
        <button
          className="danger-button"
          disabled={busy || !valid}
          onClick={() => void confirm()}
          type="button"
        >
          {busy ? '正在回退…' : restoreData ? '恢复数据并回退' : '确认回退'}
        </button>
      </footer>
    </ModalDialog>
  );
}
