// @vitest-environment node
import { expect, test, vi } from 'vitest';
import { mediaCommandNotification } from './core-channel.js';
import { MediaHost } from './index.js';
import { checkedMediaResult } from './results.js';
import type { MediaCommand, MediaProbe } from './types.js';

const command = (): MediaCommand => ({
  command_id: 'mcm-test',
  runtime_instance_id: 'runtime-test',
  session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  file_size: 4000,
  media_type: 'video/webm',
  timeout_ms: 30000,
  action: { kind: 'probe' },
});
const probe = (): MediaProbe => ({
  kind: 'probe',
  duration: 3,
  width: 320,
  height: 180,
  frame_rate: null,
  has_audio: false,
  container: 'webm',
  codec: null,
  byte_length: 4000,
});
const core = () => ({
  requestWithBudget: vi.fn<
    (method: string, params: unknown, milliseconds: number) => Promise<unknown>
  >(async () => ({ accepted: true })),
});

test('delivers a validated completed result on the authenticated Core-only route', async () => {
  const owner = core();
  const decode = vi.fn(async () => probe());
  const host = new MediaHost({ decode });
  host.bindCore(owner);
  expect(mediaCommandNotification(owner, 'media.command.v1', command(), 'runtime-test')).toBe(true);
  await vi.waitFor(() =>
    expect(owner.requestWithBudget).toHaveBeenCalledWith(
      'media.host.result',
      {
        command_id: command().command_id,
        runtime_instance_id: command().runtime_instance_id,
        session_id: command().session_id,
        outcome: { outcome: 'completed', result: probe() },
      },
      15000,
    ),
  );
  expect(decode).toHaveBeenCalledTimes(1);
});

test('a forged raw path is explicitly refused and never reaches bytes or decoder', async () => {
  const owner = core();
  const decode = vi.fn(async () => probe());
  const host = new MediaHost({ decode });
  host.bindCore(owner);
  mediaCommandNotification(
    owner,
    'media.command.v1',
    { ...command(), path: 'C:/secret.webm' },
    'runtime-test',
  );
  await vi.waitFor(() => expect(owner.requestWithBudget).toHaveBeenCalled());
  const reply = owner.requestWithBudget.mock.calls[0]![1] as {
    outcome: { outcome: string; message: string };
  };
  expect(reply.outcome.outcome).toBe('refused');
  expect(reply.outcome.message).toContain('拒绝');
  expect(decode).not.toHaveBeenCalled();
  expect(owner.requestWithBudget.mock.calls.every(([method]) => method !== 'media.host.read')).toBe(
    true,
  );
});

test('result validation checks finite metadata, matching byte size and exact fields', () => {
  expect(checkedMediaResult(probe(), command())).toEqual(probe());
  for (const mutation of [
    { duration: Infinity },
    { byte_length: 3999 },
    { path: 'C:/secret.webm' },
    { has_audio: 'unknown' },
    { width: 0 },
  ])
    expect(() => checkedMediaResult({ ...probe(), ...mutation }, command())).toThrow();
});
