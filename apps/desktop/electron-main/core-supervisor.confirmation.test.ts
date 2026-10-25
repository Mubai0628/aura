// @vitest-environment node
import { expect, test, vi } from 'vitest';
import { appendFile } from 'node:fs/promises';
import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import {
  COMMAND_OUTCOME_CONFIRMATION_POLICY,
  CommandOutcomeConfirmation,
} from './command-outcome-confirmation.js';
import { fixture } from './core-supervisor-tests/fixture.js';
import {
  isRequestLocalTimeout,
  isUnconfirmedCommandOutcome,
} from '../shared/ipc-request-failure.js';
import {
  TOOL_SWITCH,
  rejectionWith,
  civ,
  cor,
  writeTimedOut,
  confirmedSuccess,
  confirmedFailure,
  unknownOutcome,
  writeCalls,
  confirmationRecords,
} from './core-supervisor-tests/support.js';

vi.mock('node:child_process', () => ({ spawn: vi.fn() }));
vi.mock('node:fs/promises', () => ({ mkdir: vi.fn(), appendFile: vi.fn() }));
vi.mock('./core-transport.js', () => ({ CoreTransport: { connect: vi.fn() } }));

test('the caller-budget seam confirms the same timed-out write through the product policy', async () => {
  const f = await fixture();
  f.transport.request.mockImplementationOnce(async () => {
    throw rejectionWith(writeTimedOut(0));
  });
  f.transport.queryCommandOutcome.mockImplementationOnce(async () =>
    confirmedSuccess(0, { written: true }),
  );

  await expect(
    f.supervisor.requestWithBudget('fs.write_file', { path: 'a.txt' }, 100),
  ).resolves.toEqual({ written: true });
  expect(f.transport.request).toHaveBeenCalledWith('fs.write_file', { path: 'a.txt' }, 1, 100);
  expect(writeCalls(f, 'fs.write_file')).toBe(1);
  expect(f.transport.queryCommandOutcome).toHaveBeenCalledExactlyOnceWith(civ(0));
  expect(confirmationRecords()).toEqual([
    expect.objectContaining({
      command: 'fs.write_file',
      commandInvocationId: civ(0),
      correlationId: cor(0),
      outcome: 'confirmed-succeeded',
      source: 'outcome-query',
    }),
  ]);
  f.supervisor.stop();
});

test('the caller-budget seam keeps an unconfirmed write unknown and never resends it', async () => {
  const f = await fixture();
  const failure = rejectionWith(writeTimedOut(0));
  f.transport.request.mockImplementationOnce(async () => {
    throw failure;
  });
  f.transport.queryCommandOutcome.mockImplementationOnce(async () => unknownOutcome(0));

  await expect(
    f.supervisor.requestWithBudget('fs.write_file', { path: 'a.txt' }, 100),
  ).rejects.toBe(failure);
  expect(writeCalls(f, 'fs.write_file')).toBe(1);
  expect(f.transport.queryCommandOutcome).toHaveBeenCalledTimes(
    COMMAND_OUTCOME_CONFIRMATION_POLICY.maxAttempts,
  );
  expect(
    f.transport.queryCommandOutcome.mock.calls.every(([invocation]) => invocation === civ(0)),
  ).toBe(true);
  expect(f.transport.cancel).not.toHaveBeenCalled();
  expect(confirmationRecords()).toEqual([
    expect.objectContaining({
      command: 'fs.write_file',
      commandInvocationId: civ(0),
      outcome: 'outcome-unknown',
    }),
  ]);
  f.supervisor.stop();
});

test('M2: a submitted write that times out is confirmed succeeded and updates the existing state', async () => {
  const f = await fixture();
  const failure = rejectionWith(writeTimedOut(0));
  f.transport.request.mockImplementationOnce(async () => {
    throw failure;
  });
  f.transport.queryCommandOutcome.mockImplementationOnce(async () =>
    confirmedSuccess(0, { changed: true, revision: '7' }),
  );

  // The caller's success state, reached through the command's own normal entry point.
  await expect(f.supervisor.requestP05('models.catalog.refresh', {})).resolves.toEqual({
    changed: true,
    revision: '7',
  });

  // The original write was invoked exactly once, the query asked about the ORIGINAL invocation, and
  // nothing was cancelled, closed or resent to learn the result.
  expect(writeCalls(f, 'models.catalog.refresh')).toBe(1);
  expect(f.transport.queryCommandOutcome).toHaveBeenCalledTimes(1);
  expect(f.transport.queryCommandOutcome).toHaveBeenCalledWith(civ(0));
  expect(f.transport.cancel).not.toHaveBeenCalled();
  expect(f.transport.close).not.toHaveBeenCalled();
  expect(f.child.kill).not.toHaveBeenCalled();
  expect(confirmationRecords()).toEqual([
    expect.objectContaining({
      attempts: 1,
      budgetMs: COMMAND_OUTCOME_CONFIRMATION_POLICY.totalBudgetMs,
      command: 'models.catalog.refresh',
      commandInvocationId: civ(0),
      correlationId: cor(0),
      outcome: 'confirmed-succeeded',
      queryChannel: 'original',
      source: 'outcome-query',
    }),
  ]);
  f.supervisor.stop();
});

test('M2: a confirmed failure and a confirmed cancellation keep the real semantics', async () => {
  const f = await fixture();
  const realFailure: IpcErrorEnvelope = {
    category: 'conflict',
    code: 'tool.draft.idempotency_conflict',
    correlation_id: cor(0),
    details: { side_effect: 'the draft had already been replaced by this invocation' },
    message: 'This CommandInvocationId already completed.',
    retryable: false,
    terminal: 'failed',
  };
  f.transport.request.mockImplementationOnce(async () => {
    throw rejectionWith(writeTimedOut(0, 'failed'));
  });
  f.transport.queryCommandOutcome.mockImplementationOnce(async () =>
    confirmedFailure(0, realFailure),
  );
  const failed = await f.supervisor
    .requestP07('tools.tool.set_enabled', TOOL_SWITCH)
    .catch((error: unknown) => error);
  const failedEnvelope = (failed as { envelope: IpcErrorEnvelope }).envelope;
  // The Core's own error, with its code, message, terminal and side-effect description.
  expect(failedEnvelope).toEqual(realFailure);
  // A confirmed terminal is definite, so the Renderer must NOT keep showing it as pending
  // confirmation: "not confirmed" and "cancelled" are different answers.
  expect(isUnconfirmedCommandOutcome(failedEnvelope)).toBe(false);
  expect(isRequestLocalTimeout(failedEnvelope)).toBe(false);
  expect(confirmationRecords().at(-1)).toMatchObject({
    outcome: 'confirmed-failed',
    source: 'outcome-query',
  });

  const cancelled: IpcErrorEnvelope = {
    ...realFailure,
    category: 'cancelled',
    code: 'ipc.request_cancelled',
    correlation_id: cor(1),
    message: 'The command was cancelled before it ran.',
    terminal: 'cancelled',
  };
  f.transport.request.mockImplementationOnce(async () => {
    throw rejectionWith(writeTimedOut(1));
  });
  f.transport.queryCommandOutcome.mockImplementationOnce(async () =>
    confirmedFailure(1, cancelled),
  );
  const cancellation = await f.supervisor
    .requestAgent('agent.submit', {
      destination: { lane: 'next_turn' },
      session_id: 'ses_x',
      text: 'hi',
    })
    .catch((error: unknown) => error);
  expect((cancellation as { envelope: IpcErrorEnvelope }).envelope).toEqual(cancelled);
  expect(isUnconfirmedCommandOutcome(cancelled)).toBe(false);
  expect(confirmationRecords().at(-1)).toMatchObject({
    command: 'agent.submit',
    outcome: 'confirmed-cancelled',
  });
  expect(writeCalls(f, 'tools.tool.set_enabled')).toBe(1);
  expect(writeCalls(f, 'agent.submit')).toBe(1);
  f.supervisor.stop();
});

test('a confirmation owner failure preserves the original failure and other operations', async () => {
  const f = await fixture();
  const original = rejectionWith(writeTimedOut(0));
  f.transport.request.mockRejectedValueOnce(original);
  const ownerFailure = vi
    .spyOn(CommandOutcomeConfirmation.prototype, 'confirm')
    .mockRejectedValueOnce(
      Object.assign(new Error('PRIVATE_RESPONSE_SENTINEL'), {
        code: 'ERR_CONFIRMATION_OWNER',
      }),
    );
  try {
    await expect(f.supervisor.requestP07('tools.tool.set_enabled', TOOL_SWITCH)).rejects.toBe(
      original,
    );
    expect(f.transport.queryCommandOutcome).not.toHaveBeenCalled();
    await expect(f.supervisor.requestWithBudget('fixture.echo', {}, 100)).resolves.toMatchObject({
      method: 'fixture.echo',
    });
    const events = vi.mocked(appendFile).mock.calls.flatMap(([, bytes]) =>
      typeof bytes === 'string'
        ? bytes
            .split('\n')
            .filter(Boolean)
            .map((line) => JSON.parse(line))
        : [],
    );
    const failure = events.find((event) => event.event === 'command_outcome_confirmation_failed');
    expect(failure?.fields).toEqual({
      command: 'tools.tool.set_enabled',
      code: 'ERR_CONFIRMATION_OWNER',
    });
    expect(JSON.stringify(events)).not.toContain('PRIVATE_RESPONSE_SENTINEL');
    expect(writeCalls(f, 'tools.tool.set_enabled')).toBe(1);
  } finally {
    ownerFailure.mockRestore();
    f.supervisor.stop();
  }
});

test('M2: every covered surface confirms its own write and never invokes it twice', async () => {
  const f = await fixture();
  const agentToolSettings = {
    default_timeout_ms: 1_000,
    fixed_tool_ids: [],
    max_concurrent_tool_calls: 1,
    max_dynamic_tool_schemas: 50,
    max_retries: 0,
    revision: '3',
    tools: [],
  };
  // Each entry is one operation started at the surface's OWN entry point, with the parameters the
  // command really takes.
  const cases: Array<[string, string, unknown, () => Promise<unknown>]> = [
    [
      'P03',
      'session.rename',
      { title: 'renamed' },
      () =>
        f.supervisor.requestP03('session.rename', {
          expected_title: 'old',
          session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
          title: 'renamed',
        }),
    ],
    [
      'P04',
      'permission.set_session_mode',
      { mode: 'workspace_read_write' },
      () =>
        f.supervisor.requestP04('permission.set_session_mode', {
          expected_revision: 1,
          mode: 'workspace_read_write',
          session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        }),
    ],
    [
      'P05',
      'models.catalog.refresh',
      { refreshed: true },
      () => f.supervisor.requestP05('models.catalog.refresh', {}),
    ],
    [
      'P07',
      'tools.tool.set_enabled',
      { draft: true },
      () => f.supervisor.requestP07('tools.tool.set_enabled', TOOL_SWITCH),
    ],
    [
      'Agent',
      'agent.tools.configure',
      agentToolSettings,
      () =>
        f.supervisor.requestAgent('agent.tools.configure', {
          default_timeout_ms: 1_000,
          expected_revision: '3',
          fixed_tool_ids: ['read'],
          max_concurrent_tool_calls: 1,
          max_dynamic_tool_schemas: 50,
          max_retries: 0,
        }),
    ],
  ];
  let index = 0;
  for (const [surface, command, value, run] of cases) {
    const failure = rejectionWith(writeTimedOut(index));
    f.transport.request.mockImplementationOnce(async () => {
      throw failure;
    });
    f.transport.queryCommandOutcome.mockImplementationOnce(async () =>
      confirmedSuccess(index, value),
    );
    await expect(run(), surface).resolves.toEqual(value);
    expect(writeCalls(f, command), surface).toBe(1);
    expect(f.transport.queryCommandOutcome, surface).toHaveBeenLastCalledWith(civ(index));
    index += 1;
  }
  expect(confirmationRecords().map((record) => record.command)).toEqual(
    cases.map(([, command]) => command),
  );
  // Every covered write asked about its OWN invocation, and no operation borrowed another's answer.
  expect(confirmationRecords().map((record) => record.commandInvocationId)).toEqual(
    cases.map((_, position) => civ(position)),
  );
  f.supervisor.stop();
});

test('M2: a confirmed credential write keeps the secret out of the result query', async () => {
  const f = await fixture();
  const secret = Buffer.from('sk-m2-must-never-be-logged', 'utf8');
  const failure = rejectionWith(writeTimedOut(0));
  f.transport.requestWithSecretInput.mockImplementationOnce(
    async (
      _method: string,
      _params: unknown,
      _operation: unknown,
      bytes: Buffer,
    ): Promise<unknown> => {
      bytes.fill(0);
      throw failure;
    },
  );
  f.transport.queryCommandOutcome.mockImplementationOnce(async () =>
    confirmedSuccess(0, { changed: true, credential_status: 'configured' }),
  );

  await expect(
    f.supervisor.requestP05WithSecretInput(
      'models.endpoint.configure_credential',
      {
        credential_id: 'cred_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        credential_slot: 'api-key',
        endpoint_id: 'ep_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        expected_models_revision: '4',
        expected_row_revision: '4',
      },
      secret,
    ),
  ).resolves.toEqual({ changed: true, credential_status: 'configured' });

  // The credential command itself went out exactly once, the confirmation asked only about the
  // invocation, and the caller's bytes are not retained for any retry.
  expect(f.transport.requestWithSecretInput).toHaveBeenCalledTimes(1);
  expect(f.transport.queryCommandOutcome).toHaveBeenCalledTimes(1);
  expect(f.transport.queryCommandOutcome).toHaveBeenCalledWith(civ(0));
  expect([...secret].every((byte) => byte === 0)).toBe(true);
  expect(JSON.stringify(confirmationRecords())).not.toContain('sk-m2-must-never-be-logged');
  expect(confirmationRecords().at(-1)).toMatchObject({
    command: 'models.endpoint.configure_credential',
    outcome: 'confirmed-succeeded',
    attempts: 1,
    source: 'outcome-query',
  });
  f.supervisor.stop();
});
