// @vitest-environment node
import { beforeEach, expect, test, vi } from 'vitest';
import { ipcMain, ipcRenderer } from 'electron';
import {
  registerProjectionCompatibilityIpc,
  publishProjectionCompatibility,
} from './projection-compatibility-ipc.js';
import { projectionCompatibilityBridge } from '../preload/projection-compatibility.js';
import { errorEnvelope } from './core-transport.js';
import {
  PROJECTION_COMPATIBILITY_EVENT_CHANNEL,
  PROJECTION_COMPATIBILITY_SNAPSHOT_CHANNEL,
} from '../shared/projection-compatibility-api.js';

vi.mock('electron', () => ({
  ipcMain: { handle: vi.fn() },
  ipcRenderer: { invoke: vi.fn(), on: vi.fn(), removeListener: vi.fn() },
}));
const snapshot = {
  contract_version: 1,
  revision: '1',
  runtime_instance_id: 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  paused_projections: [],
};
beforeEach(() => vi.clearAllMocks());

test('Main authenticates the fixed metadata channel and rejects request arguments', () => {
  const trust = vi.fn();
  registerProjectionCompatibilityIpc(
    () => ({ projectionCompatibilitySnapshot: snapshot }) as never,
    trust,
  );
  const [, handler] = vi.mocked(ipcMain.handle).mock.calls[0]!;
  expect(handler({} as never)).toEqual({ ok: true, value: snapshot });
  expect(trust).toHaveBeenCalledTimes(1);
  expect(handler({} as never, { route: 'renderer-cannot-select' })).toMatchObject({ ok: false });
  trust.mockImplementationOnce(() => {
    throw new Error('untrusted');
  });
  expect(handler({} as never)).toMatchObject({ ok: false });
  expect(vi.mocked(ipcMain.handle).mock.calls[0]?.[0]).toBe(
    PROJECTION_COMPATIBILITY_SNAPSHOT_CHANNEL,
  );
});

test('Preload returns only validated metadata and removes its listener', async () => {
  const bridge = projectionCompatibilityBridge(
    (value): value is never => Boolean(value),
    (code, message) => errorEnvelope(code, message, 'failed'),
  );
  vi.mocked(ipcRenderer.invoke).mockResolvedValueOnce({ ok: true, value: snapshot });
  await expect(bridge.snapshot()).resolves.toEqual({ ok: true, value: snapshot });
  const listener = vi.fn();
  const stop = bridge.onChange(listener);
  const [, receive] = vi.mocked(ipcRenderer.on).mock.calls[0]!;
  receive({} as never, { ...snapshot, payload: 'FORBIDDEN' });
  receive({} as never, snapshot);
  expect(listener).toHaveBeenCalledTimes(1);
  stop();
  expect(ipcRenderer.removeListener).toHaveBeenCalledWith(
    PROJECTION_COMPATIBILITY_EVENT_CHANNEL,
    receive,
  );
  vi.mocked(ipcRenderer.invoke).mockResolvedValueOnce({
    ok: true,
    value: { ...snapshot, contract_version: 2 },
  });
  await expect(bridge.snapshot()).resolves.toMatchObject({ ok: false });
});

test('metadata publication uses the owned window and ignores its destroyed contents', () => {
  let receive: (value: unknown) => void = () => undefined;
  const stop = vi.fn();
  const supervisor = {
    subscribeProjectionCompatibility: (listener: typeof receive) => {
      receive = listener;
      return stop;
    },
  };
  const send = vi.fn();
  let destroyed = false;
  const unsubscribe = publishProjectionCompatibility(
    supervisor as never,
    () => ({ isDestroyed: () => destroyed, send }) as never,
  );
  receive(snapshot);
  destroyed = true;
  receive(snapshot);
  expect(send).toHaveBeenCalledTimes(1);
  expect(send).toHaveBeenCalledWith(PROJECTION_COMPATIBILITY_EVENT_CHANNEL, snapshot);
  unsubscribe();
  expect(stop).toHaveBeenCalledOnce();
});
