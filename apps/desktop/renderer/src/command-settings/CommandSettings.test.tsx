import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { CommandEntry } from '@aura/contracts/generated';
import { CommandSettings } from './CommandSettings';

const entry = (
  name: string,
  source: CommandEntry['source'],
  extra: Partial<CommandEntry> = {},
): CommandEntry => ({
  name,
  title: `${name} 标题`,
  description: `${name} 说明`,
  kind: source === 'builtin' ? 'immediate' : 'prompt',
  source,
  argument_hint: null,
  template: source === 'builtin' ? null : `请处理 $ARGUMENTS（${name}）`,
  enabled: true,
  issue: null,
  location: source === 'builtin' ? null : `C:\\fixture\\${source}\\${name}`,
  revision: source === 'builtin' ? null : `${name}-rev`,
  ...extra,
});
const catalog = (commands: CommandEntry[]) => ({
  ok: true,
  value: { commands, user_directory: 'C:\\fixture\\user', workspace_directory: null },
});

const list = vi.fn(),
  setEnabled = vi.fn(),
  remove = vi.fn(),
  open = vi.fn();
beforeEach(() => {
  for (const mock of [list, setEnabled, remove, open]) mock.mockReset();
  list.mockResolvedValue(
    catalog([
      entry('help', 'builtin'),
      entry('review', 'user'),
      entry('notes', 'user', { enabled: false }),
      entry('broken', 'user', { issue: '缺少 template.md', template: null }),
      entry('deploy', 'workspace'),
    ]),
  );
  setEnabled.mockResolvedValue(catalog([]));
  remove.mockResolvedValue(catalog([]));
  open.mockResolvedValue({ ok: true, value: { completed: true, message: '已打开命令目录' } });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        listCommands: list,
        setCommandEnabled: setEnabled,
        deleteCommand: remove,
        openCommandDirectory: open,
      },
    },
  });
});
afterEach(cleanup);

const rowOf = (name: string) =>
  screen.getByText(`/${name}`, { selector: 'code' }).closest('li') as HTMLElement;

test('lists my and the workspace commands with their state and previews a template', async () => {
  render(<CommandSettings coreReady workspaceId="workspace" />);
  expect(await screen.findByText('/review', { selector: 'code' })).toBeTruthy();
  expect(list).toHaveBeenCalledWith({ workspace_id: 'workspace' });
  expect(within(rowOf('review')).getByText('已启用')).toBeTruthy();
  expect(within(rowOf('notes')).getByText('已停用')).toBeTruthy();
  expect(within(rowOf('broken')).getByText('未提供：缺少 template.md')).toBeTruthy();
  expect(within(rowOf('broken')).queryByRole('button', { name: '启用' })).toBeNull();
  expect(within(rowOf('deploy')).queryByRole('button', { name: '删除' })).toBeNull();
  await userEvent.setup().click(within(rowOf('deploy')).getByRole('button', { name: '预览' }));
  expect(screen.getByLabelText('/deploy 的模板').textContent).toBe('请处理 $ARGUMENTS（deploy）');
});

test('turning a command off goes through the settings route for that source', async () => {
  const user = userEvent.setup();
  render(<CommandSettings coreReady workspaceId="workspace" />);
  await user.click(
    within(await waitFor(() => rowOf('deploy'))).getByRole('button', { name: '停用' }),
  );
  await waitFor(() =>
    expect(setEnabled).toHaveBeenCalledWith({
      workspace_id: 'workspace',
      name: 'deploy',
      source: 'workspace',
      enabled: false,
    }),
  );
  expect((await screen.findByRole('status')).textContent).toContain('已停用 /deploy');
});

test('a user command is removed only after confirming, at the revision shown', async () => {
  const user = userEvent.setup();
  render(<CommandSettings coreReady />);
  await user.click(
    within(await waitFor(() => rowOf('review'))).getByRole('button', { name: '删除' }),
  );
  const confirm = screen.getByRole('alertdialog', { name: '删除 /review' });
  expect(confirm.textContent).toContain('C:\\fixture\\user\\review');
  expect(remove).not.toHaveBeenCalled();
  await user.click(within(confirm).getByRole('button', { name: '删除' }));
  await waitFor(() =>
    expect(remove).toHaveBeenCalledWith({
      workspace_id: null,
      name: 'review',
      revision: 'review-rev',
    }),
  );
});

test('opening a folder asks Main by source and name, and reports what Main says', async () => {
  const user = userEvent.setup();
  open.mockResolvedValueOnce({
    ok: true,
    value: { completed: false, message: '此工作区还没有 .aura/commands 目录' },
  });
  render(<CommandSettings coreReady workspaceId="workspace" />);
  await user.click(await screen.findByRole('button', { name: '打开工作区命令目录' }));
  expect(open).toHaveBeenCalledWith({ workspace_id: 'workspace', source: 'workspace', name: null });
  expect((await screen.findByRole('alert')).textContent).toContain('还没有 .aura/commands');
  await user.click(within(rowOf('review')).getByRole('button', { name: '打开目录' }));
  expect(open).toHaveBeenLastCalledWith({
    workspace_id: 'workspace',
    source: 'user',
    name: 'review',
  });
});

test('without a ready Core nothing is read', () => {
  render(<CommandSettings coreReady={false} />);
  expect(screen.getByRole('status').textContent).toContain('Core 尚未就绪');
  expect(list).not.toHaveBeenCalled();
});

test('a plugin’s commands are listed in their own group, switched here and removed with the plugin', async () => {
  list.mockResolvedValue(
    catalog([
      entry('help', 'builtin'),
      entry('review', 'user'),
      entry('summarize', 'plugin', { plugin: 'kit' }),
      entry('review', 'plugin', { plugin: 'kit', issue: '与已有的 /review 重名，未启用' }),
    ]),
  );
  render(<CommandSettings coreReady workspaceId={undefined} />);
  await screen.findByRole('heading', { name: '插件的命令' });
  const group = screen.getAllByText('插件 kit')[0]?.closest('ul') as HTMLElement;
  expect(within(group).getAllByText('插件 kit')).toHaveLength(2);
  expect(within(group).getByText('未提供：与已有的 /review 重名，未启用')).toBeTruthy();
  expect(within(group).getAllByRole('button', { name: '停用' })).toHaveLength(1);
  expect(within(group).queryByRole('button', { name: '打开目录' })).toBeNull();
  expect(within(group).queryByRole('button', { name: '删除' })).toBeNull();
});

/** Many commands of one kind draw a first page. */
test('many of my commands are drawn a page at a time', async () => {
  vi.stubGlobal(
    'IntersectionObserver',
    class {
      observe() {}
      disconnect() {}
    },
  );
  list.mockResolvedValue(
    catalog(Array.from({ length: 100 }, (_, index) => entry(`mine-${index}`, 'user'))),
  );
  render(<CommandSettings coreReady />);
  await screen.findByText('/mine-0');
  expect(screen.getAllByText(/^\/mine-\d+$/u)).toHaveLength(40);
  vi.unstubAllGlobals();
});
