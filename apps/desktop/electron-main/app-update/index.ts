import { dirname } from 'node:path';
import { ipcMain, session, type IpcMainInvokeEvent, type WebContents } from 'electron';
import {
  APP_UPDATE_CHANNEL,
  APP_UPDATE_STATE_CHANNEL,
  isAppUpdateCommand,
  type AppUpdateState,
} from '../../shared/app-updates.js';
import { publishToRenderer } from '../publish-to-renderer.js';
import { UPDATE_REPOSITORY, releasesUrl } from './feed.js';
import { installationOf } from './installation.js';
import { AppUpdates } from './service.js';

export interface UpdateProxy {
  enabled: boolean;
  host: string;
  port: number;
  protocol: 'http' | 'socks5';
}

export interface RegisteredAppUpdates {
  updates: AppUpdates;
  /** Aura's own network proxy setting: used when on, the system's otherwise. */
  setProxy(proxy: UpdateProxy | null | undefined): void;
}

/**
 * Aura's own updates for the window: checks and downloads go through a session of their own, so
 * they follow the proxy the user gave Aura, or the system's. A test release may name another
 * listing in AURA_UPDATE_FEED_URL; a released product always asks GitHub.
 */
export function registerAppUpdates(options: {
  appIsPackaged: boolean;
  manifestPath: string;
  dataRoot: string;
  current: string;
  namespace: string;
  contents: () => WebContents | undefined;
  trust: (event: IpcMainInvokeEvent) => void;
  requestRestart: () => void;
  log: (event: string, fields: Record<string, unknown>) => void;
  /** Told of every state as it is published. */
  onState?: (state: AppUpdateState) => void;
}): RegisteredAppUpdates {
  const updatesSession = session.fromPartition('aura-updates');
  void updatesSession.setProxy({ mode: 'system' });
  const testFeed = options.namespace.startsWith('aura.desktop.test.')
    ? process.env.AURA_UPDATE_FEED_URL
    : undefined;
  const updates = new AppUpdates({
    installation: installationOf({
      appIsPackaged: options.appIsPackaged,
      versionDirectory: dirname(options.manifestPath),
      dataRoot: options.dataRoot,
      current: options.current,
    }),
    current: options.current,
    fetch: (url, init) => updatesSession.fetch(url, init),
    feedUrl: testFeed ?? releasesUrl(UPDATE_REPOSITORY),
    testFeed: testFeed !== undefined,
    userAgent: `Aura/${options.current}`,
    publish: (state) => {
      publishToRenderer(options.contents(), APP_UPDATE_STATE_CHANNEL, state);
      options.onState?.(state);
    },
    requestRestart: options.requestRestart,
    log: options.log,
  });
  ipcMain.handle(APP_UPDATE_CHANNEL, async (event, value: unknown) => {
    options.trust(event);
    if (!isAppUpdateCommand(value)) throw new Error('app_update.invalid_command');
    return updates.handle(value);
  });
  void updates.start();
  return {
    updates,
    setProxy(proxy) {
      void updatesSession.setProxy(
        proxy?.enabled
          ? { proxyRules: `${proxy.protocol}://${proxy.host}:${proxy.port}` }
          : { mode: 'system' },
      );
    },
  };
}
