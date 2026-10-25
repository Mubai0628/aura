// @vitest-environment node
import { readFileSync } from 'node:fs';
import { expect, test, vi } from 'vitest';
import { ProjectionCompatibility } from './projection-compatibility.js';
import { isProjectionCompatibilitySnapshot } from '../shared/projection-compatibility-validation.js';

const projections: unknown[] = JSON.parse(
  readFileSync(
    new URL('../../../fixtures/contracts/projections-full.v1.json', import.meta.url),
    'utf8',
  ),
).projections;
const runtime = 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV';

test('all 29 unified projection kinds pause independently and the next valid matching route repairs them', () => {
  const log = vi.fn();
  const guard = new ProjectionCompatibility(log);
  guard.reset(runtime);
  expect(
    new Set(projections.map((value) => (value as Record<string, unknown>).projection_kind)).size,
  ).toBe(29);
  for (const sample of projections) {
    const value = sample as Record<string, unknown>;
    expect(
      guard.read({ ...value, payload: { private_canary: 'INVALID-NEVER-RECORD' } }, runtime),
    ).toBeUndefined();
  }
  expect(guard.snapshot().paused_projections).toHaveLength(29);
  expect(isProjectionCompatibilitySnapshot(guard.snapshot())).toBe(true);
  for (const sample of projections) expect(guard.read(sample, runtime)).toBeDefined();
  expect(guard.snapshot().paused_projections).toEqual([]);
  expect(JSON.stringify(log.mock.calls)).not.toContain('INVALID-NEVER-RECORD');
});

test('reordered scope fields identify the same route and a new runtime clears compatibility state', () => {
  const guard = new ProjectionCompatibility(vi.fn());
  guard.reset(runtime);
  const value = {
    runtime_instance_id: runtime,
    revision: '1',
    projection_kind: 'context_window_correction',
    projection_scope: { type: 'session', session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV' },
    payload: null,
  };
  guard.read(value, runtime);
  guard.read(
    {
      ...value,
      projection_scope: { session_id: value.projection_scope.session_id, type: 'session' },
      payload: { session_id: value.projection_scope.session_id, active_corrections: [] },
    },
    runtime,
  );
  expect(guard.snapshot().paused_projections).toEqual([]);
  guard.read(value, runtime);
  guard.reset('run_01ARZ3NDEKTSV4RRFFQ69G5FAW');
  expect(guard.snapshot().paused_projections).toEqual([]);
});

test('an unnegotiated projection and a failed UI metadata listener preserve the connection owner', () => {
  const log = vi.fn();
  const guard = new ProjectionCompatibility(log);
  guard.reset(runtime);
  guard.subscribe(() => {
    throw new Error('observer-private-canary');
  });
  expect(() => guard.read(projections[0], runtime, undefined, () => false)).not.toThrow();
  expect(guard.snapshot().paused_projections).toHaveLength(1);
  expect(guard.read(projections[0], runtime)).toBeDefined();
  expect(guard.snapshot().paused_projections).toEqual([]);
  expect(JSON.stringify(log.mock.calls)).not.toContain('observer-private-canary');
});
