import { ipcMain, type IpcMainInvokeEvent } from 'electron';
import { isAgentParams } from '../shared/agent-validation.js';
import { isValidApiKeyBytes } from '../shared/p05-validation.js';
import { errorEnvelope, CoreRpcError } from './core-transport.js';
import type { CoreSupervisor } from './core-supervisor.js';

/**
 * One MCP server's secret on its own closed channel. The metadata is validated on its
 * own; the bytes are copied once, zeroed at every hop and sent only as a SecretInput frame.
 */
export function registerMcpSecretIpc(
  getSupervisor: () => CoreSupervisor | undefined,
  trust: (event: IpcMainInvokeEvent) => void,
) {
  ipcMain.handle('aura:v1:mcp-secret', async (event, params: unknown, source: unknown) => {
    let secret: Buffer | undefined;
    let submitted = false;
    try {
      trust(event);
      if (!isAgentParams('agent.mcp.secret.set', params) || !isValidApiKeyBytes(source))
        return {
          ok: false,
          error: errorEnvelope('mcp.secret_invalid_input', '密钥输入无效，请重新填写。', 'failed'),
        };
      secret = Buffer.from(source);
      source.fill(0);
      const supervisor = getSupervisor();
      if (!supervisor)
        return { ok: false, error: errorEnvelope('core_not_ready', 'Core 尚未就绪。', 'failed') };
      submitted = true;
      return {
        ok: true,
        value: await supervisor.requestAgentSecret('agent.mcp.secret.set', params, secret),
      };
    } catch (error) {
      return {
        ok: false,
        error:
          error instanceof CoreRpcError
            ? error.envelope
            : {
                ...errorEnvelope(
                  submitted ? 'command.outcome_unknown' : 'mcp.secret_failed',
                  '密钥没有确认保存，请刷新后查看服务器状态；不要自动重发。',
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
