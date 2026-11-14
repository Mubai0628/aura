import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { SidebarItemMenu } from '../SidebarItemMenu';
import { DeleteSessionDialog } from './LifecycleDialogs';
import { answerWorkspaceLifecycle } from './workspace-lifecycle';

// An archive stopped by work in the Workspace says what still runs and is withdrawn, so the
// Workspace is not held by it and can be archived once that work ends; a rename and a delete that
// did not happen say why.

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const workspace = { id: 'wsp_A', name: 'Project', root: 'C:/Project', sessions: [] };
const session = { id: 'ses_A', title: '整理日志', state: 'idle' };

const stub = (v1: Record<string, unknown>) =>
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1 } });

test('an archive stopped by work in the Workspace says what still runs and is withdrawn', async () => {
  let prepare: ((value: unknown) => void) | undefined;
  const v1 = {
    onWorkspaceLifecyclePrepare: vi.fn((listener: (value: unknown) => void) => {
      prepare = listener;
      return () => undefined;
    }),
    getWorkspaceSessions: vi.fn().mockResolvedValue({
      ok: true,
      value: {
        payload: {
          workspaces: [
            { workspace_id: 'wsp_A', state: 'online', revision: '3', display_name: 'Project' },
          ],
          sessions: [],
        },
      },
    }),
    archiveWorkspace: vi.fn(async () => {
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
    acknowledgeWorkspaceLifecycle: vi.fn(async (ack: { operation_id: string }) => ({
      ok: true,
      value: {
        operation_id: ack.operation_id,
        lifecycle_revision: '2',
        state: 'blocked',
        preflight: { blockers: [{ blocker_kind: 'runtime_objects_active', count: 1 }] },
      },
    })),
    resolveWorkspaceLifecycle: vi.fn(async () => ({ ok: true, value: {} })),
  };
  stub(v1);
  const stop = answerWorkspaceLifecycle();
  render(<SidebarItemMenu workspace={workspace} />);
  fireEvent.click(screen.getByRole('button', { name: 'Project 更多操作' }));
  fireEvent.click(screen.getByRole('menuitem', { name: '归档工作区' }));
  expect((await screen.findByRole('alert')).textContent).toBe(
    '没有归档：工作区里还有 1 个没结束的任务或进程。工作区保持原样；等这些结束或先停止后再归档。',
  );
  expect(v1.resolveWorkspaceLifecycle).toHaveBeenCalledWith({
    operation_id: 'wlo_A',
    expected_lifecycle_revision: '2',
    action: 'return_to_editing',
  });
  stop();
});

test('a rename refused because the name changed elsewhere says so', async () => {
  const renameSession = vi.fn().mockResolvedValue({
    ok: false,
    error: { code: 'session.title_conflict', message: 'session.title_conflict' },
  });
  stub({ renameSession });
  render(<SidebarItemMenu workspace={workspace} session={session} />);
  fireEvent.click(screen.getByRole('button', { name: '整理日志 更多操作' }));
  fireEvent.click(screen.getByRole('menuitem', { name: '重命名会话' }));
  const dialog = screen.getByRole('dialog');
  fireEvent.change(within(dialog).getByLabelText('会话名称'), { target: { value: '新名字' } });
  fireEvent.click(within(dialog).getByRole('button', { name: '保存' }));
  expect((await within(dialog).findByRole('alert')).textContent).toBe(
    '会话名称刚被改过，这次没有改名；请关闭后重新命名。',
  );
});

test('a delete Core did not answer says it may still be going on', async () => {
  const deleteSession = vi.fn().mockResolvedValue({
    ok: false,
    error: { code: 'ipc.request_timeout', message: 'Core request timed out' },
  });
  stub({ deleteSession });
  render(<DeleteSessionDialog session={{ id: 'ses_A', title: '整理日志' }} onClose={vi.fn()} />);
  fireEvent.click(screen.getByRole('button', { name: '删除会话' }));
  await waitFor(() =>
    expect(screen.getByRole('alert').textContent).toBe(
      '没有等到 Aura Core 的答复（Aura Core 没有及时回复），删除可能仍在进行；稍后看它是否还在列表里，还在就再删一次。',
    ),
  );
});
