import { ipcMain, type IpcMainInvokeEvent } from 'electron';
import { MAIN_ERROR_SNAPSHOT_CHANNEL } from '../shared/main-errors-api.js';
import type { MainErrorReporter } from './main-errors.js';
import { errorEnvelope } from './request-failure-envelopes.js';

export function registerMainErrorsIpc(
  reporter: MainErrorReporter,
  trust: (event: IpcMainInvokeEvent) => void,
) {
  ipcMain.handle(MAIN_ERROR_SNAPSHOT_CHANNEL, (event, ...args: unknown[]) => {
    try {
      trust(event);
      if (args.length) throw new Error('Unexpected arguments');
      return { ok: true, value: reporter.snapshot() };
    } catch {
      return {
        ok: false,
        error: errorEnvelope('main.error_notice.unavailable', '主进程错误记录暂不可用。', 'failed'),
      };
    }
  });
}
