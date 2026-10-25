import { ipcRenderer, type IpcRendererEvent } from 'electron';
import { isProjectionCompatibilitySnapshot } from '../shared/projection-compatibility-validation.js';
import {
  PROJECTION_COMPATIBILITY_EVENT_CHANNEL,
  PROJECTION_COMPATIBILITY_SNAPSHOT_CHANNEL,
  type ProjectionCompatibilityApiV1,
} from '../shared/projection-compatibility-api.js';
import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import { isRecord } from '../shared/projection-validation/common.js';

export function projectionCompatibilityBridge(
  isError: (value: unknown) => value is IpcErrorEnvelope,
  error: (code: string, message: string) => IpcErrorEnvelope,
): ProjectionCompatibilityApiV1 {
  return {
    snapshot: async () => {
      const result: unknown = await ipcRenderer.invoke(PROJECTION_COMPATIBILITY_SNAPSHOT_CHANNEL);
      if (isRecord(result) && result.ok === true && isProjectionCompatibilitySnapshot(result.value))
        return { ok: true, value: result.value };
      return {
        ok: false,
        error:
          isRecord(result) && result.ok === false && isError(result.error)
            ? result.error
            : error('projection.compatibility.invalid', '投影兼容性状态格式不兼容。'),
      };
    },
    onChange: (listener) => {
      if (typeof listener !== 'function')
        throw new TypeError('Projection compatibility listener must be a function');
      const wrapped = (_event: IpcRendererEvent, value: unknown) => {
        if (isProjectionCompatibilitySnapshot(value)) listener(value);
      };
      ipcRenderer.on(PROJECTION_COMPATIBILITY_EVENT_CHANNEL, wrapped);
      return () => ipcRenderer.removeListener(PROJECTION_COMPATIBILITY_EVENT_CHANNEL, wrapped);
    },
  };
}
