import { useState } from 'react';

import type { NotificationInput } from './notification-center-store';
import { AppIcon } from './app-icons';
import { ModalDialog } from './ModalDialog';
import { userMessage } from './agent-session/user-message';
import { MODEL_FAILURE_FALLBACK } from './provider-connection/notices';

interface CustomProviderDialogProps {
  expectedRevision: string;
  onClose: () => void;
  onNotify: (notification: NotificationInput) => void;
  onSaved: () => Promise<void>;
  writeUnavailableReason?: string | undefined;
}

export function CustomProviderDialog({
  expectedRevision,
  onClose,
  onNotify,
  onSaved,
  writeUnavailableReason,
}: CustomProviderDialogProps) {
  const [displayName, setDisplayName] = useState('');
  const [saving, setSaving] = useState(false);
  const titleId = 'custom-provider-dialog-title';
  const valid = displayName.trim().length > 0;

  const save = async () => {
    if (!valid || saving || writeUnavailableReason !== undefined) return;
    setSaving(true);
    const result = await window.aura.v1.createModelProvider({
      expected_revision: expectedRevision,
      input: {
        display_name: displayName,
        source: { catalog_provider_id: null, type: 'custom' },
      },
    });
    if (!result.ok) {
      onNotify({
        dedupeKey: `model-provider-create:${result.error.code}`,
        message: userMessage(result.error, MODEL_FAILURE_FALLBACK),
        title: 'Provider 未保存',
        tone: 'danger',
      });
      setSaving(false);
      return;
    }
    await onSaved();
    onNotify({
      dedupeKey: `model-provider-created:${result.value.value.id}`,
      message: `${result.value.value.display_name} 已写入 Core 模型配置。`,
      title: 'Provider 已添加',
      tone: 'success',
    });
    onClose();
  };

  return (
    <ModalDialog
      className="provider-dialog compact-provider-dialog"
      onClose={onClose}
      titleId={titleId}
    >
      <header className="modal-header">
        <div className="provider-dialog-title">
          <span className="provider-mark" aria-hidden="true">
            <AppIcon name="plus" size={17} />
          </span>
          <div>
            <h2 id={titleId}>添加自定义 Provider</h2>
          </div>
        </div>
        <button
          aria-label="关闭 Provider 配置"
          className="icon-button"
          onClick={onClose}
          type="button"
        >
          <AppIcon name="close" />
        </button>
      </header>
      <form
        className="provider-editor"
        onSubmit={(event) => {
          event.preventDefault();
          void save();
        }}
      >
        {writeUnavailableReason === undefined ? null : (
          <div className="model-settings-reconnect-note" role="status">
            <AppIcon name="activity" size={15} />
            <span>{writeUnavailableReason}</span>
          </div>
        )}
        <div className="settings-form-grid provider-form-grid no-top-rule single-field-grid">
          <label>
            <span>Provider 名称</span>
            <input
              autoComplete="off"
              onChange={(event) => setDisplayName(event.target.value)}
              type="text"
              value={displayName}
            />
          </label>
        </div>
        <footer className="modal-actions">
          <span className="modal-action-spacer" />
          <button onClick={onClose} type="button">
            取消
          </button>
          <span
            className="disabled-action-wrapper"
            tabIndex={!valid || writeUnavailableReason !== undefined ? 0 : -1}
          >
            <button
              className="primary-button"
              disabled={!valid || saving || writeUnavailableReason !== undefined}
              type="submit"
            >
              {saving ? '保存中…' : '保存 Provider'}
            </button>
            {writeUnavailableReason === undefined ? null : (
              <span role="tooltip">{writeUnavailableReason}</span>
            )}
          </span>
        </footer>
      </form>
    </ModalDialog>
  );
}
