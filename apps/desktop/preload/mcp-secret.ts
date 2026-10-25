import { ipcRenderer } from 'electron';
import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import { isAgentParams, parseAgentResult } from '../shared/agent-validation.js';
import { isValidApiKeyBytes } from '../shared/p05-validation.js';
import type { AuraApiV1 } from '../shared/aura-api.js';

/** Sends one MCP server's secret on the closed `aura:v1:mcp-secret` channel. */
export const setMcpSecret =
  (
    isError: (value: unknown) => value is IpcErrorEnvelope,
    error: (code: string, message: string) => IpcErrorEnvelope,
  ): AuraApiV1['setMcpSecret'] =>
  async (request, source) => {
    const invalid = () => ({
      ok: false as const,
      error: error('mcp.secret_invalid_input', '密钥没有确认保存，请重新填写；不要自动重发。'),
    });
    let copy: Uint8Array | undefined;
    try {
      if (!isAgentParams('agent.mcp.secret.set', request) || !isValidApiKeyBytes(source))
        return invalid();
      copy = new Uint8Array(source);
      source.fill(0);
      const result: unknown = await ipcRenderer.invoke('aura:v1:mcp-secret', request, copy);
      if (!result || typeof result !== 'object' || !('ok' in result)) return invalid();
      if (result.ok === true && 'value' in result)
        return { ok: true, value: parseAgentResult('agent.mcp.secret.set', result.value) };
      if (result.ok === false && 'error' in result && isError(result.error))
        return { ok: false, error: result.error };
      return invalid();
    } catch {
      return invalid();
    } finally {
      if (source instanceof Uint8Array) source.fill(0);
      copy?.fill(0);
    }
  };
