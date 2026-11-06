// @vitest-environment node
import { dialog, type BrowserWindow } from 'electron';
import { beforeEach, expect, test, vi } from 'vitest';
import type { CoreSupervisor } from './core-supervisor.js';
import { exportPlugin } from './plugin-directory.js';

// A plugin is exported where the user chooses: the dialog offers the plugin's name and version as
// the file's name, and Core writes there; the window only names the plugin.

vi.mock('electron', () => ({
  dialog: { showSaveDialog: vi.fn() },
  shell: { openPath: vi.fn() },
}));

const owner = {} as BrowserWindow;

/** The options the save dialog was first opened with, after the window it belongs to. */
const offered = () =>
  (vi.mocked(dialog.showSaveDialog).mock.calls[0] as unknown[] | undefined)?.[1];

const core = (version: string | null) => {
  const requestAgent = vi.fn(async (method: string, params: unknown) => {
    if (method === 'agent.plugins.list')
      return { plugins: [{ name: 'night', version, path: 'C:\\plugins\\night' }] };
    return { completed: true, message: `已导出 3 个文件 ${JSON.stringify(params)}` };
  });
  return { supervisor: { requestAgent } as unknown as CoreSupervisor, requestAgent };
};

beforeEach(() => {
  vi.clearAllMocks();
});

test('the save dialog offers the plugin and its version, and Core writes where the user chose', async () => {
  vi.mocked(dialog.showSaveDialog).mockResolvedValue({
    canceled: false,
    filePath: 'D:\\out\\night.zip',
  });
  const { supervisor, requestAgent } = core('1.2.0-beta.1');
  const exported = await exportPlugin(owner, { name: 'night' }, supervisor);
  expect(offered()).toMatchObject({
    defaultPath: 'night-1.2.0-beta.1.zip',
    filters: [{ name: '插件（ZIP）', extensions: ['zip'] }],
  });
  expect(requestAgent).toHaveBeenLastCalledWith('agent.plugins.export', {
    name: 'night',
    path: 'D:\\out\\night.zip',
  });
  expect(exported.completed).toBe(true);
});

test('characters a file name cannot take in a version become underscores', async () => {
  vi.mocked(dialog.showSaveDialog).mockResolvedValue({ canceled: true, filePath: '' });
  const { supervisor } = core('2.0/rc:1');
  const exported = await exportPlugin(owner, { name: 'night' }, supervisor);
  expect(offered()).toMatchObject({
    defaultPath: 'night-2.0_rc_1.zip',
  });
  expect(exported).toEqual({ completed: false, message: '已取消导出' });
});

test('a plugin without a version is offered under its name alone', async () => {
  vi.mocked(dialog.showSaveDialog).mockResolvedValue({ canceled: true, filePath: '' });
  const { supervisor } = core(null);
  await exportPlugin(owner, { name: 'night' }, supervisor);
  expect(offered()).toMatchObject({
    defaultPath: 'night.zip',
  });
});
