// @vitest-environment node
import { expect, test, vi } from 'vitest';
import type { CommandOutcome, IpcErrorEnvelope } from '@aura/contracts/generated';
import { COMMAND_OUTCOME_CONFIRMATION_POLICY } from './command-outcome-confirmation.js';
import { CoreTransport } from './core-transport.js';
import { fixture, pipeClosed, waitUntil } from './core-supervisor-tests/fixture.js';
import { isUnconfirmedCommandOutcome } from '../shared/ipc-request-failure.js';
import {
  TOOL_SWITCH,
  rejectionWith,
  civ,
  cor,
  writeTimedOut,
  confirmedSuccess,
  unknownOutcome,
  writeCalls,
  confirmationRecords,
  skipRecords,
} from './core-supervisor-tests/support.js';

vi.mock('node:child_process', () => ({ spawn: vi.fn() }));
vi.mock('node:fs/promises', () => ({ mkdir: vi.fn(), appendFile: vi.fn() }));
vi.mock('./core-transport.js', () => ({ CoreTransport: { connect: vi.fn() } }));

test('M2: an Agent command confirmed with a body that violates the contract is not a success', async () => {
  const f = await fixture();
  f.transport.request.mockImplementationOnce(async () => {
    throw rejectionWith(writeTimedOut(0));
  });
  // A confirmed success still has to satisfy the operation's OWN response contract: the generated
  // Agent validator runs on the confirmed value exactly as it runs on a live reply.
  f.transport.queryCommandOutcome.mockImplementationOnce(async () =>
    confirmedSuccess(0, { not: 'an AgentToolSettingsView' }),
  );
  await expect(
    f.supervisor.requestAgent('agent.tools.configure', {
      default_timeout_ms: 1_000,
      expected_revision: '3',
      fixed_tool_ids: [],
      max_concurrent_tool_calls: 1,
      max_dynamic_tool_schemas: 50,
      max_retries: 0,
    }),
  ).rejects.toThrow('Invalid Agent result contract');
  expect(writeCalls(f, 'agent.tools.configure')).toBe(1);
  expect(f.transport.queryCommandOutcome).toHaveBeenCalledTimes(1);
  f.supervisor.stop();
});

test('M2: unknown, a failed query and a local control overload keep the pending state', async () => {
  const f = await fixture();
  const overload: IpcErrorEnvelope = {
    category: 'transport',
    code: 'ipc.local_overload',
    correlation_id: cor(1),
    details: { not_sent_reason: 'control_capacity' },
    message: 'the control pool is full',
    retryable: false,
    terminal: 'failed',
  };
  const answers: Array<[string, number, () => Promise<CommandOutcome>, string]> = [
    [
      'an outcome that is not recorded',
      0,
      async (): Promise<CommandOutcome> => unknownOutcome(0),
      'outcome-unknown',
    ],
    [
      'a query the control pool refused',
      1,
      async (): Promise<CommandOutcome> => {
        throw rejectionWith(overload);
      },
      'query-failed',
    ],
  ];
  for (const [name, index, answer, expected] of answers) {
    const failure = rejectionWith(writeTimedOut(index));
    f.transport.request.mockImplementationOnce(async () => {
      throw failure;
    });
    f.transport.queryCommandOutcome.mockImplementation(answer);
    const error = await f.supervisor
      .requestP05('models.catalog.refresh', {})
      .catch((thrown) => thrown);
    // The ORIGINAL failure comes back untouched: the Renderer keeps the pending-confirmation state
    // it already knows how to show, and never reads this as "did not execute" or "safe to retry".
    expect(error, name).toBe(failure);
    expect(
      isUnconfirmedCommandOutcome((error as { envelope: IpcErrorEnvelope }).envelope),
      name,
    ).toBe(true);
    expect(confirmationRecords().at(-1), name).toMatchObject({
      outcome: expected,
      source: 'original-failure',
    });
    expect(writeCalls(f, 'models.catalog.refresh'), name).toBe(index + 1);
  }
  f.supervisor.stop();
});

test('M2: a pure read and a not_sent failure start no outcome query', async () => {
  const f = await fixture();
  const readFailure = rejectionWith(writeTimedOut(0));
  f.transport.request.mockImplementationOnce(async () => {
    throw readFailure;
  });
  // A read is a read: the transport minted an invocation id for it, but Core records no outcome for
  // a read_only route, so there is nothing an outcome query could answer.
  await expect(f.supervisor.requestP05('models.settings.get_snapshot', {})).rejects.toBe(
    readFailure,
  );
  expect(f.transport.queryCommandOutcome).not.toHaveBeenCalled();
  expect(skipRecords().at(-1)).toMatchObject({
    command: 'models.settings.get_snapshot',
    reason: 'route-not-a-command',
  });

  const notSent = rejectionWith({
    ...writeTimedOut(1),
    details: { not_sent_reason: 'send_failed', outcome_recovery: 'not_sent' },
  });
  f.transport.request.mockImplementationOnce(async () => {
    throw notSent;
  });
  await expect(
    f.supervisor.requestP03('session.rename', {
      expected_title: 'old',
      session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      title: 'renamed',
    }),
  ).rejects.toBe(notSent);
  expect(f.transport.queryCommandOutcome).not.toHaveBeenCalled();
  expect(skipRecords().at(-1)).toMatchObject({ command: 'session.rename', reason: 'not-sent' });
  expect(confirmationRecords()).toEqual([]);
  f.supervisor.stop();
});

test('M2: a cancellation control request does not trigger a confirmation', async () => {
  const f = await fixture();
  // The cancel route carries its own invocation id and its own query hint in production; nothing
  // about a control request may start a confirmation of itself.
  f.transport.cancel.mockImplementationOnce(async () => {
    throw rejectionWith(writeTimedOut(0));
  });
  const requestsBefore = f.transport.request.mock.calls.length;
  const error = await f.supervisor.cancel(cor(0)).catch((thrown) => thrown);
  expect(error).toBeInstanceOf(Error);
  expect(f.transport.cancel).toHaveBeenCalledWith(cor(0));
  expect(f.transport.queryCommandOutcome).not.toHaveBeenCalled();
  expect(confirmationRecords()).toEqual([]);
  // Cancelling wrote no command of its own: no new request reached the transport either.
  expect(f.transport.request.mock.calls.length).toBe(requestsBefore);
  f.supervisor.stop();
});

test('M2: one hanging outcome query does not stop another operation completing', async () => {
  const f = await fixture();
  const hanging = rejectionWith(writeTimedOut(0));
  const answered = rejectionWith(writeTimedOut(1));
  f.transport.request
    .mockImplementationOnce(async () => {
      throw hanging;
    })
    .mockImplementationOnce(async () => {
      throw answered;
    });
  let hangingSettled = false;
  f.transport.queryCommandOutcome.mockImplementation(
    async (commandInvocationId: string): Promise<CommandOutcome> => {
      if (commandInvocationId === civ(0)) return new Promise<never>(() => {});
      return confirmedSuccess(1, { applied: true });
    },
  );
  const first = f.supervisor.requestP05('models.catalog.refresh', {});
  const tracked = first.then(
    (value) => {
      hangingSettled = true;
      return value;
    },
    (error: unknown) => {
      hangingSettled = true;
      throw error;
    },
  );
  const startedAt = Date.now();
  // A second, independent operation is confirmed while the first operation's query is still hanging.
  await expect(f.supervisor.requestP07('tools.tool.set_enabled', TOOL_SWITCH)).resolves.toEqual({
    applied: true,
  });
  expect(Date.now() - startedAt).toBeLessThan(COMMAND_OUTCOME_CONFIRMATION_POLICY.totalBudgetMs);
  expect(hangingSettled).toBe(false);
  // And the hanging one is bounded by its OWN deadline, ending in the pending state.
  await expect(tracked).rejects.toBe(hanging);
  expect(confirmationRecords().at(-1)).toMatchObject({
    commandInvocationId: civ(0),
    outcome: 'query-failed',
    source: 'original-failure',
  });
  expect(writeCalls(f, 'models.catalog.refresh')).toBe(1);
  f.supervisor.stop();
});

test('M2: a late answer cannot be adopted by another operation', async () => {
  const f = await fixture();
  const failure = rejectionWith(writeTimedOut(0));
  const answer = Promise.withResolvers<CommandOutcome>();
  f.transport.request.mockImplementationOnce(async () => {
    throw failure;
  });
  f.transport.queryCommandOutcome.mockImplementationOnce(async () => answer.promise);
  const older = f.supervisor.requestP03('session.rename', {
    expected_title: 'old',
    session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    title: 'renamed',
  });
  const tracked = older.catch((error: unknown) => error);
  // A NEWER operation completes first and keeps its own result.
  f.push('core_health', 2);
  const newer = await f.supervisor.refreshSnapshot();
  expect(newer.payload).toMatchObject({ status_message: 'Ready 2' });
  // The older operation's query is finally answered - with a well-formed response addressed to the
  // NEWER request. It is not this operation's result, so it is refused and the pending state stays.
  answer.resolve({
    command_invocation_id: civ(0),
    outcome: 'confirmed',
    response: {
      id: cor(1),
      jsonrpc: '2.0',
      result: { terminal: 'succeeded', value: { stolen: true } },
    },
    terminal: 'succeeded',
  });
  await expect(tracked).resolves.toBe(failure);
  expect(confirmationRecords().at(-1)).toMatchObject({
    outcome: 'verification-refused',
    source: 'original-failure',
  });
  expect(f.supervisor.snapshot).toEqual(newer);
  f.supervisor.stop();
});

test('M2: across a reconnect the confirmation only queries, never resends the write', async () => {
  const f = await fixture();
  const reconnectedQueries = vi.fn(async () => confirmedSuccess(0, { changed: true }));
  const failure = rejectionWith({
    ...pipeClosed,
    correlation_id: cor(0),
    details: {
      command_invocation_id: civ(0),
      outcome_recovery: 'query_runtime.command_outcome_without_replay',
    },
  });
  const closesBefore = f.transport.close.mock.calls.length;
  let reconnectedConnects = 0;
  f.transport.request.mockImplementationOnce(async () => {
    // The channel that carried the write dies and the SAME Core process comes back on a NEW channel
    // before this caller is even handed its failure.
    vi.mocked(CoreTransport.connect).mockImplementationOnce(async () => {
      reconnectedConnects += 1;
      return {
        ...f.transport,
        queryCommandOutcome: reconnectedQueries,
      } as unknown as CoreTransport;
    });
    f.callbacks().onClosed(pipeClosed);
    await waitUntil(() => f.transport.close.mock.calls.length > closesBefore);
    throw failure;
  });

  await expect(f.supervisor.requestP05('models.catalog.refresh', {})).resolves.toEqual({
    changed: true,
  });
  // The reconnect really happened: a second connect was made and the old channel was retired.
  expect(reconnectedConnects).toBe(1);
  expect(f.transport.close.mock.calls.length).toBeGreaterThan(closesBefore);
  // The query went out on the reconnected channel, about the ORIGINAL invocation, and the write was
  // handed to the transport exactly once across the whole episode.
  expect(reconnectedQueries).toHaveBeenCalledTimes(1);
  expect(reconnectedQueries).toHaveBeenCalledWith(civ(0));
  expect(f.transport.queryCommandOutcome).not.toHaveBeenCalled();
  expect(writeCalls(f, 'models.catalog.refresh')).toBe(1);
  expect(confirmationRecords().at(-1)).toMatchObject({
    attempts: 1,
    outcome: 'confirmed-succeeded',
    queryChannel: 'reconnected',
    source: 'outcome-query',
  });
  f.supervisor.stop();
});

test('M2: after the Core process is replaced nothing is queried and nothing is inferred', async () => {
  const f = await fixture();
  const failure = rejectionWith(writeTimedOut(0));
  f.transport.request.mockImplementationOnce(async () => {
    // The Core process ends and a new generation starts while this caller is still waiting; the
    // ledger that could have answered for the write is gone with it.
    f.child.exitCode = 1;
    f.child.emit('exit', 1, null);
    await waitUntil(
      () => f.connectAttempts() === 2 && f.supervisor.snapshot.payload.state === 'ready',
    );
    throw failure;
  });
  await expect(f.supervisor.requestP05('models.catalog.refresh', {})).rejects.toBe(failure);
  expect(f.transport.queryCommandOutcome).not.toHaveBeenCalled();
  expect(writeCalls(f, 'models.catalog.refresh')).toBe(1);
  expect(confirmationRecords().at(-1)).toMatchObject({
    outcome: 'query-failed',
    source: 'original-failure',
  });
  f.supervisor.stop();
});
