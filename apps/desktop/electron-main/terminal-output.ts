import type { TerminalOutputBatchV1 } from '@aura/contracts/generated';
import { parseAgentResult } from '../shared/agent-validation.js';
import {
  TERMINAL_OUTPUT_CHANNEL,
  TERMINAL_STATE_CHANNEL,
  TERMINAL_SENSITIVE_CHANNEL,
} from '../shared/terminal-api.js';
import type { WebContents } from 'electron';
import { readTerminalStateNotification } from './auxiliary-projections.js';
import type { ProjectionCompatibility } from './projection-compatibility.js';

type Hub = {
  pending: TerminalOutputBatchV1[];
  timer?: ReturnType<typeof setTimeout>;
  publish?: (batches: TerminalOutputBatchV1[]) => void;
  state?: (view: unknown) => void;
  sensitive?: (pending: unknown) => void;
};
const hubs = new WeakMap<object, Hub>();
const hub = (owner: object) => {
  let value = hubs.get(owner);
  if (!value) {
    value = { pending: [] };
    hubs.set(owner, value);
  }
  return value;
};
export function registerTerminalOutput(owner: object, getContents: () => WebContents | undefined) {
  hub(owner).sensitive = (pending) => {
    const contents = getContents();
    if (contents && !contents.isDestroyed()) contents.send(TERMINAL_SENSITIVE_CHANNEL, pending);
  };
  hub(owner).state = (view) => {
    const contents = getContents();
    if (contents && !contents.isDestroyed()) contents.send(TERMINAL_STATE_CHANNEL, view);
  };
  hub(owner).publish = (batches) => {
    const contents = getContents();
    if (contents && !contents.isDestroyed()) contents.send(TERMINAL_OUTPUT_CHANNEL, batches);
  };
}
export function terminalOutputNotification(
  owner: object,
  method: string,
  params: unknown,
  runtimeId: string,
  compatibility: ProjectionCompatibility,
): boolean {
  if (method === 'terminal.sensitive.v1') {
    if (
      !params ||
      typeof params !== 'object' ||
      Object.keys(params).length !== 2 ||
      !('runtime_instance_id' in params) ||
      params.runtime_instance_id !== runtimeId ||
      !('pending' in params)
    )
      throw new Error('Terminal sensitive runtime instance changed');
    hub(owner).sensitive?.(parseAgentResult('terminal.user.sensitive.pending', params.pending));
    return true;
  }
  if (method === 'terminal.state.v1') {
    const view = readTerminalStateNotification(params, runtimeId, compatibility);
    if (view) hub(owner).state?.(view);
    return true;
  }
  if (method !== 'terminal.output.v1') return false;
  const value = parseAgentResult('terminal.user.subscribe', params);
  if (value.runtime_instance_id !== runtimeId) throw new Error('Terminal runtime instance changed');
  const state = hub(owner);
  state.pending.push(value);
  if (state.timer === undefined)
    state.timer = setTimeout(() => {
      delete state.timer;
      const pending = state.pending;
      state.pending = [];
      state.publish?.(pending);
    }, 16);
  return true;
}
