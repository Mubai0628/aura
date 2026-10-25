import { ipcRenderer, type IpcRendererEvent } from 'electron';
import { isAgentParams, parseAgentResult } from '../shared/agent-validation.js';
import { isValidApiKeyBytes } from '../shared/p05-validation.js';
import { TERMINAL_SENSITIVE_CHANNEL, type TerminalApiV1 } from '../shared/terminal-api.js';
import type { IpcErrorEnvelope } from '@aura/contracts/generated';
export const submitTerminalSensitive =
  (
    isError: (value: unknown) => value is IpcErrorEnvelope,
    error: (code: string, message: string) => IpcErrorEnvelope,
  ): TerminalApiV1['submitSensitive'] =>
  async (request, source) => {
    const invalid = () => ({
      ok: false as const,
      error: error(
        'terminal.sensitive.invalid_input',
        '安全输入未确认提交，请检查当前请求；不要自动重发。',
      ),
    });
    let copy: Uint8Array | undefined;
    try {
      if (!isAgentParams('terminal.user.sensitive.submit', request) || !isValidApiKeyBytes(source))
        return invalid();
      copy = new Uint8Array(source);
      source.fill(0);
      const result: unknown = await ipcRenderer.invoke('aura:v1:terminal-secret', request, copy);
      if (!result || typeof result !== 'object' || !('ok' in result)) return invalid();
      if (result.ok === true && 'value' in result)
        return {
          ok: true,
          value: parseAgentResult('terminal.user.sensitive.submit', result.value),
        };
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
export const onTerminalSensitive: TerminalApiV1['onSensitive'] = (listener) => {
  if (typeof listener !== 'function') throw new TypeError('Terminal listener must be a function');
  const wrapped = (_event: IpcRendererEvent, value: unknown) => {
    try {
      listener(parseAgentResult('terminal.user.sensitive.pending', value));
    } catch {
      /* untrusted metadata */
    }
  };
  ipcRenderer.on(TERMINAL_SENSITIVE_CHANNEL, wrapped);
  return () => ipcRenderer.removeListener(TERMINAL_SENSITIVE_CHANNEL, wrapped);
};
