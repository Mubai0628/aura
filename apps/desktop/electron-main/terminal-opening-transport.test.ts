// @vitest-environment node
import { afterEach, expect, test, vi } from 'vitest';
import type { CorrelationId } from '@aura/contracts/generated';
import { fixture } from './core-transport.fixture.js';
vi.mock('./main-pipe-bridge.js', () => ({ connectMainPipe: vi.fn() }));
afterEach(() => vi.clearAllMocks());
test('production request identity callback binds concurrent opens to exact wire IDs without a lifecycle observer', async () => {
  const f = await fixture();
  const first = vi.fn<(id: CorrelationId) => void>();
  const second = vi.fn<(id: CorrelationId) => void>();
  const params = {
    session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    shell: 'cmd',
    columns: 80,
    rows: 24,
  };
  try {
    const a = f.transport.request('terminal.user.open', params, 1, 1_000, undefined, first);
    const b = f.transport.request('terminal.user.open', params, 1, 1_000, undefined, second);
    await vi.waitFor(() =>
      expect(f.sent.filter((value) => value.method === 'terminal.user.open')).toHaveLength(2),
    );
    const opens = f.sent.filter((value) => value.method === 'terminal.user.open');
    expect(first).toHaveBeenCalledExactlyOnceWith(opens[0]?.id);
    expect(second).toHaveBeenCalledExactlyOnceWith(opens[1]?.id);
    expect(opens[0]?.id).not.toBe(opens[1]?.id);
    expect(opens.every((value) => typeof value.command_invocation_id === 'string')).toBe(true);
    expect(f.transport.channelFacts().observer_attached).toBe(false);
    f.deliver({
      jsonrpc: '2.0',
      id: opens[0]?.id,
      result: { terminal: 'succeeded', value: 'first' },
    });
    expect(await a).toBe('first');
    f.deliver({
      jsonrpc: '2.0',
      id: opens[1]?.id,
      result: { terminal: 'succeeded', value: 'second' },
    });
    expect(await b).toBe('second');
    expect(f.onClosed).not.toHaveBeenCalled();
  } finally {
    f.transport.close();
  }
});

test('early cancellation waits for the original frame to own the writer and cannot overtake an unsent open', async () => {
  const f = await fixture();
  let cancellation: Promise<unknown> | undefined;
  const identity = vi.fn((id: CorrelationId) => {
    cancellation = f.transport.cancel(id).catch((error: unknown) => error);
  });
  f.holdWrites();
  const blocker = f.transport
    .request('agent.status', {}, 1, 2_000)
    .catch((error: unknown) => error);
  const opening = f.transport
    .request('terminal.user.open', {}, 1, 2_000, undefined, identity)
    .catch((error: unknown) => error);
  try {
    await Promise.resolve();
    expect(
      identity,
      'an allocated ID alone cannot make cancellation overtake the queued open',
    ).not.toHaveBeenCalled();
    f.releaseWrites();
    await vi.waitFor(() =>
      expect(
        f.sent.filter(
          (value) => value.method === 'terminal.user.open' || value.method === 'aura.cancel',
        ),
      ).toHaveLength(2),
    );
    const messages = f.sent.filter(
      (value) => value.method === 'terminal.user.open' || value.method === 'aura.cancel',
    );
    expect(messages.map((value) => value.method)).toEqual(['terminal.user.open', 'aura.cancel']);
    expect(messages[1]?.params).toEqual({ request_id: messages[0]?.id });
    for (const message of f.sent.filter((value) => value.method !== 'runtime.handshake.finish')) {
      f.deliver({
        jsonrpc: '2.0',
        id: message.id,
        result: {
          terminal: 'succeeded',
          value: message.method === 'aura.cancel' ? 'accepted' : {},
        },
      });
      await Promise.resolve();
    }
    await Promise.all([blocker, opening, cancellation]);
  } finally {
    f.releaseWrites();
    f.transport.close();
    await Promise.all([blocker, opening, cancellation]);
  }
});

test('pre-send opening cancellation withdraws only its own queued frame and returns cancelled/not_sent without wire or outcome queries', async () => {
  const f = await fixture();
  let cancelIfUnsent: (() => boolean) | undefined;
  const identity = vi.fn<(id: CorrelationId) => void>();
  f.holdWrites();
  const blocker = f.transport
    .request('agent.status', {}, 1, 2_000)
    .catch((error: unknown) => error);
  const opening = f.transport
    .request('terminal.user.open', {}, 1, 2_000, undefined, identity, (cancel: () => boolean) => {
      cancelIfUnsent = cancel;
    })
    .catch((error: unknown) => error);
  try {
    await Promise.resolve();
    expect(
      cancelIfUnsent,
      'Main receives only this request owned closure before the queued frame starts',
    ).toBeTypeOf('function');
    expect(cancelIfUnsent?.()).toBe(true);
    expect(cancelIfUnsent?.()).toBe(false);
    const error = (await opening) as {
      envelope: { terminal: string; details: { outcome_recovery: string } };
    };
    expect(error.envelope.terminal).toBe('cancelled');
    expect(error.envelope.details.outcome_recovery).toBe('not_sent');
    expect(identity).not.toHaveBeenCalled();
    f.releaseWrites();
    await vi.waitFor(() =>
      expect(f.sent.some((message) => message.method === 'agent.status')).toBe(true),
    );
    expect(
      f.sent.some((message) =>
        ['terminal.user.open', 'aura.cancel', 'runtime.command_outcome'].includes(
          String(message.method),
        ),
      ),
    ).toBe(false);
    const status = f.sent.find((message) => message.method === 'agent.status');
    f.deliver({ jsonrpc: '2.0', id: status?.id, result: { terminal: 'succeeded', value: {} } });
    await blocker;
    expect(f.onClosed).not.toHaveBeenCalled();
  } finally {
    f.releaseWrites();
    f.transport.close();
    await Promise.all([blocker, opening]);
  }
});

test('pre-send ownership capability is unavailable to other methods', async () => {
  const f = await fixture();
  const queued = vi.fn();
  try {
    await expect(
      f.transport.request('agent.status', {}, 1, 1_000, undefined, undefined, queued),
    ).rejects.toThrow('Only a terminal opening');
    expect(queued).not.toHaveBeenCalled();
    expect(f.sent.some((message) => message.method === 'agent.status')).toBe(false);
  } finally {
    f.transport.close();
  }
});
