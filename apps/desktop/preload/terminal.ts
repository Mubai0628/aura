import { ipcRenderer, type IpcRendererEvent } from 'electron';
import { submitTerminalSensitive, onTerminalSensitive } from './terminal-secret.js';
import type {
  AgentMainCommand,
  AgentMainParams,
  AgentMainResult,
} from '../shared/generated/routes.js';
import { parseAgentResult } from '../shared/agent-validation.js';
import {
  TERMINAL_OUTPUT_CHANNEL,
  TERMINAL_STATE_CHANNEL,
  type TerminalApiV1,
} from '../shared/terminal-api.js';
import type { ResultEnvelope } from '../shared/aura-api.js';
import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import { terminalOpeningBridge } from './terminal-opening.js';
type OrderedTerminalCommand =
  'terminal.user.write' | 'terminal.user.control' | 'terminal.user.resize' | 'terminal.user.close';
export function terminalBridge(
  invoke: <C extends AgentMainCommand>(
    command: C,
    request: AgentMainParams[C],
  ) => Promise<ResultEnvelope<AgentMainResult[C]>>,
  error: (code: string, message: string) => IpcErrorEnvelope,
  isError: (value: unknown) => value is IpcErrorEnvelope,
): TerminalApiV1 {
  // Input and lifecycle commands share call order for one session/terminal.
  // A failed/unknown result releases the next call without replaying any request.
  const operations = new Map<string, Promise<void>>();
  const ordered = <C extends OrderedTerminalCommand>(
    command: C,
    request: AgentMainParams[C],
  ): Promise<ResultEnvelope<AgentMainResult[C]>> => {
    const key = JSON.stringify([request?.session_id, request?.terminal_id]);
    const response = (operations.get(key) ?? Promise.resolve()).then(() =>
      invoke(command, request),
    );
    const settled = response.then(
      () => undefined,
      () => undefined,
    );
    operations.set(key, settled);
    void settled.then(() => {
      if (operations.get(key) === settled) operations.delete(key);
    });
    return response;
  };
  return {
    ...terminalOpeningBridge(isError, error),
    open: (request) => invoke('terminal.user.open', request),
    list: (request) => invoke('terminal.user.list', request),
    write: (request) => ordered('terminal.user.write', request),
    resize: (request) => ordered('terminal.user.resize', request),
    close: (request) => ordered('terminal.user.close', request),
    subscribe: (request) => invoke('terminal.user.subscribe', request),
    control: (request) => ordered('terminal.user.control', request),
    pendingSensitive: (request) => invoke('terminal.user.sensitive.pending', request),
    cancelSensitive: (request) => invoke('terminal.user.sensitive.cancel', request),
    submitSensitive: submitTerminalSensitive(isError, error),
    listProcesses: (request) => invoke('agent.process.list', request),
    stopProcess: (request) => invoke('agent.process.stop', request),
    onSensitive: onTerminalSensitive,
    onState: (listener) => {
      if (typeof listener !== 'function')
        throw new TypeError('Terminal listener must be a function');
      const wrapped = (_event: IpcRendererEvent, value: unknown) => {
        try {
          const view = parseAgentResult('terminal.user.list', {
            terminals: [value],
            next_cursor: null,
          }).terminals[0];
          if (view) listener(view);
        } catch {
          /* invalid state cannot enter renderer */
        }
      };
      ipcRenderer.on(TERMINAL_STATE_CHANNEL, wrapped);
      return () => ipcRenderer.removeListener(TERMINAL_STATE_CHANNEL, wrapped);
    },
    onOutput: (listener) => {
      if (typeof listener !== 'function')
        throw new TypeError('Terminal listener must be a function');
      const wrapped = (_event: IpcRendererEvent, value: unknown) => {
        if (!Array.isArray(value)) return;
        try {
          listener(value.map((item) => parseAgentResult('terminal.user.subscribe', item)));
        } catch {
          /* invalid output cannot enter renderer */
        }
      };
      ipcRenderer.on(TERMINAL_OUTPUT_CHANNEL, wrapped);
      return () => ipcRenderer.removeListener(TERMINAL_OUTPUT_CHANNEL, wrapped);
    },
  };
}
