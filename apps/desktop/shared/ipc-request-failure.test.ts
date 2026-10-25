import { expect, test } from 'vitest';
import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import {
  COMMAND_OUTCOME_UNKNOWN_CODE,
  isRequestLocalTimeout,
  isUnconfirmedCommandOutcome,
  OUTCOME_RECOVERY_NOT_SENT,
  OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
  REQUEST_TIMEOUT_CODE,
} from './ipc-request-failure.js';

const envelope = (overrides: Partial<IpcErrorEnvelope> = {}): IpcErrorEnvelope => ({
  category: 'cancelled',
  code: REQUEST_TIMEOUT_CODE,
  correlation_id: 'cor_00000000000000000000000000',
  details: {
    command_invocation_id: 'civ_00000000000000000000000000',
    outcome_recovery: OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
  },
  message: 'Core request timed out: models.settings.get_snapshot',
  retryable: false,
  terminal: 'cancelled',
  ...overrides,
});

test('one request timing out is not the channel being lost', () => {
  // What the transport settles a single waiter with: the channel stays open and Core keeps running.
  expect(isRequestLocalTimeout(envelope())).toBe(true);
  // A timeout reported as a channel terminal is a different thing and stays classified as one.
  expect(isRequestLocalTimeout(envelope({ terminal: 'transport_lost' }))).toBe(false);
  expect(isRequestLocalTimeout(envelope({ terminal: 'core_restarted' }))).toBe(false);
  const channelLoss = envelope({ code: 'ipc.transport_lost', terminal: 'transport_lost' });
  expect(isRequestLocalTimeout(channelLoss)).toBe(false);
});

test('the transport declaring a queryable outcome makes a command unconfirmed', () => {
  expect(isUnconfirmedCommandOutcome(envelope())).toBe(true);
  // The declaration alone is enough, whatever code carries it.
  const declared = envelope({ code: 'ipc.transport_lost', terminal: 'transport_lost' });
  expect(isUnconfirmedCommandOutcome(declared)).toBe(true);
});

test('a real channel loss stays unconfirmed without the recovery hint', () => {
  const lost = envelope({ code: 'ipc.transport_lost', details: {}, terminal: 'transport_lost' });
  expect(isUnconfirmedCommandOutcome(lost)).toBe(true);
  const restarted = envelope({
    code: 'ipc.core_restarted',
    details: {},
    terminal: 'core_restarted',
  });
  expect(isUnconfirmedCommandOutcome(restarted)).toBe(true);
});

test('a bridge failure keeps its own unconfirmed codes', () => {
  for (const code of ['agent_transport_failed', 'invalid_agent_result']) {
    const bridged = envelope({ code, details: {}, terminal: 'failed' });
    expect(isUnconfirmedCommandOutcome(bridged)).toBe(true);
  }
});

test('a request the transport declares unsent is definite, not unconfirmed', () => {
  // The opposite declaration to the query hint: no byte reached the writer, so there is nothing to
  // query and the command must not be shown as possibly executed. It keeps working when the sender
  // gave up first, which is when the client timeout itself is still request-local.
  const notSentTimeout = envelope({
    details: {
      command_invocation_id: 'civ_00000000000000000000000000',
      outcome_recovery: OUTCOME_RECOVERY_NOT_SENT,
    },
  });
  expect(isRequestLocalTimeout(notSentTimeout)).toBe(true);
  expect(isUnconfirmedCommandOutcome(notSentTimeout)).toBe(false);

  const notSentFailure = envelope({
    code: 'ipc.request_not_sent',
    details: { outcome_recovery: OUTCOME_RECOVERY_NOT_SENT },
    terminal: 'failed',
  });
  expect(isUnconfirmedCommandOutcome(notSentFailure)).toBe(false);

  // A locally refused request behaves the same way: nothing was written, nothing may be queried.
  const overloaded = envelope({
    code: 'ipc.local_overload',
    details: { outcome_recovery: OUTCOME_RECOVERY_NOT_SENT },
    terminal: 'failed',
  });
  expect(isUnconfirmedCommandOutcome(overloaded)).toBe(false);
});

test('an existing unknown-outcome terminal remains unconfirmed without a recovery hint', () => {
  expect(
    isUnconfirmedCommandOutcome({
      code: 'existing.error',
      details: {},
      terminal: 'outcome_unknown',
    }),
  ).toBe(true);
});

test('a rejected request with a definite answer is not unconfirmed', () => {
  // A validation, compatibility or permission answer is a result the caller DID receive.
  const rejected = [
    { category: 'validation' as const, code: 'ipc.invalid_preload_request' },
    { category: 'compatibility' as const, code: 'ipc.incompatible' },
    { category: 'permission' as const, code: 'ipc.denied' },
  ];
  for (const answer of rejected) {
    const envelopeWithAnswer = envelope({ ...answer, details: {}, terminal: 'failed' });
    expect(isUnconfirmedCommandOutcome(envelopeWithAnswer)).toBe(false);
    expect(isRequestLocalTimeout(envelopeWithAnswer)).toBe(false);
  }
});

test("Core's own started-but-unanswered envelope is unconfirmed by its code", () => {
  // The exact envelope Core produces for a synchronous business operation that did not answer
  // inside its budget: `terminal: failed` (there really is no result), the recovery field, and the
  // original invocation. It is NOT a definite failure, and the classification must not depend on
  // the message sentence.
  const coreUnknown = envelope({
    category: 'unavailable',
    code: COMMAND_OUTCOME_UNKNOWN_CODE,
    details: {
      command_invocation_id: 'civ_00000000000000000000000000',
      outcome_recovery: OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
    },
    message: 'the session write did not answer inside its budget after 85 ms',
    retryable: false,
    terminal: 'failed',
  });
  expect(isUnconfirmedCommandOutcome(coreUnknown)).toBe(true);

  // The code alone carries the meaning even when the recovery field is absent: a caller that has
  // the code must still not treat a started command as definitely failed.
  expect(
    isUnconfirmedCommandOutcome({
      code: COMMAND_OUTCOME_UNKNOWN_CODE,
      details: {},
      terminal: 'failed',
    }),
  ).toBe(true);

  // Rewording the message must not change the classification.
  expect(
    isUnconfirmedCommandOutcome({
      code: coreUnknown.code,
      details: coreUnknown.details,
      terminal: coreUnknown.terminal,
    }),
  ).toBe(true);
  expect(
    isUnconfirmedCommandOutcome({
      code: coreUnknown.code,
      details: coreUnknown.details,
      terminal: 'failed',
    }),
  ).toBe(true);
});
