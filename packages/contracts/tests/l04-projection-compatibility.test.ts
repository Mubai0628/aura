import { readFileSync } from 'node:fs';
import type { AnySchema } from 'ajv';
import { expect, test } from 'vitest';
import { createContractValidator } from '../src/validation.js';
import type { ProjectionCompatibilityKey } from '../src/generated/contracts.js';

const catalog = JSON.parse(
  readFileSync(
    new URL('../../../resources/schemas/aura-contracts.schema.json', import.meta.url),
    'utf8',
  ),
) as { $defs: Record<string, AnySchema> };
const validate = createContractValidator<ProjectionCompatibilityKey>({
  $defs: catalog.$defs,
  $ref: '#/$defs/ProjectionCompatibilityKey',
});
const runtime = 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const session = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';

test('generated compatibility schema accepts existing and all four auxiliary identities', () => {
  for (const [kind, scope] of [
    ['core_health', { type: 'global' }],
    ['session_permission', { type: 'session', session_id: session }],
    ['pending_approval', { type: 'global' }],
    ['security_runtime', { type: 'global' }],
    ['terminal_state', { type: 'terminal', session_id: session, terminal_id: 'term_fixture' }],
  ])
    expect(
      validate({ runtime_instance_id: runtime, projection_kind: kind, projection_scope: scope }),
      JSON.stringify(validate.errors),
    ).toBe(true);
});

test('generated compatibility schema rejects wider or malformed auxiliary scope and payload bytes', () => {
  const key = {
    runtime_instance_id: runtime,
    projection_kind: 'terminal_state',
    projection_scope: { type: 'terminal', session_id: session, terminal_id: 'term_fixture' },
  };
  for (const invalid of [
    { ...key, projection_scope: { type: 'global' } },
    { ...key, projection_kind: 'session_permission' },
    { ...key, projection_scope: { ...key.projection_scope, terminal_id: 'term_' } },
    { ...key, projection_scope: { ...key.projection_scope, terminal_id: '../private' } },
    // ID schemas are existing opaque string aliases; the receiving bridge additionally
    // checks canonical IDs (shared/auxiliary-projection-validation.test.ts).
    { ...key, projection_scope: { ...key.projection_scope, session_id: 1 } },
    { ...key, payload: { private: 'must not enter compatibility metadata' } },
  ])
    expect(validate(invalid), JSON.stringify(invalid)).toBe(false);
});
