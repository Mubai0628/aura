import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, test } from 'vitest';

import { parseProjection } from './projection-validation.js';

const fixturePath = resolve(
  import.meta.dirname,
  '../../../fixtures/contracts/projections-full.v1.json',
);

type JsonSchema = {
  $ref?: string;
  type?: string | string[];
  const?: unknown;
  enum?: unknown[];
  properties?: Record<string, JsonSchema>;
  oneOf?: JsonSchema[];
  anyOf?: JsonSchema[];
  items?: JsonSchema;
  additionalProperties?: boolean | JsonSchema;
};

function collectFields(
  value: unknown,
  schema: JsonSchema,
  definitions: Record<string, JsonSchema>,
  path: string,
  expected: Set<string>,
  populated: Set<string>,
): void {
  if (schema.$ref !== undefined) {
    const name = schema.$ref.replace('#/$defs/', '');
    const target = definitions[name];
    expect(target, path).toBeDefined();
    collectFields(value, target!, definitions, path, expected, populated);
    return;
  }
  const variants = schema.oneOf ?? schema.anyOf;
  if (variants !== undefined) {
    const variant = variants.find((candidate) => {
      if (candidate.type === 'null') return false;
      if (value === null || value === undefined) return true;
      return Object.entries(candidate.properties ?? {}).every(
        ([key, property]) =>
          property.const === undefined ||
          (value as Record<string, unknown>)?.[key] === property.const,
      );
    });
    expect(variant, path).toBeDefined();
    collectFields(value, variant!, definitions, path, expected, populated);
    return;
  }
  if (schema.properties !== undefined) {
    for (const [key, property] of Object.entries(schema.properties)) {
      const fieldPath = `${path}.${key}`;
      expected.add(fieldPath);
      const field = (value as Record<string, unknown> | null)?.[key];
      if (field !== null && field !== undefined) populated.add(fieldPath);
      collectFields(field, property, definitions, fieldPath, expected, populated);
    }
  }
  if (schema.items !== undefined) {
    const itemPath = `${path}[]`;
    expected.add(itemPath);
    if (Array.isArray(value) && value.length > 0) {
      populated.add(itemPath);
      for (const item of value) {
        collectFields(item, schema.items, definitions, itemPath, expected, populated);
      }
    } else {
      collectFields(undefined, schema.items, definitions, itemPath, expected, populated);
    }
  }
  if (typeof schema.additionalProperties === 'object' && schema.properties === undefined) {
    const itemPath = `${path}{}`;
    expected.add(itemPath);
    const entries = value !== null && typeof value === 'object' ? Object.values(value) : [];
    if (entries.length > 0) populated.add(itemPath);
    for (const item of entries.length === 0 ? [undefined] : entries) {
      collectFields(item, schema.additionalProperties, definitions, itemPath, expected, populated);
    }
  }
}

describe('Rust projection samples across the handwritten desktop boundary', () => {
  test('every registered kind has a full optional-field Rust sample', () => {
    const fixture = JSON.parse(readFileSync(fixturePath, 'utf8')) as {
      schema_version: number;
      projections: unknown[];
      semantic_empty_fields: Record<string, string>;
    };
    expect(fixture.schema_version).toBe(1);
    const schema = JSON.parse(
      readFileSync(
        resolve(import.meta.dirname, '../../../resources/schemas/aura-contracts.schema.json'),
        'utf8',
      ),
    ) as { $defs: Record<string, JsonSchema> };
    const variants = schema.$defs.ProjectionChange!.oneOf!;
    const expectedKinds = variants
      .map((variant) => variant.properties!.projection_kind!.const)
      .sort();
    const actualKinds = [
      ...new Set(
        fixture.projections.map(
          (projection) => (projection as { projection_kind: string }).projection_kind,
        ),
      ),
    ].sort();
    expect(actualKinds).toEqual(expectedKinds);
    const rejected = fixture.projections.filter(
      (projection) => parseProjection(projection) === undefined,
    );
    expect(rejected, JSON.stringify(rejected, null, 2)).toEqual([]);
    for (const kind of actualKinds) {
      const expected = new Set<string>();
      const populated = new Set<string>();
      const variant = variants.find(
        (candidate) => candidate.properties!.projection_kind!.const === kind,
      )!;
      for (const projection of fixture.projections.filter(
        (candidate) => (candidate as { projection_kind: string }).projection_kind === kind,
      )) {
        collectFields(projection, variant, schema.$defs, kind, expected, populated);
      }
      expect(Object.keys(fixture.semantic_empty_fields).sort()).toEqual([
        'model_profile.payload.active_availability_test.cancellation_reason',
        'model_profile.payload.active_availability_test.completed_at',
        'model_profile.payload.active_availability_test.result',
      ]);
      const legalEmpty = Object.keys(fixture.semantic_empty_fields);
      expect(
        [...expected].filter(
          (path) =>
            !populated.has(path) &&
            !legalEmpty.some(
              (parent) =>
                path === parent || path.startsWith(`${parent}.`) || path.startsWith(`${parent}[]`),
            ),
        ),
        `${kind}: optional or nested fields missing from all legal samples`,
      ).toEqual([]);
    }
  });
});
