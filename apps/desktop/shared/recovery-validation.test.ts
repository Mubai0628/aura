import { expect, test } from 'vitest';
import { isRecoveryParams, parseRecoveryResult } from './recovery-validation.js';

test('generated Rust schema rejects missing confirmation, unknown fields and actions', () => {
  expect(
    isRecoveryParams('config.recovery.apply', {
      action: 'retry',
      domain: 'tools',
      domains: [],
      preview_id: 'shown',
    }),
  ).toBe(true);
  for (const value of [
    { action: 'retry', domain: 'tools' },
    { action: 'erase_credentials', preview_id: 'x' },
    { action: 'retry', domain: 'tools', preview_id: 'x', secret: 'canary' },
    { action: 'retry', domain: 'not-a-domain', preview_id: 'x' },
  ]) {
    expect(isRecoveryParams('config.recovery.apply', value)).toBe(false);
  }
});

test('a bundle response must carry the final confirmation token', () => {
  const plan = {
    modules: [],
    options: [],
    consistent_revision: null,
    domains: [],
    inconsistency_code: null,
    consistent_set: [],
    preview_id: 'core-token',
  };
  expect(parseRecoveryResult('config.recovery.plan', plan)).toEqual(plan);
  const { preview_id: _token, ...missing } = plan;
  expect(_token).toBe('core-token');
  expect(() => parseRecoveryResult('config.recovery.plan', missing)).toThrow();
});
