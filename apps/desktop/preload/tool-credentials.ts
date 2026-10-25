import { ipcRenderer } from 'electron';
import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import type { AuraApiV1, ResultEnvelope } from '../shared/aura-api.js';
import {
  isToolCredentialParams,
  parseToolCredentialResult,
  type ToolCredentialMainCommand,
  type ToolCredentialMainParams,
  type ToolCredentialMainResult,
  type ToolCredentialSecretCommand,
} from '../shared/tool-credential-validation.js';
import { isValidApiKeyBytes } from '../shared/p05-validation.js';

export function toolCredentialBridge(
  invoke: (command: string, params: unknown) => Promise<unknown>,
  isError: (value: unknown) => value is IpcErrorEnvelope,
  invalid: () => IpcErrorEnvelope,
): Pick<
  AuraApiV1,
  | 'listToolCredentials'
  | 'updateToolCredentialOrigins'
  | 'createToolCredential'
  | 'rotateToolCredential'
  | 'clearToolCredential'
  | 'recoverToolCredential'
  | 'readToolCredentialOutcome'
> {
  function parse<C extends ToolCredentialMainCommand>(
    command: C,
    result: unknown,
  ): ResultEnvelope<ToolCredentialMainResult[C]> {
    if (result && typeof result === 'object' && 'ok' in result) {
      if (result.ok === false && 'error' in result && isError(result.error))
        return { ok: false, error: result.error };
      if (result.ok === true && 'value' in result) {
        try {
          return { ok: true, value: parseToolCredentialResult(command, result.value) };
        } catch {
          /* closed failure below */
        }
      }
    }
    const error = invalid();
    return {
      ok: false,
      error:
        command === 'tools.credential.list' || command === 'tools.credential.outcome'
          ? error
          : {
              ...error,
              code: 'command.outcome_unknown',
              details: { outcome_recovery: 'query_runtime.command_outcome_without_replay' },
            },
    };
  }
  async function normal<C extends ToolCredentialMainCommand>(
    command: C,
    params: ToolCredentialMainParams[C],
  ): Promise<ResultEnvelope<ToolCredentialMainResult[C]>> {
    if (!isToolCredentialParams(command, params)) return { ok: false, error: invalid() };
    return parse(command, await invoke(command, params));
  }
  async function withSecret<C extends ToolCredentialSecretCommand>(
    command: C,
    params: ToolCredentialMainParams[C],
    source: Uint8Array,
  ): Promise<ResultEnvelope<ToolCredentialMainResult[C]>> {
    let copy: Uint8Array | undefined;
    try {
      if (!isToolCredentialParams(command, params) || !isValidApiKeyBytes(source))
        return { ok: false, error: invalid() };
      copy = new Uint8Array(source);
      source.fill(0);
      return parse(
        command,
        await ipcRenderer.invoke('aura:v1:tool-secret', { command, params }, copy),
      );
    } finally {
      if (source instanceof Uint8Array) source.fill(0);
      copy?.fill(0);
    }
  }
  return {
    updateToolCredentialOrigins: (request) => normal('tools.credential.origins', request),
    listToolCredentials: (request) => normal('tools.credential.list', request),
    readToolCredentialOutcome: (request) => normal('tools.credential.outcome', request),
    createToolCredential: (request, secret) =>
      withSecret('tools.credential.create', request, secret),
    rotateToolCredential: (request, secret) =>
      withSecret('tools.credential.rotate', request, secret),
    clearToolCredential: (request) => normal('tools.credential.clear', request),
    recoverToolCredential: (request) => normal('tools.credential.recover', request),
  };
}
