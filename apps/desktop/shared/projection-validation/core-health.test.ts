import { expect, test } from 'vitest';
import { parseProjection } from '../projection-validation.js';

// The health projection may say what long work holds Core's
// stores; anything else in that place is refused, as any other unknown field is.

const change = (payload: Record<string, unknown>) => ({
  payload: {
    read_only: false,
    restart_attempt: 0,
    safe_mode: false,
    state: 'ready',
    status_code: 'core.ready',
    status_message: 'Aura Core 已就绪。',
    ...payload,
  },
  projection_kind: 'core_health',
  projection_scope: { type: 'global' },
  revision: '3',
  runtime_instance_id: 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV',
});

test('a health projection names the long work Core is busy with, or nothing', () => {
  expect(parseProjection(change({}))).toBeDefined();
  expect(
    parseProjection(
      change({ busy: { activity: 'deleting_session', since: '2026-10-19T08:00:00Z' } }),
    ),
  ).toBeDefined();
  for (const busy of [
    { activity: 'vacuuming', since: '2026-10-19T08:00:00Z' },
    { activity: 'deleting_session' },
    { activity: 'deleting_session', since: 'yesterday' },
    { activity: 'deleting_session', since: '2026-10-19T08:00:00Z', detail: 'x' },
    'deleting_session',
  ])
    expect(parseProjection(change({ busy })), JSON.stringify(busy)).toBeUndefined();
});
