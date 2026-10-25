// @vitest-environment node
import { afterEach, expect, test, vi } from 'vitest';
import { CoreRpcError } from './core-transport.js';
import { fixture, CONTROL_FRAME_KIND } from './core-transport.fixture.js';
import { confirmationEntry } from './command-outcome-confirmation.js';
vi.mock('./main-pipe-bridge.js', () => ({ connectMainPipe: vi.fn() }));

afterEach(() => {
  vi.useRealTimers();
  vi.clearAllMocks();
});

test('terminal input omits CIV and consumes only one authenticated control request', async () => {
  const f = await fixture();
  try {
    const onInvocation = vi.fn();
    const pending = f.transport.request('terminal.user.write', {}, 1, 1000, onInvocation);
    void pending.catch(() => undefined);
    await vi.waitFor(() =>
      expect(f.sent.some((item) => item.method === 'terminal.user.write')).toBe(true),
    );
    const frame = f.sent.find((item) => item.method === 'terminal.user.write');
    expect(frame).not.toHaveProperty('command_invocation_id');
    expect(onInvocation).not.toHaveBeenCalled();
    f.deliver({
      jsonrpc: '2.0',
      id: frame?.id,
      result: { terminal: 'succeeded', value: { accepted: true } },
    });
    await expect(pending).resolves.toEqual({ accepted: true });
    expect(f.sent.filter((item) => item.method === 'terminal.user.write')).toHaveLength(1);
  } finally {
    f.transport.close();
  }
});

test('submitted terminal timeout stays unknown without query or replay', async () => {
  const f = await fixture();
  vi.useFakeTimers();
  try {
    const pending = f.transport
      .request('terminal.user.write', {}, 1, 10)
      .catch((error: unknown) => error);
    await vi.advanceTimersByTimeAsync(20);
    const failure = (await pending) as CoreRpcError;
    expect(failure).toBeInstanceOf(CoreRpcError);
    expect(failure.envelope).toMatchObject({
      retryable: false,
      details: { outcome_recovery: 'terminal_input_unconfirmed_do_not_replay' },
    });
    expect(failure.envelope.details).not.toHaveProperty('command_invocation_id');
    expect(confirmationEntry('terminal.user.write', failure)).toEqual({
      kind: 'skip',
      reason: 'route-not-a-command',
    });
    await vi.advanceTimersByTimeAsync(100);
    expect(f.sent.filter((item) => item.method === 'terminal.user.write')).toHaveLength(1);
    expect(f.sent.some((item) => item.method === 'runtime.command_outcome')).toBe(false);
  } finally {
    f.transport.close();
  }
});

test('submitted terminal input on disconnect is unconfirmed and cannot be retried', async () => {
  const f = await fixture();
  const pending = f.transport
    .request('terminal.user.write', {}, 1, 1000)
    .catch((error: unknown) => error);
  await vi.waitFor(() =>
    expect(f.sent.some((item) => item.method === 'terminal.user.write')).toBe(true),
  );
  f.transport.close();
  const failure = (await pending) as CoreRpcError;
  expect(failure.envelope).toMatchObject({
    terminal: 'transport_lost',
    retryable: false,
    details: { outcome_recovery: 'terminal_input_unconfirmed_do_not_replay' },
  });
  expect(failure.envelope.details).not.toHaveProperty('command_invocation_id');
  expect(f.sent.filter((item) => item.method === 'terminal.user.write')).toHaveLength(1);
  expect(f.sent.some((item) => item.method === 'runtime.command_outcome')).toBe(false);
});

test('terminal control input still reports not_sent when withdrawn before its writer turn', async () => {
  const f = await fixture();
  vi.useFakeTimers();
  try {
    f.holdWrites(CONTROL_FRAME_KIND);
    const holder = f.transport.request('workspace.create', {}, 1, 1000).catch(() => undefined);
    await vi.advanceTimersByTimeAsync(1);
    const pending = f.transport
      .request('terminal.user.write', {}, 1, 10)
      .catch((error: unknown) => error);
    await vi.advanceTimersByTimeAsync(20);
    const failure = (await pending) as CoreRpcError;
    expect(failure.envelope).toMatchObject({
      retryable: false,
      details: { outcome_recovery: 'not_sent' },
    });
    expect(failure.envelope.details).not.toHaveProperty('command_invocation_id');
    f.releaseWrites();
    await vi.advanceTimersByTimeAsync(1);
    expect(f.sent.some((item) => item.method === 'terminal.user.write')).toBe(false);
    f.transport.close();
    await holder;
  } finally {
    f.releaseWrites();
    f.transport.close();
  }
});

test('terminal transient input cannot enter confirmation even with a forged historical CIV hint', () => {
  const failure = new CoreRpcError({
    code: 'ipc.request_timeout',
    category: 'cancelled',
    terminal: 'cancelled',
    retryable: false,
    correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    message: 'temporary input',
    details: {
      command_invocation_id: 'civ_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      outcome_recovery: 'query_runtime.command_outcome_without_replay',
    },
  } as CoreRpcError['envelope']);
  expect(confirmationEntry('terminal.user.write', failure)).toEqual({
    kind: 'skip',
    reason: 'route-not-a-command',
  });
});

test('closed terminal input is provably not_sent and is never marked retryable', async () => {
  const f = await fixture();
  f.transport.close();
  const failure = (await f.transport
    .request('terminal.user.write', {}, 1, 1000)
    .catch((error: unknown) => error)) as CoreRpcError;
  expect(failure.envelope).toMatchObject({
    retryable: false,
    details: { outcome_recovery: 'not_sent' },
  });
  expect(f.sent.some((item) => item.method === 'terminal.user.write')).toBe(false);
});
