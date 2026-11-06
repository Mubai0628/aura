import { ipcRenderer, type IpcRendererEvent } from 'electron';
import {
  PLUGIN_PANEL_COMMAND_CHANNEL,
  PLUGIN_PANEL_STATE_CHANNEL,
  readPanelResult,
  readPanelState,
  type PluginPanelsApiV1,
} from '../shared/plugin-panel-api.js';

/** The window's side of plugin panels: placing, hiding and closing them. */
export function pluginPanelsBridge(): PluginPanelsApiV1 {
  return {
    command: async (command) => {
      try {
        return readPanelResult(await ipcRenderer.invoke(PLUGIN_PANEL_COMMAND_CHANNEL, command));
      } catch {
        return { ok: false, message: '插件面板没有回应' };
      }
    },
    onState: (listener) => {
      if (typeof listener !== 'function')
        throw new TypeError('Plugin panel listener must be a function');
      const wrapped = (_event: IpcRendererEvent, value: unknown) => {
        const state = readPanelState(value);
        if (state !== undefined) listener(state);
      };
      ipcRenderer.on(PLUGIN_PANEL_STATE_CHANNEL, wrapped);
      return () => ipcRenderer.removeListener(PLUGIN_PANEL_STATE_CHANNEL, wrapped);
    },
  };
}
