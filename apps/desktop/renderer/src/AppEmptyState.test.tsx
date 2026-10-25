import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { AppEmptyState } from './AppEmptyState';

const api = {
  selectWorkspaceDirectory: vi.fn(),
  createWorkspace: vi.fn(),
  getProjectionCompatibility: vi.fn(),
};
beforeEach(() => {
  for (const mock of Object.values(api)) mock.mockReset();
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1: api } });
});
afterEach(cleanup);

test('Core’s state is said once: a message that repeats the title is not shown again', () => {
  render(
    <AppEmptyState
      loading={false}
      runtimeNotice={{
        title: '正在启动 Aura Core',
        message: '正在启动 Aura Core…',
        tone: 'warning',
      }}
      onCreated={() => {}}
    />,
  );
  expect(screen.getByRole('alert').textContent).toBe('正在启动 Aura Core');
  // While Core is not ready the next step waits for it.
  expect(screen.getByRole('button', { name: '选择文件夹创建工作区' })).toHaveProperty(
    'disabled',
    true,
  );
});

test('a different message is kept next to the title', () => {
  render(
    <AppEmptyState
      loading={false}
      runtimeNotice={{
        title: '无法启动 Aura Core',
        message: '未找到或无法启动 aura-core.exe。',
        tone: 'danger',
      }}
      onCreated={() => {}}
    />,
  );
  expect(screen.getByRole('alert').textContent).toBe(
    '无法启动 Aura Core 未找到或无法启动 aura-core.exe。',
  );
});

// The page with no workspace is the new conversation's page without its message box: the
// sea and its title, and the folder choice in the middle.
test('with no workspace, choosing a folder in the middle of the new conversation page adds the first workspace', async () => {
  api.selectWorkspaceDirectory.mockResolvedValue({
    ok: true,
    value: { status: 'selected', selected_root: 'C:\\code\\demo', suggested_display_name: 'demo' },
  });
  api.createWorkspace.mockResolvedValue({
    ok: true,
    value: { workspace: { workspace_id: 'ws_1' }, disposition: 'created' },
  });
  const created = vi.fn();
  const { container } = render(<AppEmptyState loading={false} onCreated={created} />);
  expect(screen.getByRole('heading', { level: 2 }).textContent).toBe('欢迎使用 Aura');
  expect(screen.getByRole('heading', { level: 1 }).textContent).toBe('从一个想法开始');
  expect(container.querySelector('.new-session-body > .ocean-backdrop')).not.toBeNull();
  expect(screen.queryByRole('textbox')).toBeNull();
  expect(screen.queryByText('选择一个本地文件夹作为工作区，开始第一个任务。')).toBeNull();
  const choose = screen.getByRole('button', { name: '选择文件夹创建工作区' });
  expect(choose.closest('.new-session-welcome')).not.toBeNull();
  await userEvent.click(choose);
  await waitFor(() => expect(created).toHaveBeenCalledWith('ws_1'));
  expect(api.createWorkspace).toHaveBeenCalledWith({
    selected_root: 'C:\\code\\demo',
    display_name: 'demo',
  });
  expect((await screen.findByRole('status')).textContent).toBe('工作区已添加。');
});

test('with a workspace and no session, the next step is a new session there', async () => {
  const start = vi.fn();
  render(
    <AppEmptyState
      loading={false}
      workspaceName="demo"
      onCreated={() => {}}
      onNewSession={start}
    />,
  );
  expect(screen.getByRole('heading', { level: 2 }).textContent).toBe('还没有会话');
  expect(screen.queryByText('在“demo”里开始一个新任务。')).toBeNull();
  await userEvent.click(screen.getByRole('button', { name: '新建会话' }));
  expect(start).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole('button', { name: '选择文件夹创建工作区' })).toBeNull();
});

// While Core starts there is nothing to choose yet — the sea and what Core is
// doing, nothing else.
test('while Core starts the page is the sea and one line saying so', () => {
  const { container } = render(
    <AppEmptyState
      loading={false}
      starting="正在启动 Aura Core"
      runtimeNotice={{
        title: '正在启动 Aura Core',
        message: '正在启动 Aura Core…',
        tone: 'warning',
      }}
      onCreated={() => {}}
    />,
  );
  expect(screen.getByRole('status').textContent).toBe('正在启动 Aura Core');
  expect(container.querySelector('.ocean-backdrop')).not.toBeNull();
  expect(screen.queryByRole('alert')).toBeNull();
  expect(screen.queryByRole('heading')).toBeNull();
  expect(screen.queryByRole('button')).toBeNull();
});
