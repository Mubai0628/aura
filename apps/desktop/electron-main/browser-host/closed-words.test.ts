// @vitest-environment node
import { ipcMain, session } from 'electron';
import { expect, test, vi } from 'vitest';
import { BROWSER_COMMAND_CHANNEL } from '../../shared/browser-api.js';
import { S } from './executor-fakes.js';
import { installBrowserHost } from './index.js';

vi.mock('electron', async () => {
  const { Contents, Tools } = await import('./executor-fakes.js');
  return {
    ipcMain: { handle: vi.fn() },
    session: { fromPartition: vi.fn() },
    WebContentsView: class {
      webContents = Object.assign(new Contents(), { debugger: new Tools() });
      setBackgroundColor() {}
      setBounds() {}
      setVisible() {}
    },
  };
});
vi.mock('./profile.js', () => ({ BROWSER_PARTITION: 'test-browser', guardProfile: vi.fn() }));

// A page the browser did not open says why: the browser was turned off, its settings need
// recovery, or they were not read from Core yet; and a command that failed inside Aura says so
// and is logged.

const settings = (enabled: boolean, issue: string | null = null) => ({
  revision: 1,
  issue,
  settings: { enabled, allowed_sites: [], blocked_sites: [], debugging: false },
});

function host(read: () => Promise<unknown>) {
  const core = {
    requestAgent: vi.fn(read),
    requestWithBudget: vi.fn(async () => ({ accepted: true })),
    refreshWorkspaceSessionSnapshot: vi.fn(),
  };
  const log = vi.fn();
  installBrowserHost({
    core: () => core as never,
    window: () => undefined,
    assertTrustedSender() {},
    log,
  });
  const handler = vi
    .mocked(ipcMain.handle)
    .mock.calls.filter(([channel]) => channel === BROWSER_COMMAND_CHANNEL)
    .at(-1)?.[1] as (event: unknown, value: unknown) => Promise<unknown>;
  return { handler: (value: unknown) => handler({}, value), log };
}

const open = { kind: 'open', sessionId: S, pageId: 'tab-1' };

test('a browser turned off in the settings says where to turn it on', async () => {
  const { handler } = host(async () => settings(false));
  expect(await handler(open)).toEqual({
    ok: false,
    message:
      '内置浏览器已在设置中关闭，页面没有打开。到 设置 → 网络与浏览器 打开“启用内置浏览器”后再试。',
  });
});

test('a browser whose settings need recovery says so, not that it was turned off', async () => {
  const { handler } = host(async () => settings(true, '浏览器设置不可用'));
  expect(await handler(open)).toEqual({
    ok: false,
    message:
      '浏览器设置需要恢复，内置浏览器暂不能用，页面没有打开；请到 设置 → 网络与浏览器 查看。',
  });
});

test('a browser whose settings were not read from Core yet says to try again shortly', async () => {
  const { handler } = host(async () => {
    throw new Error('Core request timed out');
  });
  expect(await handler(open)).toEqual({
    ok: false,
    message: '浏览器设置还没从 Aura Core 读到（Core 可能正在启动），页面没有打开；稍后再试。',
  });
  expect(await handler({ kind: 'hand-over', sessionId: S, pageId: 'tab-1' })).toEqual({
    ok: false,
    message: '浏览器设置还没从 Aura Core 读到（Core 可能正在启动），页面没有交给模型；稍后再试。',
  });
});

test('a browser command that failed inside Aura says so, and the failure is logged', async () => {
  const { handler, log } = host(async () => settings(true));
  vi.mocked(session.fromPartition).mockReturnValue({
    clearStorageData: async () => {
      throw new Error('EBUSY: resource busy or locked');
    },
    clearCache: async () => undefined,
  } as never);
  expect(await handler({ kind: 'clear-data' })).toEqual({
    ok: false,
    message: '浏览数据没有清除（Aura 内部出错，已写入日志）；请重试，仍不行请重启 Aura。',
  });
  expect(log).toHaveBeenCalledWith('browser_command_failed', {
    kind: 'clear-data',
    error: 'EBUSY: resource busy or locked',
  });
});

test('answering a request the model no longer waits for says so', async () => {
  const { handler } = host(async () => settings(true));
  expect(await handler({ kind: 'assist-done', sessionId: S, pageId: 'model' })).toEqual({
    ok: false,
    message: '模型已不再等待这次操作（可能已超时或任务已停止），无需再点。',
  });
});
