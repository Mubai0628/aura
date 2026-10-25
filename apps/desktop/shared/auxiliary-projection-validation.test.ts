import { expect, test } from 'vitest';
import {
  auxiliaryNotificationKey,
  isAuxiliaryProjectionKey,
  isAuxiliaryRevision,
} from './auxiliary-projection-validation.js';
import { isProjectionCompatibilitySnapshot } from './projection-compatibility-validation.js';

const runtime = 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const session = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const key = {
  runtime_instance_id: runtime,
  projection_kind: 'terminal_state',
  projection_scope: { type: 'terminal', session_id: session, terminal_id: 'term_fixture' },
};

test('only bounded auxiliary identities can pause a projection, never arbitrary payload bytes', () => {
  expect(isAuxiliaryProjectionKey(key)).toBe(true);
  for (const invalid of [
    { ...key, projection_kind: 'session_permission' },
    { ...key, projection_scope: { type: 'global' } },
    { ...key, projection_scope: { ...key.projection_scope, terminal_id: '../private' } },
    {
      ...key,
      projection_scope: { ...key.projection_scope, terminal_id: 'term_' + 'x'.repeat(129) },
    },
    { ...key, projection_scope: { ...key.projection_scope, session_id: 'foreign' } },
    { ...key, payload: { secret: 'must not cross compatibility metadata' } },
  ])
    expect(isAuxiliaryProjectionKey(invalid)).toBe(false);
  expect(
    auxiliaryNotificationKey('terminal.state.v1', {
      runtime_instance_id: runtime,
      view: { ...key.projection_scope, controller: 'invalid', secret: 'never copy' },
    }),
  ).toEqual(key);
  expect(
    auxiliaryNotificationKey('security.projection.change', {
      runtime_instance_id: runtime,
      projection_kind: 'session_permission',
      payload: null,
    }),
  ).toBeUndefined();
});

test('auxiliary compatibility snapshots reject duplicates and foreign generations', () => {
  const snapshot = {
    contract_version: 1,
    runtime_instance_id: runtime,
    revision: '1',
    paused_projections: [key],
  };
  expect(isProjectionCompatibilitySnapshot(snapshot)).toBe(true);
  expect(isProjectionCompatibilitySnapshot({ ...snapshot, paused_projections: [key, key] })).toBe(
    false,
  );
  expect(
    isProjectionCompatibilitySnapshot({
      ...snapshot,
      runtime_instance_id: 'run_01ARZ3NDEKTSV4RRFFQ69G5FAW',
    }),
  ).toBe(false);
  for (const valid of ['0', '18446744073709551615']) expect(isAuxiliaryRevision(valid)).toBe(true);
  for (const invalid of ['01', '-1', '18446744073709551616', '9'.repeat(1000), 1])
    expect(isAuxiliaryRevision(invalid)).toBe(false);
});
