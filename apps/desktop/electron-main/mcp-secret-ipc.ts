import { ipcMain, type IpcMainInvokeEvent } from 'electron';
import { isAgentParams } from '../shared/agent-validation.js';
import { isValidApiKeyBytes } from '../shared/p05-validation.js';
import { errorEnvelope, CoreRpcError } from './core-transport.js';
import type { CoreSupervisor } from './core-supervisor.js';

/** Each extension secret's closed channel, its route and what a failure is called. */
const EXTENSION_SECRETS = [
  {
    channel: 'aura:v1:mcp-secret',
    command: 'agent.mcp.secret.set',
    code: 'mcp',
    unconfirmed: '密钥没有确认保存，请刷新后查看服务器状态；不要自动重发。',
  },
  {
    channel: 'aura:v1:plugin-secret',
    command: 'agent.plugins.secret.set',
    code: 'plugins',
    unconfirmed: '保密设置没有确认保存，请刷新插件页查看；不要自动重发。',
  },
] as const;

/**
 * An MCP server's secret, and one of a plugin's secret settings, each on its own closed channel.
 * The metadata is validated on its own; the bytes are copied once, zeroed at every hop and sent
 * only as a SecretInput frame.
 */
export function registerExtensionSecretIpc(
  getSupervisor: () => CoreSupervisor | undefined,
  trust: (event: IpcMainInvokeEvent) => void,
) {
  for (const { channel, command, code, unconfirmed } of EXTENSION_SECRETS)
    ipcMain.handle(channel, async (event, params: unknown, source: unknown) => {
      let secret: Buffer | undefined;
      let submitted = false;
      try {
        trust(event);
        if (!isAgentParams(command, params) || !isValidApiKeyBytes(source))
          return {
            ok: false,
            error: errorEnvelope(
              `${code}.secret_invalid_input`,
              '密钥输入无效，请重新填写。',
              'failed',
            ),
          };
        secret = Buffer.from(source);
        source.fill(0);
        const supervisor = getSupervisor();
        if (!supervisor)
          return { ok: false, error: errorEnvelope('core_not_ready', 'Core 尚未就绪。', 'failed') };
        submitted = true;
        return { ok: true, value: await supervisor.requestAgentSecret(command, params, secret) };
      } catch (error) {
        return {
          ok: false,
          error:
            error instanceof CoreRpcError
              ? error.envelope
              : {
                  ...errorEnvelope(
                    submitted ? 'command.outcome_unknown' : `${code}.secret_failed`,
                    unconfirmed,
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
