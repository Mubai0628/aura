import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { SidebarItemMenu } from '../SidebarItemMenu';
import { answerWorkspaceLifecycle } from '../session-lifecycle/workspace-lifecycle';
import { WorkspaceRelocateButton } from './WorkspaceRelocateButton';

const workspace = { id: 'wsp_A', name: 'Project', root: 'C:\\Dev\\Project', sessions: [] };
const CONFIRM = { code: 'workspace.relocate_confirmation_required' };
let operation = 0;
let stop: () => void = () => undefined;

afterEach(() => {
  stop();
  cleanup();
  vi.restoreAllMocks();
});

/**
 * The bridge for relocations of `wsp_A`: Main's picker answers `picked`, Core answers each
 * relocate call with the next of `answers` (`'begun'` starts the lifecycle, whose preparation
 * Main delivers before its own call returns), and the lifecycle ends in `outcome`.
 */
function bridge(
  picked: string | undefined,
  answers: ('begun' | { code: string })[],
  outcome: 'completed' | 'blocked' | 'failed' = 'completed',
) {
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
            { workspace_id: 'wsp_A', state: 'offline', revision: '7', display_name: 'Project' },
          ],
          sessions: [],
        },
      },
    }),
    selectWorkspaceDirectory: vi.fn().mockResolvedValue(
      picked === undefined
        ? { ok: true, value: { status: 'cancelled' } }
        : {
            ok: true,
            value: { status: 'selected', selected_root: picked, suggested_display_name: 'Project' },
          },
    ),
    relocateWorkspace: vi.fn<(request: { confirmation: string }) => Promise<unknown>>(async () => {
      const next = answers.shift();
      if (next !== 'begun')
        return { ok: false, error: { code: next?.code ?? 'unexpected', message: 'refused' } };
      operation += 1;
      const operation_id = `wlo_${operation}`;
      prepare?.({
        operation_id,
        lifecycle_revision: '1',
        workspace_id: 'wsp_A',
        operation: 'relocate',
        deadline_at: '2026-11-06T00:00:30Z',
      });
      return {
        ok: true,
        value: {
          operation_id,
          lifecycle_revision: '1',
          state: 'preparing',
          ack_deadline_at: '2026-11-06T00:00:30Z',
        },
      };
    }),
    acknowledgeWorkspaceLifecycle: vi.fn(async (ack: { operation_id: string }) => ({
      ok: true,
      value: {
        operation_id: ack.operation_id,
        lifecycle_revision: '3',
        state: outcome,
        operation: 'relocate',
        workspace_id: 'wsp_A',
      },
    })),
    resolveWorkspaceLifecycle: vi.fn().mockResolvedValue({ ok: true, value: {} }),
  };
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1 } });
  stop = answerWorkspaceLifecycle();
  return v1;
}

const openMenuItem = () => {
  fireEvent.click(screen.getByRole('button', { name: 'Project 更多操作' }));
  fireEvent.click(screen.getByRole('menuitem', { name: '更改位置…' }));
};

test('the Workspace menu offers 更改位置… before archiving', () => {
  bridge('D:\\Dev\\Project', []);
  render(<SidebarItemMenu workspace={workspace} onNotify={vi.fn()} />);
  fireEvent.click(screen.getByRole('button', { name: 'Project 更多操作' }));
  expect(screen.getAllByRole('menuitem').map((item) => item.textContent)).toEqual([
    '工作区信息',
    '复制路径',
    '更改位置…',
    '归档工作区',
  ]);
});

test('the same folder at a new path switches at once, without a question', async () => {
  const v1 = bridge('D:\\Dev\\Project', ['begun']);
  const notify = vi.fn();
  render(<SidebarItemMenu workspace={workspace} onNotify={notify} />);
  openMenuItem();
  await waitFor(() =>
    expect(notify).toHaveBeenCalledWith(expect.objectContaining({ tone: 'success' })),
  );
  expect(v1.relocateWorkspace.mock.calls).toEqual([
    [
      {
        workspace_id: 'wsp_A',
        selected_root: 'D:\\Dev\\Project',
        confirmation: 'require_same_object',
        expected_workspace_revision: '7',
      },
    ],
  ]);
  expect(v1.acknowledgeWorkspaceLifecycle).toHaveBeenCalledWith(
    expect.objectContaining({ buffers: [], workspace_id: 'wsp_A' }),
  );
  expect(notify.mock.calls[0]![0].message).toContain('D:\\Dev\\Project');
  expect(screen.queryByRole('dialog')).toBeNull();
});

test('another folder is asked about first: 取消 leaves the Workspace, 是，切换 moves it', async () => {
  const v1 = bridge('D:\\Dev\\Project', [CONFIRM, CONFIRM, 'begun']);
  const notify = vi.fn();
  render(<SidebarItemMenu workspace={workspace} onNotify={notify} />);
  openMenuItem();
  const dialog = await screen.findByRole('dialog', { name: '这是同一个项目吗？' });
  expect(within(dialog).getByText('C:\\Dev\\Project')).toBeTruthy();
  expect(within(dialog).getByText('D:\\Dev\\Project')).toBeTruthy();
  fireEvent.click(within(dialog).getByRole('button', { name: '取消' }));
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(v1.relocateWorkspace).toHaveBeenCalledTimes(1);
  expect(notify).not.toHaveBeenCalled();

  openMenuItem();
  const again = await screen.findByRole('dialog', { name: '这是同一个项目吗？' });
  fireEvent.click(within(again).getByRole('button', { name: '是，切换' }));
  await waitFor(() =>
    expect(notify).toHaveBeenCalledWith(expect.objectContaining({ tone: 'success' })),
  );
  expect(v1.relocateWorkspace.mock.calls.map(([request]) => request.confirmation)).toEqual([
    'require_same_object',
    'require_same_object',
    'user_confirmed_same_logical_project',
  ]);
  expect(screen.queryByRole('dialog')).toBeNull();
});

test('cancelling the folder picker changes nothing', async () => {
  const v1 = bridge(undefined, []);
  const notify = vi.fn();
  render(<SidebarItemMenu workspace={workspace} onNotify={notify} />);
  openMenuItem();
  await waitFor(() => expect(v1.selectWorkspaceDirectory).toHaveBeenCalledTimes(1));
  await new Promise((done) => setTimeout(done, 20));
  expect(v1.relocateWorkspace).not.toHaveBeenCalled();
  expect(notify).not.toHaveBeenCalled();
});

test('a folder that is already another Workspace is refused with why', async () => {
  bridge('D:\\Dev\\Other', [{ code: 'workspace.root_conflict' }]);
  const notify = vi.fn();
  render(<SidebarItemMenu workspace={workspace} onNotify={notify} />);
  openMenuItem();
  await waitFor(() =>
    expect(notify).toHaveBeenCalledWith(expect.objectContaining({ tone: 'danger' })),
  );
  expect(notify.mock.calls[0]![0].message).toContain('已经是另一个工作区');
});

test('a move held up by unfinished file work is withdrawn and says so', async () => {
  const v1 = bridge('D:\\Dev\\Project', ['begun'], 'blocked');
  const notify = vi.fn();
  render(<SidebarItemMenu workspace={workspace} onNotify={notify} />);
  openMenuItem();
  await waitFor(() =>
    expect(notify).toHaveBeenCalledWith(expect.objectContaining({ tone: 'danger' })),
  );
  expect(v1.resolveWorkspaceLifecycle).toHaveBeenCalledWith({
    operation_id: `wlo_${operation}`,
    expected_lifecycle_revision: '3',
    action: 'return_to_editing',
  });
  expect(notify.mock.calls[0]![0].message).toContain('仍在原来的位置');
});

test('a Workspace Core cannot find offers 选择新位置 on its row, which runs the same move', async () => {
  const v1 = bridge('D:\\Dev\\Project', ['begun']);
  const notify = vi.fn();
  render(<WorkspaceRelocateButton workspace={workspace} onNotify={notify} />);
  const button = screen.getByRole('button', { name: '为 Project 选择新位置' });
  expect(button.textContent).toBe('选择新位置');
  fireEvent.click(button);
  await waitFor(() =>
    expect(notify).toHaveBeenCalledWith(expect.objectContaining({ tone: 'success' })),
  );
  expect(v1.relocateWorkspace).toHaveBeenCalledWith(
    expect.objectContaining({
      confirmation: 'require_same_object',
      selected_root: 'D:\\Dev\\Project',
    }),
  );
});
