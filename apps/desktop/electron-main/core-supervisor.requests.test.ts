// @vitest-environment node
import { expect, test, vi } from 'vitest';
import {
  fixture,
  pipeClosed,
  waitUntil,
  delay,
  loggedFields,
} from './core-supervisor-tests/fixture.js';
import { requestTimeout, rejectionWith } from './core-supervisor-tests/support.js';

vi.mock('node:child_process', () => ({ spawn: vi.fn() }));
vi.mock('node:fs/promises', () => ({ mkdir: vi.fn(), appendFile: vi.fn() }));
vi.mock('./core-transport.js', () => ({ CoreTransport: { connect: vi.fn() } }));

/**
 * A gap refresh is one read that no caller awaits. Its failure has to have an owner.
 *
 * A revision jump makes the supervisor re-read the snapshot; that read is exactly the kind of
 * request that can now time out while the channel stays open. Before this, the rejection belonged
 * to nobody and reached the process-level rejection path, which is how a single slow read becomes a
 * Main-process event that has nothing to do with Core health.
 */
test('a failed gap refresh is owned instead of escaping as an unhandled rejection', async () => {
  const f = await fixture();
  const before = f.supervisor.snapshot;
  const unhandled: unknown[] = [];
  const listener = (reason: unknown): void => {
    unhandled.push(reason);
  };
  process.on('unhandledRejection', listener);
  f.transport.request.mockImplementationOnce(async () => {
    throw rejectionWith(requestTimeout);
  });
  try {
    // Revision 5 after 1: not the next increment, so the gap is reconciled by re-reading.
    f.push('core_health', 5);
    // Wait for the failure to become visible either way: owned by the supervisor, or escaped.
    await waitUntil(
      () => loggedFields('core_projection_refresh_failed').length > 0 || unhandled.length > 0,
    );
    await delay(50);
    expect(unhandled).toEqual([]);
  } finally {
    process.off('unhandledRejection', listener);
  }
  expect(loggedFields('core_projection_refresh_failed')[0]).toMatchObject({
    projection: 'core_health',
    requestLocal: true,
  });
  // The read failed; the projection keeps what Core last published and nothing else happened.
  expect(f.supervisor.snapshot).toEqual(before);
  expect(f.transport.close).not.toHaveBeenCalled();
  expect(f.child.kill).not.toHaveBeenCalled();
  f.supervisor.stop();
});

/**
 * The additive acceptance seams.
 *
 * These delegations exist so the acceptance layer can observe and drive ONE designated request
 * without a test route, a debug RPC or a second cancellation path. Each case asserts what the real
 * transport received, and the two that could change product behaviour assert what they must NOT do:
 * a caller budget never exceeds the production default, and severing the connection never ends the
 * Core.
 */
test('the caller-budget seam refuses anything outside [1, 15000] and forwards the rest unchanged', async () => {
  const f = await fixture();
  for (const budget of [0, -1, 1.5, 15_001, Number.NaN]) {
    await expect(f.supervisor.requestWithBudget('fixture.echo', {}, budget)).rejects.toThrow(
      /positive safe integer|must not exceed the production default/u,
    );
  }
  expect(f.transport.request).not.toHaveBeenCalledWith(
    'fixture.echo',
    expect.anything(),
    expect.anything(),
    expect.anything(),
  );
  await expect(f.supervisor.requestWithBudget('fixture.echo', { at: 1 }, 1_500)).resolves.toEqual({
    method: 'fixture.echo',
    priority: 1,
    timeoutMilliseconds: 1_500,
  });
  await expect(f.supervisor.requestWithBudget('fixture.echo', { at: 2 }, 15_000)).resolves.toEqual({
    method: 'fixture.echo',
    priority: 1,
    timeoutMilliseconds: 15_000,
  });
  f.supervisor.stop();
});

test('cancel and queryCommandOutcome reach the transport methods that had no caller', async () => {
  const f = await fixture();
  await expect(f.supervisor.cancel('cor_01ARZ3NDEKTSV4RRFFQ69G5FAV')).resolves.toEqual({
    request_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  });
  expect(f.transport.cancel).toHaveBeenCalledWith('cor_01ARZ3NDEKTSV4RRFFQ69G5FAV');
  await expect(
    f.supervisor.queryCommandOutcome('civ_01ARZ3NDEKTSV4RRFFQ69G5FAV'),
  ).resolves.toMatchObject({ outcome: 'unknown' });
  expect(f.transport.queryCommandOutcome).toHaveBeenCalledWith('civ_01ARZ3NDEKTSV4RRFFQ69G5FAV');
  f.supervisor.stop();
});

test('the lifecycle observer attaches to the live transport and detaches again', async () => {
  const f = await fixture();
  expect(f.supervisor.channelFacts()).toMatchObject({ connection_epoch: 7 });
  const records: unknown[] = [];
  const detach = f.supervisor.observeClientLifecycle((record) => records.push(record));
  expect(f.transport.attachLifecycleObserver).toHaveBeenCalledTimes(1);
  expect(f.transport.attachLifecycleObserver).toHaveBeenLastCalledWith(expect.any(Function));
  detach();
  expect(f.transport.attachLifecycleObserver).toHaveBeenLastCalledWith(undefined);
  // The supervisor itself never builds a record: only the transport's own emission points do, so a
  // detached observer sees nothing at all.
  expect(records).toEqual([]);
  f.supervisor.stop();
});

test('severing the active connection is followed by the same-process reconnect, not a Core kill', async () => {
  const f = await fixture();
  expect(f.connectAttempts()).toBe(1);
  f.supervisor.closeActiveConnection();
  expect(f.transport.close).toHaveBeenCalledTimes(1);
  expect(f.child.kill).not.toHaveBeenCalled();
  // The production transport-loss path owns what happens next; this seam only severs.
  f.callbacks().onClosed(pipeClosed);
  await waitUntil(
    () => f.connectAttempts() === 2 && f.supervisor.snapshot.payload.state === 'ready',
  );
  expect(f.child.kill).not.toHaveBeenCalled();
  expect(f.child.exitCode).toBeNull();
  expect(f.supervisor.snapshot.payload.state).toBe('ready');
  f.supervisor.stop();
});
