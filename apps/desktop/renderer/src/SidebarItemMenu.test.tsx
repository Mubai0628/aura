import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { SidebarItemMenu } from './SidebarItemMenu';

afterEach(cleanup);
const workspace = { id: 'workspace', name: 'Project', root: 'C:\\Project', sessions: [] };
test('workspace menu exposes the real folder and closes the information dialog', () => {
  render(<SidebarItemMenu workspace={workspace} />);
  fireEvent.click(screen.getByRole('button', { name: 'Project 更多操作' }));
  fireEvent.click(screen.getByRole('menuitem', { name: '工作区信息' }));
  expect(screen.getByRole('dialog').textContent).toContain('C:\\Project');
  fireEvent.click(screen.getByRole('button', { name: '关闭' }));
  expect(screen.queryByRole('dialog')).toBeNull();
});
/** The conversation scrolling beside the sidebar (a running turn) must not close its menu. */
test('only a scroll that moves the menu button closes the menu', () => {
  const beside = document.createElement('div');
  document.body.append(beside);
  render(<SidebarItemMenu workspace={workspace} />);
  fireEvent.click(screen.getByRole('button', { name: 'Project 更多操作' }));
  fireEvent.scroll(beside);
  expect(screen.getByRole('menu')).toBeTruthy();
  fireEvent.scroll(document);
  expect(screen.queryByRole('menu')).toBeNull();
  beside.remove();
});
test('renaming submits the original title and exposes a readable conflict without closing', async () => {
  const rename = vi
    .fn()
    .mockResolvedValue({ ok: false, error: { message: 'session.title_conflict' } });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { renameSession: rename } },
  });
  render(
    <SidebarItemMenu
      workspace={workspace}
      session={{ id: 'session', title: '原名称', state: 'idle' }}
    />,
  );
  fireEvent.click(screen.getByRole('button', { name: '原名称 更多操作' }));
  fireEvent.click(screen.getByRole('menuitem', { name: '重命名会话' }));
  fireEvent.change(screen.getByRole('textbox', { name: '会话名称' }), {
    target: { value: '新名称' },
  });
  fireEvent.click(screen.getByRole('button', { name: '保存' }));
  await waitFor(() =>
    expect(screen.getByRole('alert').textContent).toContain('会话名称已发生变化'),
  );
  expect(rename).toHaveBeenCalledWith({
    session_id: 'session',
    expected_title: '原名称',
    title: '新名称',
  });
  expect(screen.getByRole('dialog')).toBeTruthy();
});
test('archiving shows that it runs, takes one click, and says why it did not finish', async () => {
  let answer: (value: unknown) => void = () => undefined;
  const archiveSession = vi.fn(() => new Promise((resolve) => (answer = resolve)));
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { archiveSession } },
  });
  render(
    <SidebarItemMenu
      workspace={workspace}
      session={{ id: 'session', title: '会话', state: 'idle' }}
    />,
  );
  fireEvent.click(screen.getByRole('button', { name: '会话 更多操作' }));
  fireEvent.click(screen.getByRole('menuitem', { name: '归档会话' }));
  const running = await screen.findByRole('menuitem', { name: '正在归档…' });
  expect((running as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(running);
  expect(archiveSession).toHaveBeenCalledTimes(1);
  answer({ ok: false, error: { message: 'session.busy' } });
  expect((await screen.findByRole('alert')).textContent).toBe('归档没有完成，请刷新后查看。');
  expect(screen.getByRole('menuitem', { name: '归档会话' })).toBeTruthy();
});
