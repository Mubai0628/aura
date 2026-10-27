import { app, type BrowserWindow, type IpcMainInvokeEvent } from 'electron';

import type { CoreSupervisor } from '../core-supervisor.js';
import { notificationIdentity, registerNotificationIdentity } from './identity.js';
import { registerNotifications, type RegisteredNotifications } from './index.js';

export type { RegisteredNotifications } from './index.js';

type Core = Pick<CoreSupervisor, 'requestAgent' | 'workspaceSessionSnapshot'>;

/**
 * Sends this installation's notifications under its own identity (named for Windows when Aura is
 * installed) and starts them, reading the conversations from Core while it runs.
 */
export function startNotifications(options: {
  namespace: string;
  dataRoot: string;
  window: () => BrowserWindow | undefined;
  trust: (event: IpcMainInvokeEvent) => void;
  core: () => Core | undefined;
  log: (event: string, fields: Record<string, unknown>) => void;
}): RegisteredNotifications {
  const identity = notificationIdentity(options.namespace);
  app.setAppUserModelId(identity);
  if (app.isPackaged)
    void registerNotificationIdentity(identity).catch((error: unknown) => {
      options.log('notifications.identity_failed', { message: String(error) });
    });
  return registerNotifications({
    dataRoot: options.dataRoot,
    window: options.window,
    trust: options.trust,
    readActivity: async (request) => {
      const core = options.core();
      if (core === undefined) throw new Error('Aura Core is not running');
      return core.requestAgent('agent.sessions.activity', request);
    },
    sessionTitle: (sessionId) =>
      options
        .core()
        ?.workspaceSessionSnapshot.payload.sessions.find(
          (session) => session.session_id === sessionId,
        )?.title,
    log: options.log,
  });
}
