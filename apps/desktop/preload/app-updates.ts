import { ipcRenderer, type IpcRendererEvent } from 'electron';
import {
  APP_UPDATE_CHANNEL,
  APP_UPDATE_STATE_CHANNEL,
  UNAVAILABLE_UPDATE_STATE,
  isAppUpdateCommand,
  readAppUpdateState,
  type AppUpdateCommand,
  type AppUpdateState,
} from '../shared/app-updates.js';

/** Aura's own updates: a command to Main, and the state Main reports as it changes. */
export function appUpdateBridge() {
  return {
    async appUpdate(command: AppUpdateCommand): Promise<AppUpdateState> {
      if (!isAppUpdateCommand(command)) return UNAVAILABLE_UPDATE_STATE;
      return readAppUpdateState(
        await ipcRenderer.invoke(APP_UPDATE_CHANNEL, command).catch(() => undefined),
      );
    },
    onAppUpdateState(listener: (state: AppUpdateState) => void): () => void {
      const received = (_event: IpcRendererEvent, value: unknown) =>
        listener(readAppUpdateState(value));
      ipcRenderer.on(APP_UPDATE_STATE_CHANNEL, received);
      return () => {
        ipcRenderer.off(APP_UPDATE_STATE_CHANNEL, received);
      };
    },
  };
}
