import { expect, test } from 'vitest';
import {
  isToolCredentialParams,
  isToolCredentialCommand,
  isToolCredentialSecretCommand,
  parseToolCredentialResult,
} from './tool-credential-validation.js';

test('credential routes are closed and only create/rotate accept SecretInput', () => {
  expect(isToolCredentialCommand('tools.credential.dump')).toBe(false);
  expect(isToolCredentialSecretCommand('tools.credential.create')).toBe(true);
  expect(isToolCredentialSecretCommand('tools.credential.rotate')).toBe(true);
  expect(isToolCredentialSecretCommand('tools.credential.clear')).toBe(false);
  expect(isToolCredentialSecretCommand('tools.credential.outcome')).toBe(false);
});
test('generated validator refuses plaintext, absent expected identity, and unknown outcome shapes', () => {
  expect(
    isToolCredentialParams('tools.credential.create', {
      display_name: 'x',
      scope: { scope: 'user' },
      secret: 'never',
    }),
  ).toBe(false);
  expect(
    isToolCredentialParams('tools.credential.rotate', {
      credential_id: 'cred_01AAAAAAAAAAAAAAAAAAAAAAAA',
    }),
  ).toBe(false);
  expect(() =>
    parseToolCredentialResult('tools.credential.outcome', { state: 'committed' }),
  ).toThrow();
  expect(() =>
    parseToolCredentialResult('tools.credential.list', {
      credentials: [],
      pending_credential_ids: [],
      secret: 'never',
    }),
  ).toThrow();
});
