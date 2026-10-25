import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import type { AnySchema } from 'ajv';
import { expect, test } from 'vitest';
import type { ResourceDescriptor, SecuritySourceRef } from '../src/generated/contracts.js';
import { createContractValidator } from '../src/validation.js';

async function validator<T>(name: string) {
  const catalog = JSON.parse(
    await readFile(
      resolve(import.meta.dirname, '../../..', 'resources/schemas/aura-contracts.schema.json'),
      'utf8',
    ),
  ) as { $defs: Record<string, AnySchema> };
  return createContractValidator<T>({ $defs: catalog.$defs, $ref: `#/$defs/${name}` });
}

const digest = {
  algorithm: 'sha256',
  canonicalization_version: 'rfc8785_jcs_v1',
  hex: 'a'.repeat(64),
} as const;

test('current Native consent scopes reject extra and predecessor-only fields', async () => {
  const validate = await validator('NativeSessionConsentScope');
  const scope = {
    principal_id: 'prn_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    approved_shell: 'cmd',
    shells: ['cmd', 'powershell'],
  };
  expect(validate(scope), JSON.stringify(validate.errors)).toBe(true);
  expect(validate({ ...scope, extra: true })).toBe(false);
  expect(validate({ ...scope, shell: 'cmd' })).toBe(false);
  expect(validate({ principal_id: scope.principal_id, shell: 'cmd' })).toBe(false);
  expect(validate({ principal_id: scope.principal_id, shells: ['cmd'] })).toBe(false);
});

test('source references validate exact identity hashes and reject embedded Tool authority', async () => {
  const validate = await validator<SecuritySourceRef>('SecuritySourceRef');
  const source: SecuritySourceRef = { source_kind: 'tool', identity_hash: digest };
  expect(validate(source), JSON.stringify(validate.errors)).toBe(true);
  expect(validate({ ...source, tool: {} })).toBe(false);
  expect(validate({ source_kind: 'tool', identity_hash: { ...digest, algorithm: 'blake3' } })).toBe(
    false,
  );
  expect(validate({ source_kind: 'tool', identity_hash: { ...digest, hex: 'short' } })).toBe(false);
  expect(validate({ source_kind: 'tool' })).toBe(false);
  expect(validate({ source_kind: 'unrecognized', identity_hash: digest })).toBe(false);
});

test('opaque resources reject invented access and normal scopes retain field types', async () => {
  const validate = await validator<ResourceDescriptor>('ResourceDescriptor');
  const runtime: ResourceDescriptor = {
    scope_kind: 'package_runtime',
    resource_fingerprint: digest,
  };
  expect(validate(runtime), JSON.stringify(validate.errors)).toBe(true);
  expect(validate({ ...runtime, root: 'C:\\' })).toBe(false);
  expect(validate({ ...runtime, resource_fingerprint: { ...digest, hex: 'not-a-hash' } })).toBe(
    false,
  );
  expect(
    validate({
      scope_kind: 'workspace_path',
      workspace_id: 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      canonical_relative_path: 42,
    }),
  ).toBe(false);
});
