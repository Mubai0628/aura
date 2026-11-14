import { ipcRenderer, type IpcRendererEvent } from 'electron';
import {
  DEFAULT_NOTIFICATION_PREFERENCES,
  NOTIFICATION_FOREGROUND_CHANNEL,
  NOTIFICATION_OPEN_CHANNEL,
  NOTIFICATION_PREFERENCES_CHANNEL,
  NOTIFICATION_UNSEEN_CHANNEL,
  isNotificationPreferencesCommand,
  readNotificationPreferences,
  readNotificationTarget,
  readUnseen,
  type NotificationsApiV1,
} from '../shared/notifications.js';
import { mainSentence } from '../shared/main-sentence.js';

/** 设置 → 通知's switches, the conversation in front, and what a clicked notification opens. */
export function notificationsBridge(): NotificationsApiV1 {
  return {
    async notificationPreferences(command) {
      if (!isNotificationPreferencesCommand(command)) return DEFAULT_NOTIFICATION_PREFERENCES;
      try {
        return readNotificationPreferences(
          await ipcRenderer.invoke(NOTIFICATION_PREFERENCES_CHANNEL, command),
        );
      } catch (error) {
        throw new Error(mainSentence(error), { cause: error });
      }
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
    onUnseenSessions(listener) {
      // A change sent while the first read is under way wins over that read.
      let changed = false;
      const received = (_event: IpcRendererEvent, value: unknown) => {
        changed = true;
        listener(readUnseen(value));
      };
      ipcRenderer.on(NOTIFICATION_UNSEEN_CHANNEL, received);
      void ipcRenderer
        .invoke(NOTIFICATION_UNSEEN_CHANNEL)
        .then((value: unknown) => {
          if (!changed) listener(readUnseen(value));
        })
        .catch(() => undefined);
      return () => {
        ipcRenderer.off(NOTIFICATION_UNSEEN_CHANNEL, received);
      };
    },
  };
}
