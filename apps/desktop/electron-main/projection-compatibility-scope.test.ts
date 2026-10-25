// @vitest-environment node
import { readFileSync } from 'node:fs';
import { expect, test, vi } from 'vitest';
import { ProjectionCompatibility } from './projection-compatibility.js';
import { parseProjection } from '../shared/projection-validation.js';

const projections: Record<string, unknown>[] = JSON.parse(
  readFileSync(
    new URL('../../../fixtures/contracts/projections-full.v1.json', import.meta.url),
    'utf8',
  ),
).projections;
const runtime = 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV';

test.each([
  ['endpoint_detail', 'endpoint_id'],
  ['model_profile', 'model_profile_id'],
  ['session_model_selector', 'session_id'],
  ['model_availability_test', 'model_availability_test_id'],
  ['model_discovery', 'model_discovery_run_id'],
  ['context_window_correction', 'session_id'],
] as const)(
  '%s with another scope identity cannot restore or publish that route',
  (kind, field) => {
    const sample = projections.find((value) => value.projection_kind === kind)!;
    const scope = sample.projection_scope as Record<string, unknown>;
    const identifier = scope[field] as string;
    const wrong = {
      ...sample,
      projection_scope: { ...scope, [field]: `${identifier.slice(0, -1)}W` },
    };
    const log = vi.fn();
    const guard = new ProjectionCompatibility(log);
    guard.reset(runtime);
    expect(guard.read({ ...wrong, payload: null }, runtime)).toBeUndefined();
    const before = guard.snapshot();
    expect(before.paused_projections).toHaveLength(1);
    expect(guard.read(wrong, runtime)).toBeUndefined();
    expect(guard.snapshot(), 'another payload ID must not clear the paused route').toEqual(before);
    expect(parseProjection(wrong)).toBeUndefined();
    expect(guard.read(sample, runtime)).toBeDefined();
    expect(guard.snapshot(), 'a different compatible route cannot repair this one').toEqual(before);
  },
);
