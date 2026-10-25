import type { RefObject } from 'react';
import { useLayoutEffect, useRef } from 'react';

import { AppIcon } from './app-icons';
import { ModalDialog } from './ModalDialog';
import './model-dialogs.css';

interface ConfirmationDialogProps {
  busy: boolean;
  cancelLabel?: string;
  confirmLabel: string;
  description: string;
  fallbackFocusRef?: RefObject<HTMLElement | null> | undefined;
  onCancel: () => void;
  onConfirm: () => void;
  returnFocusRef?: RefObject<HTMLElement | null> | undefined;
  title: string;
  titleId: string;
}

export function ConfirmationDialog({
  busy,
  cancelLabel = '取消',
  confirmLabel,
  description,
  fallbackFocusRef,
  onCancel,
  onConfirm,
  returnFocusRef,
  title,
  titleId,
}: ConfirmationDialogProps) {
  const cancelButtonRef = useRef<HTMLButtonElement>(null);

  useLayoutEffect(() => {
    const applicationRoot = document.querySelector<HTMLElement>('#root');
    if (applicationRoot === null) return;
    const previousAriaHidden = applicationRoot.getAttribute('aria-hidden');
    const previousInert = applicationRoot.inert;
    applicationRoot.inert = true;
    applicationRoot.setAttribute('aria-hidden', 'true');
    return () => {
      applicationRoot.inert = previousInert;
      if (previousAriaHidden === null) applicationRoot.removeAttribute('aria-hidden');
      else applicationRoot.setAttribute('aria-hidden', previousAriaHidden);
    };
  }, []);

  return (
    <ModalDialog
      className="confirmation-dialog"
      fallbackFocusRef={fallbackFocusRef}
      initialFocusRef={cancelButtonRef}
      layer="confirmation"
      onClose={busy ? () => undefined : onCancel}
      returnFocusRef={returnFocusRef}
      titleId={titleId}
    >
      <header className="modal-header">
        <div className="confirmation-copy">
          <span className="confirmation-icon">
            <AppIcon name="warning" size={22} />
          </span>
          <h2 id={titleId}>{title}</h2>
          <p>{description}</p>
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
        <button disabled={busy} onClick={onCancel} ref={cancelButtonRef} type="button">
          {cancelLabel}
        </button>
        <button className="danger-button" disabled={busy} onClick={onConfirm} type="button">
          {busy ? '处理中…' : confirmLabel}
        </button>
      </footer>
    </ModalDialog>
  );
}
