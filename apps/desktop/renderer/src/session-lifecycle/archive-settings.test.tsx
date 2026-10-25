import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { ArchiveSettings } from './ArchiveSettings';
import { answerWorkspaceLifecycle } from './workspace-lifecycle';

vi.mock('../workspace-session-store', () => ({
  useWorkspaceSessions: () => ({
    loading: false,
    projection: {
      payload: {
        workspaces: [
          {
            workspace_id: 'wsp_old',
            display_name: '旧项目',
            selected_root: 'C:\\Old',
            state: 'archived',
            revision: '5',
          },
          {
            workspace_id: 'wsp_live',
            display_name: '当前项目',
            selected_root: 'C:\\Live',
            state: 'online',
            revision: '2',
          },
        ],
        sessions: [
          { session_id: 'ses_old', workspace_id: 'wsp_old', title: '旧会话', state: 'idle' },
          {
            session_id: 'ses_kept',
            workspace_id: 'wsp_live',
            title: '收起的会话',
            state: 'archived',
          },
          { session_id: 'ses_live', workspace_id: 'wsp_live', title: '进行中', state: 'idle' },
        ],
      },
    },
  }),
}));

afterEach(cleanup);

test('lists archived Workspaces and conversations; restores at once and deletes after confirming', async () => {
  const v1 = {
    restoreWorkspace: vi.fn().mockResolvedValue({ ok: true, value: {} }),
    restoreSession: vi.fn().mockResolvedValue({ ok: true, value: {} }),
    deleteSession: vi.fn().mockResolvedValue({ ok: true, value: { session_id: 'ses_kept' } }),
  };
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1 } });
  render(<ArchiveSettings />);

  const workspaces = screen.getByRole('region', { name: '已归档的工作区' });
  expect(within(workspaces).getAllByRole('listitem')).toHaveLength(1);
  expect(workspaces.textContent).toContain('旧项目');
  expect(workspaces.textContent).toContain('1 个会话');
  const sessions = screen.getByRole('region', { name: '已归档的会话' });
  expect(within(sessions).getAllByRole('listitem')).toHaveLength(1);
  expect(sessions.textContent).toContain('收起的会话');
  expect(sessions.textContent).toContain('当前项目');
  expect(sessions.textContent).not.toContain('进行中');

  fireEvent.click(within(workspaces).getByRole('button', { name: '恢复' }));
  await waitFor(() =>
    expect(v1.restoreWorkspace).toHaveBeenCalledWith({ workspace_id: 'wsp_old' }),
  );
  fireEvent.click(within(sessions).getByRole('button', { name: '恢复' }));
  await waitFor(() => expect(v1.restoreSession).toHaveBeenCalledWith({ session_id: 'ses_kept' }));

  fireEvent.click(within(sessions).getByRole('button', { name: '删除' }));
  const dialog = screen.getByRole('dialog');
  expect(dialog.textContent).toContain('删除会话「收起的会话」？');
  fireEvent.click(within(dialog).getByRole('button', { name: '删除会话' }));
  await waitFor(() => expect(v1.deleteSession).toHaveBeenCalledWith({ session_id: 'ses_kept' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
});

// Deleting happens here, after archiving, and only here.
test('an archived Workspace is deleted after confirming, its preparation answered with no buffers', async () => {
  let prepare: ((value: unknown) => void) | undefined;
  const v1 = {
    onWorkspaceLifecyclePrepare: vi.fn((listener: (value: unknown) => void) => {
      prepare = listener;
      return () => undefined;
    }),
    archiveWorkspace: vi.fn(),
    deleteWorkspace: vi.fn(async () => {
      prepare?.({
        operation_id: 'wlo_B',
        lifecycle_revision: '1',
        workspace_id: 'wsp_old',
        operation: 'permanent_delete',
        deadline_at: '2026-10-18T00:00:30Z',
      });
      return {
        ok: true,
        value: { operation_id: 'wlo_B', lifecycle_revision: '1', state: 'preparing' },
      };
    }),
    acknowledgeWorkspaceLifecycle: vi.fn(async (ack: { operation_id: string }) => ({
      ok: true,
      value: { operation_id: ack.operation_id, state: 'completed' },
    })),
  };
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1 } });
  const stop = answerWorkspaceLifecycle();
  render(<ArchiveSettings />);

  const workspaces = screen.getByRole('region', { name: '已归档的工作区' });
  fireEvent.click(within(workspaces).getByRole('button', { name: '删除' }));
  const dialog = screen.getByRole('dialog');
  expect(within(dialog).getByRole('heading').textContent).toBe('删除工作区「旧项目」？');
  expect(dialog.textContent).toContain('无法恢复');
  expect(dialog.textContent).toContain('.memories');
  expect(v1.deleteWorkspace).not.toHaveBeenCalled();
  fireEvent.click(within(dialog).getByRole('button', { name: '删除工作区' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
  expect(v1.deleteWorkspace).toHaveBeenCalledWith({ workspace_id: 'wsp_old' });
  // Already archived: deleting never archives on the way.
  expect(v1.archiveWorkspace).not.toHaveBeenCalled();
  expect(v1.acknowledgeWorkspaceLifecycle.mock.calls.map(([ack]) => ack)).toEqual([
    { buffers: [], lifecycle_revision: '1', operation_id: 'wlo_B', workspace_id: 'wsp_old' },
  ]);
  stop();
});

test('a refused delete says why in its dialog and can be tried again', async () => {
  const deleteSession = vi
    .fn()
    .mockResolvedValueOnce({
      ok: false,
      error: {
        code: 'broker.dispatch_unavailable',
        message: 'session.not_archived: 请先归档这个会话，再到“设置 → 归档”里删除。',
      },
    })
    .mockResolvedValueOnce({ ok: true, value: { session_id: 'ses_kept' } });
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1: { deleteSession } } });
  render(<ArchiveSettings />);

  const sessions = screen.getByRole('region', { name: '已归档的会话' });
  fireEvent.click(within(sessions).getByRole('button', { name: '删除' }));
  const dialog = screen.getByRole('dialog');
  expect(dialog.textContent).toContain('工作区里的文件与记忆不受影响');
  fireEvent.click(within(dialog).getByRole('button', { name: '删除会话' }));
  await waitFor(() =>
    expect(within(dialog).getByRole('alert').textContent).toBe(
      '请先归档这个会话，再到“设置 → 归档”里删除。',
    ),
  );
  fireEvent.click(within(dialog).getByRole('button', { name: '删除会话' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
  expect(deleteSession).toHaveBeenCalledTimes(2);
});

test('an empty archive says so', async () => {
  vi.resetModules();
  vi.doMock('../workspace-session-store', () => ({
    useWorkspaceSessions: () => ({
      loading: false,
      projection: { payload: { workspaces: [], sessions: [] } },
    }),
  }));
  const { ArchiveSettings: Empty } = await import('./ArchiveSettings');
  render(<Empty />);
  expect(screen.getByText('没有归档的工作区。')).toBeTruthy();
  expect(screen.getByText('没有归档的会话。')).toBeTruthy();
  vi.doUnmock('../workspace-session-store');
});
