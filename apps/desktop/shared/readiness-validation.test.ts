import { describe, expect, it } from 'vitest';

import {
  CORE_ROUTES,
  MAIN_ROUTES,
  coreRoute,
  mainRoute,
  ROUTE_COUNTS,
} from './generated/routes.js';
import { parseRuntimeReadiness } from './readiness-validation.js';

export const readyCore = {
  schema_version: 1,
  revision: '1',
  core_health: 'ready',
  capabilities: [
    { capability: 'workspace.files', readiness: 'ready', reason_code: 'service.ready' },
  ],
  backends: [
    {
      backend: 'strict_appcontainer',
      lifecycle: 'degraded',
      readiness: { readiness: 'unavailable', reason: 'backend.start_failed' },
    },
  ],
  extensions: [
    { extension: 'browser', readiness: 'disabled', reason_code: 'extension.not_enabled' },
  ],
} as const;

describe('R03 generated route and readiness boundaries', () => {
  it('validates the Rust contract without conflating Core and optional backend readiness', () => {
    expect(parseRuntimeReadiness(readyCore).core_health).toBe('ready');
    for (const value of [
      { ...readyCore, core_health: 'degraded' },
      { ...readyCore, unexpected: true },
      { ...readyCore, schema_version: 2 },
      {
        ...readyCore,
        backends: [{ ...readyCore.backends[0], readiness: { readiness: 'unknown' } }],
      },
    ])
      expect(() => parseRuntimeReadiness(value)).toThrow();
  });

  it('preserves closed generated counts and rejects prototype and driver routes', () => {
    expect(Object.keys(MAIN_ROUTES)).toHaveLength(ROUTE_COUNTS.main);
    expect(Object.keys(CORE_ROUTES)).toHaveLength(ROUTE_COUNTS.core);
    for (const method of [
      'toString',
      '__proto__',
      'internal.e2e.p08_contract_driver.run_vertical_suite',
    ]) {
      expect(mainRoute(method)).toBeUndefined();
      expect(coreRoute(method)).toBeUndefined();
    }
    expect(mainRoute('runtime.readiness')?.auth).toBe('trusted_renderer');
    expect(coreRoute('runtime.readiness')?.idempotency).toBe('read_only');
    for (const route of Object.values(MAIN_ROUTES)) {
      if (route.auth === 'secret_input') {
        expect(coreRoute(route.method)?.auth).toBe('secret_input');
        // The terminal's sensitive input and the MCP secret are the Agent's own secret inputs.
        if (['terminal.user.sensitive.submit', 'agent.mcp.secret.set'].includes(route.method))
          expect(route.owner).toBe('agent');
        else expect(['model', 'tool']).toContain(route.owner);
      }
      expect(route.test_only).toBe(false);
    }
    // The exact secret-input surface, matching the aura-ipc route audit (`SECRET_MAIN`).
    expect(
      new Set(
        Object.values(MAIN_ROUTES)
          .filter((route) => route.auth === 'secret_input')
          .map((route) => route.method),
      ),
    ).toEqual(
      new Set([
        'models.endpoint.configure_credential',
        'models.endpoint.create_with_credential',
        'models.endpoint.replace_credential',
        'tools.credential.create',
        'tools.credential.rotate',
        'terminal.user.sensitive.submit',
        'agent.mcp.secret.set',
      ]),
    );
  });
});
