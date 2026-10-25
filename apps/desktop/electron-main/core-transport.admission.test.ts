import { afterEach, describe, expect, it, vi } from 'vitest';
import { CoreRpcError } from './core-transport.js';
import { idFromBytes } from './ids.js';
import { fixture, binding } from './core-transport.fixture.js';
import { isUnconfirmedCommandOutcome } from '../shared/ipc-request-failure.js';
import {
  CONTROL_UNRESOLVED_TERMINAL_CAPACITY,
  MAX_ACTIVE_REQUESTS,
  MAX_CONTROL_REQUESTS,
  UNRESOLVED_TERMINAL_CAPACITY,
} from './request-registry.js';

vi.mock('./main-pipe-bridge.js', () => ({ connectMainPipe: vi.fn() }));

afterEach(() => {
  vi.useRealTimers();
  vi.clearAllMocks();
});

/**
 * Control admission on the production transport.
 *
 * The defect these cases pin down: `begin` applied ONE active/unresolved admission to every request,
 * so 256 real outbound business requests that then timed out locally left 256 unresolved identities
 * and `cancel` / `queryCommandOutcome` were refused LOCALLY - no `aura.cancel` and no
 * `runtime.command_outcome` frame ever reached the sink. Keeping unresolved identity is correct;
 * letting it block the entry point that resolves it is not. Cancel and outcome query now have their own
 * bounded pools, and the ordinary pool stays exactly as strict as it was.
 */
describe('Core transport control admission', () => {
  /** One real outbound business request per call, all submitted, then all given up on locally. */
  async function fillBusinessIdentityPool(
    f: Awaited<ReturnType<typeof fixture>>,
  ): Promise<unknown[]> {
    const business = Array.from({ length: UNRESOLVED_TERMINAL_CAPACITY }, () =>
      f.transport.request('agent.status', {}, 1, 5).catch((error: unknown) => error),
    );
    // Every frame really reached the writer before any caller gave up, then every caller gave up.
    await vi.advanceTimersByTimeAsync(0);
    expect(f.transport.requestStats().active).toBe(UNRESOLVED_TERMINAL_CAPACITY);
    await vi.advanceTimersByTimeAsync(5);
    return Promise.all(business);
  }

  it('admits cancel and an outcome query while business identity capacity is full', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      const failures = await fillBusinessIdentityPool(f);
      expect(failures.every((failure) => failure instanceof CoreRpcError)).toBe(true);
      expect(f.transport.requestStats()).toMatchObject({
        active: 0,
        admissionRefusals: 0,
        unresolved: UNRESOLVED_TERMINAL_CAPACITY,
        unresolvedControl: 0,
      });
      expect(f.onClosed).not.toHaveBeenCalled();

      // Ordinary business work is refused locally, and nothing was written for it.
      const refused = (await f.transport
        .request('workspace.create', {}, 1, 5)
        .catch((error: unknown) => error)) as CoreRpcError;
      expect(refused.envelope).toMatchObject({
        code: 'ipc.local_overload',
        details: { not_sent_reason: 'unresolved-capacity', outcome_recovery: 'not_sent' },
        terminal: 'failed',
      });
      expect(f.sent.some((message) => message.method === 'workspace.create')).toBe(false);

      // The control entry points are NOT refused by that capacity: both frames reach the sink.
      const cancel = f.transport.cancel(idFromBytes('cor', new Uint8Array(16)));
      const query = f.transport.queryCommandOutcome(idFromBytes('civ', new Uint8Array(16)));
      await vi.advanceTimersByTimeAsync(0);
      const control = f.sent.filter(
        (message) =>
          message.method === 'aura.cancel' || message.method === 'runtime.command_outcome',
      );
      expect(control.map((message) => message.method)).toEqual([
        'aura.cancel',
        'runtime.command_outcome',
      ]);
      expect(f.transport.requestStats()).toMatchObject({ active: 2, activeControl: 2 });
      expect(f.transport.requestStats().controlAdmissionRefusals).toBe(0);

      // ... and each of them receives its OWN simulated terminal.
      f.deliver({
        jsonrpc: '2.0',
        id: control[0]?.id,
        result: {
          terminal: 'succeeded',
          value: { disposition: 'accepted', state: 'cancellation_requested' },
        },
      });
      await vi.advanceTimersByTimeAsync(0);
      f.deliver({
        jsonrpc: '2.0',
        id: control[1]?.id,
        result: {
          terminal: 'succeeded',
          value: { terminal: 'succeeded', value: { replayed: false } },
        },
      });
      await expect(cancel).resolves.toMatchObject({ disposition: 'accepted' });
      await expect(query).resolves.toMatchObject({ value: { replayed: false } });
      expect(f.onClosed).not.toHaveBeenCalled();
      // Both control identities were settled by their real terminal; the business backlog is untouched
      // and no ordinary identity was evicted to make this work.
      expect(f.transport.requestStats()).toMatchObject({
        active: 0,
        activeControl: 0,
        late: 0,
        unresolved: UNRESOLVED_TERMINAL_CAPACITY,
        unresolvedControl: 0,
      });

      // The business pool is still full, so ordinary work is STILL refused after the controls ran.
      const stillRefused = (await f.transport
        .request('workspace.create', {}, 1, 5)
        .catch((error: unknown) => error)) as CoreRpcError;
      expect(stillRefused.envelope.details.not_sent_reason).toBe('unresolved-capacity');
    } finally {
      f.transport.close();
    }
  }, 5_000);

  it('admits cancel and an outcome query while every ordinary active slot is taken', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      // Admitted and written, and none of them settles: the ACTIVE pool is what is saturated here.
      void Array.from({ length: MAX_ACTIVE_REQUESTS }, () =>
        f.transport.request('agent.status', {}, 1, 60_000).catch((error: unknown) => error),
      );
      await vi.advanceTimersByTimeAsync(0);
      expect(f.transport.requestStats()).toMatchObject({
        active: MAX_ACTIVE_REQUESTS,
        activeControl: 0,
      });
      const refused = (await f.transport
        .request('workspace.create', {}, 1, 5)
        .catch((error: unknown) => error)) as CoreRpcError;
      expect(refused.envelope.details.not_sent_reason).toBe('active-capacity');

      const before = f.sent.length;
      const cancel = f.transport.cancel(idFromBytes('cor', new Uint8Array(16)));
      const query = f.transport.queryCommandOutcome(idFromBytes('civ', new Uint8Array(16)));
      await vi.advanceTimersByTimeAsync(0);
      expect(f.sent.slice(before).map((message) => message.method)).toEqual([
        'aura.cancel',
        'runtime.command_outcome',
      ]);
      expect(f.transport.requestStats()).toMatchObject({
        active: MAX_ACTIVE_REQUESTS + 2,
        activeControl: 2,
      });

      for (const frame of f.sent.slice(before)) {
        f.deliver({
          jsonrpc: '2.0',
          id: frame.id,
          result: { terminal: 'succeeded', value: { ok: true } },
        });
        await vi.advanceTimersByTimeAsync(0);
      }
      await expect(cancel).resolves.toEqual({ ok: true });
      await expect(query).resolves.toEqual({ ok: true });
      expect(f.transport.requestStats()).toMatchObject({
        active: MAX_ACTIVE_REQUESTS,
        activeControl: 0,
      });
    } finally {
      f.transport.close();
    }
  }, 5_000);

  it('refuses control work locally once its own in-flight pool is full', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      void Array.from({ length: MAX_CONTROL_REQUESTS }, () =>
        f.transport.cancel(idFromBytes('cor', new Uint8Array(16))).catch((error: unknown) => error),
      );
      await vi.advanceTimersByTimeAsync(0);
      expect(f.transport.requestStats()).toMatchObject({
        active: MAX_CONTROL_REQUESTS,
        activeControl: MAX_CONTROL_REQUESTS,
        unresolvedControl: 0,
      });

      const refused = (await f.transport
        .queryCommandOutcome(idFromBytes('civ', new Uint8Array(16)))
        .catch((error: unknown) => error)) as CoreRpcError;
      expect(refused).toBeInstanceOf(CoreRpcError);
      expect(refused.envelope).toMatchObject({
        code: 'ipc.local_overload',
        details: { not_sent_reason: 'control-capacity', outcome_recovery: 'not_sent' },
        terminal: 'failed',
      });
      expect(isUnconfirmedCommandOutcome(refused.envelope)).toBe(false);
      expect(f.transport.requestStats()).toMatchObject({
        controlAdmissionRefusals: 1,
        unresolvedControl: 0,
      });

      // A full CONTROL pool is not a business refusal: ordinary work is admitted as usual.
      const business = f.transport.request('agent.status', {}, 1, 1_000);
      await vi.advanceTimersByTimeAsync(0);
      const frame = f.sent.find((message) => message.method === 'agent.status');
      expect(frame).toBeDefined();
      f.deliver({
        jsonrpc: '2.0',
        id: frame?.id,
        result: { terminal: 'succeeded', value: { state: 'completed' } },
      });
      await expect(business).resolves.toEqual({ state: 'completed' });
      expect(f.transport.requestStats()).toMatchObject({
        activeControl: MAX_CONTROL_REQUESTS,
        admissionRefusals: 1,
      });
    } finally {
      f.transport.close();
    }
  }, 5_000);

  it('refuses control work once its control identity pool is full, and reflows on a real terminal', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    const target = idFromBytes('cor', new Uint8Array(16));
    try {
      const givenUp: unknown[] = [];
      // Each control call is really written, then times out locally: its identity stays owed a terminal.
      for (let round = 0; round < CONTROL_UNRESOLVED_TERMINAL_CAPACITY; round += 1) {
        const outstanding = f.transport.requestStats().unresolvedControl;
        if (outstanding >= CONTROL_UNRESOLVED_TERMINAL_CAPACITY) break;
        const batch = Array.from(
          {
            length: Math.min(
              MAX_CONTROL_REQUESTS,
              CONTROL_UNRESOLVED_TERMINAL_CAPACITY - outstanding,
            ),
          },
          () => f.transport.cancel(target).catch((error: unknown) => error),
        );
        await vi.advanceTimersByTimeAsync(0);
        await vi.advanceTimersByTimeAsync(15_000);
        givenUp.push(...(await Promise.all(batch)));
      }
      expect(givenUp.every((failure) => failure instanceof CoreRpcError)).toBe(true);
      expect(f.transport.requestStats()).toMatchObject({
        activeControl: 0,
        unresolved: 0,
        unresolvedControl: CONTROL_UNRESOLVED_TERMINAL_CAPACITY,
      });
      expect(f.onClosed).not.toHaveBeenCalled();

      const refused = (await f.transport
        .cancel(target)
        .catch((error: unknown) => error)) as CoreRpcError;
      expect(refused.envelope).toMatchObject({
        code: 'ipc.local_overload',
        details: { not_sent_reason: 'control-unresolved-capacity', outcome_recovery: 'not_sent' },
      });

      // Nothing was evicted to make room: the FIRST control give-up is still the one legal late answer.
      const firstId = (givenUp[0] as CoreRpcError).envelope.correlation_id;
      f.deliver({
        jsonrpc: '2.0',
        id: firstId,
        result: { terminal: 'succeeded', value: { disposition: 'too_late_to_cancel' } },
      });
      await vi.advanceTimersByTimeAsync(0);
      expect(f.onClosed).not.toHaveBeenCalled();
      expect(f.transport.requestStats()).toMatchObject({
        late: 1,
        unresolvedControl: CONTROL_UNRESOLVED_TERMINAL_CAPACITY - 1,
      });

      // One real terminal freed one control identity slot: control admission works again.
      const admitted = f.transport.cancel(target);
      await vi.advanceTimersByTimeAsync(0);
      expect(f.sent.at(-1)?.method).toBe('aura.cancel');
      f.deliver({
        jsonrpc: '2.0',
        id: f.sent.at(-1)?.id,
        result: { terminal: 'succeeded', value: { disposition: 'unknown_request' } },
      });
      await expect(admitted).resolves.toMatchObject({ disposition: 'unknown_request' });
      expect(f.transport.requestStats()).toMatchObject({
        activeControl: 0,
        unresolvedControl: CONTROL_UNRESOLVED_TERMINAL_CAPACITY - 1,
      });
    } finally {
      f.transport.close();
    }
  }, 10_000);

  it('gives a timed-out control request the same identity rules: first late, then duplicate', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      const cancel = f.transport
        .cancel(idFromBytes('cor', new Uint8Array(16)))
        .catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(0);
      await vi.advanceTimersByTimeAsync(15_000);
      const failure = (await cancel) as CoreRpcError;
      expect(failure.envelope).toMatchObject({
        code: 'ipc.request_timeout',
        details: { outcome_recovery: 'query_runtime.command_outcome_without_replay' },
        terminal: 'cancelled',
      });
      expect(isUnconfirmedCommandOutcome(failure.envelope)).toBe(true);
      expect(f.transport.requestStats()).toMatchObject({ unresolved: 0, unresolvedControl: 1 });
      // The route's declared shape is unchanged: `aura.cancel` is `idempotency: command_invocation`
      // and still carries a freshly minted CommandInvocationId, which is what puts the
      // query-without-replay hint on this envelope.
      expect(f.sent.at(-1)?.command_invocation_id).toMatch(/^civ_/u);
      expect(failure.envelope.details.command_invocation_id).toBe(
        f.sent.at(-1)?.command_invocation_id,
      );

      // The one real terminal after the client gave up is legal, consumed exactly once, and delivered
      // to nobody: the caller that stopped waiting is never handed a late answer.
      const terminal = {
        jsonrpc: '2.0',
        id: failure.envelope.correlation_id,
        result: { terminal: 'succeeded', value: { disposition: 'accepted' } },
      };
      f.deliver(terminal);
      await vi.advanceTimersByTimeAsync(0);
      expect(f.onClosed).not.toHaveBeenCalled();
      expect(f.transport.requestStats()).toMatchObject({
        duplicates: 0,
        late: 1,
        unresolvedControl: 0,
      });

      // A SECOND terminal for the same control id is a duplicate, exactly as for business work.
      f.deliver(terminal);
      await vi.advanceTimersByTimeAsync(0);
      expect(f.onClosed).toHaveBeenCalledOnce();
      expect(f.onClosed).toHaveBeenCalledWith(
        expect.objectContaining({
          code: 'ipc.protocol_violation',
          message: expect.stringContaining('repeated'),
        }),
      );
      expect(f.transport.requestStats().duplicates).toBe(1);
    } finally {
      f.transport.close();
    }
  }, 5_000);

  it('does not consume a control identity with a frame from another connection epoch', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      const cancel = f.transport
        .cancel(idFromBytes('cor', new Uint8Array(16)))
        .catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(0);
      await vi.advanceTimersByTimeAsync(15_000);
      const failure = (await cancel) as CoreRpcError;

      // Same channel, another epoch: this is another connection's frame, so it is a protocol violation
      // and it must NOT be consumed as this control request's legal late terminal.
      f.deliverRaw(
        {
          jsonrpc: '2.0',
          id: failure.envelope.correlation_id,
          result: { terminal: 'succeeded', value: { disposition: 'accepted' } },
        },
        { channelId: binding.channelId, connectionEpoch: 2n },
        1n,
      );
      await vi.advanceTimersByTimeAsync(0);
      expect(f.onClosed).toHaveBeenCalledOnce();
      expect(f.onClosed).toHaveBeenCalledWith(
        expect.objectContaining({ code: 'ipc.protocol_violation' }),
      );
      expect(f.transport.requestStats()).toMatchObject({ late: 0, unresolvedControl: 1 });
    } finally {
      f.transport.close();
    }
  }, 5_000);

  it('rejects in-flight control work when the channel ends and keeps the identity it still owes', async () => {
    vi.useFakeTimers();
    const f = await fixture();
    try {
      // One control request already gave up: its identity is owed a real terminal.
      const gone = f.transport
        .cancel(idFromBytes('cor', new Uint8Array(16)))
        .catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(0);
      await vi.advanceTimersByTimeAsync(15_000);
      expect(((await gone) as CoreRpcError).envelope.code).toBe('ipc.request_timeout');
      expect(f.transport.requestStats().unresolvedControl).toBe(1);

      const inFlight = f.transport
        .cancel(idFromBytes('cor', new Uint8Array(16)))
        .catch((error: unknown) => error);
      await vi.advanceTimersByTimeAsync(0);
      f.transport.close();
      const failure = (await inFlight) as CoreRpcError;
      expect(failure).toBeInstanceOf(CoreRpcError);
      expect(failure.envelope).toMatchObject({ terminal: 'transport_lost' });
      // The channel's in-flight control work is settled by the ONE channel-close path, and the identity
      // that is still owed a terminal is not evicted to tidy the store up.
      expect(f.transport.requestStats()).toMatchObject({
        active: 0,
        activeControl: 0,
        unresolvedControl: 1,
      });
    } finally {
      f.transport.close();
    }
  }, 5_000);

  it('leaves another channel ordinary and control admission untouched', async () => {
    vi.useFakeTimers();
    const saturated = await fixture();
    const other = await fixture();
    try {
      await fillBusinessIdentityPool(saturated);
      expect(saturated.transport.requestStats().unresolved).toBe(UNRESOLVED_TERMINAL_CAPACITY);
      const refused = (await saturated.transport
        .request('workspace.create', {}, 1, 5)
        .catch((error: unknown) => error)) as CoreRpcError;
      expect(refused.envelope.details.not_sent_reason).toBe('unresolved-capacity');

      // The second channel keeps carrying both classes: nothing about the first one is channel-wide.
      const healthy = other.transport.request('workspace.create', {}, 1, 1_000);
      await vi.advanceTimersByTimeAsync(0);
      const businessFrame = other.sent.find((message) => message.method === 'workspace.create');
      expect(businessFrame).toBeDefined();
      other.deliver({
        jsonrpc: '2.0',
        id: businessFrame?.id,
        result: { terminal: 'succeeded', value: { created: true } },
      });
      await expect(healthy).resolves.toEqual({ created: true });

      const cancel = other.transport.cancel(idFromBytes('cor', new Uint8Array(16)));
      await vi.advanceTimersByTimeAsync(0);
      const controlFrame = other.sent.find((message) => message.method === 'aura.cancel');
      expect(controlFrame).toBeDefined();
      other.deliver({
        jsonrpc: '2.0',
        id: controlFrame?.id,
        result: { terminal: 'succeeded', value: { disposition: 'accepted' } },
      });
      await expect(cancel).resolves.toMatchObject({ disposition: 'accepted' });
      expect(other.onClosed).not.toHaveBeenCalled();
      expect(other.transport.requestStats()).toMatchObject({
        admissionRefusals: 0,
        controlAdmissionRefusals: 0,
        unresolved: 0,
        unresolvedControl: 0,
      });
    } finally {
      saturated.transport.close();
      other.transport.close();
    }
  }, 5_000);
});
