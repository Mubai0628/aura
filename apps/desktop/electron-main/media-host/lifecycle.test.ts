// @vitest-environment node
import { afterEach, expect, test, vi } from 'vitest';
import { mediaCommandNotification } from './core-channel.js';
import { MediaHost, type DecodeMedia } from './index.js';
import type { MediaCommand, MediaCore, MediaProbe } from './types.js';

const command = (id = 'mcm-lifecycle'): MediaCommand => ({
  command_id: id,
  runtime_instance_id: 'runtime',
  session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  file_size: 3,
  media_type: 'video/webm',
  timeout_ms: 30_000,
  action: { kind: 'probe' },
});
const result: MediaProbe = {
  kind: 'probe',
  duration: 3,
  width: 2,
  height: 2,
  byte_length: 3,
  frame_rate: null,
  has_audio: null,
  container: null,
  codec: null,
};
const core = () => ({ requestWithBudget: vi.fn<MediaCore['requestWithBudget']>(async () => ({})) });
const notify = (owner: MediaCore, method: string, value: unknown) =>
  mediaCommandNotification(owner, `media.${method}.v1`, value, 'runtime');
const identity = () => {
  const { command_id, runtime_instance_id, session_id } = command();
  return { command_id, runtime_instance_id, session_id };
};
afterEach(() => vi.useRealTimers());

test('cancellation stops active bytes and ignores a late decoder answer', async () => {
  const owner = core();
  let finish!: (value: MediaProbe) => void;
  const decode = vi.fn<DecodeMedia>(
    () =>
      new Promise((done) => {
        finish = done;
      }),
  );
  const host = new MediaHost({ decode });
  host.bindCore(owner);
  notify(owner, 'command', command());
  await vi.waitFor(() => expect(decode).toHaveBeenCalledTimes(1));
  const [, source, signal] = decode.mock.calls[0]!;
  notify(owner, 'cancel', identity());
  expect(signal.aborted).toBe(true);
  expect((await source.serve(new Request(source.url))).status).toBe(403);
  finish(result);
  await Promise.resolve();
  await Promise.resolve();
  expect(owner.requestWithBudget).not.toHaveBeenCalled();
  host.dispose();
});

test('a stale Core connection and a duplicate command cannot start another decoder', async () => {
  const first = core(),
    current = core();
  const decode = vi.fn<DecodeMedia>(async () => result);
  const host = new MediaHost({ decode });
  host.bindCore(first);
  host.bindCore(current);
  notify(first, 'command', command());
  notify(current, 'command', command());
  notify(current, 'command', command());
  await vi.waitFor(() => expect(current.requestWithBudget).toHaveBeenCalledTimes(1));
  expect(first.requestWithBudget).not.toHaveBeenCalled();
  expect(decode).toHaveBeenCalledTimes(1);
  host.dispose();
});

test('timeout aborts the decoder, revokes bytes and delivers a failed result', async () => {
  vi.useFakeTimers();
  const owner = core();
  const decode = vi.fn<DecodeMedia>(
    async (_command, _source, signal) =>
      new Promise((_done, reject) =>
        signal.addEventListener('abort', () => reject(signal.reason), { once: true }),
      ),
  );
  const host = new MediaHost({ decode });
  host.bindCore(owner);
  notify(owner, 'command', { ...command(), timeout_ms: 50 });
  await vi.advanceTimersByTimeAsync(51);
  const [, source, signal] = decode.mock.calls[0]!;
  expect(signal.aborted).toBe(true);
  expect((await source.serve(new Request(source.url))).status).toBe(403);
  expect(owner.requestWithBudget.mock.calls[0]![1]).toMatchObject({
    outcome: { outcome: 'failed', message: expect.stringContaining('超时') },
  });
  host.dispose();
});

test('same-supervisor transport loss stops old bytes before the next runtime can decode', async () => {
  type Health = Parameters<NonNullable<MediaCore['subscribe']>>[0] extends (
    change: infer Change,
  ) => void
    ? Change
    : never;
  let publish!: (change: Health) => void;
  const owner = {
    ...core(),
    subscribe: vi.fn((listener: (change: Health) => void) => {
      publish = listener;
      return vi.fn();
    }),
  };
  const decode = vi.fn<DecodeMedia>(
    async (_command, _source, signal) =>
      new Promise((_done, reject) =>
        signal.addEventListener('abort', () => reject(signal.reason), { once: true }),
      ),
  );
  const host = new MediaHost({ decode });
  host.bindCore(owner);
  const health = (state: string, runtime = 'runtime') =>
    publish({ runtime_instance_id: runtime, payload: { state } } as Health);
  health('ready');
  notify(owner, 'command', command());
  await vi.waitFor(() => expect(decode).toHaveBeenCalledTimes(1));
  const [, source, signal] = decode.mock.calls[0]!;
  health('reconnecting');
  expect(signal.aborted).toBe(true);
  expect((await source.serve(new Request(source.url))).status).toBe(403);
  notify(owner, 'command', { ...command(), command_id: 'mcm-disconnected' });
  health('ready', 'runtime-next');
  notify(owner, 'command', { ...command(), command_id: 'mcm-stale' });
  decode.mockResolvedValueOnce(result);
  mediaCommandNotification(
    owner,
    'media.command.v1',
    { ...command('mcm-next'), runtime_instance_id: 'runtime-next' },
    'runtime-next',
  );
  await vi.waitFor(() => expect(owner.requestWithBudget).toHaveBeenCalledTimes(1));
  expect(decode).toHaveBeenCalledTimes(2);
  expect(owner.requestWithBudget.mock.calls[0]![1]).toMatchObject({
    runtime_instance_id: 'runtime-next',
    command_id: 'mcm-next',
    outcome: { outcome: 'completed' },
  });
  host.dispose();
  expect(owner.subscribe.mock.results[0]!.value).toHaveBeenCalledTimes(1);
});
