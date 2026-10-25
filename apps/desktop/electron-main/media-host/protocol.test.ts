// @vitest-environment node
import { expect, test } from 'vitest';
import { mediaIdentity, readMediaCommand } from './protocol.js';
import type { MediaCommand } from './types.js';

export const sample = (): MediaCommand => ({
  command_id: 'mcm-fixture-command',
  runtime_instance_id: 'runtime-fixture',
  session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  file_size: 1234,
  media_type: 'video/webm',
  timeout_ms: 30_000,
  action: { kind: 'frames', times: [0.2, 1.2], count: null, sheet: false, max_side: 1024 },
});

test('accepts precisely the frozen Core-only command and its three identities', () => {
  expect(readMediaCommand(sample(), 'runtime-fixture')).toEqual(sample());
  expect(
    readMediaCommand({ ...sample(), action: { kind: 'probe' } }, 'runtime-fixture'),
  ).toBeDefined();
  expect(mediaIdentity(sample(), 'runtime-fixture')).toEqual({
    command_id: sample().command_id,
    runtime_instance_id: sample().runtime_instance_id,
    session_id: sample().session_id,
  });
});

test.each([
  { path: 'C:/unapproved/video.webm' },
  { raw_path: 'C:/unapproved/video.webm' },
  { runtime_instance_id: 'other-runtime' },
  { session_id: 'other-session' },
  { file_size: 0 },
  { file_size: Number.MAX_SAFE_INTEGER + 1 },
  { timeout_ms: 30_001 },
  { media_type: 'text/html' },
  { action: { kind: 'probe', path: 'secret' } },
  { action: { ...sample().action, times: [], count: null } },
  { action: { ...sample().action, times: [0, 1], count: 2 } },
  { action: { ...sample().action, times: [Number.NaN], count: null } },
  { action: { ...sample().action, times: [-1], count: null } },
  { action: { ...sample().action, times: null, count: 13 } },
  { action: { ...sample().action, max_side: 2049 } },
])('explicitly rejects a forged, stale or unbounded command %j', (mutation) => {
  expect(readMediaCommand({ ...sample(), ...mutation }, 'runtime-fixture')).toBeUndefined();
});
