import { ipcRenderer, type IpcRendererEvent } from 'electron';
import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import { parseAgentResult } from '../shared/agent-validation.js';
import { isRecord } from '../shared/projection-validation/common.js';
import {
  isTerminalOpeningStart,
  isTerminalOpeningRef,
  isTerminalOpeningChange,
  isCancelDisposition,
} from '../shared/terminal-opening-validation.js';
import {
  TERMINAL_OPENING_START_CHANNEL,
  TERMINAL_OPENING_CANCEL_CHANNEL,
  TERMINAL_OPENING_EVENT_CHANNEL,
  type TerminalOpeningApiV1,
} from '../shared/terminal-opening-api.js';
export function terminalOpeningBridge(
  isError: (value: unknown) => value is IpcErrorEnvelope,
  error: (code: string, message: string) => IpcErrorEnvelope,
): TerminalOpeningApiV1 {
  const invalid = () => ({
    ok: false as const,
    error: error('terminal.opening.invalid', '终端打开状态格式不兼容。'),
  });
  const invoke = async <T>(channel: string, source: unknown, parse: (value: unknown) => T) => {
    try {
      const result: unknown = await ipcRenderer.invoke(channel, source);
      if (isRecord(result) && result.ok === true)
        return { ok: true as const, value: parse(result.value) };
      if (isRecord(result) && result.ok === false && isError(result.error))
        return { ok: false as const, error: result.error };
    } catch {
      /* no retry or request replay */
    }
    return invalid();
  };
  return {
    beginOpen: (source) =>
      isTerminalOpeningStart(source)
        ? invoke(TERMINAL_OPENING_START_CHANNEL, source, (value) =>
            parseAgentResult('terminal.user.open', value),
          )
        : Promise.resolve(invalid()),
    cancelOpen: (source) =>
      isTerminalOpeningRef(source)
        ? invoke(TERMINAL_OPENING_CANCEL_CHANNEL, source, (value) => {
            if (!isCancelDisposition(value)) throw new Error('invalid cancellation');
            return value;
          })
        : Promise.resolve(invalid()),
    onOpening: (listener) => {
      if (typeof listener !== 'function')
        throw new TypeError('Terminal opening listener must be a function');
      const wrapped = (_event: IpcRendererEvent, value: unknown) => {
        if (isTerminalOpeningChange(value)) listener(value);
      };
      ipcRenderer.on(TERMINAL_OPENING_EVENT_CHANNEL, wrapped);
      return () => {
        ipcRenderer.removeListener(TERMINAL_OPENING_EVENT_CHANNEL, wrapped);
      };
    },
  };
}
