import assert from 'node:assert/strict';
import { test } from 'node:test';
import { sizeLimit } from './export-public.mjs';

test('only the generated contract validators may pass the general size limit', () => {
  const general = 8 * 1024 * 1024;
  assert.equal(sizeLimit('apps/desktop/shared/generated/agent-validator.ts'), 16 * 1024 * 1024);
  assert.equal(sizeLimit('apps/desktop/shared/generated/routes.ts'), 16 * 1024 * 1024);
  assert.equal(sizeLimit('apps/desktop/shared/generated/nested/agent-validator.ts'), general);
  assert.equal(sizeLimit('apps/desktop/shared/generated/agent-validator.js'), general);
  assert.equal(sizeLimit('apps/desktop/shared/agent-validator.ts'), general);
  assert.equal(sizeLimit('resources/fixtures/large.bin'), general);
});
