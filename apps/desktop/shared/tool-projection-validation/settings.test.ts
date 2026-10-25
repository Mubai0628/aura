import { describe, expect, it } from 'vitest';
import { parseProjection } from '../projection-validation.js';

const settings = (extra: Record<string, unknown> = {}) => ({
  content_hash: {
    algorithm: 'sha256',
    canonicalization_version: 'rfc8785_jcs_v1',
    hex: '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
  },
  default_timeout_ms: 600_000,
  max_concurrent_tool_calls: 10,
  max_dynamic_tool_schemas: 50,
  max_retries: 3,
  revision: '4',
  ...extra,
});
const change = (payload: unknown) => ({
  payload,
  projection_kind: 'tool_settings',
  projection_scope: { type: 'global' },
  revision: '3',
  runtime_instance_id: 'run_3A3PFZHX0RMQ0SVQSERYGGH5KG',
});
const proxy = { enabled: true, protocol: 'socks5', host: '127.0.0.1', port: 10808 };

describe('Tool Settings projection with a saved network proxy', () => {
  // Core publishes the proxy once one is saved, enabled or not. The desktop rejected the
  // projection as a protocol violation and reconnected in a loop until it gave up.
  it('accepts a saved proxy whether or not it is enabled', () => {
    for (const saved of [
      { ...proxy, enabled: false },
      proxy,
      { ...proxy, protocol: 'http', host: 'localhost', port: 10809 },
      { ...proxy, host: '::1', port: 1 },
      { ...proxy, host: '127.255.0.1', port: 65_535 },
    ]) {
      const payload = { configuration_issue: null, settings: settings({ network_proxy: saved }) };
      expect(parseProjection(change(payload)), JSON.stringify(saved)).toBeDefined();
    }
  });

  it('rejects a proxy the Core contract cannot produce', () => {
    for (const bad of [
      null,
      { ...proxy, password: 'x' },
      { ...proxy, host: '10.0.0.2' },
      { ...proxy, host: 'proxy.example' },
      { ...proxy, port: 0 },
      { ...proxy, port: 65_536 },
      { ...proxy, port: 10_808.5 },
      { ...proxy, protocol: 'https' },
      { ...proxy, enabled: 'yes' },
      { enabled: true, protocol: 'socks5', host: '127.0.0.1' },
    ]) {
      const payload = { settings: settings({ network_proxy: bad }) };
      expect(parseProjection(change(payload)), JSON.stringify(bad)).toBeUndefined();
    }
  });
});
