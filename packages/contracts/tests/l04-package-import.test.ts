import { readFileSync } from 'node:fs';
import type { AnySchema } from 'ajv';
import { expect, test } from 'vitest';
import { createContractValidator } from '../src/validation.js';
import type { AuraContractCatalog, PackageImportPreviewV1 } from '../src/generated/contracts.js';

const catalog = JSON.parse(
  readFileSync(
    new URL('../../../resources/schemas/aura-contracts.schema.json', import.meta.url),
    'utf8',
  ),
) as { $defs: Record<string, AnySchema> };
const fixture = JSON.parse(
  readFileSync(
    new URL('../../../fixtures/contracts/canonical-contracts.json', import.meta.url),
    'utf8',
  ),
) as AuraContractCatalog;
const manifest = fixture.tools.canonical_package_manifest!;
const runtime = manifest.tools[0]!.runtime;
if (runtime.type !== 'aura_stdio') throw Error('Rust fixture must remain the staged stdio package');
const preview: PackageImportPreviewV1 = {
  contract_version: 1,
  version: {
    package_id: fixture.ids.tool_package_id,
    package_version: manifest.package_version,
    package_fingerprint: fixture.tools.package_fingerprint!,
    revision: '1',
  },
  content: {
    manifest,
    package_fingerprint: fixture.tools.package_fingerprint!,
    security_fingerprint: fixture.tools.package_security_fingerprint!,
    runtime_closure_hash: fixture.tools.package_runtime_closure_hash!,
  },
  runtimes: [
    {
      tool_key: manifest.tools[0]!.tool_key,
      execution_location: 'user_job',
      working_directory: {
        type: 'resolved',
        declared: runtime.working_directory,
        path: 'C:\\Aura\\fixtures\\package',
      },
      support: 'ready_for_conformance',
    },
  ],
  conformance: 'not_run',
  install_effect: 'trust_current_fingerprint_and_enable',
};
const validate = createContractValidator<PackageImportPreviewV1>({
  $defs: catalog.$defs,
  $ref: '#/$defs/PackageImportPreviewV1',
});

test('generated package preview schema preserves the authoritative Rust manifest and content identities', () => {
  expect(validate(preview), JSON.stringify(validate.errors)).toBe(true);
  expect(preview.content.manifest.tools[0]!.resource_claims).toEqual(
    manifest.tools[0]!.resource_claims,
  );
});
test('generated package preview schema refuses incompatible versions, runtimes and extra metadata', () => {
  const entry = preview.runtimes[0]!;
  for (const invalid of [
    { ...preview, contract_version: 2 },
    { ...preview, installed: true },
    { ...preview, runtimes: [{ ...entry, execution_location: 'app_container' }] },
    {
      ...preview,
      runtimes: [
        {
          ...entry,
          working_directory: { type: 'resolved', declared: runtime.working_directory, path: '' },
        },
      ],
    },
    { ...preview, runtimes: [{ ...entry, support: 'silently_ignore_credentials' }] },
    {
      ...preview,
      content: { ...preview.content, credential_value: 'synthetic-never-allowed-here' },
    },
  ])
    expect(validate(invalid)).toBe(false);
});
