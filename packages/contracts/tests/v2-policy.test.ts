import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import type { AnySchema } from 'ajv';
import { expect, test } from 'vitest';
import type { PolicyAuthorization, V2KernelFixture } from '../src/generated/contracts.js';
import { createContractValidator } from '../src/validation.js';

test('V2 policy decisions and requirements use Rust-owned closed runtime contracts', async () => {
  const root = resolve(import.meta.dirname, '../../..');
  const catalog = JSON.parse(
    await readFile(resolve(root, 'resources/schemas/aura-contracts.schema.json'), 'utf8'),
  ) as { $defs: Record<string, AnySchema> };
  const fixture = JSON.parse(
    await readFile(resolve(root, 'fixtures/contracts/v2-kernel.json'), 'utf8'),
  ) as V2KernelFixture;
  const validate = createContractValidator<PolicyAuthorization>({
    $defs: catalog.$defs,
    $ref: '#/$defs/PolicyAuthorization',
  });
  const allow: PolicyAuthorization = { decision: 'allow', authorization: fixture.authority };
  expect(validate(allow), JSON.stringify(validate.errors)).toBe(true);
  const ask: PolicyAuthorization = {
    decision: 'ask',
    proposal: {
      scope: fixture.grant.scope,
      allowed_decisions: ['allow_once', 'deny'],
      reasons: ['backend.guarantees_changed'],
    },
  };
  expect(validate(ask), JSON.stringify(validate.errors)).toBe(true);
  expect(validate({ decision: 'deny', reason: 'hard_boundary' })).toBe(true);
  const invalid = structuredClone(ask);
  Object.assign(invalid.proposal.scope.backend_requirement, { isolation_score: 100 });
  expect(validate(invalid)).toBe(false);
  const native = structuredClone(ask);
  Object.assign(native.proposal, { automatic_native_fallback: true });
  expect(validate(native)).toBe(false);
  const kernel = createContractValidator<V2KernelFixture>({
    $defs: catalog.$defs,
    $ref: '#/$defs/V2KernelFixture',
  });
  const badNetwork = structuredClone(fixture);
  Object.assign(badNetwork.dispatch.operation.content, { network: 'exact_origin_cli' });
  expect(kernel(badNetwork)).toBe(false);
  const secret = structuredClone(fixture);
  Object.assign(secret.dispatch.operation.content.credentials[0] ?? {}, {
    password: 'credential-canary',
  });
  expect(kernel(secret)).toBe(false);
});
