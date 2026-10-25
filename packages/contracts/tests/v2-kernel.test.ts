import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import type { AnySchema } from 'ajv';
import { expect, test } from 'vitest';
import type { V2KernelFixture } from '../src/generated/contracts.js';
import { createContractValidator } from '../src/validation.js';

test('Rust-generated V2 kernel facts validate in TypeScript and reject malformed authority', async () => {
  const root = resolve(import.meta.dirname, '../../..');
  const catalog = JSON.parse(
    await readFile(resolve(root, 'resources/schemas/aura-contracts.schema.json'), 'utf8'),
  ) as { $defs: Record<string, AnySchema> };
  const fixture = JSON.parse(
    await readFile(resolve(root, 'fixtures/contracts/v2-kernel.json'), 'utf8'),
  ) as V2KernelFixture;
  const validate = createContractValidator<V2KernelFixture>({
    $defs: catalog.$defs,
    $ref: '#/$defs/V2KernelFixture',
  });
  expect(validate(fixture), JSON.stringify(validate.errors)).toBe(true);
  const invalid = structuredClone(fixture);
  Object.assign(invalid.dispatch.operation.content.credentials[0] ?? {}, {
    secret: 'credential-canary',
  });
  expect(validate(invalid)).toBe(false);
  const malformed = structuredClone(fixture);
  Object.assign(malformed.dispatch.binding.attempt, { ordinal: 0 });
  expect(validate(malformed)).toBe(false);
  const profile = structuredClone(fixture);
  Object.assign(profile.authority.scope, { profile: 'automatic_native_fallback' });
  expect(validate(profile)).toBe(false);
});
