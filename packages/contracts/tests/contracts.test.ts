import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

import type { AnySchema } from 'ajv';
import { describe, expect, test } from 'vitest';

import { ensureContractCompatibility } from '../src/compatibility.js';
import type {
  AuraContractCatalog,
  CanonicalToolPackageManifestV1,
  RawToolPackageManifestV1,
  ToolErrorCatalogV1,
} from '../src/generated/contracts.js';
import { TOOL_ERROR_CODES_V1 } from '../src/generated/tool-errors.js';
import { compareAuraIds, parseAuraId } from '../src/ids.js';
import { createContractValidator } from '../src/validation.js';

const workspaceRoot = resolve(import.meta.dirname, '../../..');
const loadJson = async <Value = unknown>(relativePath: string): Promise<Value> =>
  JSON.parse(await readFile(resolve(workspaceRoot, relativePath), 'utf8')) as Value;

describe('canonical contracts', () => {
  test('validates and round-trips the Rust fixture', async () => {
    const schema = await loadJson<AnySchema>('resources/schemas/aura-contracts.schema.json');
    const fixture = await loadJson<AuraContractCatalog>(
      'fixtures/contracts/canonical-contracts.json',
    );
    const validate = createContractValidator<AuraContractCatalog>(schema);

    expect(validate(fixture), JSON.stringify(validate.errors)).toBe(true);
    expect(JSON.parse(JSON.stringify(fixture))).toEqual(fixture);
    expect(fixture.windows_path).toBe('C:\\Users\\测试用户\\Aura Workspace\\src\\main.rs');
    expect(fixture.models.execution_snapshot?.id).toBe(fixture.ids.model_execution_snapshot_id);
    expect(fixture.models.stream_event?.event).toMatchObject({
      type: 'tool_call_started',
      provider_call_id: 'Call_Contract_ExactCase',
    });
  });

  test('rejects unknown contract fields at runtime', async () => {
    const schema = await loadJson<AnySchema>('resources/schemas/aura-contracts.schema.json');
    const fixture = await loadJson<AuraContractCatalog>(
      'fixtures/contracts/canonical-contracts.json',
    );
    const validate = createContractValidator<AuraContractCatalog>(schema);
    const invalid = { ...fixture, unknown: true };

    expect(validate(invalid)).toBe(false);
    expect(validate.errors?.some((error) => error.keyword === 'additionalProperties')).toBe(true);
  });

  test('rejects unknown fields inside P05 model contracts', async () => {
    const schema = await loadJson<AnySchema>('resources/schemas/aura-contracts.schema.json');
    const fixture = await loadJson<AuraContractCatalog>(
      'fixtures/contracts/canonical-contracts.json',
    );
    const validate = createContractValidator<AuraContractCatalog>(schema);
    const invalid = structuredClone(fixture);
    const registry = invalid.models.registry;
    if (!registry) {
      throw new Error('canonical fixture must contain the P05 model registry');
    }
    const provider = registry.providers[fixture.ids.provider_id];
    if (!provider) {
      throw new Error('canonical fixture must contain the registered P05 provider');
    }
    Object.assign(provider, { unknown: true });

    expect(validate(invalid)).toBe(false);
    expect(validate.errors?.some((error) => error.keyword === 'additionalProperties')).toBe(true);
  });

  test('rejects every illegal projection kind and scope pairing at runtime', async () => {
    const schema = await loadJson<AnySchema>('resources/schemas/aura-contracts.schema.json');
    const fixture = await loadJson<AuraContractCatalog>(
      'fixtures/contracts/canonical-contracts.json',
    );
    const validate = createContractValidator<AuraContractCatalog>(schema);
    type ProjectionContractFixture = {
      projection_contract: Record<
        | 'projection_key'
        | 'projection_subscription'
        | 'projection_snapshot_request'
        | 'projection_change',
        { projection_kind: string }
      >;
    };

    for (const field of [
      'projection_key',
      'projection_subscription',
      'projection_snapshot_request',
      'projection_change',
    ] as const) {
      const invalid = structuredClone(fixture) as unknown as ProjectionContractFixture;
      invalid.projection_contract[field].projection_kind = 'endpoint_detail';
      expect(validate(invalid as unknown as AuraContractCatalog), field).toBe(false);
    }
  });

  test('rejects unsigned integers that JavaScript cannot represent safely', async () => {
    const schema = await loadJson<AnySchema>('resources/schemas/aura-contracts.schema.json');
    const fixture = await loadJson<AuraContractCatalog>(
      'fixtures/contracts/canonical-contracts.json',
    );
    const validate = createContractValidator<AuraContractCatalog>(schema);
    const unsafeSequence = structuredClone(fixture);
    unsafeSequence.runtime.heartbeat.sequence = Number.MAX_SAFE_INTEGER + 1;
    expect(validate(unsafeSequence)).toBe(false);
    expect(
      validate.errors?.some(
        (error) => error.keyword === 'format' && error.params.format === 'uint64',
      ),
    ).toBe(true);

    const oversizedChunk = structuredClone(fixture);
    oversizedChunk.runtime.binary_stream_frame.data_length = 2 ** 32;
    expect(validate(oversizedChunk)).toBe(false);
    expect(
      validate.errors?.some(
        (error) => error.keyword === 'format' && error.params.format === 'uint32',
      ),
    ).toBe(true);
  });

  test('applies the frozen major/minor compatibility rule', () => {
    expect(ensureContractCompatibility({ major: 1, minor: 2 }, { major: 1, minor: 0 })).toEqual({
      compatible: true,
    });
    expect(
      ensureContractCompatibility({ major: 1, minor: 2 }, { major: 2, minor: 0 }),
    ).toMatchObject({
      compatible: false,
      reason: 'major_version_mismatch',
    });
    expect(
      ensureContractCompatibility({ major: 1, minor: 2 }, { major: 1, minor: 3 }),
    ).toMatchObject({
      compatible: false,
      reason: 'minor_version_too_new',
    });
  });

  test('parses and orders same-type ULID identifiers', () => {
    const early = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
    const late = 'ses_01BX5ZZKBKACTAV9WEVGEMMVRZ';
    expect(parseAuraId(early)).toEqual({ prefix: 'ses', ulid: early.slice(4) });
    expect([late, early].toSorted(compareAuraIds)).toEqual([early, late]);
    expect(() => parseAuraId(early.toLowerCase())).toThrow(TypeError);
    expect(() => compareAuraIds(early, early.replace('ses_', 'wsp_'))).toThrow(TypeError);
  });

  test('validates the registered P05 identities and opaque provider tool call IDs', async () => {
    const schema = await loadJson<AnySchema>('resources/schemas/aura-contracts.schema.json');
    const fixture = await loadJson<AuraContractCatalog>(
      'fixtures/contracts/canonical-contracts.json',
    );
    const validate = createContractValidator<AuraContractCatalog>(schema);

    expect(parseAuraId(fixture.ids.model_execution_snapshot_id).prefix).toBe('mex');
    expect(parseAuraId(fixture.ids.transport_attempt_id).prefix).toBe('tat');
    expect(parseAuraId(fixture.ids.model_content_block_id).prefix).toBe('mcb');
    expect(parseAuraId(fixture.ids.model_failure_id).prefix).toBe('mfl');
    expect(parseAuraId(fixture.ids.context_projection_snapshot_id).prefix).toBe('cps');
    expect(parseAuraId(fixture.ids.prompt_bundle_snapshot_id).prefix).toBe('pbs');
    expect(parseAuraId(fixture.ids.model_discovery_run_id).prefix).toBe('mdr');
    expect(parseAuraId(fixture.ids.model_availability_test_id).prefix).toBe('mav');
    expect(parseAuraId(fixture.ids.tool_id).prefix).toBe('tol');
    expect(parseAuraId(fixture.ids.credential_id).prefix).toBe('crd');
    expect(parseAuraId(fixture.ids.browser_share_id).prefix).toBe('bsh');
    expect(parseAuraId(fixture.ids.attachment_grant_id).prefix).toBe('atg');
    expect(parseAuraId(fixture.ids.tool_instance_id).prefix).toBe('tin');
    expect(parseAuraId(fixture.ids.tool_package_id).prefix).toBe('tpk');
    expect(parseAuraId(fixture.ids.tool_draft_id).prefix).toBe('tdr');
    expect(parseAuraId(fixture.ids.tool_batch_id).prefix).toBe('tbt');
    expect(parseAuraId(fixture.ids.tool_execution_attempt_id).prefix).toBe('tea');
    expect(parseAuraId(fixture.ids.tool_conformance_run_id).prefix).toBe('tcr');
    expect(parseAuraId(fixture.ids.tool_registry_snapshot_id).prefix).toBe('trs');

    const exactProviderId = structuredClone(fixture);
    exactProviderId.models.provider_tool_call_id = '  Call_MixedCase-01  ';
    expect(validate(exactProviderId), JSON.stringify(validate.errors)).toBe(true);

    const blankProviderId = structuredClone(fixture);
    blankProviderId.models.provider_tool_call_id = '   ';
    expect(validate(blankProviderId)).toBe(false);
  });

  test('validates the closed generated P07 error catalog', async () => {
    const schema = await loadJson<AnySchema>('resources/schemas/tool-error-catalog.v1.schema.json');
    const catalog = await loadJson<ToolErrorCatalogV1>(
      'fixtures/contracts/tool-error-catalog.v1.json',
    );
    const validate = createContractValidator<ToolErrorCatalogV1>(schema);

    expect(validate(catalog), JSON.stringify(validate.errors)).toBe(true);
    expect(catalog.entries.map((entry) => entry.code)).toEqual([...TOOL_ERROR_CODES_V1]);

    const unknownCode = structuredClone(catalog);
    unknownCode.entries[0]!.code = 'tool.schema.dynamic_unregistered_code';
    expect(validate(unknownCode)).toBe(false);
  });

  test('validates the P07 package manifest golden fixtures', async () => {
    const rawSchema = await loadJson<AnySchema>(
      'resources/schemas/aura-tool-package-manifest.v1.schema.json',
    );
    const canonicalSchema = await loadJson<AnySchema>(
      'resources/schemas/aura-tool-package-canonical-manifest.v1.schema.json',
    );
    const validateRaw = createContractValidator<RawToolPackageManifestV1>(rawSchema);
    const validateCanonical =
      createContractValidator<CanonicalToolPackageManifestV1>(canonicalSchema);

    for (const path of [
      'fixtures/tool-package/manifest-v1/valid/aura-stdio.json',
      'fixtures/tool-package/manifest-v1/valid/declarative-command.json',
      'fixtures/tool-package/manifest-v1/valid/declarative-http.json',
    ]) {
      const fixture = await loadJson<RawToolPackageManifestV1>(path);
      expect(validateRaw(fixture), `${path}: ${JSON.stringify(validateRaw.errors)}`).toBe(true);
    }

    for (const path of [
      'fixtures/tool-package/manifest-v1/invalid/schema-version.json',
      'fixtures/tool-package/manifest-v1/invalid/internal-id-field.json',
      'fixtures/tool-package/manifest-v1/invalid/nested-unknown-field.json',
      'fixtures/tool-package/manifest-v1/invalid/runtime-variant.json',
      'fixtures/tool-package/manifest-v1/invalid/empty-tools.json',
      'fixtures/tool-package/manifest-v1/invalid/empty-conformance.json',
    ]) {
      const fixture = await loadJson<RawToolPackageManifestV1>(path);
      expect(validateRaw(fixture), path).toBe(false);
    }

    const canonical = await loadJson<CanonicalToolPackageManifestV1>(
      'fixtures/tool-package/manifest-v1/canonical.json',
    );
    expect(validateCanonical(canonical), JSON.stringify(validateCanonical.errors)).toBe(true);
    expect(canonical.tools[0]?.semantic_version).toBeNull();
    expect(canonical.tools[0]?.audience).toBe('subagent_configurable');
    expect(canonical.tools[0]?.runtime).toMatchObject({
      type: 'declarative_command',
      command: { success_exit_codes: [0] },
    });
  });
});
