import { ipcRenderer, type IpcRendererEvent } from 'electron';
import {
  MAIN_ERROR_EVENT_CHANNEL,
  MAIN_ERROR_SNAPSHOT_CHANNEL,
  isMainErrorNotice,
  type MainErrorsApiV1,
} from '../shared/main-errors-api.js';
import { isRecord } from '../shared/projection-validation/common.js';
import { isIpcErrorEnvelope, preloadError } from './errors.js';

export function mainErrorsBridge(): MainErrorsApiV1 {
  return {
    snapshot: async () => {
      try {
        const result: unknown = await ipcRenderer.invoke(MAIN_ERROR_SNAPSHOT_CHANNEL);
        if (
          isRecord(result) &&
          result.ok === true &&
          (result.value === null || isMainErrorNotice(result.value))
        )
          return { ok: true, value: result.value };
        if (isRecord(result) && result.ok === false && isIpcErrorEnvelope(result.error))
          return { ok: false, error: result.error };
      } catch {
        /* Return a fixed diagnostic; never repeat the request. */
      }
      return {
        ok: false,
        error: preloadError('main.error_notice.invalid', '主进程错误记录暂不可用。'),
      };
    },
    onChange: (listener) => {
      if (typeof listener !== 'function')
        throw new TypeError('Main error listener must be a function');
      const receive = (_event: IpcRendererEvent, value: unknown) => {
        if (isMainErrorNotice(value)) listener(value);
      };
      ipcRenderer.on(MAIN_ERROR_EVENT_CHANNEL, receive);
      return () => ipcRenderer.removeListener(MAIN_ERROR_EVENT_CHANNEL, receive);
    },
  };
}
