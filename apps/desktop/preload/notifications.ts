import { ipcRenderer, type IpcRendererEvent } from 'electron';
import {
  DEFAULT_NOTIFICATION_PREFERENCES,
  NOTIFICATION_FOREGROUND_CHANNEL,
  NOTIFICATION_OPEN_CHANNEL,
  NOTIFICATION_PREFERENCES_CHANNEL,
  isNotificationPreferencesCommand,
  readNotificationPreferences,
  readNotificationTarget,
  type NotificationsApiV1,
} from '../shared/notifications.js';

/** 设置 → 通知's switches, the conversation in front, and what a clicked notification opens. */
export function notificationsBridge(): NotificationsApiV1 {
  return {
    async notificationPreferences(command) {
      if (!isNotificationPreferencesCommand(command)) return DEFAULT_NOTIFICATION_PREFERENCES;
      return readNotificationPreferences(
        await ipcRenderer.invoke(NOTIFICATION_PREFERENCES_CHANNEL, command),
      );
    },
    setForegroundSession(sessionId) {
      void ipcRenderer
        .invoke(NOTIFICATION_FOREGROUND_CHANNEL, typeof sessionId === 'string' ? sessionId : null)
        .catch(() => undefined);
    },
    onNotificationOpen(listener) {
      const received = (_event: IpcRendererEvent, value: unknown) => {
        const target = readNotificationTarget(value);
        if (target !== undefined) listener(target);
      };
      ipcRenderer.on(NOTIFICATION_OPEN_CHANNEL, received);
      return () => {
        ipcRenderer.off(NOTIFICATION_OPEN_CHANNEL, received);
      };
    },
  };
}
