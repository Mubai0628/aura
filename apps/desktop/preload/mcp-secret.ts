import { ipcRenderer } from 'electron';
import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import { isAgentParams, parseAgentResult } from '../shared/agent-validation.js';
import { isValidApiKeyBytes } from '../shared/p05-validation.js';
import type { AuraApiV1 } from '../shared/aura-api.js';

type ExtensionSecretCommand = 'agent.mcp.secret.set' | 'agent.plugins.secret.set';

/** Sends one extension secret on its own closed channel; the source bytes are zeroed. */
function sendExtensionSecret<C extends ExtensionSecretCommand>(
  channel: string,
  command: C,
  invalidCode: string,
  isError: (value: unknown) => value is IpcErrorEnvelope,
  error: (code: string, message: string) => IpcErrorEnvelope,
) {
  return async (request: unknown, source: Uint8Array) => {
    const invalid = () => ({
      ok: false as const,
      error: error(invalidCode, '密钥没有确认保存，请重新填写；不要自动重发。'),
    });
    let copy: Uint8Array | undefined;
    try {
      if (!isAgentParams(command, request) || !isValidApiKeyBytes(source)) return invalid();
      copy = new Uint8Array(source);
      source.fill(0);
      const result: unknown = await ipcRenderer.invoke(channel, request, copy);
      if (!result || typeof result !== 'object' || !('ok' in result)) return invalid();
      if (result.ok === true && 'value' in result)
        return { ok: true as const, value: parseAgentResult(command, result.value) };
      if (result.ok === false && 'error' in result && isError(result.error))
        return { ok: false as const, error: result.error };
      return invalid();
    } catch {
      return invalid();
    } finally {
      if (source instanceof Uint8Array) source.fill(0);
      copy?.fill(0);
    }
  };
}

/** Sends one MCP server's secret on the closed `aura:v1:mcp-secret` channel. */
export const setMcpSecret = (
  isError: (value: unknown) => value is IpcErrorEnvelope,
  error: (code: string, message: string) => IpcErrorEnvelope,
): AuraApiV1['setMcpSecret'] =>
  sendExtensionSecret(
    'aura:v1:mcp-secret',
    'agent.mcp.secret.set',
    'mcp.secret_invalid_input',
    isError,
    error,
  );

/** Sends one of a plugin's secret settings on the closed `aura:v1:plugin-secret` channel. */
export const setPluginSecret = (
  isError: (value: unknown) => value is IpcErrorEnvelope,
  error: (code: string, message: string) => IpcErrorEnvelope,
): AuraApiV1['setPluginSecret'] =>
  sendExtensionSecret(
    'aura:v1:plugin-secret',
    'agent.plugins.secret.set',
    'plugins.secret_invalid_input',
    isError,
    error,
  );
