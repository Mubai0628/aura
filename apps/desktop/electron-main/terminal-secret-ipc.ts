import { ipcMain, type IpcMainInvokeEvent } from 'electron';
import { isAgentParams } from '../shared/agent-validation.js';
import { isValidApiKeyBytes } from '../shared/p05-validation.js';
import { errorEnvelope, CoreRpcError } from './core-transport.js';
import type { CoreSupervisor } from './core-supervisor.js';
export function registerTerminalSecretIpc(
  getSupervisor: () => CoreSupervisor | undefined,
  trust: (event: IpcMainInvokeEvent) => void,
) {
  ipcMain.handle('aura:v1:terminal-secret', async (event, params: unknown, source: unknown) => {
    let secret: Buffer | undefined;
    let submitted = false;
    try {
      trust(event);
      if (!isAgentParams('terminal.user.sensitive.submit', params) || !isValidApiKeyBytes(source))
        return {
          ok: false,
          error: errorEnvelope(
            'terminal.sensitive.invalid_input',
            '安全输入无效，请检查当前请求。',
            'failed',
          ),
        };
      secret = Buffer.from(source);
      source.fill(0);
      const supervisor = getSupervisor();
      if (!supervisor)
        return { ok: false, error: errorEnvelope('core_not_ready', 'Core 尚未就绪。', 'failed') };
      submitted = true;
      return {
        ok: true,
        value: await supervisor.requestAgentSecret(
          'terminal.user.sensitive.submit',
          params,
          secret,
        ),
      };
    } catch (error) {
      return {
        ok: false,
        error:
          error instanceof CoreRpcError
            ? error.envelope
            : {
                ...errorEnvelope(
                  submitted ? 'command.outcome_unknown' : 'terminal.sensitive.failed',
                  '安全输入未确认提交，请查看当前请求；不要自动重发。',
                  'failed',
                ),
                details: submitted
                  ? { outcome_recovery: 'query_runtime.command_outcome_without_replay' }
                  : {},
              },
      };
    } finally {
      if (source instanceof Uint8Array) source.fill(0);
      secret?.fill(0);
    }
  });
}
