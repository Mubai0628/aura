import { describe, expect, it } from 'vitest';
import type { RevokeFileSessionConsentInput } from '@aura/contracts/generated';
import { isP04MainParams } from './p04-validation.js';

describe('S06 file consent revocation at the Electron boundary', () => {
  const request: RevokeFileSessionConsentInput = {
    session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    grant_id: 'pgr_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    expected_revision: '1',
  };
  const command = 'security.file_session_consent.revoke';

  it('accepts the string revision returned by the real file grant projection', () => {
    expect(isP04MainParams(command, request)).toBe(true);
    expect(isP04MainParams(command, { ...request, expected_revision: '9007199254740993' })).toBe(
      true,
    );
  });

  it('refuses a numeric revision, malformed identity, or additional authority', () => {
    expect(isP04MainParams(command, { ...request, expected_revision: 1 })).toBe(false);
    expect(isP04MainParams(command, { ...request, grant_id: 'invalid' })).toBe(false);
    expect(isP04MainParams(command, { ...request, recursive: true })).toBe(false);
  });
});
