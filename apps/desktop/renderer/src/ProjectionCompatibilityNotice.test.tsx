import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import type { ProjectionCompatibilitySnapshotV1 } from '@aura/contracts/generated';
import { ProjectionCompatibilityNotice } from './ProjectionCompatibilityNotice';
import {
  projectionCompatibilityStore,
  ProjectionCompatibilityStore,
} from './projection-compatibility-store';

const runtime = 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const session = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const other = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAW';
const value = (
  revision: number,
  paused: ProjectionCompatibilitySnapshotV1['paused_projections'],
): ProjectionCompatibilitySnapshotV1 => ({
  contract_version: 1,
  revision: String(revision),
  runtime_instance_id: runtime,
  paused_projections: paused,
});
afterEach(() => {
  cleanup();
  projectionCompatibilityStore.reset();
  vi.restoreAllMocks();
});

test('only the affected settings page shows the fixed notice and valid data clears it', () => {
  const { rerender } = render(
    <>
      <ProjectionCompatibilityNotice surface="tools" />
      <p>上一版工具列表仍可查看</p>
    </>,
  );
  act(() =>
    projectionCompatibilityStore.accept(
      value(1, [
        {
          runtime_instance_id: runtime,
          projection_kind: 'tool_settings',
          projection_scope: { type: 'global' },
        },
      ]),
    ),
  );
  expect(screen.getByRole('status').textContent).toContain('数据格式不兼容，已停止刷新');
  expect(screen.getByText('上一版工具列表仍可查看')).toBeTruthy();
  rerender(<ProjectionCompatibilityNotice surface="models" />);
  expect(screen.queryByRole('status')).toBeNull();
  rerender(<ProjectionCompatibilityNotice surface="tools" />);
  act(() => projectionCompatibilityStore.accept(value(2, [])));
  expect(screen.queryByRole('status')).toBeNull();
});

test('only active matching session routes show a notice and repairing another scope cannot clear it', () => {
  const own = projectionCompatibilityStore.ownRoute({
    projection_kind: 'context_window_correction',
    projection_scope: { type: 'session', session_id: session },
  });
  render(<ProjectionCompatibilityNotice surface="workspace" sessionId={session} />);
  const key = (session_id: string) => ({
    runtime_instance_id: runtime,
    projection_kind: 'context_window_correction' as const,
    projection_scope: { type: 'session' as const, session_id },
  });
  act(() => projectionCompatibilityStore.accept(value(1, [key(other)])));
  expect(screen.queryByRole('status')).toBeNull();
  act(() => projectionCompatibilityStore.accept(value(2, [key(session)])));
  expect(screen.getByRole('status').textContent).toContain('模型配置');
  act(() => projectionCompatibilityStore.accept(value(3, [key(session)])));
  expect(screen.queryByRole('status')).not.toBeNull();
  act(() => own());
  expect(screen.queryByRole('status')).toBeNull();
});

test('one metadata subscription ignores a late old snapshot after a newer push', async () => {
  const pending = Promise.withResolvers<{ ok: true; value: ProjectionCompatibilitySnapshotV1 }>();
  let receive: (value: ProjectionCompatibilitySnapshotV1) => void = () => undefined;
  const api = {
    snapshot: vi.fn(() => pending.promise),
    onChange: vi.fn((listener: typeof receive) => {
      receive = listener;
      return vi.fn();
    }),
  };
  const store = new ProjectionCompatibilityStore(() => api);
  const stopA = store.subscribe(vi.fn());
  const stopB = store.subscribe(vi.fn());
  receive(value(2, []));
  pending.resolve({
    ok: true,
    value: value(1, [
      {
        runtime_instance_id: runtime,
        projection_kind: 'tool_settings',
        projection_scope: { type: 'global' },
      },
    ]),
  });
  await pending.promise;
  await Promise.resolve();
  expect(store.getSnapshot().snapshot?.revision).toBe('2');
  expect(api.snapshot).toHaveBeenCalledTimes(1);
  expect(api.onChange).toHaveBeenCalledTimes(1);
  stopA();
  stopB();
});

test('security and terminal notices are scoped and automatically recover', () => {
  const { rerender } = render(
    <ProjectionCompatibilityNotice surface="security" sessionId={session} />,
  );
  act(() =>
    projectionCompatibilityStore.accept(
      value(1, [
        {
          runtime_instance_id: runtime,
          projection_kind: 'session_permission',
          projection_scope: { type: 'session', session_id: session },
        },
      ]),
    ),
  );
  expect(screen.getByRole('status').textContent).toContain('会话权限');
  rerender(<ProjectionCompatibilityNotice surface="security" sessionId={other} />);
  expect(screen.queryByRole('status')).toBeNull();
  act(() =>
    projectionCompatibilityStore.accept(
      value(2, [
        {
          runtime_instance_id: runtime,
          projection_kind: 'terminal_state',
          projection_scope: { type: 'terminal', session_id: session, terminal_id: 'term_fixture' },
        },
      ]),
    ),
  );
  rerender(
    <ProjectionCompatibilityNotice
      surface="terminal"
      sessionId={session}
      terminalId="term_fixture"
    />,
  );
  expect(screen.getByRole('status').textContent).toContain('终端状态');
  rerender(
    <ProjectionCompatibilityNotice
      surface="terminal"
      sessionId={session}
      terminalId="term_other"
    />,
  );
  expect(screen.queryByRole('status')).toBeNull();
  rerender(
    <ProjectionCompatibilityNotice
      surface="terminal"
      sessionId={session}
      terminalId="term_fixture"
    />,
  );
  act(() => projectionCompatibilityStore.accept(value(3, [])));
  expect(screen.queryByRole('status')).toBeNull();
});
