// @vitest-environment node
import { expect, test, vi } from 'vitest';
import type {
  IpcErrorEnvelope,
  TerminalViewV1,
  UserTerminalWriteRequestV1,
} from '@aura/contracts/generated';
import type { ResultEnvelope } from '../shared/aura-api.js';
import { terminalBridge } from '../preload/terminal.js';

vi.mock('electron', () => ({
  ipcRenderer: { on: vi.fn(), removeListener: vi.fn(), invoke: vi.fn() },
}));

function deferred() {
  let resolve!: (value: ResultEnvelope<TerminalViewV1>) => void;
  let reject!: (value: Error) => void;
  const promise = new Promise<ResultEnvelope<TerminalViewV1>>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
const request = (terminal: string, input: string): UserTerminalWriteRequestV1 => ({
  session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV' as UserTerminalWriteRequestV1['session_id'],
  terminal_id: terminal,
  input,
  interrupt: false,
});
const success = { ok: true, value: {} } as ResultEnvelope<TerminalViewV1>;
const bridgeFor = (invoke: ReturnType<typeof vi.fn>) =>
  terminalBridge(
    invoke as Parameters<typeof terminalBridge>[0],
    () => {
      throw new Error('unused error factory');
    },
    (value: unknown): value is IpcErrorEnvelope => {
      void value;
      return false;
    },
  );

test('terminal keys are sent once in order while other terminals can progress', async () => {
  const first = deferred();
  const invoke = vi
    .fn()
    .mockImplementationOnce(() => first.promise)
    .mockResolvedValue(success);
  const bridge = bridgeFor(invoke);
  const pendingFirst = bridge.write(request('A', 'first'));
  const pendingSecond = bridge.write(request('A', 'second'));
  const other = bridge.write(request('B', 'other'));
  await Promise.resolve();
  expect(invoke.mock.calls.map((call) => call[1].input)).toEqual(['first', 'other']);
  first.resolve(success);
  await Promise.all([pendingFirst, pendingSecond, other]);
  expect(invoke.mock.calls.map((call) => call[1].input)).toEqual(['first', 'other', 'second']);
});

test('an unconfirmed input is never replayed and releases the lane for fresh input', async () => {
  const first = deferred();
  const invoke = vi
    .fn()
    .mockImplementationOnce(() => first.promise)
    .mockResolvedValue(success);
  const bridge = bridgeFor(invoke);
  const pendingFirst = bridge.write(request('A', 'possibly-sent'));
  const rejection = expect(pendingFirst).rejects.toThrow('input unconfirmed');
  const next = bridge.write(request('A', 'fresh'));
  await Promise.resolve();
  expect(invoke).toHaveBeenCalledTimes(1);
  first.reject(new Error('input unconfirmed'));
  await rejection;
  await next;
  expect(invoke.mock.calls.map((call) => call[1].input)).toEqual(['possibly-sent', 'fresh']);
});

test.each(['control', 'resize', 'close'] as const)(
  '%s follows earlier input while another terminal can progress',
  async (method) => {
    const first = deferred();
    const invoke = vi.fn((_command: string, payload: { input?: string }) =>
      payload.input === 'first' ? first.promise : Promise.resolve(success),
    );
    const bridge = bridgeFor(invoke);
    const target = { session_id: request('A', '').session_id, terminal_id: 'A' };
    const pendingFirst = bridge.write(request('A', 'first'));
    const pendingSecond = bridge.write(request('A', 'second'));
    const operation =
      method === 'control'
        ? bridge.control({ ...target, controller: 'user' })
        : method === 'resize'
          ? bridge.resize({ ...target, columns: 100, rows: 30 })
          : bridge.close(target);
    const other = bridge.resize({ ...target, terminal_id: 'B', columns: 80, rows: 24 });
    await Promise.resolve();
    expect(invoke.mock.calls.map(([command, payload]) => [command, payload])).toEqual([
      ['terminal.user.write', request('A', 'first')],
      ['terminal.user.resize', { ...target, terminal_id: 'B', columns: 80, rows: 24 }],
    ]);
    first.resolve(success);
    await Promise.all([pendingFirst, pendingSecond, operation, other]);
    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      'terminal.user.write',
      'terminal.user.resize',
      'terminal.user.write',
      `terminal.user.${method}`,
    ]);
  },
);

test('a failed lifecycle operation releases queued input once with session isolation', async () => {
  const first = deferred();
  const invoke = vi.fn((command: string, payload: { input?: string }) => {
    void payload;
    return command === 'terminal.user.control' ? first.promise : Promise.resolve(success);
  });
  const bridge = bridgeFor(invoke);
  const target = { session_id: request('A', '').session_id, terminal_id: 'A' };
  const control = bridge.control({ ...target, controller: 'user' });
  const rejection = expect(control).rejects.toThrow('control unconfirmed');
  const input = bridge.write(request('A', 'fresh'));
  const otherSession = bridge.write({
    ...request('A', 'other-session'),
    session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAW' as UserTerminalWriteRequestV1['session_id'],
  });
  await Promise.resolve();
  expect(invoke.mock.calls.map(([command]) => command)).toEqual([
    'terminal.user.control',
    'terminal.user.write',
  ]);
  first.reject(new Error('control unconfirmed'));
  await rejection;
  await Promise.all([input, otherSession]);
  expect(invoke.mock.calls.map(([command]) => command)).toEqual([
    'terminal.user.control',
    'terminal.user.write',
    'terminal.user.write',
  ]);
  expect(invoke.mock.calls.filter(([, payload]) => payload.input === 'fresh')).toHaveLength(1);
});
