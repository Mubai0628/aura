import {
  Notification,
  ipcMain,
  nativeImage,
  type BrowserWindow,
  type IpcMainInvokeEvent,
} from 'electron';
import type { AgentSessionsActivity } from '@aura/contracts/generated';

import type { AppUpdateState } from '../../shared/app-updates.js';
import {
  NOTIFICATION_FOREGROUND_CHANNEL,
  NOTIFICATION_OPEN_CHANNEL,
  NOTIFICATION_PREFERENCES_CHANNEL,
  isNotificationPreferencesCommand,
  type NotificationTarget,
} from '../../shared/notifications.js';
import { publishToRenderer } from '../publish-to-renderer.js';
import { AttentionBadge, RED_DOT_SIZE, redDotPixels } from './attention-badge.js';
import { NotificationPreferencesStore } from './preferences.js';
import { SessionNotifier, type SessionNoticeKind } from './session-notifier.js';

/** How often the conversations are looked at for what they need. */
const POLL_EVERY_MS = 2_000;

const SESSION_TEXT: Record<SessionNoticeKind, { title: string; body: string }> = {
  approval: { title: '需要审批', body: '有操作等你批准' },
  question: { title: '等待你的回答', body: '模型在等你回答或决定' },
  completed: { title: '任务完成', body: '任务已完成' },
  failed: { title: '任务没有完成', body: '出错或暂停了，需要你看一下' },
};

export interface RegisteredNotifications {
  /** Aura's update state as it changes: a new version is announced once. */
  updateState(state: AppUpdateState): void;
  stop(): void;
}

/**
 * Windows notifications: a new version of Aura, and what a conversation the user is not looking
 * at needs or has done. A conversation is in front while Aura's window has focus and shows it.
 * Clicking a notification brings the window forward with what it was about. A conversation's
 * notification also puts a red dot on Aura's taskbar button, and flashes it while the window is
 * in the background, until that conversation has been in front.
 */
export function registerNotifications(options: {
  dataRoot: string;
  window: () => BrowserWindow | undefined;
  trust: (event: IpcMainInvokeEvent) => void;
  /** Core's activity read; fails while Core is not running. */
  readActivity: (request: { ended_turns: string[] }) => Promise<AgentSessionsActivity>;
  sessionTitle: (sessionId: string) => string | undefined;
  log: (event: string, fields: Record<string, unknown>) => void;
}): RegisteredNotifications {
  const store = new NotificationPreferencesStore(options.dataRoot);
  let foreground: string | null = null;
  const live = () => {
    const window = options.window();
    return window === undefined || window.isDestroyed() ? undefined : window;
  };
  const inFront = (sessionId: string) => {
    const window = live();
    return (
      window !== undefined &&
      window.isFocused() &&
      !window.isMinimized() &&
      foreground === sessionId
    );
  };
  const dot = nativeImage.createFromBitmap(redDotPixels(), {
    width: RED_DOT_SIZE,
    height: RED_DOT_SIZE,
  });
  const badge = new AttentionBadge({
    inFront,
    apply: (count) =>
      live()?.setOverlayIcon(count > 0 ? dot : null, count > 0 ? `${count} 个会话需要你查看` : ''),
  });
  // The window a focus listener was added to: the dot goes and the flashing stops on focus.
  const watched = new WeakSet<BrowserWindow>();
  const watch = () => {
    const window = live();
    if (window === undefined || watched.has(window)) return;
    watched.add(window);
    window.on('focus', () => {
      window.flashFrame(false);
      badge.look();
    });
  };
  // A shown notification is kept until it is clicked or closed, or its click is lost.
  const shown = new Set<Notification>();

  const open = (target: NotificationTarget) => {
    const window = options.window();
    if (window === undefined || window.isDestroyed()) return;
    if (window.isMinimized()) window.restore();
    window.show();
    window.focus();
    publishToRenderer(window.webContents, NOTIFICATION_OPEN_CHANNEL, target);
  };
  const show = (title: string, body: string, target: NotificationTarget) => {
    if (!Notification.isSupported()) return;
    const notification = new Notification({ title, body });
    shown.add(notification);
    notification.on('click', () => {
      shown.delete(notification);
      open(target);
    });
    notification.on('close', () => shown.delete(notification));
    notification.show();
  };

  ipcMain.handle(NOTIFICATION_PREFERENCES_CHANNEL, async (event, value: unknown) => {
    options.trust(event);
    if (!isNotificationPreferencesCommand(value)) throw new Error('notifications.invalid_command');
    return value.kind === 'get' ? store.read() : store.save(value.preferences);
  });
  ipcMain.handle(NOTIFICATION_FOREGROUND_CHANNEL, (event, value: unknown) => {
    options.trust(event);
    foreground = typeof value === 'string' ? value : null;
    badge.look();
  });

  const notifier = new SessionNotifier({
    read: options.readActivity,
    preferences: () => store.current,
    inFront,
    notify: ({ sessionId, kind }) => {
      const title = options.sessionTitle(sessionId);
      const text = SESSION_TEXT[kind];
      show(text.title, title ? `「${title}」${text.body}` : text.body, {
        kind: 'session',
        sessionId,
      });
      watch();
      badge.add(sessionId);
      const window = live();
      if (window !== undefined && !window.isFocused()) window.flashFrame(true);
    },
  });
  let polling = false;
  let failing = false;
  const timer = setInterval(() => {
    badge.look();
    const switches = store.current;
    if (
      polling ||
      !(switches.approvals || switches.questions || switches.completions || switches.failures)
    )
      return;
    polling = true;
    notifier
      .poll()
      .then(() => {
        failing = false;
      })
      .catch((error: unknown) => {
        // Core is starting or restarting; the next look tries again. Said once per stretch.
        if (!failing)
          options.log('notifications.activity_unavailable', {
            message: error instanceof Error ? error.message : String(error),
          });
        failing = true;
      })
      .finally(() => {
        polling = false;
      });
  }, POLL_EVERY_MS);

  return {
    updateState(state) {
      const version = state.latest?.version;
      if (state.phase !== 'available' || version === undefined) return;
      void store.read().then(async (switches) => {
        if (!switches.updates || store.announcedUpdate === version) return;
        await store.announced(version);
        show('Aura 有新版本', `${version} 可以下载更新`, { kind: 'updates' });
      });
    },
    stop() {
      clearInterval(timer);
    },
  };
}
