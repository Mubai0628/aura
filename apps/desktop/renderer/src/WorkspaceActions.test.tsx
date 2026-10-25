import { cleanup, render, screen, waitFor, fireEvent } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { WorkspaceActions } from './WorkspaceActions';
const pick = vi.fn();
const create = vi.fn();
const selected = vi.fn();
const notify = vi.fn();
beforeEach(() => {
  notify.mockReset();
  pick.mockReset().mockResolvedValue({
    ok: true,
    value: {
      status: 'selected',
      selected_root: 'C:/synthetic/项目',
      suggested_display_name: '项目',
    },
  });
  create.mockReset().mockResolvedValue({
    ok: true,
    value: { disposition: 'created', workspace: { workspace_id: 'workspace' } },
  });
  selected.mockReset();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { selectWorkspaceDirectory: pick, createWorkspace: create } },
  });
});
afterEach(cleanup);
test('keyboard selection immediately adds the folder using its basename without a name form', async () => {
  const user = userEvent.setup();
  render(<WorkspaceActions onCreated={selected} onNotify={notify} />);
  await user.tab();
  await user.keyboard('{Enter}');
  // The notice is the app's own: it leaves by itself, it is laid out like
  // every other notice, and nothing stays open over the sidebar.
  await waitFor(() =>
    expect(notify).toHaveBeenCalledWith(
      expect.objectContaining({ tone: 'success', title: '工作区已添加' }),
    ),
  );
  expect(notify.mock.calls[0]![0].message).toContain('项目');
  expect(document.querySelector('.sidebar-action-notice')).toBeNull();
  expect(create).toHaveBeenCalledWith({ selected_root: 'C:/synthetic/项目', display_name: '项目' });
  expect(selected).toHaveBeenCalledWith('workspace');
  expect(screen.queryByRole('textbox')).toBeNull();
});
test('cancellation creates nothing and duplicate selection clearly locates the existing workspace', async () => {
  const user = userEvent.setup();
  pick.mockResolvedValueOnce({ ok: true, value: { status: 'cancelled' } });
  render(<WorkspaceActions onCreated={selected} onNotify={notify} />);
  await user.click(screen.getByRole('button', { name: '添加工作区' }));
  expect(create).not.toHaveBeenCalled();
  create.mockResolvedValueOnce({
    ok: true,
    value: { disposition: 'existing', workspace: { workspace_id: 'existing' } },
  });
  await user.click(screen.getByRole('button', { name: '添加工作区' }));
  await waitFor(() =>
    expect(notify).toHaveBeenCalledWith(
      expect.objectContaining({ message: expect.stringMatching(/这个文件夹已在工作区中/u) }),
    ),
  );
  expect(selected).toHaveBeenCalledWith('existing');
});
test('repeated activation opens one native dialog and never retries an unknown create', async () => {
  let resolve!: (value: unknown) => void;
  pick.mockReturnValueOnce(
    new Promise((done) => {
      resolve = done;
    }),
  );
  render(<WorkspaceActions onCreated={selected} onNotify={notify} />);
  const button = screen.getByRole('button', { name: '添加工作区' });
  fireEvent.click(button);
  fireEvent.click(button);
  expect(pick).toHaveBeenCalledTimes(1);
  create.mockRejectedValueOnce(new Error('transport lost'));
  resolve({
    ok: true,
    value: {
      status: 'selected',
      selected_root: 'C:/synthetic',
      suggested_display_name: 'synthetic',
    },
  });
  await waitFor(() =>
    expect(notify).toHaveBeenCalledWith(expect.objectContaining({ tone: 'danger' })),
  );
  expect(create).toHaveBeenCalledTimes(1);
  expect(selected).not.toHaveBeenCalled();
});
