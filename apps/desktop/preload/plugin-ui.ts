import { ipcRenderer, type IpcRendererEvent } from 'electron';
import {
  PLUGIN_UI_CHANGE_CHANNEL,
  PLUGIN_UI_STATUSES_CHANNEL,
  readPluginStatuses,
  readPluginUiChange,
  type PluginUiApiV1,
} from '../shared/plugin-ui-api.js';

/** Plugins' notices and status entries, as Main shows them. */
export function pluginUiBridge(): PluginUiApiV1 {
  return {
    statuses: async () => {
      try {
        return readPluginStatuses(await ipcRenderer.invoke(PLUGIN_UI_STATUSES_CHANNEL));
      } catch {
        return [];
      }
    },
    onChange: (listener) => {
      if (typeof listener !== 'function')
        throw new TypeError('Plugin UI listener must be a function');
      const wrapped = (_event: IpcRendererEvent, value: unknown) => {
        const change = readPluginUiChange(value);
        if (change !== undefined) listener(change);
      };
      ipcRenderer.on(PLUGIN_UI_CHANGE_CHANNEL, wrapped);
      return () => ipcRenderer.removeListener(PLUGIN_UI_CHANGE_CHANNEL, wrapped);
    },
  };
}
