import { AppIcon, type IconName } from './app-icons';
import type { NotificationItem, NotificationTone } from './notification-center-store';
import { createPortal } from 'react-dom';

const TONE_ICONS: Record<NotificationTone, IconName> = {
  danger: 'activity',
  info: 'spark',
  success: 'check',
  warning: 'activity',
};

export function NotificationViewport({
  items,
  onDismiss,
}: {
  items: readonly NotificationItem[];
  onDismiss: (id: string) => void;
}) {
  return createPortal(
    <div aria-label="系统消息" className="notification-viewport">
      {items.map((item) => (
        <article
          className={`notification-toast notification-${item.tone}`}
          key={item.id}
          role={item.tone === 'danger' ? 'alert' : 'status'}
        >
          <span className="notification-icon" aria-hidden="true">
            <AppIcon name={TONE_ICONS[item.tone]} size={16} />
          </span>
          <div className="notification-copy">
            <strong>{item.title}</strong>
            <p>{item.message}</p>
          </div>
          <button
            aria-label="关闭消息"
            className="icon-button notification-close"
            onClick={() => onDismiss(item.id)}
            type="button"
          >
            <AppIcon name="close" size={15} />
          </button>
        </article>
      ))}
    </div>,
    document.body,
  );
}
