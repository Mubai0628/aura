import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import type { AnySchema } from 'ajv';
import { expect, test } from 'vitest';
import type { ImportReport } from '../src/generated/contracts.js';
import { createContractValidator } from '../src/validation.js';

test('Rust R02 import report covers every disposition and rejects secret extensions', async () => {
  const root = resolve(import.meta.dirname, '../../..');
  const schema = JSON.parse(
    await readFile(resolve(root, 'resources/schemas/aura-contracts.schema.json'), 'utf8'),
  ) as { $defs: Record<string, AnySchema> };
  const report = JSON.parse(
    await readFile(resolve(root, 'fixtures/contracts/v2-import-report.json'), 'utf8'),
  ) as ImportReport;
  const validate = createContractValidator<ImportReport>({
    $defs: schema.$defs,
    $ref: '#/$defs/ImportReport',
  });
  expect(validate(report)).toBe(true);
  expect(new Set(report.items.map((i) => i.disposition)).size).toBe(5);
  const bad = structuredClone(report);
  Object.assign(bad.items[0] ?? {}, { secret: 'secret-canary' });
  expect(validate(bad)).toBe(false);
  const invalid = structuredClone(report);
  Object.assign(invalid.items[0] ?? {}, { disposition: 'silently_dropped' });
  expect(validate(invalid)).toBe(false);
});
