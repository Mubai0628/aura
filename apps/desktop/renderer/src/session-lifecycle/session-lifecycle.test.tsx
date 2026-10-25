import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { SidebarItemMenu } from '../SidebarItemMenu';
import { answerWorkspaceLifecycle } from './workspace-lifecycle';

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const workspace = { id: 'wsp_A', name: 'Project', root: 'C:\\Project', sessions: [] };
const session = { id: 'ses_A', title: '整理日志', state: 'idle' };

const stub = (v1: Record<string, unknown>) =>
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1 } });

// "工作区和会话删除改为必须先归档，再去设置页的归档页选择恢复或删除，而不是直接删除".
test('the sidebar archives a conversation at once and offers no delete', async () => {
  const archiveSession = vi.fn().mockResolvedValue({ ok: true, value: { ...session } });
  const deleteSession = vi.fn();
  stub({ archiveSession, deleteSession });
  render(<SidebarItemMenu workspace={workspace} session={session} />);

  fireEvent.click(screen.getByRole('button', { name: '整理日志 更多操作' }));
  expect(screen.getAllByRole('menuitem').map((item) => item.textContent)).toEqual([
    '重命名会话',
    '归档会话',
  ]);
  const archive = screen.getByRole('menuitem', { name: '归档会话' });
  expect(archive.getAttribute('title')).toBe('归档后可在“设置 → 归档”里恢复或删除');
  fireEvent.click(archive);
  await waitFor(() => expect(archiveSession).toHaveBeenCalledWith({ session_id: 'ses_A' }));
  await waitFor(() => expect(screen.queryByRole('menu')).toBeNull());
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(deleteSession).not.toHaveBeenCalled();
});

test('a draft has no archive or delete, only its own discard', () => {
  render(
    <SidebarItemMenu
      workspace={workspace}
      session={{ id: 'draft_1', title: '', state: 'draft' }}
    />,
  );
  fireEvent.click(screen.getByRole('button', { name: '新会话 更多操作' }));
  expect(screen.getAllByRole('menuitem').map((item) => item.textContent)).toEqual(['删除草稿']);
});

test('the sidebar archives a Workspace, answering its preparation with no buffers, and offers no removal', async () => {
  let prepare: ((value: unknown) => void) | undefined;
  const projection = (state: string, revision: string) => ({
    ok: true,
    value: {
      payload: {
        workspaces: [{ workspace_id: 'wsp_A', state, revision, display_name: 'Project' }],
        sessions: [],
      },
    },
  });
  const v1 = {
    onWorkspaceLifecyclePrepare: vi.fn((listener: (value: unknown) => void) => {
      prepare = listener;
      return () => undefined;
    }),
    getWorkspaceSessions: vi.fn().mockResolvedValue(projection('online', '3')),
    archiveWorkspace: vi.fn(async () => {
      // Main delivers the preparation before its own call returns.
      prepare?.({
        operation_id: 'wlo_A',
        lifecycle_revision: '1',
        workspace_id: 'wsp_A',
        operation: 'archive',
        deadline_at: '2026-10-18T00:00:30Z',
      });
      return {
        ok: true,
        value: { operation_id: 'wlo_A', lifecycle_revision: '1', state: 'preparing' },
      };
    }),
    deleteWorkspace: vi.fn(),
    acknowledgeWorkspaceLifecycle: vi.fn(async (ack: { operation_id: string }) => ({
      ok: true,
      value: { operation_id: ack.operation_id, state: 'completed' },
    })),
  };
  stub(v1);
  const stop = answerWorkspaceLifecycle();
  render(<SidebarItemMenu workspace={workspace} />);

  fireEvent.click(screen.getByRole('button', { name: 'Project 更多操作' }));
  expect(screen.getAllByRole('menuitem').map((item) => item.textContent)).toEqual([
    '工作区信息',
    '复制路径',
    '归档工作区',
  ]);
  fireEvent.click(screen.getByRole('menuitem', { name: '归档工作区' }));
  await waitFor(() => expect(screen.queryByRole('menu')).toBeNull());
  expect(v1.archiveWorkspace).toHaveBeenCalledWith({
    workspace_id: 'wsp_A',
    expected_workspace_revision: '3',
  });
  expect(v1.acknowledgeWorkspaceLifecycle.mock.calls.map(([ack]) => ack)).toEqual([
    { buffers: [], lifecycle_revision: '1', operation_id: 'wlo_A', workspace_id: 'wsp_A' },
  ]);
  expect(v1.deleteWorkspace).not.toHaveBeenCalled();
  expect(screen.queryByRole('dialog')).toBeNull();
  stop();
});
