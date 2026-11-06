/**
 * Plugins' notices and status entries in Main: one hub for every source (panels, and background
 * services through Core), the window told of each change and able to ask for the entries shown
 * now (after it starts again).
 */
import { ipcMain, type BrowserWindow, type IpcMainInvokeEvent } from 'electron';
import {
  PLUGIN_UI_CHANGE_CHANNEL,
  PLUGIN_UI_STATUSES_CHANNEL,
} from '../../shared/plugin-ui-api.js';
import { connectServiceUi } from './core-channel.js';
import { PluginUiHub } from './hub.js';

export { PluginUiHub } from './hub.js';

export interface PluginUiOptions {
  window(): BrowserWindow | undefined;
  assertTrustedSender(event: IpcMainInvokeEvent): void;
}

export function installPluginUi(options: PluginUiOptions): PluginUiHub {
  const hub = new PluginUiHub((change) => {
    const contents = options.window()?.webContents;
    if (contents !== undefined && !contents.isDestroyed())
      contents.send(PLUGIN_UI_CHANGE_CHANNEL, change);
  });
  ipcMain.handle(PLUGIN_UI_STATUSES_CHANNEL, (event) => {
    options.assertTrustedSender(event);
    return hub.statuses();
  });
  connectServiceUi(hub);
  return hub;
}
