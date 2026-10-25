import { ipcMain, type IpcMainInvokeEvent, type WebContents } from 'electron';
import type { CoreSupervisor } from './core-supervisor.js';
import { errorEnvelope } from './core-transport.js';
import { publishToRenderer } from './publish-to-renderer.js';
import {
  PROJECTION_COMPATIBILITY_EVENT_CHANNEL,
  PROJECTION_COMPATIBILITY_SNAPSHOT_CHANNEL,
} from '../shared/projection-compatibility-api.js';

export function registerProjectionCompatibilityIpc(
  getSupervisor: () => CoreSupervisor | undefined,
  trust: (event: IpcMainInvokeEvent) => void,
): void {
  ipcMain.handle(PROJECTION_COMPATIBILITY_SNAPSHOT_CHANNEL, (event, ...args: unknown[]) => {
    try {
      trust(event);
      const supervisor = getSupervisor();
      if (args.length !== 0 || supervisor === undefined) throw new Error('unavailable metadata');
      return { ok: true, value: supervisor.projectionCompatibilitySnapshot };
    } catch {
      return {
        ok: false,
        error: errorEnvelope(
          'projection.compatibility.unavailable',
          '投影兼容性状态暂不可用。',
          'failed',
        ),
      };
    }
  });
}

export function publishProjectionCompatibility(
  supervisor: CoreSupervisor,
  getContents: () => WebContents | undefined,
): () => void {
  return supervisor.subscribeProjectionCompatibility((snapshot) =>
    publishToRenderer(getContents(), PROJECTION_COMPATIBILITY_EVENT_CHANNEL, snapshot),
  );
}
