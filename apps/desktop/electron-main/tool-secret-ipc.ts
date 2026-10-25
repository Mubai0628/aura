import { CORE_NOT_READY_MESSAGE } from './core-unavailable.js';
import { ipcMain, type IpcMainInvokeEvent } from 'electron';
import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import {
  isToolCredentialParams,
  isToolCredentialSecretCommand,
} from '../shared/tool-credential-validation.js';
import { isValidApiKeyBytes } from '../shared/p05-validation.js';
import { CoreSupervisor } from './core-supervisor.js';
import { errorEnvelope } from './core-transport.js';

/** One closed channel: metadata is validated independently of zeroized SecretInput bytes. */
export function registerToolSecretIpc(
  getSupervisor: () => CoreSupervisor | undefined,
  trust: (event: IpcMainInvokeEvent) => void,
) {
  ipcMain.handle('aura:v1:tool-secret', async (event, request: unknown, source: unknown) => {
    let secret: Buffer | undefined;
    let submitted = false;
    try {
      trust(event);
      if (
        request === null ||
        typeof request !== 'object' ||
        !('command' in request) ||
        !('params' in request) ||
        typeof request.command !== 'string' ||
        !isToolCredentialSecretCommand(request.command) ||
        !isToolCredentialParams(request.command, request.params) ||
        !isValidApiKeyBytes(source)
      ) {
        return {
          ok: false,
          error: errorEnvelope('invalid_secret_input', 'Invalid tool credential input.', 'failed'),
        };
      }
      secret = Buffer.from(source);
      source.fill(0);
      const supervisor = getSupervisor();
      if (!supervisor)
        return {
          ok: false,
          error: errorEnvelope('core_not_ready', CORE_NOT_READY_MESSAGE, 'failed'),
        };
      submitted = true;
      return {
        ok: true,
        value: await supervisor.requestToolCredential(request.command, request.params, secret),
      };
    } catch (error) {
      const envelope =
        error instanceof Error && 'envelope' in error
          ? (error as { envelope: IpcErrorEnvelope }).envelope
          : {
              ...errorEnvelope(
                submitted ? 'command.outcome_unknown' : 'tool_credential_request_failed',
                '工具凭据操作没有完成，请先刷新查看当前状态再重试。',
                'failed',
              ),
              details: submitted
                ? { outcome_recovery: 'query_runtime.command_outcome_without_replay' }
                : {},
            };
      return { ok: false, error: envelope };
    } finally {
      if (source instanceof Uint8Array) source.fill(0);
      secret?.fill(0);
    }
  });
}
