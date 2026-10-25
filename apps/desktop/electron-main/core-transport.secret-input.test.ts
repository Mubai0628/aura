import { afterEach, describe, expect, it, vi } from 'vitest';
import { CoreRpcError, SECRET_INPUT_BINDING_BUDGET_MS } from './core-transport.js';
import { fixture, SECRET_FRAME_KIND } from './core-transport.fixture.js';
import { isUnconfirmedCommandOutcome } from '../shared/ipc-request-failure.js';
import { RETIRED_CAPACITY, UNRESOLVED_TERMINAL_CAPACITY } from './request-registry.js';

vi.mock('./main-pipe-bridge.js', () => ({ connectMainPipe: vi.fn() }));

afterEach(() => {
  vi.useRealTimers();
  vi.clearAllMocks();
});

describe('Core transport terminals', () => {
  it('does not abandon a submitted SecretInput binding with zeroed bytes and a healthy channel', async () => {
    // Independent counterexample 3 (transport-probes.test.ts): the control frame was held inside a
    // write barrier, the client timeout zeroed the secret, and the now-zeroed bytes then failed
    // encoding inside a catch that found the waiter already retired.
    vi.useFakeTimers();
    const f = await fixture();
    const secret = Buffer.from('synthetic-probe-value');
    try {
      f.holdWrites();
      const result = f.transport
        .requestWithSecretInput(
          'models.endpoint.configure_credential',
          {},
          'models_endpoint_configure_credential',
          secret,
          5,
        )
        .catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(5);
      expect(await result).toBeInstanceOf(CoreRpcError);
      f.releaseWrites();
      await vi.advanceTimersByTimeAsync(0);

      expect(secret.every((byte) => byte === 0)).toBe(true);
      expect(
        f.sent.some((message) => message.method === 'models.endpoint.configure_credential'),
      ).toBe(true);
      // Once the command control frame was submitted, either the atomic binding is finished or the
      // untrustworthy channel is retired. Here it is finished: the secret frame really is sent.
      expect(f.kinds).toContain(SECRET_FRAME_KIND);
      expect(f.onClosed).not.toHaveBeenCalled();
      expect(f.transport.requestStats().active).toBe(0);
      // Retained state is counters and identity only - never the synthetic secret.
      expect(JSON.stringify(f.transport.requestStats())).not.toContain('synthetic-probe-value');
    } finally {
      f.releaseWrites();
      f.transport.close();
      secret.fill(0);
    }
  });

  it('rejects invalid SecretInput bytes before the command frame is committed', async () => {
    const f = await fixture();
    const secret = Buffer.from([0x61, 0x0a, 0x62]);
    try {
      const result = await f.transport
        .requestWithSecretInput(
          'models.endpoint.configure_credential',
          {},
          'models_endpoint_configure_credential',
          secret,
          1_000,
        )
        .catch((error: unknown) => error);
      const failure = result as CoreRpcError;
      expect(failure).toBeInstanceOf(CoreRpcError);
      // Validation is NOT relaxed to make this pass: an invalid secret is a request-local rejection
      // with nothing on the wire, so no half-committed credential operation can exist.
      expect(failure.envelope).toMatchObject({
        code: 'ipc.request_not_sent',
        details: { outcome_recovery: 'not_sent' },
        terminal: 'failed',
      });
      expect(isUnconfirmedCommandOutcome(failure.envelope)).toBe(false);
      expect(
        f.sent.some((message) => message.method === 'models.endpoint.configure_credential'),
      ).toBe(false);
      expect(f.kinds).not.toContain(SECRET_FRAME_KIND);
      expect(f.onClosed).not.toHaveBeenCalled();
      expect(secret.every((byte) => byte === 0)).toBe(true);

      // An ordinary request still works on the untouched channel.
      const healthy = f.transport.request('agent.status', {}, 1, 1_000);
      await Promise.resolve();
      f.deliver({
        jsonrpc: '2.0',
        id: f.sent.at(-1)?.id,
        result: { terminal: 'succeeded', value: { state: 'completed' } },
      });
      await expect(healthy).resolves.toEqual({ state: 'completed' });
    } finally {
      secret.fill(0);
      f.transport.close();
    }
  });

  it('retires the channel when a committed SecretInput binding cannot complete in its budget', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    const secret = Buffer.from('synthetic-half-commit-value');
    try {
      // Only the SecretInput frame is held, so the command control frame really is delivered first.
      f.holdWrites(SECRET_FRAME_KIND);
      const result = f.transport
        .requestWithSecretInput(
          'models.endpoint.configure_credential',
          {},
          'models_endpoint_configure_credential',
          secret,
          60_000,
        )
        .catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(0);
      expect(
        f.sent.filter((message) => message.method === 'models.endpoint.configure_credential'),
      ).toHaveLength(1);
      expect(f.kinds).not.toContain(SECRET_FRAME_KIND);

      await vi.advanceTimersByTimeAsync(SECRET_INPUT_BINDING_BUDGET_MS + 1);
      const failure = (await result) as CoreRpcError;
      expect(failure).toBeInstanceOf(CoreRpcError);
      expect(failure.envelope.terminal).toBe('transport_lost');
      // The command frame WAS committed, so the outcome stays Unknown - never `not_sent`.
      expect(isUnconfirmedCommandOutcome(failure.envelope)).toBe(true);
      expect(f.onClosed).toHaveBeenCalledOnce();
      expect(f.pipe.close).toHaveBeenCalledOnce();
      // No zeroed secret was sent, no secret frame was written, and the command was not re-sent.
      expect(f.kinds).not.toContain(SECRET_FRAME_KIND);
      expect(
        f.sent.filter((message) => message.method === 'models.endpoint.configure_credential'),
      ).toHaveLength(1);
      expect(secret.every((byte) => byte === 0)).toBe(true);
    } finally {
      f.releaseWrites();
      f.transport.close();
      secret.fill(0);
    }
  }, 5_000);

  it('retires the channel when a begun SecretInput binding never completes', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    const secret = Buffer.from('synthetic-stuck-binding-value');
    try {
      // The command control frame itself never leaves the writer. Once the atomic binding has begun,
      // the send owner must complete it or terminate the channel: holding the secret copy (or quietly
      // abandoning the operation) is not an option.
      f.holdWrites();
      const result = f.transport
        .requestWithSecretInput(
          'models.endpoint.configure_credential',
          {},
          'models_endpoint_configure_credential',
          secret,
          60_000,
        )
        .catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(0);
      expect(
        f.sent.some((message) => message.method === 'models.endpoint.configure_credential'),
      ).toBe(false);

      await vi.advanceTimersByTimeAsync(SECRET_INPUT_BINDING_BUDGET_MS + 1);
      const failure = (await result) as CoreRpcError;
      expect(failure).toBeInstanceOf(CoreRpcError);
      expect(failure.envelope.terminal).toBe('transport_lost');
      // Submission had begun, so the outcome stays Unknown and never becomes `not_sent`.
      expect(isUnconfirmedCommandOutcome(failure.envelope)).toBe(true);
      expect(f.onClosed).toHaveBeenCalledOnce();
      expect(f.pipe.close).toHaveBeenCalledOnce();
      expect(f.kinds).not.toContain(SECRET_FRAME_KIND);
      expect(secret.every((byte) => byte === 0)).toBe(true);
    } finally {
      f.releaseWrites();
      f.transport.close();
      secret.fill(0);
    }
  }, 5_000);

  it('leaves another channel untouched when one channel is retired', async () => {
    const failing = await fixture();
    const healthy = await fixture();
    try {
      failing.fail();
      const result = await failing.transport
        .request('workspace.create', {}, 1, 500)
        .catch((error: unknown) => error);
      expect(result).toBeInstanceOf(CoreRpcError);
      expect(failing.onClosed).toHaveBeenCalledOnce();
      expect(healthy.onClosed).not.toHaveBeenCalled();

      // Retirement is scoped to the channel whose writer broke: the other channel still carries work.
      const answer = healthy.transport.request('agent.status', {}, 1, 1_000);
      await Promise.resolve();
      healthy.deliver({
        jsonrpc: '2.0',
        id: healthy.sent.at(-1)?.id,
        result: { terminal: 'succeeded', value: { state: 'completed' } },
      });
      await expect(answer).resolves.toEqual({ state: 'completed' });
    } finally {
      failing.transport.close();
      healthy.transport.close();
    }
  });

  it('reports a timeout before the frame reached the writer as not sent', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      f.holdWrites();
      const holder = f.transport.request('workspace.create', {}, 1, 60).catch(() => undefined);
      await vi.advanceTimersByTimeAsync(0);
      const gone = f.transport.request('agent.status', {}, 1, 5).catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(5);
      const failure = (await gone) as CoreRpcError;
      // The client gave up before ANY byte was written, so the outcome is definite and must not be
      // reported as "possibly executed, query the outcome".
      expect(failure.envelope).toMatchObject({
        code: 'ipc.request_timeout',
        details: { outcome_recovery: 'not_sent' },
        terminal: 'cancelled',
      });
      expect(isUnconfirmedCommandOutcome(failure.envelope)).toBe(false);

      f.releaseWrites();
      await vi.advanceTimersByTimeAsync(60);
      await holder;
      expect(f.transport.requestStats().withdrawn).toBe(1);
      expect(f.sent.some((message) => message.method === 'agent.status')).toBe(false);
      expect(f.onClosed).not.toHaveBeenCalled();

      // The channel is untouched and still carries ordinary work.
      const healthy = f.transport.request('agent.status', {}, 1, 1_000);
      await vi.advanceTimersByTimeAsync(0);
      f.deliver({
        jsonrpc: '2.0',
        id: f.sent.at(-1)?.id,
        result: { terminal: 'succeeded', value: { state: 'completed' } },
      });
      await expect(healthy).resolves.toEqual({ state: 'completed' });
    } finally {
      f.transport.close();
    }
  }, 5_000);

  it('treats a terminal for a request that was never written as a protocol violation', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      f.holdWrites();
      const holder = f.transport.request('workspace.create', {}, 1, 60).catch(() => undefined);
      await vi.advanceTimersByTimeAsync(0);
      const gone = f.transport.request('agent.status', {}, 1, 5).catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(5);
      const failure = (await gone) as CoreRpcError;
      const id = failure.envelope.correlation_id;
      f.releaseWrites();
      await vi.advanceTimersByTimeAsync(60);
      await holder;
      expect(f.transport.requestStats().withdrawn).toBe(1);

      // No byte of that id ever reached the writer, so this terminal has no legitimate source: it is
      // the strict safe path, not a legal late answer, and the reason stays distinguishable.
      f.deliver({ jsonrpc: '2.0', id, result: { terminal: 'succeeded', value: {} } });
      await vi.advanceTimersByTimeAsync(0);
      expect(f.onClosed).toHaveBeenCalledOnce();
      expect(f.onClosed).toHaveBeenCalledWith(
        expect.objectContaining({
          code: 'ipc.protocol_violation',
          message: expect.stringContaining('never submitted'),
        }),
      );
      expect(f.transport.requestStats().late).toBe(0);
      expect(f.transport.requestStats().unsentTerminals).toBe(1);
    } finally {
      f.transport.close();
    }
  }, 5_000);

  it('keeps the outcome unknown for a timeout while the frame is being written', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      f.holdWrites();
      const gone = f.transport.request('agent.status', {}, 1, 5).catch((error: unknown) => error);
      // The frame has been handed to the writer, so its bytes may already be partly out: the outcome
      // stays Unknown even though the caller gave up.
      await vi.advanceTimersByTimeAsync(5);
      const failure = (await gone) as CoreRpcError;
      expect(failure.envelope).toMatchObject({
        code: 'ipc.request_timeout',
        details: { outcome_recovery: 'query_runtime.command_outcome_without_replay' },
      });
      expect(isUnconfirmedCommandOutcome(failure.envelope)).toBe(true);

      f.releaseWrites();
      await vi.advanceTimersByTimeAsync(0);
      expect(f.sent.some((message) => message.method === 'agent.status')).toBe(true);
      // A request that HAD begun writing is never claimed as `not_sent`; its real terminal is the one
      // legal late answer.
      f.deliver({
        jsonrpc: '2.0',
        id: failure.envelope.correlation_id,
        result: { terminal: 'succeeded', value: {} },
      });
      await vi.advanceTimersByTimeAsync(0);
      expect(f.onClosed).not.toHaveBeenCalled();
      expect(f.transport.requestStats().late).toBe(1);
    } finally {
      f.transport.close();
    }
  });

  it('keeps the outcome unknown for a timeout after the frame was written', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      const gone = f.transport.request('agent.status', {}, 1, 5).catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(0);
      expect(f.sent.some((message) => message.method === 'agent.status')).toBe(true);
      await vi.advanceTimersByTimeAsync(5);
      const failure = (await gone) as CoreRpcError;
      expect(failure.envelope.details.outcome_recovery).toBe(
        'query_runtime.command_outcome_without_replay',
      );
      f.deliver({
        jsonrpc: '2.0',
        id: failure.envelope.correlation_id,
        result: { terminal: 'succeeded', value: {} },
      });
      await vi.advanceTimersByTimeAsync(0);
      expect(f.onClosed).not.toHaveBeenCalled();
      expect(f.transport.requestStats().late).toBe(1);
    } finally {
      f.transport.close();
    }
  });

  it('keeps serving ordinary requests after a legal late terminal was consumed', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      const gone = f.transport.request('agent.status', {}, 1, 5).catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(5);
      const failure = (await gone) as CoreRpcError;
      f.deliver({
        jsonrpc: '2.0',
        id: failure.envelope.correlation_id,
        result: { terminal: 'succeeded', value: {} },
      });
      await vi.advanceTimersByTimeAsync(0);
      expect(f.onClosed).not.toHaveBeenCalled();

      const healthy = f.transport.request('workspace.create', {}, 1, 1_000);
      await vi.advanceTimersByTimeAsync(0);
      f.deliver({
        jsonrpc: '2.0',
        id: f.sent.at(-1)?.id,
        result: { terminal: 'succeeded', value: { created: true } },
      });
      await expect(healthy).resolves.toEqual({ created: true });
      expect(f.onClosed).not.toHaveBeenCalled();
    } finally {
      f.transport.close();
    }
  });

  it('keeps a legitimate first late response when a burst fills the completed history', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      // The client gives up on a submitted request whose real terminal has not arrived yet.
      const timedOut = f.transport
        .request('agent.status', {}, 1, 5)
        .catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(5);
      const failure = (await timedOut) as CoreRpcError;
      expect(failure.envelope.code).toBe('ipc.request_timeout');
      const awaitedId = failure.envelope.correlation_id;

      // A burst of fast, normally answered calls: exactly the traffic that used to be able to evict a
      // timed-out id out of a 256-entry store before its response arrived.
      for (let index = 0; index < RETIRED_CAPACITY + 4; index += 1) {
        const answered = f.transport.request('workspace.create', {}, 1, 1_000);
        await vi.advanceTimersByTimeAsync(0);
        f.deliver({
          jsonrpc: '2.0',
          id: f.sent.at(-1)?.id,
          result: { terminal: 'succeeded', value: { index } },
        });
        await answered;
        await vi.advanceTimersByTimeAsync(0);
      }
      expect(f.transport.requestStats().evicted).toBeGreaterThan(0);

      // The real terminal for the timed-out request is still the ONE legal late answer, and it is not
      // disguised as a hostile unknown response.
      f.deliver({
        jsonrpc: '2.0',
        id: awaitedId,
        result: { terminal: 'succeeded', value: { state: 'completed' } },
      });
      await vi.advanceTimersByTimeAsync(0);
      expect(f.onClosed).not.toHaveBeenCalled();
      expect(f.transport.requestStats().late).toBe(1);
      expect(f.transport.requestStats().unresolved).toBe(0);
    } finally {
      f.transport.close();
    }
  }, 5_000);

  it('refuses new work locally rather than evicting an outstanding terminal identity', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      const pending: Promise<unknown>[] = [];
      for (let index = 0; index < UNRESOLVED_TERMINAL_CAPACITY; index += 1) {
        pending.push(
          f.transport.request('agent.status', {}, 1, 5).catch((error: unknown) => error),
        );
      }
      // Let every queued frame reach the writer first, then let every caller give up.
      await vi.advanceTimersByTimeAsync(0);
      expect(f.transport.requestStats().active).toBe(UNRESOLVED_TERMINAL_CAPACITY);
      await vi.advanceTimersByTimeAsync(5);
      const failures = await Promise.all(pending);
      expect(f.transport.requestStats().unresolved).toBe(UNRESOLVED_TERMINAL_CAPACITY);
      expect(f.onClosed).not.toHaveBeenCalled();

      // Bounded admission: the channel refuses new work locally instead of forgetting an id whose
      // real terminal is still outstanding.
      const refused = (await f.transport
        .request('workspace.create', {}, 1, 5)
        .catch((error: unknown) => error)) as CoreRpcError;
      expect(refused).toBeInstanceOf(CoreRpcError);
      expect(refused.envelope).toMatchObject({
        code: 'ipc.local_overload',
        details: { outcome_recovery: 'not_sent' },
        terminal: 'failed',
      });
      expect(isUnconfirmedCommandOutcome(refused.envelope)).toBe(false);
      expect(f.sent.some((message) => message.method === 'workspace.create')).toBe(false);
      expect(f.onClosed).not.toHaveBeenCalled();
      expect(f.transport.requestStats().admissionRefusals).toBe(1);

      // The identity of the FIRST give-up is intact: its real terminal is still the one legal late
      // answer rather than an unknown response.
      const firstId = (failures[0] as CoreRpcError).envelope.correlation_id;
      f.deliver({
        jsonrpc: '2.0',
        id: firstId,
        result: { terminal: 'succeeded', value: { state: 'completed' } },
      });
      await vi.advanceTimersByTimeAsync(0);
      expect(f.onClosed).not.toHaveBeenCalled();
      expect(f.transport.requestStats().late).toBe(1);

      // One real terminal frees one identity slot, and the channel takes work again.
      const healthy = f.transport.request('agent.status', {}, 1, 1_000);
      await vi.advanceTimersByTimeAsync(0);
      f.deliver({
        jsonrpc: '2.0',
        id: f.sent.at(-1)?.id,
        result: { terminal: 'succeeded', value: { state: 'completed' } },
      });
      await expect(healthy).resolves.toEqual({ state: 'completed' });
    } finally {
      f.transport.close();
    }
  }, 5_000);
});
