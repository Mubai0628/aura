// @vitest-environment node
import { ipcMain, ipcRenderer } from 'electron';
import { beforeEach, expect, test, vi } from 'vitest';
import { registerMainErrorsIpc } from './main-errors-ipc.js';
import type { MainErrorReporter } from './main-errors.js';
import { mainErrorsBridge } from '../preload/main-errors.js';
import {
  MAIN_ERROR_EVENT_CHANNEL,
  MAIN_ERROR_SNAPSHOT_CHANNEL,
  isMainErrorNotice,
} from '../shared/main-errors-api.js';

vi.mock('electron', () => ({
  ipcMain: { handle: vi.fn() },
  ipcRenderer: { invoke: vi.fn(), on: vi.fn(), removeListener: vi.fn() },
}));
const notice = {
  version: 1,
  id: 'main_error_1',
  kind: 'uncaught_exception',
  logged: true,
  log_path: 'C:/temporary/main.jsonl',
};
beforeEach(() => vi.clearAllMocks());
test('Main diagnostic snapshot authenticates the current window and accepts no authority arguments', () => {
  const trust = vi.fn();
  const reporter = { snapshot: () => notice } as unknown as MainErrorReporter;
  registerMainErrorsIpc(reporter, trust);
  const [channel, receive] = vi.mocked(ipcMain.handle).mock.calls[0]!;
  expect(channel).toBe(MAIN_ERROR_SNAPSHOT_CHANNEL);
  expect(receive({} as never)).toEqual({ ok: true, value: notice });
  expect(receive({} as never, {})).toMatchObject({ ok: false });
  trust.mockImplementationOnce(() => {
    throw new Error('foreign frame');
  });
  expect(receive({} as never)).toMatchObject({ ok: false });
});
test('Preload exposes only diagnostics, validates snapshots and notifications, and releases its listener', async () => {
  const bridge = mainErrorsBridge();
  expect(Object.keys(bridge).sort()).toEqual(['onChange', 'snapshot']);
  vi.mocked(ipcRenderer.invoke)
    .mockResolvedValueOnce({ ok: true, value: null })
    .mockResolvedValueOnce({ ok: true, value: notice })
    .mockRejectedValueOnce(new Error('temporary disconnect'));
  expect(await bridge.snapshot()).toEqual({ ok: true, value: null });
  expect(await bridge.snapshot()).toEqual({ ok: true, value: notice });
  expect((await bridge.snapshot()).ok).toBe(false);
  expect(ipcRenderer.invoke).toHaveBeenCalledTimes(3);
  const listener = vi.fn();
  const unsubscribe = bridge.onChange(listener);
  const [channel, receive] = vi.mocked(ipcRenderer.on).mock.calls[0]!;
  expect(channel).toBe(MAIN_ERROR_EVENT_CHANNEL);
  receive({} as never, { ...notice, stack: 'forbidden' });
  receive({} as never, { ...notice, version: 2 });
  receive({} as never, notice);
  expect(listener).toHaveBeenCalledExactlyOnceWith(notice);
  unsubscribe();
  expect(ipcRenderer.removeListener).toHaveBeenCalledWith(channel, receive);
});
test('Main error notice is a bounded closed contract without arbitrary error payloads', () => {
  expect(isMainErrorNotice(notice)).toBe(true);
  for (const value of [
    { ...notice, kind: 'execute' },
    { ...notice, id: 'main_error_0' },
    { ...notice, logged: 'yes' },
    { ...notice, log_path: '\0' },
    { ...notice, log_path: 'x'.repeat(32769) },
    { ...notice, message: 'private failure' },
  ])
    expect(isMainErrorNotice(value)).toBe(false);
});
