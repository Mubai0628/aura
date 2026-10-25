import { expect, test } from 'vitest';
import { isP04MainParams } from './p04-validation.js';

const input = {
  session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  grant_id: 'pgr_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  expected_revision: '1',
  correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
};
test('Native consent withdrawal accepts the generated Revision wire representation', () => {
  expect(isP04MainParams('security.native_session_consent.revoke', input)).toBe(true);
});
test('Native consent withdrawal rejects noncanonical revisions and unrelated identities', () => {
  for (const expected_revision of [1, '', '01', '-1', null]) {
    expect(
      isP04MainParams('security.native_session_consent.revoke', {
        ...input,
        expected_revision,
      }),
    ).toBe(false);
  }
  expect(
    isP04MainParams('security.native_session_consent.revoke', {
      ...input,
      correlation_id: 'not-an-Aura-correlation',
    }),
  ).toBe(false);
});
