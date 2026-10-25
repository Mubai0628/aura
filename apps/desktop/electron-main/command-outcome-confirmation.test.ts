// @vitest-environment node
import { expect, test } from 'vitest';
import type {
  CommandInvocationId,
  CommandOutcome,
  CorrelationId,
  IpcErrorEnvelope,
} from '@aura/contracts/generated';
import {
  CommandOutcomeConfirmation,
  COMMAND_OUTCOME_CONFIRMATION_POLICY,
  confirmationEntry,
  verifyConfirmedOutcome,
  type CommandOutcomeConfirmationPolicy,
  type ConfirmationEntry,
  type ConfirmationQueryRequest,
  type ConfirmationRecord,
} from './command-outcome-confirmation.js';

const COR = 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV' as CorrelationId;
const CIV = 'civ_01ARZ3NDEKTSV4RRFFQ69G5FAV' as CommandInvocationId;

/** A write whose caller stopped waiting: the transport's own query-without-replay failure. */
const timedOut = (
  overrides: {
    code?: string;
    correlation_id?: string;
    details?: Record<string, unknown>;
    terminal?: string;
  } = {},
): IpcErrorEnvelope =>
  ({
    category: 'cancelled',
    code: overrides.code ?? 'ipc.request_timeout',
    correlation_id: overrides.correlation_id ?? COR,
    details: overrides.details ?? {
      command_invocation_id: CIV,
      outcome_recovery: 'query_runtime.command_outcome_without_replay',
    },
    message: 'Core request timed out',
    retryable: false,
    terminal: overrides.terminal ?? 'cancelled',
  }) as IpcErrorEnvelope;

const rejectionWith = (envelope: IpcErrorEnvelope): Error & { envelope: IpcErrorEnvelope } =>
  Object.assign(new Error(envelope.message), { envelope });

/** The stored response Core keeps for a side-effecting command, as `record_command_outcome` writes it. */
const confirmedSucceeded = (value: unknown, id: string = COR): CommandOutcome =>
  ({
    command_invocation_id: CIV,
    outcome: 'confirmed',
    response: { id, jsonrpc: '2.0', result: { terminal: 'succeeded', value } },
    terminal: 'succeeded',
  }) as CommandOutcome;

const confirmedFailed = (envelope: IpcErrorEnvelope, id: string = COR): CommandOutcome =>
  ({
    command_invocation_id: CIV,
    outcome: 'confirmed',
    response: {
      error: { code: -32_800, data: envelope, message: envelope.message },
      id,
      jsonrpc: '2.0',
    },
    terminal: envelope.terminal,
  }) as CommandOutcome;

const unknownOutcome = (): CommandOutcome => ({
  command_invocation_id: CIV,
  outcome: 'unknown',
  reason: 'not_recorded_or_incomplete',
});

const TEST_POLICY: CommandOutcomeConfirmationPolicy = {
  attemptBudgetMs: 60,
  maxAttempts: 2,
  maxConcurrent: 4,
  retryDelayMs: 10,
  totalBudgetMs: 200,
};

const ENTRY: ConfirmationEntry = {
  command: 'fs.write_file',
  commandInvocationId: CIV,
  correlationId: COR,
  failureCode: 'ipc.request_timeout',
};

test('confirmation refuses incomplete or contradictory response wrappers', () => {
  const success = confirmedSucceeded({ written: true });
  const response = success.outcome === 'confirmed' ? success.response : undefined;
  expect(
    verifyConfirmedOutcome(ENTRY, {
      ...success,
      response: { id: COR, jsonrpc: '2.0', result: { terminal: 'succeeded' } },
    }),
  ).toEqual({ kind: 'refused' });
  expect(
    verifyConfirmedOutcome(ENTRY, {
      ...success,
      response: { ...response, error: null },
    }),
  ).toEqual({ kind: 'refused' });
});

test('confirmation requires the complete error envelope of the original request', () => {
  const envelope = timedOut({ code: 'ipc.request_failed', terminal: 'failed' });
  for (const changed of [
    { ...envelope, correlation_id: 'cor_3123456789ABCDEFGHJKMNPQRS' },
    { ...envelope, retryable: undefined },
    { ...envelope, category: 'invented' },
    { ...envelope, details: null },
  ]) {
    expect(verifyConfirmedOutcome(ENTRY, confirmedFailed(changed as IpcErrorEnvelope))).toEqual({
      kind: 'refused',
    });
  }
  expect(verifyConfirmedOutcome(ENTRY, confirmedFailed(envelope)).kind).toBe('confirmed-failure');
});

interface Harness {
  readonly confirmation: CommandOutcomeConfirmation<string>;
  readonly queries: ConfirmationQueryRequest<string>[];
  readonly records: ConfirmationRecord[];
}

const harness = (
  answer: (attempt: number, request: ConfirmationQueryRequest<string>) => Promise<CommandOutcome>,
  policy: CommandOutcomeConfirmationPolicy = TEST_POLICY,
): Harness => {
  const queries: ConfirmationQueryRequest<string>[] = [];
  const records: ConfirmationRecord[] = [];
  const confirmation = new CommandOutcomeConfirmation<string>(
    {
      channel: () => 'original',
      now: () => performance.now(),
      onEvent: (record) => records.push(record),
      query: (request) => {
        queries.push(request);
        return answer(queries.length, request);
      },
    },
    policy,
  );
  return { confirmation, queries, records };
};

test('the entry gate covers exactly the writes whose outcome Core records', () => {
  const cases: Array<[string, string, unknown, string]> = [
    [
      'a write with the query hint and a usable identity',
      'fs.write_file',
      rejectionWith(timedOut()),
      'enter',
    ],
    ['a P07 domain-request write', 'tools.tool.set_enabled', rejectionWith(timedOut()), 'enter'],
    ['a P05 mutation', 'models.default.set', rejectionWith(timedOut()), 'enter'],
    ['an Agent command', 'agent.submit', rejectionWith(timedOut()), 'enter'],
    ['a pure read', 'fs.read_text', rejectionWith(timedOut()), 'route-not-a-command'],
    [
      'a pure read on the model surface',
      'models.settings.get_snapshot',
      rejectionWith(timedOut()),
      'route-not-a-command',
    ],
    [
      'a route outside the Core registry',
      'fixture.echo',
      rejectionWith(timedOut()),
      'route-unknown',
    ],
    ['the cancel control route', 'aura.cancel', rejectionWith(timedOut()), 'control-route'],
    [
      'the outcome query itself',
      'runtime.command_outcome',
      rejectionWith(timedOut()),
      'control-route',
    ],
    [
      'a request the transport proved unsent',
      'fs.write_file',
      rejectionWith(
        timedOut({
          details: { not_sent_reason: 'send_failed', outcome_recovery: 'not_sent' },
        }),
      ),
      'not-sent',
    ],
    [
      'a definite failure',
      'fs.write_file',
      rejectionWith(timedOut({ details: {}, terminal: 'failed' })),
      'failure-not-unconfirmed',
    ],
    [
      'a rejection without a typed envelope',
      'fs.write_file',
      new Error('boom'),
      'no-failure-envelope',
    ],
    [
      'a write without an invocation id',
      'fs.write_file',
      rejectionWith(
        timedOut({ details: { outcome_recovery: 'query_runtime.command_outcome_without_replay' } }),
      ),
      'invocation-unusable',
    ],
    [
      'a write whose invocation id is not canonical',
      'fs.write_file',
      rejectionWith(
        timedOut({
          details: {
            command_invocation_id: 'civ_not-a-ulid',
            outcome_recovery: 'query_runtime.command_outcome_without_replay',
          },
        }),
      ),
      'invocation-unusable',
    ],
    [
      'a write whose correlation id is missing',
      'fs.write_file',
      rejectionWith(timedOut({ correlation_id: '' })),
      'invocation-unusable',
    ],
  ];
  for (const [name, command, failure, expected] of cases) {
    const decision = confirmationEntry(command, failure);
    if (expected === 'enter') {
      expect(decision.kind, name).toBe('enter');
      expect(decision.kind === 'enter' ? decision.entry.commandInvocationId : undefined, name).toBe(
        CIV,
      );
      expect(decision.kind === 'enter' ? decision.entry.correlationId : undefined, name).toBe(COR);
    } else {
      expect(decision.kind === 'skip' ? decision.reason : decision.kind, name).toBe(expected);
    }
  }
});

test('a confirmed outcome is adopted only when it is provably this operation’s own', () => {
  const success = verifyConfirmedOutcome(ENTRY, confirmedSucceeded({ status: 'ok' }));
  expect(success).toEqual({
    kind: 'confirmed-success',
    terminal: 'succeeded',
    value: { status: 'ok' },
  });

  const failureEnvelope = timedOut({ code: 'core.recovery_required', terminal: 'failed' });
  expect(verifyConfirmedOutcome(ENTRY, confirmedFailed(failureEnvelope))).toEqual({
    envelope: failureEnvelope,
    kind: 'confirmed-failure',
    terminal: 'failed',
  });

  // `unknown` is the pending answer, never a refusal and never a result.
  expect(verifyConfirmedOutcome(ENTRY, unknownOutcome())).toEqual({ kind: 'unknown' });
  expect(verifyConfirmedOutcome(ENTRY, undefined)).toEqual({ kind: 'refused' });

  // A response addressed to another request is another operation's result.
  expect(
    verifyConfirmedOutcome(ENTRY, confirmedSucceeded({}, 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAW')),
  ).toEqual({ kind: 'refused' });
  // A foreign invocation id is refused even when the shift is one character.
  expect(
    verifyConfirmedOutcome(ENTRY, {
      ...confirmedSucceeded({}),
      command_invocation_id: 'civ_01ARZ3NDEKTSV4RRFFQ69G5FAW',
    }),
  ).toEqual({ kind: 'refused' });
  // The terminal must agree with the stored response.
  expect(
    verifyConfirmedOutcome(ENTRY, {
      ...confirmedSucceeded({}),
      terminal: 'failed',
    }),
  ).toEqual({ kind: 'refused' });
  // Exactly one terminal, and a success may not carry a failure terminal.
  expect(
    verifyConfirmedOutcome(ENTRY, {
      command_invocation_id: CIV,
      outcome: 'confirmed',
      response: { id: COR, jsonrpc: '2.0' },
      terminal: 'succeeded',
    }),
  ).toEqual({ kind: 'refused' });
  expect(
    verifyConfirmedOutcome(ENTRY, {
      command_invocation_id: CIV,
      outcome: 'confirmed',
      response: {
        error: { code: -32_603, data: { code: 'x' }, message: 'x' },
        id: COR,
        jsonrpc: '2.0',
      },
      terminal: 'failed',
    }),
  ).toEqual({ kind: 'refused' });
});

test('a confirmed success is returned with the Core’s value and one attempt', async () => {
  const h = harness(async () => confirmedSucceeded({ status: 'written' }));
  const result = await h.confirmation.confirm(ENTRY, 'owner');
  expect(result.kind).toBe('confirmed-success');
  expect(result.kind === 'confirmed-success' ? result.value : undefined).toEqual({
    status: 'written',
  });
  expect(h.queries).toHaveLength(1);
  expect(h.queries[0]).toMatchObject({
    commandInvocationId: CIV,
    correlationId: COR,
    owner: 'owner',
  });
  expect(h.records).toEqual([
    {
      attempts: 1,
      budgetMs: TEST_POLICY.totalBudgetMs,
      command: 'fs.write_file',
      commandInvocationId: CIV,
      correlationId: COR,
      elapsedMs: expect.any(Number),
      originalFailureCode: 'ipc.request_timeout',
      outcome: 'confirmed-succeeded',
      queryChannel: 'original',
      source: 'outcome-query',
      terminal: 'succeeded',
    },
  ]);
  expect(h.confirmation.active).toBe(0);
});

test('a confirmed failure and a confirmed cancellation keep the Core’s own envelope', async () => {
  const failed = timedOut({ code: 'tool.draft.idempotency_conflict', terminal: 'failed' });
  const failureHarness = harness(async () => confirmedFailed(failed));
  const failure = await failureHarness.confirmation.confirm(ENTRY, 'owner');
  expect(failure.kind).toBe('confirmed-failure');
  expect(failure.kind === 'confirmed-failure' ? failure.envelope : undefined).toEqual(failed);
  expect(failureHarness.records[0]?.outcome).toBe('confirmed-failed');

  const cancelled = timedOut({ code: 'ipc.request_cancelled', terminal: 'cancelled' });
  const cancelHarness = harness(async () => confirmedFailed(cancelled));
  const cancellation = await cancelHarness.confirmation.confirm(ENTRY, 'owner');
  expect(cancellation.kind === 'confirmed-failure' ? cancellation.envelope : undefined).toEqual(
    cancelled,
  );
  expect(cancelHarness.records[0]?.outcome).toBe('confirmed-cancelled');
});

test('an outcome recorded a moment late is still confirmed inside the one budget', async () => {
  const h = harness(async (attempt) => (attempt === 1 ? unknownOutcome() : confirmedSucceeded(7)));
  const result = await h.confirmation.confirm(ENTRY, 'owner');
  expect(result.kind).toBe('confirmed-success');
  expect(h.queries).toHaveLength(2);
  expect(h.records[0]?.attempts).toBe(2);
  expect(h.records[0]?.outcome).toBe('confirmed-succeeded');
  // The record carries the two classification facts an acceptance reader needs to state that a REAL
  // local failure was really confirmed: the code of the failure that entered confirmation, and the
  // terminal the Core's ledger named. Neither is a response body.
  expect(h.records[0]).toMatchObject({
    originalFailureCode: 'ipc.request_timeout',
    source: 'outcome-query',
    terminal: 'succeeded',
  });
});

test('a record of an unconfirmed operation names its original failure and NO terminal', async () => {
  const h = harness(async () => unknownOutcome());
  await h.confirmation.confirm(ENTRY, 'owner');
  expect(h.records[0]?.originalFailureCode).toBe('ipc.request_timeout');
  expect(h.records[0]?.source).toBe('original-failure');
  expect(h.records[0]?.terminal).toBeUndefined();
});

test('an outcome that stays unknown is pending after a finite number of attempts', async () => {
  const h = harness(async () => unknownOutcome());
  const result = await h.confirmation.confirm(ENTRY, 'owner');
  expect(result.kind).toBe('unconfirmed');
  expect(h.queries).toHaveLength(TEST_POLICY.maxAttempts);
  expect(h.records[0]).toMatchObject({
    attempts: TEST_POLICY.maxAttempts,
    outcome: 'outcome-unknown',
    source: 'original-failure',
  });
});

test('a query that never answers is bounded by the attempt and then by the total budget', async () => {
  const h = harness(async () => new Promise<CommandOutcome>(() => {}));
  const startedAt = performance.now();
  const result = await h.confirmation.confirm(ENTRY, 'owner');
  const elapsed = performance.now() - startedAt;
  expect(result.kind).toBe('unconfirmed');
  expect(h.queries).toHaveLength(TEST_POLICY.maxAttempts);
  expect(h.records[0]).toMatchObject({
    outcome: 'query-failed',
    lastFailureCode: 'attempt-deadline',
  });
  // The one deadline is the bound: two attempts of 60 ms plus the retry pause do not add up to more
  // than the 200 ms total, and the record is measured on the same monotonic clock.
  expect(elapsed).toBeLessThan(TEST_POLICY.totalBudgetMs + 60);
  expect(h.confirmation.active).toBe(0);
});

test('the total budget is not re-initialised for each attempt', async () => {
  const policy: CommandOutcomeConfirmationPolicy = {
    attemptBudgetMs: 100,
    maxAttempts: 3,
    maxConcurrent: 4,
    retryDelayMs: 20,
    totalBudgetMs: 150,
  };
  const h = harness(async () => new Promise<CommandOutcome>(() => {}), policy);
  const startedAt = performance.now();
  const result = await h.confirmation.confirm(ENTRY, 'owner');
  const elapsed = performance.now() - startedAt;
  expect(result.kind).toBe('unconfirmed');
  // Per-attempt deadlines would have allowed 3 x 100 ms + 2 x 20 ms = 340 ms; the single budget
  // holds the whole confirmation to one deadline instead.
  expect(elapsed).toBeLessThan(300);
  expect(h.queries.length).toBeGreaterThanOrEqual(2);
  expect(h.queries.length).toBeLessThanOrEqual(3);
});

test('a failed query keeps the pending state and names the failure it saw', async () => {
  const overload: IpcErrorEnvelope = {
    category: 'transport',
    code: 'ipc.local_overload',
    correlation_id: COR,
    details: { not_sent_reason: 'control_capacity' },
    message: 'the control pool is full',
    retryable: false,
    terminal: 'failed',
  };
  const h = harness(async () => {
    throw rejectionWith(overload);
  });
  const result = await h.confirmation.confirm(ENTRY, 'owner');
  expect(result.kind).toBe('unconfirmed');
  expect(h.records[0]).toMatchObject({
    outcome: 'query-failed',
    lastFailureCode: 'ipc.local_overload',
  });
});

test('an answer that is not this operation’s is refused instead of adopted', async () => {
  const h = harness(async () => confirmedSucceeded({}, 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAW'));
  const result = await h.confirmation.confirm(ENTRY, 'owner');
  expect(result.kind).toBe('unconfirmed');
  expect(h.queries).toHaveLength(1);
  expect(h.records[0]).toMatchObject({ attempts: 1, outcome: 'verification-refused' });
});

test('a hanging confirmation does not stop another operation from completing', async () => {
  const releases: Array<() => void> = [];
  const h = harness(async (attempt) =>
    attempt === 1
      ? new Promise<CommandOutcome>((resolve) =>
          releases.push(() => resolve(confirmedSucceeded('hung'))),
        )
      : confirmedSucceeded('second'),
  );
  const hanging = h.confirmation.confirm(ENTRY, 'first');
  // The second operation's query answers at once and is confirmed while the first one is still
  // waiting: confirmations are per operation, not a serialized queue.
  const quick = await h.confirmation.confirm({ ...ENTRY, command: 'fs.write_file' }, 'second');
  expect(quick.kind).toBe('confirmed-success');
  expect(quick.kind === 'confirmed-success' ? quick.value : undefined).toBe('second');
  releases.shift()?.();
  const first = await hanging;
  expect(first.kind).toBe('confirmed-success');
  expect(first.kind === 'confirmed-success' ? first.value : undefined).toBe('hung');
});

test('no more than the declared number of confirmations run at once, and a slot is handed back', async () => {
  const policy: CommandOutcomeConfirmationPolicy = {
    attemptBudgetMs: 1_000,
    maxAttempts: 1,
    maxConcurrent: 2,
    retryDelayMs: 5,
    totalBudgetMs: 1_000,
  };
  const holders: Array<(outcome: CommandOutcome) => void> = [];
  // The first two queries hang (the two slot holders); the queued operation's query answers at once,
  // so what the assertions observe is the slot handoff and not a second timeout.
  const h = harness(
    async (attempt) =>
      attempt <= 2
        ? new Promise<CommandOutcome>((resolve) => holders.push(resolve))
        : confirmedSucceeded('queued'),
    policy,
  );
  const first = h.confirmation.confirm(ENTRY, 'a');
  const second = h.confirmation.confirm(ENTRY, 'b');
  let queued: 'pending' | 'settled' = 'pending';
  const third = h.confirmation.confirm(ENTRY, 'c').then((result) => {
    queued = 'settled';
    return result;
  });
  // The third operation is waiting for a slot; the two holders are the only ones running.
  await new Promise((resolve) => setTimeout(resolve, 5));
  expect(h.confirmation.active).toBe(policy.maxConcurrent);
  expect(queued).toBe('pending');
  holders[0]?.(confirmedSucceeded('holder'));
  // The freed slot is handed to the queued operation, which is confirmed on its own query.
  const thirdResult = await third;
  expect(thirdResult.kind).toBe('confirmed-success');
  expect(queued).toBe('settled');
  holders[1]?.(confirmedSucceeded('holder'));
  expect((await first).kind).toBe('confirmed-success');
  expect((await second).kind).toBe('confirmed-success');
  expect(h.confirmation.active).toBe(0);
});

test('a slot that cannot be taken inside this operation’s own budget is a bounded refusal', async () => {
  const policy: CommandOutcomeConfirmationPolicy = {
    attemptBudgetMs: 1_000,
    maxAttempts: 1,
    maxConcurrent: 1,
    retryDelayMs: 5,
    totalBudgetMs: 1_000,
  };
  // A controlled monotonic clock: every reading steps forward, so the queued operation's own
  // deadline is already spent when it asks for the slot. Without the step the wait would simply last
  // until the holder's deadline, which is the bound this asserts.
  let clock = 0;
  const records: ConfirmationRecord[] = [];
  const confirmation = new CommandOutcomeConfirmation<string>(
    {
      channel: () => 'original',
      now: () => {
        clock += 5_000;
        return clock;
      },
      onEvent: (record) => records.push(record),
      query: () => new Promise<CommandOutcome>(() => {}),
    },
    policy,
  );
  const holder = confirmation.confirm(ENTRY, 'holder');
  const refused = await confirmation.confirm(ENTRY, 'queued');
  expect(refused.kind).toBe('unconfirmed');
  const refusedRecord = records.find((record) => record.outcome === 'concurrency-refused');
  expect(refusedRecord).toMatchObject({ attempts: 0, source: 'original-failure' });
  expect(refused.kind === 'unconfirmed' ? refused.record : undefined).toEqual(refusedRecord);
  expect((await holder).kind).toBe('unconfirmed');
});
test('a query that answers after its attempt was abandoned cannot become an unhandled rejection', async () => {
  const unhandled: unknown[] = [];
  const listener = (reason: unknown): void => {
    unhandled.push(reason);
  };
  process.on('unhandledRejection', listener);
  const h = harness(
    async () =>
      new Promise<CommandOutcome>((_resolve, reject) => {
        setTimeout(() => reject(new Error('late failure')), TEST_POLICY.attemptBudgetMs * 3);
      }),
  );
  try {
    const result = await h.confirmation.confirm(ENTRY, 'owner');
    expect(result.kind).toBe('unconfirmed');
    await new Promise((resolve) => setTimeout(resolve, TEST_POLICY.attemptBudgetMs * 4));
    expect(unhandled).toEqual([]);
  } finally {
    process.off('unhandledRejection', listener);
  }
});

test('the shipped production policy states every bound once', () => {
  expect(COMMAND_OUTCOME_CONFIRMATION_POLICY).toEqual({
    attemptBudgetMs: 4_000,
    maxAttempts: 2,
    maxConcurrent: 4,
    retryDelayMs: 250,
    totalBudgetMs: 6_000,
  });
  // The single total budget must be able to hold the first attempt and a second, shorter one.
  expect(COMMAND_OUTCOME_CONFIRMATION_POLICY.attemptBudgetMs).toBeLessThan(
    COMMAND_OUTCOME_CONFIRMATION_POLICY.totalBudgetMs,
  );
});

test('the record carries identity and counts only', async () => {
  const h = harness(async () => confirmedSucceeded({ secret: 'must-not-be-logged' }));
  await h.confirmation.confirm(ENTRY, 'owner');
  const serialized = JSON.stringify(h.records);
  expect(serialized).not.toContain('must-not-be-logged');
  expect(serialized).not.toContain('secret');
});
