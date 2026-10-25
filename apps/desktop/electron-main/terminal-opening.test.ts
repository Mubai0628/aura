// @vitest-environment node
import { readFileSync } from 'node:fs';
import { afterEach, expect, test, vi } from 'vitest';
import type { CorrelationId, TerminalOpeningStartV1 } from '@aura/contracts/generated';
import { TerminalOpenings } from './terminal-opening.js';
import { terminalOpeningNotification } from './terminal-opening-progress.js';
import { registerTerminalOpeningIpc } from './terminal-opening-ipc.js';
import { terminalOpeningBridge } from '../preload/terminal-opening.js';
import {
  isTerminalOpeningStart,
  isTerminalOpeningRef,
  isTerminalOpeningProgress,
  isTerminalOpeningChange,
} from '../shared/terminal-opening-validation.js';
import {
  TERMINAL_OPENING_START_CHANNEL,
  TERMINAL_OPENING_CANCEL_CHANNEL,
  TERMINAL_OPENING_EVENT_CHANNEL,
} from '../shared/terminal-opening-api.js';
const ipc = vi.hoisted(() => ({
  handlers: new Map<string, (...args: unknown[]) => Promise<unknown>>(),
  renderer: { invoke: vi.fn(), on: vi.fn(), removeListener: vi.fn() },
}));
vi.mock('electron', () => ({
  ipcMain: {
    handle: (name: string, fn: (...args: unknown[]) => Promise<unknown>) =>
      ipc.handlers.set(name, fn),
  },
  ipcRenderer: ipc.renderer,
}));
const fixture = JSON.parse(
  readFileSync(
    new URL('../../../fixtures/contracts/terminal-opening.v1.json', import.meta.url),
    'utf8',
  ),
);
const start = fixture.start as TerminalOpeningStartV1;
const coreProgress = fixture.progress[0];
const coreRuntime = coreProgress.runtime_instance_id;
function core(request = coreProgress.request_id) {
  let resolve!: (value: unknown) => void;
  let reject!: (value: unknown) => void;
  const result = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  const fake = {
    snapshot: { runtime_instance_id: coreRuntime },
    requestAgent: vi.fn(
      (_method: string, _params: unknown, identify: (id: CorrelationId) => void) => {
        identify(request);
        return result;
      },
    ),
    cancel: vi.fn(async () => 'accepted'),
  };
  return {
    fake,
    service: fake as unknown as Parameters<TerminalOpenings['start']>[2],
    resolve,
    reject,
    progress: (phase = 'waiting_runtime', changes = {}) =>
      terminalOpeningNotification(
        fake,
        'terminal.opening.v1',
        { ...coreProgress, request_id: request, phase, ...changes },
        fake.snapshot.runtime_instance_id,
      ),
  };
}
afterEach(() => {
  vi.clearAllMocks();
  ipc.handlers.clear();
});

test('source-owned opening fixtures roundtrip every boundary and reject unknown fields/version/IDs', () => {
  expect(isTerminalOpeningStart(fixture.start)).toBe(true);
  expect(isTerminalOpeningRef(fixture.reference)).toBe(true);
  expect(fixture.progress.map((sample: { phase: string }) => sample.phase)).toEqual([
    'waiting_runtime',
    'waiting_launch',
    'launching',
    'cancelled',
  ]);
  expect(fixture.changes.map((sample: { phase: string }) => sample.phase)).toEqual([
    'waiting_runtime',
    'waiting_launch',
    'launching',
    'cancelled',
  ]);
  for (const sample of fixture.progress) expect(isTerminalOpeningProgress(sample)).toBe(true);
  for (const sample of fixture.changes) expect(isTerminalOpeningChange(sample)).toBe(true);
  for (const [validate, sample] of [
    [isTerminalOpeningStart, fixture.start],
    [isTerminalOpeningRef, fixture.reference],
    [isTerminalOpeningProgress, fixture.progress[0]],
    [isTerminalOpeningChange, fixture.changes[0]],
  ] as const) {
    expect(validate({ ...sample, contract_version: 2 })).toBe(false);
    expect(validate({ ...sample, payload: 'temporary-untrusted-value' })).toBe(false);
  }
  expect(isTerminalOpeningRef({ ...fixture.reference, request_id: coreProgress.request_id })).toBe(
    false,
  );
  expect(isTerminalOpeningStart({ ...fixture.start, opening_id: 'not-an-opening-id' })).toBe(false);
  expect(isTerminalOpeningProgress({ ...coreProgress, request_id: 'other' })).toBe(false);
  expect(isTerminalOpeningChange({ ...fixture.changes[0], runtime_instance_id: 'other' })).toBe(
    false,
  );
});

test('cancel is owner-bound, waits for real request admission, and never replays the open', async () => {
  const f = core();
  const published = vi.fn();
  const manager = new TerminalOpenings(published);
  const owner = {};
  const result = manager.start(owner, start, f.service);
  const cancellation = manager.cancel(owner, start.opening_id);
  expect(await manager.cancel({}, start.opening_id)).toBe('unknown_request');
  expect(f.fake.cancel).toHaveBeenCalledExactlyOnceWith(coreProgress.request_id);
  f.progress('waiting_runtime', { runtime_instance_id: 'run_01ARZ3NDEKTSV4RRFFQ69G5FAW' });
  f.progress('waiting_runtime', { session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAW' });
  expect(f.fake.cancel).toHaveBeenCalledTimes(1);
  f.progress();
  expect(await cancellation).toBe('accepted');
  expect(f.fake.cancel).toHaveBeenCalledWith(coreProgress.request_id);
  expect(await manager.cancel(owner, start.opening_id)).toBe('accepted');
  expect(f.fake.cancel).toHaveBeenCalledTimes(1);
  const cancelled = { envelope: { code: 'terminal.open_cancelled', terminal: 'cancelled' } };
  f.reject(cancelled);
  await expect(result).rejects.toBe(cancelled);
  expect(f.fake.requestAgent).toHaveBeenCalledTimes(1);
  expect(await manager.cancel(owner, start.opening_id)).toBe('unknown_request');
  expect(published.mock.calls[0]?.[1]).toEqual(fixture.changes[0]);
  f.progress('cancelled');
  expect(published).toHaveBeenCalledTimes(1);
});

test('concurrent opens keep exact IDs and launch phase makes cancellation too late', async () => {
  const f = core();
  const g = core('cor_01ARZ3NDEKTSV4RRFFQ69G5FAW');
  f.fake.requestAgent
    .mockImplementationOnce(f.fake.requestAgent.getMockImplementation()!)
    .mockImplementationOnce(g.fake.requestAgent);
  const manager = new TerminalOpenings(vi.fn());
  const owner = {};
  const first = manager.start(owner, start, f.service);
  const secondSource = { ...start, opening_id: '00000000-0000-4000-8000-000000000002' };
  const second = manager.start(owner, secondSource, f.service);
  f.progress('launching');
  f.progress('waiting_runtime', { request_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAW' });
  expect(await manager.cancel(owner, start.opening_id)).toBe('too_late_to_cancel');
  expect(await manager.cancel(owner, secondSource.opening_id)).toBe('accepted');
  expect(f.fake.cancel).toHaveBeenCalledExactlyOnceWith('cor_01ARZ3NDEKTSV4RRFFQ69G5FAW');
  expect(g.fake.cancel).not.toHaveBeenCalled();
  f.resolve({ terminal_id: 'first' });
  g.resolve({ terminal_id: 'second' });
  await Promise.all([first, second]);
});

test('Main IPC rejects arbitrary request IDs and publishes only to the opening owner', async () => {
  const f = core();
  const trust = vi.fn();
  registerTerminalOpeningIpc(() => f.service as never, trust);
  const owner = { isDestroyed: () => false, once: vi.fn(), send: vi.fn() };
  const other = { ...owner, send: vi.fn() };
  const begin = ipc.handlers.get(TERMINAL_OPENING_START_CHANNEL)!;
  const cancel = ipc.handlers.get(TERMINAL_OPENING_CANCEL_CHANNEL)!;
  expect(
    await cancel({ sender: owner }, { ...fixture.reference, request_id: coreProgress.request_id }),
  ).toMatchObject({ ok: false });
  const response = begin({ sender: owner }, start);
  expect(await cancel({ sender: other }, fixture.reference)).toEqual({
    ok: true,
    value: 'unknown_request',
  });
  f.progress();
  expect(owner.send).toHaveBeenCalledWith(TERMINAL_OPENING_EVENT_CHANNEL, fixture.changes[0]);
  expect(other.send).not.toHaveBeenCalled();
  expect(await cancel({ sender: owner }, fixture.reference)).toEqual({
    ok: true,
    value: 'accepted',
  });
  f.resolve({ terminal_id: 'temporary' });
  expect(await response).toMatchObject({ ok: true });
  expect(trust).toHaveBeenCalled();
});

test('closing owner and failed publication still cancel the exact request without replay', async () => {
  const f = core();
  const manager = new TerminalOpenings(() => {
    throw new Error('temporary closed renderer');
  });
  const owner = {};
  const pending = manager.start(owner, start, f.service);
  manager.closeOwner(owner);
  f.progress();
  await Promise.resolve();
  expect(f.fake.cancel).toHaveBeenCalledExactlyOnceWith(coreProgress.request_id);
  f.reject({ envelope: { code: 'terminal.open_cancelled', terminal: 'cancelled' } });
  await expect(pending).rejects.toMatchObject({ envelope: { code: 'terminal.open_cancelled' } });
  expect(f.fake.requestAgent).toHaveBeenCalledTimes(1);
});

test('Preload validates metadata and fixed channels, detaches listeners, and never retries', async () => {
  const error = (code: string, message: string) => ({ code, message }) as never;
  const bridge = terminalOpeningBridge(
    (value): value is never => !!value && typeof value === 'object' && 'code' in value,
    error,
  );
  ipc.renderer.invoke.mockResolvedValue({ ok: true, value: 'accepted' });
  expect(await bridge.cancelOpen(fixture.reference)).toEqual({ ok: true, value: 'accepted' });
  expect(ipc.renderer.invoke).toHaveBeenCalledWith(
    TERMINAL_OPENING_CANCEL_CHANNEL,
    fixture.reference,
  );
  expect(
    await bridge.cancelOpen({ ...fixture.reference, request_id: coreProgress.request_id } as never),
  ).toMatchObject({ ok: false });
  expect(ipc.renderer.invoke).toHaveBeenCalledTimes(1);
  const listener = vi.fn();
  const detach = bridge.onOpening(listener);
  const callback = ipc.renderer.on.mock.calls[0]?.[1];
  callback({}, { ...fixture.changes[0], secret: 'temporary-test-value' });
  callback({}, fixture.changes[0]);
  expect(listener).toHaveBeenCalledTimes(1);
  detach();
  expect(ipc.renderer.removeListener).toHaveBeenCalledWith(
    TERMINAL_OPENING_EVENT_CHANNEL,
    callback,
  );
  ipc.renderer.invoke.mockRejectedValueOnce(new Error('temporary transport error'));
  expect(await bridge.beginOpen(start)).toMatchObject({ ok: false });
  expect(ipc.renderer.invoke).toHaveBeenCalledTimes(2);
});
