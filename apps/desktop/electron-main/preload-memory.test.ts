// @vitest-environment node
import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import { expect, test, vi } from 'vitest';
import { memoryBridge } from '../preload/memory.js';
const error = (code: string, message: string) =>
  ({ code, message, retryable: false }) as IpcErrorEnvelope;
const isError = (value: unknown): value is IpcErrorEnvelope =>
  typeof value === 'object' && value !== null && 'code' in value && 'message' in value;
test('Memory bridge refuses arbitrary roots and Core-only export commands and never replays', async () => {
  const send = vi
    .fn()
    .mockResolvedValueOnce({ ok: true, value: { vaults: [], recent_runs: [] } })
    .mockRejectedValueOnce(new Error('local disconnect'))
    .mockResolvedValue({ ok: true, value: { vaults: [{}], recent_runs: [] } });
  const bridge = memoryBridge(send, error, isError);
  expect(Object.keys(bridge)).toEqual(['invoke']);
  expect((await bridge.invoke('memory.vaults.get', { workspace_id: null })).ok).toBe(true);
  expect(await bridge.invoke('memory.vaults.get', { root: 'C:/outside' } as never)).toMatchObject({
    ok: false,
    error: { code: 'memory.invalid_request' },
  });
  expect(await bridge.invoke('memory.export.chunk' as never, {} as never)).toMatchObject({
    ok: false,
    error: { code: 'memory.invalid_request' },
  });
  expect(send).toHaveBeenCalledTimes(1);
  expect(await bridge.invoke('memory.settings.get', {})).toMatchObject({
    ok: false,
    error: { code: 'memory.transport_failed' },
  });
  expect(send).toHaveBeenCalledTimes(2);
  expect(await bridge.invoke('memory.vaults.get', { workspace_id: null })).toMatchObject({
    ok: false,
    error: { code: 'memory.invalid_response' },
  });
});
