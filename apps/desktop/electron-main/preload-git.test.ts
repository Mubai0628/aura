// @vitest-environment node
import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import { expect, test, vi } from 'vitest';
import { gitBridge } from '../preload/git.js';
const error = (code: string, message: string) =>
  ({ code, message, retryable: false }) as IpcErrorEnvelope;
const isError = (value: unknown): value is IpcErrorEnvelope =>
  typeof value === 'object' && value !== null && 'code' in value && 'message' in value;
test('Git preload exposes a fixed whitelist, validates responses, and never replays a transport failure', async () => {
  const send = vi
    .fn()
    .mockResolvedValueOnce({ ok: true, value: { opened: true } })
    .mockRejectedValueOnce(new Error('test disconnect'))
    .mockResolvedValue({ ok: true, value: { opened: 'bad' } });
  const bridge = gitBridge(send, error, isError);
  expect(Object.keys(bridge).sort()).toEqual(['openInstallation', 'read', 'runtime']);
  expect(await bridge.openInstallation()).toEqual({ ok: true, value: { opened: true } });
  const failed = await bridge.runtime();
  expect(failed.ok).toBe(false);
  expect(send).toHaveBeenCalledTimes(2);
  expect(await bridge.openInstallation()).toMatchObject({
    ok: false,
    error: { code: 'git.invalid_response' },
  });
  expect(send.mock.calls.map((call) => call[0])).toEqual([
    'git.installation.open',
    'git.runtime.get',
    'git.installation.open',
  ]);
});
