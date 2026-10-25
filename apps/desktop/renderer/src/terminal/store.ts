import { userMessage } from '../agent-session/user-message';
import type { TerminalOutputBatchV1, TerminalViewV1 } from '@aura/contracts/generated';
import type { TerminalApiV1 } from '../../../shared/terminal-api';

export interface TerminalRecord {
  view: TerminalViewV1;
  offset: number;
  runtimeId?: string;
  pending: Map<number, TerminalOutputBatchV1>;
  accept?: (bytes: Uint8Array) => void;
  queued: Uint8Array[];
  error?: string;
}
const records = new Map<string, TerminalRecord>();
const listeners = new Set<() => void>();
let snapshot: TerminalRecord[] = [];
let started = false;
let prepareRenderer: ((record: TerminalRecord) => void) | undefined;
export const configureTerminalRenderer = (prepare: (record: TerminalRecord) => void) => {
  prepareRenderer = prepare;
};
const publish = () => {
  snapshot = [...records.values()];
  for (const listener of listeners) listener();
};
export const subscribeTerminals = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};
export const terminalSnapshot = () => snapshot;
export const terminalApi = () => window.aura?.v1.terminal;
const bytes = (value: string) => Uint8Array.from(atob(value), (char) => char.charCodeAt(0));
export function receiveTerminalOutput(batch: TerminalOutputBatchV1) {
  const record = records.get(batch.terminal_id);
  if (!record || record.view.session_id !== batch.session_id) return;
  if (record.runtimeId && record.runtimeId !== batch.runtime_instance_id) return;
  record.runtimeId = batch.runtime_instance_id;
  if (batch.next_offset < record.offset || (batch.next_offset === record.offset && !batch.end))
    return;
  record.pending.set(batch.start_offset, batch);
  for (;;) {
    const next = [...record.pending.values()].find(
      (item) =>
        item.start_offset === record.offset ||
        (item.truncated && item.start_offset > record.offset),
    );
    if (!next) break;
    record.pending.delete(next.start_offset);
    if (next.truncated)
      deliver(record, new TextEncoder().encode('\r\n[较早的输出已超过保留上限]\r\n'));
    const data = bytes(next.data_base64);
    if (data.byteLength !== next.next_offset - next.start_offset) {
      record.error = '终端输出偏移不匹配，请重新打开终端页。';
      publish();
      break;
    }
    deliver(record, data);
    record.offset = next.next_offset;
    if (next.end) {
      record.view = { ...record.view, end: next.end };
      publish();
    }
  }
}
function deliver(record: TerminalRecord, data: Uint8Array) {
  if (data.length) {
    if (record.accept) record.accept(data);
    else record.queued.push(data);
  }
}
export function attachTerminalConsumer(record: TerminalRecord, accept: (data: Uint8Array) => void) {
  record.accept = accept;
  for (const data of record.queued) accept(data);
  record.queued = [];
}
export function startTerminalStore() {
  if (started || !terminalApi()) return;
  started = true;
  terminalApi()!.onOutput((batches) => {
    for (const batch of batches) receiveTerminalOutput(batch);
  });
  terminalApi()!.onState((view) => {
    if (view.end?.reason === 'user_closed' && !records.has(view.terminal_id)) return;
    void addTerminal(view);
  });
  window.aura.v1.onCoreHealth((change) => {
    for (const record of records.values()) {
      if (record.view.end) continue;
      if (
        (record.runtimeId && record.runtimeId !== change.runtime_instance_id) ||
        [
          'starting',
          'restarting',
          'stopped',
          'incompatible',
          'crashed',
          'recovery_required',
        ].includes(change.payload.state)
      ) {
        record.view = {
          ...record.view,
          end: { reason: 'core_stopped', exit_code: null, stop_confirmed: false, error: null },
        };
      }
    }
    publish();
  });
}
export async function addTerminal(view: TerminalViewV1, api: TerminalApiV1 = terminalApi()!) {
  let record = records.get(view.terminal_id);
  if (record) {
    if (
      BigInt(view.revision) > BigInt(record.view.revision) ||
      (view.revision === record.view.revision && (!record.view.end || view.end))
    ) {
      record.view = view;
      publish();
    }
    return record;
  }
  record = { view, offset: 0, pending: new Map(), queued: [] };
  records.set(view.terminal_id, record);
  prepareRenderer?.(record);
  publish();
  const result = await api.subscribe({
    session_id: view.session_id,
    terminal_id: view.terminal_id,
    offset: 0,
  });
  if (result.ok) receiveTerminalOutput(result.value);
  else {
    record.error = userMessage(result.error, '终端操作没有完成，请重试。');
    publish();
  }
  return record;
}
export function removeTerminal(id: string) {
  records.delete(id);
  publish();
}
export function terminalFailure(record: TerminalRecord, message: string) {
  record.error = message;
  publish();
}

/** Tests start from no terminals and no subscriptions. */
export function resetTerminalStoreForTests() {
  records.clear();
  started = false;
  publish();
}
