import { useId, type CSSProperties, type ReactNode } from 'react';

import { AppIcon } from '../app-icons';
import { ModalDialog } from '../ModalDialog';
import './settings-item.css';

/**
 * One item of a long settings list (a plugin, Skill, MCP server or Hook) as one short row: its
 * heading, one line about it, what is wrong with it, and its most used buttons. Everything else
 * is in its dialog.
 */
export function SettingsRow({
  className,
  headingClassName,
  state,
  tone,
  index,
  heading,
  summary,
  alerts,
  actions,
}: {
  className?: string;
  /** The page's own class for the heading, kept in the dialog's heading too. */
  headingClassName?: string;
  state: string;
  tone: string | undefined;
  index: number;
  heading: ReactNode;
  summary?: ReactNode;
  alerts?: ReactNode;
  actions: ReactNode;
}) {
  return (
    <li
      className={`settings-card settings-row${className ? ` ${className}` : ''}`}
      data-state={state}
      data-tone={tone}
      style={{ '--item': index } as CSSProperties}
    >
      <div className="settings-row-main">
        <div className={`settings-card-heading${headingClassName ? ` ${headingClassName}` : ''}`}>
          {heading}
        </div>
        {summary ? (
          <p
            className="settings-row-summary"
            title={typeof summary === 'string' ? summary : undefined}
          >
            {summary}
          </p>
        ) : null}
        {alerts}
      </div>
      <div className="settings-row-actions">{actions}</div>
    </li>
  );
}

/** The row's 配置 button: it opens the item's dialog. */
export function SettingsRowOpen({ name, onOpen }: { name: string; onOpen: () => void }) {
  return (
    <button type="button" className="button is-small" aria-label={`配置 ${name}`} onClick={onOpen}>
      <AppIcon name="settings" size={14} />
      配置
    </button>
  );
}

/**
 * The whole of one item over its settings page, its name and state at the top. The page's latest
 * outcome is repeated at the top of the dialog, so what a button in it did is seen there.
 */
export function SettingsItemDialog({
  title,
  heading,
  headingClassName,
  notice,
  onClose,
  children,
}: {
  title: string;
  heading?: ReactNode;
  headingClassName?: string;
  notice?: { alert: boolean; text: string } | undefined;
  onClose: () => void;
  children: ReactNode;
}) {
  const titleId = useId();
  return (
    <ModalDialog className="settings-item-dialog" titleId={titleId} onClose={onClose}>
      <header className="modal-header">
        <div
          className={`settings-card-heading settings-item-dialog-heading${headingClassName ? ` ${headingClassName}` : ''}`}
        >
          <h2 id={titleId}>{title}</h2>
          {heading}
        </div>
        <button type="button" className="icon-button" aria-label="关闭" onClick={onClose}>
          <AppIcon name="close" />
        </button>
      </header>
      <div className="settings-item-dialog-body">
        {notice ? (
          <p
            className={notice.alert ? 'settings-status-line' : 'settings-status-line is-done'}
            role={notice.alert ? 'alert' : 'status'}
          >
            {notice.text}
          </p>
        ) : null}
        {children}
      </div>
    </ModalDialog>
  );
}
