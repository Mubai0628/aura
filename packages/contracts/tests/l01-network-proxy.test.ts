import { readFileSync } from 'node:fs';
import type { AnySchema } from 'ajv';
import { expect, test } from 'vitest';
import { createContractValidator } from '../src/validation.js';

const catalog = JSON.parse(
  readFileSync(
    new URL('../../../resources/schemas/aura-contracts.schema.json', import.meta.url),
    'utf8',
  ),
) as { $defs: Record<string, AnySchema> };
const validate = createContractValidator<Record<string, unknown>>({
  $defs: catalog.$defs,
  $ref: '#/$defs/ToolSettingsSnapshotV1',
});
const legacy = {
  revision: '7',
  content_hash: {
    algorithm: 'sha256',
    canonicalization_version: 'rfc8785_jcs_v1',
    hex: 'a'.repeat(64),
  },
  max_dynamic_tool_schemas: 50,
  max_concurrent_tool_calls: 4,
  default_timeout_ms: 120_000,
  max_retries: 3,
};
const proxy = { enabled: true, protocol: 'socks5', host: '127.0.0.1', port: 10808 };

test('old snapshots without a proxy still validate', () => {
  expect(validate(legacy), JSON.stringify(validate.errors)).toBe(true);
});
test('runtime schema accepts both loopback proxy protocols', () => {
  for (const protocol of ['http', 'socks5']) {
    for (const host of ['127.0.0.1', '127.2.3.4', '::1', 'localhost']) {
      expect(
        validate({ ...legacy, network_proxy: { ...proxy, protocol, host } }),
        JSON.stringify(validate.errors),
      ).toBe(true);
    }
  }
});
test('remote hosts, invalid ports, protocols and authentication are rejected', () => {
  for (const changed of [
    { host: '10.0.0.1' },
    { host: '8.8.8.8' },
    { host: 'example.test' },
    { host: '::ffff:127.0.0.1' },
    { host: '127.300.0.1' },
    { host: '0.0.0.0' },
    { port: 0 },
    { port: 65536 },
    { protocol: 'socks5h' },
    { username: 'user' },
  ]) {
    expect(validate({ ...legacy, network_proxy: { ...proxy, ...changed } })).toBe(false);
  }
});
