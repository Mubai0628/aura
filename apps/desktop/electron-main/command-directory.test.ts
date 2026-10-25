// @vitest-environment node
import { mkdir, mkdtemp, rm, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { dialog, shell, type BrowserWindow } from 'electron';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { CommandCatalog, CommandEntry } from '@aura/contracts/generated';
import { agentRouteHandler, openCommandDirectory } from './command-directory.js';
import type { CoreSupervisor } from './core-supervisor.js';

vi.mock('electron', () => ({
  shell: { openPath: vi.fn(async () => '') },
  dialog: { showOpenDialog: vi.fn() },
}));
beforeEach(() => vi.clearAllMocks());

const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function fixture() {
  const root = await mkdtemp(join(tmpdir(), 'aura-command-directory-'));
  roots.push(root);
  const catalog: CommandCatalog = {
    commands: [],
    user_directory: join(root, 'Roaming', 'Aura', 'commands'),
    workspace_directory: join(root, 'workspace', '.aura', 'commands'),
  };
  const requestAgent = vi.fn(async () => catalog);
  return { catalog, requestAgent, supervisor: { requestAgent } as unknown as CoreSupervisor };
}

const workspaceCommand = (name: string, location: string) =>
  ({ name, source: 'workspace', location }) as CommandEntry;

const exists = (path: string) =>
  stat(path).then(
    () => true,
    () => false,
  );

test('Aura creates and opens its own commands folder but never creates one in a project', async () => {
  const { catalog, requestAgent, supervisor } = await fixture();
  await expect(openCommandDirectory({ source: 'user' }, supervisor)).resolves.toEqual({
    completed: true,
    message: '已打开命令目录',
  });
  expect((await stat(catalog.user_directory)).isDirectory()).toBe(true);
  expect(shell.openPath).toHaveBeenCalledWith(catalog.user_directory);
  expect(requestAgent).toHaveBeenCalledWith('agent.commands.catalog', { workspace_id: null });

  vi.mocked(shell.openPath).mockClear();
  const missing = await openCommandDirectory(
    { source: 'workspace', workspace_id: 'workspace' },
    supervisor,
  );
  expect(missing.completed).toBe(false);
  expect(missing.message).toContain('.aura/commands');
  expect(await exists(catalog.workspace_directory!)).toBe(false);
  expect((await openCommandDirectory({ source: 'builtin' }, supervisor)).completed).toBe(false);
  catalog.workspace_directory = null;
  expect((await openCommandDirectory({ source: 'workspace' }, supervisor)).message).toBe(
    '请先选择工作区。',
  );
  expect(shell.openPath).not.toHaveBeenCalled();
});

test('a named command opens the folder the catalog names, and only a folder', async () => {
  const { catalog, supervisor } = await fixture();
  const commands = catalog.workspace_directory!;
  await mkdir(join(commands, 'review'), { recursive: true });
  // Something replaced the folder with a file after Core listed it: the shell would run it.
  await writeFile(join(commands, 'swapped'), 'not a folder');
  catalog.commands = [
    workspaceCommand('review', join(commands, 'review')),
    workspaceCommand('swapped', join(commands, 'swapped')),
  ];
  const open = (name: string) =>
    openCommandDirectory({ source: 'workspace', workspace_id: 'workspace', name }, supervisor);

  expect((await open('review')).completed).toBe(true);
  expect(shell.openPath).toHaveBeenCalledWith(join(commands, 'review'));
  vi.mocked(shell.openPath).mockClear();
  expect(await open('missing')).toEqual({
    completed: false,
    message: '没有找到这个命令，请刷新后再试。',
  });
  expect((await open('swapped')).completed).toBe(false);
  expect(shell.openPath).not.toHaveBeenCalled();

  vi.mocked(shell.openPath).mockResolvedValueOnce('Access is denied.');
  expect((await open('review')).completed).toBe(false);
});

// Core names a workspace's folders as it holds them, in Windows' verbatim spelling, and the
// Windows shell does not open `\\?\C:\…`: "打开命令目录" failed for every workspace.
test('a folder Core names in the verbatim spelling opens in the ordinary one', async () => {
  const { catalog, supervisor } = await fixture();
  const commands = catalog.workspace_directory!;
  await mkdir(join(commands, 'review'), { recursive: true });
  catalog.workspace_directory = `\\\\?\\${commands}`;
  catalog.commands = [workspaceCommand('review', `\\\\?\\${join(commands, 'review')}`)];

  const folder = await openCommandDirectory(
    { source: 'workspace', workspace_id: 'workspace' },
    supervisor,
  );
  expect(folder.completed).toBe(true);
  expect(shell.openPath).toHaveBeenLastCalledWith(commands);
  const named = await openCommandDirectory(
    { source: 'workspace', workspace_id: 'workspace', name: 'review' },
    supervisor,
  );
  expect(named.completed).toBe(true);
  expect(shell.openPath).toHaveBeenLastCalledWith(join(commands, 'review'));
});

test('Main answers only its folder routes itself and passes every other agent route to Core', async () => {
  const { requestAgent, supervisor } = await fixture();
  const window = { isDestroyed: () => false } as BrowserWindow;
  const owner = vi.fn(() => window);
  const handle = agentRouteHandler(supervisor, owner);
  const event = {} as never;
  const params = { session_id: 'session' };
  await handle(event, 'agent.commands.catalog', params as never);
  expect(requestAgent).toHaveBeenLastCalledWith('agent.commands.catalog', params);
  await handle(event, 'agent.commands.open_directory', { source: 'builtin' } as never);
  expect(requestAgent).toHaveBeenLastCalledWith('agent.commands.catalog', { workspace_id: null });
  expect(shell.openPath).not.toHaveBeenCalled();
  // The Hook folder dialog belongs to the calling window; Core never sees the request.
  requestAgent.mockClear();
  vi.mocked(dialog.showOpenDialog).mockResolvedValue({
    canceled: false,
    filePaths: ['C:/hooks/guard'],
  });
  await expect(handle(event, 'agent.hooks.select_directory', {} as never)).resolves.toEqual({
    path: 'C:/hooks/guard',
  });
  expect(owner).toHaveBeenCalledWith(event);
  expect(dialog.showOpenDialog).toHaveBeenCalledWith(window, expect.anything());
  expect(requestAgent).not.toHaveBeenCalled();
  await handle(event, 'agent.hooks.list', {} as never);
  expect(requestAgent).toHaveBeenLastCalledWith('agent.hooks.list', {});
  // The Skill import dialog is Main's too; importing the chosen folder is Core's.
  requestAgent.mockClear();
  vi.mocked(dialog.showOpenDialog).mockResolvedValue({
    canceled: false,
    filePaths: ['C:/skills/pdf'],
  });
  await expect(handle(event, 'agent.skills.select_directory', {} as never)).resolves.toEqual({
    path: 'C:/skills/pdf',
  });
  expect(dialog.showOpenDialog).toHaveBeenLastCalledWith(
    window,
    expect.objectContaining({ title: '选择 Skill 文件夹（含 SKILL.md）' }),
  );
  expect(requestAgent).not.toHaveBeenCalled();
  // The ZIP dialog offers only Skill archives and opens files, not folders; Core never sees it.
  vi.mocked(dialog.showOpenDialog).mockResolvedValue({
    canceled: false,
    filePaths: ['C:/skills/pdf.zip'],
  });
  await expect(handle(event, 'agent.skills.select_archive', {} as never)).resolves.toEqual({
    path: 'C:/skills/pdf.zip',
  });
  expect(dialog.showOpenDialog).toHaveBeenLastCalledWith(
    window,
    expect.objectContaining({
      properties: ['openFile', 'dontAddToRecent'],
      filters: [{ name: 'Skill（ZIP）', extensions: ['zip', 'skill'] }],
    }),
  );
  expect(requestAgent).not.toHaveBeenCalled();
  await handle(event, 'agent.skills.list', { workspace_id: null } as never);
  expect(requestAgent).toHaveBeenLastCalledWith('agent.skills.list', { workspace_id: null });
  // Opening a Skill's folder is Main's: Core is only asked for its list.
  requestAgent.mockResolvedValueOnce({ skills: [] } as never);
  await expect(
    handle(event, 'agent.skills.open_directory', { id: 'skl_x' } as never),
  ).resolves.toEqual({ completed: false, message: '没有找到这个 Skill，请刷新后再试。' });
  expect(requestAgent).toHaveBeenLastCalledWith('agent.skills.list', { workspace_id: null });
});

test('L10: the plugin dialogs and opening a plugin folder are Main’s; importing is Core’s', async () => {
  const { requestAgent, supervisor } = await fixture();
  const window = { isDestroyed: () => false } as BrowserWindow;
  const handle = agentRouteHandler(supervisor, () => window);
  const event = {} as never;
  requestAgent.mockClear();
  vi.mocked(dialog.showOpenDialog).mockResolvedValue({
    canceled: false,
    filePaths: ['C:/plugins/kit'],
  });
  await expect(handle(event, 'agent.plugins.select_directory', {} as never)).resolves.toEqual({
    path: 'C:/plugins/kit',
  });
  expect(dialog.showOpenDialog).toHaveBeenLastCalledWith(
    window,
    expect.objectContaining({ title: '选择插件文件夹（含 aura-plugin.toml 或 .claude-plugin）' }),
  );
  vi.mocked(dialog.showOpenDialog).mockResolvedValue({
    canceled: false,
    filePaths: ['C:/plugins/kit.zip'],
  });
  await expect(handle(event, 'agent.plugins.select_archive', {} as never)).resolves.toEqual({
    path: 'C:/plugins/kit.zip',
  });
  expect(dialog.showOpenDialog).toHaveBeenLastCalledWith(
    window,
    expect.objectContaining({ filters: [{ name: '插件（ZIP）', extensions: ['zip'] }] }),
  );
  expect(requestAgent).not.toHaveBeenCalled();
  await handle(event, 'agent.plugins.import', {
    path: 'C:/plugins/kit',
    expected_revision: 1,
  } as never);
  expect(requestAgent).toHaveBeenLastCalledWith('agent.plugins.import', {
    path: 'C:/plugins/kit',
    expected_revision: 1,
  });
  // Opening a plugin's folder: Core is only asked for its list; the renderer names no path.
  requestAgent.mockResolvedValueOnce({ plugins: [] } as never);
  await expect(
    handle(event, 'agent.plugins.open_directory', { name: 'kit', pending: false } as never),
  ).resolves.toEqual({ completed: false, message: '没有找到这个插件，请刷新后再试。' });
  expect(requestAgent).toHaveBeenLastCalledWith('agent.plugins.list', {});
  expect(shell.openPath).not.toHaveBeenCalled();
});
