import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import type { AnySchema } from 'ajv';
import { expect, test } from 'vitest';
import { createContractValidator } from '../src/validation.js';

test('Broker runtime contracts reject arbitrary execution, headers and untyped context actions', async () => {
  const root = resolve(import.meta.dirname, '../../..');
  const schema = JSON.parse(
    await readFile(resolve(root, 'resources/schemas/aura-contracts.schema.json'), 'utf8'),
  ) as { $defs: Record<string, AnySchema> };
  const validate = (name: string) =>
    createContractValidator({ $defs: schema.$defs, $ref: `#/$defs/${name}` });
  const context = validate('ContextBrokerReadRequest');
  expect(context({ kind: 'memory_settings' })).toBe(true);
  expect(context({ kind: 'shell', command: 'whoami' })).toBe(false);
  expect(context({ kind: 'memory_settings', credentials: 'canary' })).toBe(false);
  // Replaced the old Broker `GitReadQuery` with the isolated native read query.
  const git = validate('GitReadInputV1');
  expect(git({ kind: 'status', input: {} })).toBe(true);
  expect(git({ kind: 'diff', input: { target: { kind: 'unstaged' } } })).toBe(true);
  expect(git({ kind: 'exec', input: { args: ['push'] } })).toBe(false);
  expect(git({ kind: 'status', input: {}, executable: 'other.exe' })).toBe(false);
  expect(git({ kind: 'status', input: { executable: 'other.exe' } })).toBe(false);
  const fixture = JSON.parse(
    await readFile(resolve(root, 'fixtures/contracts/v2-kernel.json'), 'utf8'),
  );
  const receipt = validate('BrokerEvidence');
  const evidence = {
    action_kind: 'fs.write_file',
    request_fingerprint: fixture.dispatch.operation.fingerprint,
    before: { absent: true },
    after: { committed: true },
    domain_journal: { state: 'completed' },
    side_effect: 'fully_applied',
  };
  expect(receipt(evidence), JSON.stringify(receipt.errors)).toBe(true);
  expect(receipt({ ...evidence, retry_unknown: true })).toBe(false);
  expect(receipt({ ...evidence, side_effect: 'assumed_applied' })).toBe(false);
});
