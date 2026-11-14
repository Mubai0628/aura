import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { StorageCheck, StorageOverview } from '@aura/contracts/generated';

import { StorageSettings } from './StorageSettings';
import { size } from './size';

const GIB = 1024 ** 3;
const overview = (over: Partial<StorageOverview> = {}): StorageOverview => ({
  data: { path: 'C:\\Users\\me\\AppData\\Local\\Aura\\namespaces\\a4718', bytes: 8.7 * GIB },
  data_moved: false,
  install: { path: 'C:\\Users\\me\\AppData\\Local\\Programs\\Aura', bytes: 0.4 * GIB },
  requested: null,
  failure: null,
  old_copies: false,
  ...over,
});
const accepted = (over: Partial<StorageCheck> = {}): StorageCheck => ({
  destination: 'D:\\AuraData',
  bytes: 8.7 * GIB,
  parts: [
    { part: 'data', bytes: 8 * GIB },
    { part: 'core_log', bytes: 120 * 1024 ** 2 },
    { part: 'window_profile', bytes: 40 * 1024 ** 2 },
    { part: 'global_memory', bytes: 2 * 1024 ** 2 },
  ],
  needed: 10.6 * GIB,
  free: 400 * GIB,
  refusals: [],
  ...over,
});

let current: StorageOverview;
let checked: StorageCheck;
const ok = <T,>(value: T) => ({ ok: true as const, value });
const api = {
  overview: vi.fn(async () => ok(current)),
  check: vi.fn(async () => ok(checked)),
  requestMove: vi.fn(async (request: { folder: string | null }) => {
    current = { ...current, requested: { destination: request.folder } };
    return ok(current);
  }),
  withdrawMove: vi.fn(async () => {
    current = { ...current, requested: null };
    return ok(current);
  }),
  dismissFailure: vi.fn(async () => {
    current = { ...current, failure: null };
    return ok(current);
  }),
  selectFolder: vi.fn(async () => ok({ path: 'D:\\AuraData' as string | null })),
  openFolder: vi.fn(async () => ok({ completed: true, message: '已打开' })),
  restart: vi.fn(async () => ok({ restarting: true })),
  moveInstall: vi.fn(async () => ok({ restarting: true })),
  oldData: vi.fn(async () => ok({ items: [] })),
  removeOldData: vi.fn(async () => ok({ removed: [], in_use: [], failed: [], items: [] })),
};

const worktrees = {
  readWorktreeLocation: vi.fn(async () =>
    ok({
      location: 'C:\\Users\\me\\AuraWorktrees',
      chosen: false,
      default_location: 'C:\\Users\\me\\AuraWorktrees',
      revision: 1,
      worktrees: 2,
      size_bytes: 3 * GIB,
      free_bytes: 120 * GIB,
    }),
  ),
  selectWorktreeLocation: vi.fn(async () => ok({ path: null as string | null })),
  setWorktreeLocation: vi.fn(),
};

beforeEach(() => {
  current = overview();
  checked = accepted();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { storage: api, ...worktrees } },
  });
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('设置 → 系统 → 存储', () => {
  it('shows where the data and the installation are, with their sizes, and opens their folders', async () => {
    const user = userEvent.setup();
    render(<StorageSettings coreReady />);
    const data = await screen.findByRole('region', { name: '数据' });
    await waitFor(() => expect(api.oldData).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole('region', { name: '旧数据' })).toBeNull();
    expect(data.textContent).toContain('8.70 GB');
    expect(data.textContent).toContain('C:\\Users\\me\\AppData\\Local\\Aura\\namespaces\\a4718');
    expect(within(data).queryByRole('button', { name: '移回默认位置' })).toBeNull();
    const install = screen.getByRole('region', { name: '安装' });
    expect(install.textContent).toContain('410 MB');
    await user.click(within(install).getByRole('button', { name: '打开文件夹' }));
    expect(api.openFolder).toHaveBeenCalledWith({ part: 'install' });
    await user.click(within(data).getByRole('button', { name: '打开文件夹' }));
    expect(api.openFolder).toHaveBeenLastCalledWith({ part: 'data' });
  });

  it('lists the worktree folder after the installation', async () => {
    render(<StorageSettings coreReady />);
    const row = await screen.findByRole('region', { name: '工作副本' });
    await waitFor(() => expect(row.textContent).toContain('C:\\Users\\me\\AuraWorktrees'));
    expect(row.textContent).toContain('2 个 · 剩余 120.00 GB');
    expect(
      screen
        .getAllByRole('region')
        .map((region) => region.getAttribute('aria-label'))
        .filter((name) => name !== '旧数据'),
    ).toEqual(['数据', '安装', '工作副本']);
  });

  it('moves the data after the user confirms the folder Core accepted, and closes Aura for it', async () => {
    const user = userEvent.setup();
    render(<StorageSettings coreReady />);
    const data = await screen.findByRole('region', { name: '数据' });
    await user.click(within(data).getByRole('button', { name: '移动到…' }));
    expect(api.check).toHaveBeenCalledWith({ folder: 'D:\\AuraData' });
    const dialog = await screen.findByRole('dialog');
    expect(dialog.textContent).toContain('搬到D:\\AuraData？');
    expect(dialog.textContent).toContain('要复制 8.70 GB，这个磁盘还有 400.00 GB。');
    expect(
      within(dialog)
        .getAllByRole('listitem')
        .map((item) => item.textContent),
    ).toEqual(['数据8.00 GB', 'Core 日志120 MB', '界面配置40 MB', '全局记忆2.0 MB']);
    expect(api.requestMove).not.toHaveBeenCalled();
    await user.click(within(dialog).getByRole('button', { name: '关闭并搬' }));
    await waitFor(() => expect(api.restart).toHaveBeenCalledTimes(1));
    expect(api.requestMove).toHaveBeenCalledWith({ folder: 'D:\\AuraData' });
  });

  it('moves the installation in the installed setup program after the user agrees to close Aura', async () => {
    const user = userEvent.setup();
    render(<StorageSettings coreReady />);
    const install = await screen.findByRole('region', { name: '安装' });
    await user.click(within(install).getByRole('button', { name: '移动到…' }));
    const dialog = await screen.findByRole('dialog');
    expect(dialog.textContent).toContain('打开安装程序移动 Aura？');
    expect(api.moveInstall).not.toHaveBeenCalled();
    await user.click(within(dialog).getByRole('button', { name: '关闭并打开' }));
    await waitFor(() => expect(api.moveInstall).toHaveBeenCalledTimes(1));
    expect(api.check).not.toHaveBeenCalled();
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('says so when the installation has no setup program to open', async () => {
    api.moveInstall.mockResolvedValueOnce(ok({ restarting: false }));
    const user = userEvent.setup();
    render(<StorageSettings coreReady />);
    const install = await screen.findByRole('region', { name: '安装' });
    await user.click(within(install).getByRole('button', { name: '移动到…' }));
    await user.click(await screen.findByRole('button', { name: '关闭并打开' }));
    expect((await screen.findByRole('alert')).textContent).toContain('没有找到安装程序');
  });

  it('the installation does not move while a data move waits for the next start', async () => {
    current = overview({ requested: { destination: 'D:\\AuraData' } });
    render(<StorageSettings coreReady />);
    const install = await screen.findByRole('region', { name: '安装' });
    const move = within(install).getByRole<HTMLButtonElement>('button', { name: '移动到…' });
    expect(move.disabled).toBe(true);
  });

  it('in a development run the move waits for the next start and can be taken back', async () => {
    current = overview({ install: null });
    const user = userEvent.setup();
    render(<StorageSettings coreReady />);
    await user.click(await screen.findByRole('button', { name: '移动到…' }));
    await user.click(await screen.findByRole('button', { name: '下次启动时搬' }));
    expect(await screen.findByText(/下次启动时搬到D:\\AuraData/u)).toBeTruthy();
    expect(api.restart).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: '移动到…' }).hasAttribute('disabled')).toBe(true);
    await user.click(screen.getByRole('button', { name: '取消' }));
    expect(api.withdrawMove).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.queryByText(/下次启动时搬到/u)).toBeNull());
  });

  it('a folder Core refuses says every reason under the data and asks nothing', async () => {
    checked = accepted({
      destination: 'E:\\',
      parts: [],
      refusals: ['这是U 盘或移动硬盘，只能选本机的固定磁盘', '路径有 81 个字符，最多 79 个'],
    });
    const user = userEvent.setup();
    render(<StorageSettings coreReady />);
    const data = await screen.findByRole('region', { name: '数据' });
    await user.click(within(data).getByRole('button', { name: '移动到…' }));
    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('不能搬到E:\\：');
    expect(
      within(alert)
        .getAllByRole('listitem')
        .map((item) => item.textContent),
    ).toEqual(['这是U 盘或移动硬盘，只能选本机的固定磁盘', '路径有 81 个字符，最多 79 个']);
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(api.requestMove).not.toHaveBeenCalled();
  });

  it('moved data can go back to the default places', async () => {
    current = overview({ data: { path: 'D:\\AuraData', bytes: 8.7 * GIB }, data_moved: true });
    checked = accepted({ destination: null, free: 80 * GIB });
    const user = userEvent.setup();
    render(<StorageSettings coreReady />);
    await user.click(await screen.findByRole('button', { name: '移回默认位置' }));
    expect(api.check).toHaveBeenCalledWith({ folder: null });
    expect((await screen.findByRole('dialog')).textContent).toContain('搬到默认位置？');
    await user.click(screen.getByRole('button', { name: '关闭并搬' }));
    await waitFor(() => expect(api.requestMove).toHaveBeenCalledWith({ folder: null }));
  });

  it('says why the last move did not happen until the user dismisses it', async () => {
    current = overview({
      failure: {
        at: '2026-11-07T02:00:00Z',
        destination: 'D:\\AuraData',
        reason: '数据库检查没有通过（测试）',
      },
    });
    const user = userEvent.setup();
    render(<StorageSettings coreReady />);
    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain(
      '没有搬到D:\\AuraData：数据库检查没有通过（测试）。数据仍在原来的位置，Aura 照常使用；可以换个位置再搬，或点“知道了”清掉这条记录。',
    );
    await user.click(within(alert).getByRole('button', { name: '知道了' }));
    expect(api.dismissFailure).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.queryByRole('alert')).toBeNull());
  });

  it('a move that was not recorded says the data stays and nothing moves at the next start', async () => {
    current = overview({ install: null });
    api.requestMove.mockResolvedValueOnce({
      ok: false,
      error: { code: 'storage.move_failed', message: 'move failed' },
    } as never);
    const user = userEvent.setup();
    render(<StorageSettings coreReady />);
    await user.click(await screen.findByRole('button', { name: '移动到…' }));
    await user.click(await screen.findByRole('button', { name: '下次启动时搬' }));
    expect((await screen.findByRole('alert')).textContent).toBe(
      '没有记下要搬的位置，数据还在原处，下次启动不会搬；请重试。',
    );
  });

  it('a move that was recorded but Aura did not close says how it will still happen', async () => {
    api.restart.mockResolvedValueOnce({
      ok: false,
      error: { code: 'ipc.main_request_failed', message: 'restart failed' },
    } as never);
    const user = userEvent.setup();
    render(<StorageSettings coreReady />);
    const data = await screen.findByRole('region', { name: '数据' });
    await user.click(within(data).getByRole('button', { name: '移动到…' }));
    await user.click(await screen.findByRole('button', { name: '关闭并搬' }));
    expect((await screen.findByRole('alert')).textContent).toBe(
      'Aura 没能自动关闭，数据还没搬（搬动已记下）。请手动退出 Aura 再打开，启动时会搬。',
    );
  });

  it('a withdrawal that did not happen says the move still happens at the next start', async () => {
    current = overview({ install: null, requested: { destination: 'D:\\AuraData' } });
    api.withdrawMove.mockResolvedValueOnce({
      ok: false,
      error: { code: 'storage.withdraw_failed', message: 'withdraw failed' },
    } as never);
    const user = userEvent.setup();
    render(<StorageSettings coreReady />);
    await user.click(await screen.findByRole('button', { name: '取消' }));
    expect((await screen.findByRole('alert')).textContent).toBe(
      '没有取消，下次启动时仍会搬到D:\\AuraData；请重试。',
    );
  });

  it("the installer's own sentence says why it did not start", async () => {
    const words =
      '安装程序没能启动（可能被 Windows 或安全软件拦截），安装位置没有移动，Aura 保持打开；请稍后再试，或手动运行安装目录里的 Uninstall Aura.exe。';
    api.moveInstall.mockResolvedValueOnce({
      ok: false,
      error: { code: 'ipc.main_request_failed', message: words },
    } as never);
    const user = userEvent.setup();
    render(<StorageSettings coreReady />);
    const install = await screen.findByRole('region', { name: '安装' });
    await user.click(within(install).getByRole('button', { name: '移动到…' }));
    await user.click(await screen.findByRole('button', { name: '关闭并打开' }));
    expect((await screen.findByRole('alert')).textContent).toBe(words);
  });

  it('says the page waits for Core while Core is not ready', () => {
    render(<StorageSettings coreReady={false} />);
    expect(screen.getByRole('status').textContent).toBe('Core 尚未就绪，存储设置暂不可用。');
  });

  it('sizes read the way the setup windows show them', () => {
    const MIB = 1024 ** 2;
    expect(size(0)).toBe('< 0.1 MB');
    expect(size(52_000)).toBe('< 0.1 MB');
    expect(size(104_857)).toBe('< 0.1 MB');
    expect(size(104_858)).toBe('0.1 MB');
    expect(size(3.5 * MIB)).toBe('3.5 MB');
    expect(size(10 * MIB)).toBe('10 MB');
    expect(size(917_294_454)).toBe('875 MB');
    expect(size(8.61 * GIB)).toBe('8.61 GB');
  });
});
