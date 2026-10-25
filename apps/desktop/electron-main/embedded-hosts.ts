import { app, type BrowserWindow } from 'electron';
import type { BrowserHost } from './browser-host/index.js';
import { MediaHost } from './media-host/index.js';
import type { MediaCore } from './media-host/types.js';
import { WorkspaceMedia } from './workspace-media.js';

/** Native media capabilities own no Renderer bridge to the Core-only decoder channel. */
export function createMediaHosts(owner: () => BrowserWindow | undefined) {
  const mediaHost = new MediaHost();
  app.once('will-quit', () => mediaHost.dispose());
  app.on('browser-window-created', (_event, window) => {
    window.once('closed', () => {
      // Recovery installs the replacement as owner before destroying the old renderer window.
      if (owner() === window) mediaHost.dispose();
    });
  });
  return { workspaceMedia: new WorkspaceMedia(), mediaHost };
}

export function bindEmbeddedHosts(
  browser: BrowserHost | undefined,
  media: MediaHost,
  core: MediaCore,
): void {
  browser?.bindCore(core);
  media.bindCore(core);
}
