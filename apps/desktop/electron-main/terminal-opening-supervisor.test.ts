// @vitest-environment node
import { readFileSync } from 'node:fs';
import { expect, test, vi } from 'vitest';
import type { CorrelationId, TerminalOpeningStartV1 } from '@aura/contracts/generated';
import { createFixture } from './core-supervisor-tests/fixture.js';
import { TerminalOpenings } from './terminal-opening.js';
vi.mock('node:child_process', () => ({ spawn: vi.fn() }));
vi.mock('node:fs/promises', () => ({ mkdir: vi.fn(), appendFile: vi.fn() }));
vi.mock('./core-transport.js', () => ({ CoreTransport: { connect: vi.fn() } }));
const source = JSON.parse(
  readFileSync(
    new URL('../../../fixtures/contracts/terminal-opening.v1.json', import.meta.url),
    'utf8',
  ),
);
test('real supervisor request and notification assembly reach the owner-bound opening cancellation bridge', async () => {
  const f = await createFixture();
  await f.supervisor.start();
  let finish!: (value: unknown) => void;
  const response = new Promise((resolve) => {
    finish = resolve;
  });
  const requestId = source.progress[0].request_id as CorrelationId;
  f.transport.request.mockImplementationOnce(
    (_method, _params, _priority, _timeout, _invocation, identify) => {
      identify?.(requestId);
      return response;
    },
  );
  f.transport.cancel.mockResolvedValue('accepted' as never);
  const publish = vi.fn();
  const manager = new TerminalOpenings(publish);
  const owner = {};
  const pending = manager.start(owner, source.start as TerminalOpeningStartV1, f.supervisor);
  const cancel = manager.cancel(owner, source.start.opening_id);
  expect(f.transport.cancel).toHaveBeenCalledExactlyOnceWith(requestId);
  f.callbacks().onNotification('terminal.opening.v1', {
    ...source.progress[0],
    runtime_instance_id: f.supervisor.snapshot.runtime_instance_id,
  });
  expect(await cancel).toBe('accepted');
  expect(f.transport.cancel).toHaveBeenCalledExactlyOnceWith(requestId);
  expect(publish).toHaveBeenCalledTimes(1);
  finish({
    session_id: source.start.request.session_id,
    terminal_id: 'temporary',
    shell: 'cmd',
    process_id: 1,
    started_at: '2026-10-15T00:00:00Z',
    end: null,
    owner: 'user',
    controller: 'user',
    title: '',
    tool_call_id: null,
    revision: '1',
  });
  expect((await pending).terminal_id).toBe('temporary');
  expect(
    f.transport.request.mock.calls.filter(([method]) => method === 'terminal.user.open'),
  ).toHaveLength(1);
  await f.supervisor.stop();
});

test('real supervisor assembly locally withdraws an unsent opening and never calls cancel or outcome query', async () => {
  const f = await createFixture();
  await f.supervisor.start();
  let fail!: (error: unknown) => void;
  const response = new Promise((_resolve, reject) => {
    fail = reject;
  });
  let withdrawn = false;
  f.transport.request.mockImplementationOnce(
    (_method, _params, _priority, _timeout, _invocation, _identity, queued) => {
      queued?.(() => {
        if (withdrawn) return false;
        withdrawn = true;
        fail({
          envelope: {
            category: 'cancelled',
            code: 'ipc.request_cancelled',
            correlation_id: source.progress[0].request_id,
            terminal: 'cancelled',
            message: '已取消打开。',
            retryable: false,
            details: { outcome_recovery: 'not_sent' },
          },
        });
        return true;
      });
      return response;
    },
  );
  const manager = new TerminalOpenings(vi.fn());
  const owner = {};
  const pending = manager.start(owner, source.start as TerminalOpeningStartV1, f.supervisor);
  const cancelled = manager.cancel(owner, source.start.opening_id);
  await expect(pending).rejects.toMatchObject({
    envelope: { terminal: 'cancelled', details: { outcome_recovery: 'not_sent' } },
  });
  expect(await cancelled).toBe('accepted');
  expect(f.transport.cancel).not.toHaveBeenCalled();
  expect(f.transport.queryCommandOutcome).not.toHaveBeenCalled();
  expect(
    f.transport.request.mock.calls.filter(([method]) => method === 'terminal.user.open'),
  ).toHaveLength(1);
  await f.supervisor.stop();
});
