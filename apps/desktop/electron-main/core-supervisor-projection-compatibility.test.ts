// @vitest-environment node
import { expect, test, vi } from 'vitest';
import { appendFile } from 'node:fs/promises';
import { createFixture, fixture, waitUntil } from './core-supervisor-tests/fixture.js';

vi.mock('node:child_process', () => ({ spawn: vi.fn() }));
vi.mock('node:fs/promises', () => ({ mkdir: vi.fn(), appendFile: vi.fn() }));
vi.mock('./core-transport.js', () => ({ CoreTransport: { connect: vi.fn() } }));

test('a malformed projection keeps its last good value and the authenticated connection', async () => {
  const f = await fixture();
  const previous = f.supervisor.workspaceSessionSnapshot;
  const compatible = vi.fn();
  f.supervisor.subscribeProjectionCompatibility(compatible);
  f.callbacks().onNotification('projection.change', {
    ...f.snapshot('workspace_session', 2),
    payload: { sessions: 'malformed', private_canary: 'DO-NOT-LOG-PAYLOAD' },
  });
  expect(f.supervisor.workspaceSessionSnapshot).toEqual(previous);
  expect(f.transport.close).not.toHaveBeenCalled();
  expect(f.child.kill).not.toHaveBeenCalled();
  expect(f.supervisor.snapshot.payload.state).toBe('ready');
  expect(f.supervisor.projectionCompatibilitySnapshot.paused_projections).toEqual([
    {
      runtime_instance_id: previous.runtime_instance_id,
      projection_kind: 'workspace_session',
      projection_scope: { type: 'global' },
    },
  ]);
  expect(JSON.stringify(vi.mocked(appendFile).mock.calls)).not.toContain('DO-NOT-LOG-PAYLOAD');
  f.push('workspace_session', 3);
  await waitUntil(
    () => f.supervisor.workspaceSessionSnapshot.payload.sessions[0]?.title === 'Title 3',
  );
  expect(f.supervisor.workspaceSessionSnapshot.payload.sessions[0]?.title).toBe('Title 3');
  expect(f.supervisor.projectionCompatibilitySnapshot.paused_projections).toEqual([]);
  expect(compatible).toHaveBeenCalledTimes(2);
});

test('a malformed snapshot preserves the previous projection instead of killing startup', async () => {
  const f = await createFixture();
  await f.supervisor.start();
  const previous = f.supervisor.workspaceFilesSnapshot;
  f.setRead(async (kind) => ({ ...f.snapshot(kind, 2), payload: null }) as never);
  await expect(f.supervisor.refreshWorkspaceFilesSnapshot()).resolves.toEqual(previous);
  expect(f.transport.close).not.toHaveBeenCalled();
  expect(f.child.kill).not.toHaveBeenCalled();
});

test('foreign runtime and invalid scope cannot pause or repair another projection', async () => {
  const f = await fixture();
  const invalid = { ...f.snapshot('workspace_session', 2), payload: null };
  f.callbacks().onNotification('projection.change', invalid);
  const paused = f.supervisor.projectionCompatibilitySnapshot;
  f.callbacks().onNotification('projection.change', {
    ...f.snapshot('workspace_session', 3),
    runtime_instance_id: 'run_01ARZ3NDEKTSV4RRFFQ69G5FAW',
  });
  f.callbacks().onNotification('projection.change', {
    ...f.snapshot('workspace_session', 3),
    projection_scope: { type: 'session', session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV' },
  });
  f.push('workspace_files', 2);
  expect(f.supervisor.projectionCompatibilitySnapshot).toEqual(paused);
  expect(f.transport.close).not.toHaveBeenCalled();
});

test.each(['core_health', 'workspace_session', 'workspace_files'] as const)(
  'an unsafe %s revision pauses only that projection before Number conversion',
  async (kind) => {
    const f = await fixture();
    const previous = [
      f.supervisor.snapshot,
      f.supervisor.workspaceSessionSnapshot,
      f.supervisor.workspaceFilesSnapshot,
    ];
    for (const revision of ['9007199254740992', '18446744073709551616']) {
      f.callbacks().onNotification('projection.change', { ...f.snapshot(kind, 2), revision });
      expect(f.transport.close).not.toHaveBeenCalled();
      expect(f.child.kill).not.toHaveBeenCalled();
      expect([
        f.supervisor.snapshot,
        f.supervisor.workspaceSessionSnapshot,
        f.supervisor.workspaceFilesSnapshot,
      ]).toEqual(previous);
      expect(f.supervisor.projectionCompatibilitySnapshot.paused_projections).toEqual([
        {
          runtime_instance_id: previous[0]!.runtime_instance_id,
          projection_kind: kind,
          projection_scope: { type: 'global' },
        },
      ]);
    }
    f.push(kind, 2);
    expect(f.supervisor.projectionCompatibilitySnapshot.paused_projections).toEqual([]);
    expect(f.transport.close).not.toHaveBeenCalled();
  },
);
