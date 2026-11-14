import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { OldDataEntry, OldDataRemoval } from '@aura/contracts/generated';

import { OldData } from './OldData';

const NAMESPACE = 'namespace-961d23db09870b4258ab1f48d367ca5d';
// Midday UTC, so the day shown is the same in every time zone the tests run in.
const early: OldDataEntry = {
  id: 'early',
  kind: 'early_layout',
  paths: [
    'C:\\Users\\me\\AppData\\Local\\Aura\\data',
    'C:\\Users\\me\\AppData\\Roaming\\Aura\\aura.toml',
  ],
  bytes: 52_000,
  modified_at: '2026-09-01T12:00:00Z',
};
const orphan: OldDataEntry = {
  id: NAMESPACE,
  kind: 'namespace',
  paths: ['C:\\Users\\me\\AppData\\Local\\Aura\\namespaces\\961d23db09870b4258ab1f48d367ca5d'],
  bytes: 875 * 1024 ** 2,
  modified_at: '2026-09-24T12:00:00Z',
};

const ok = <T,>(value: T) => ({ ok: true as const, value });
let listed: OldDataEntry[];
let removal: OldDataRemoval;
const api = {
  oldData: vi.fn(async () => ok({ items: listed })),
  removeOldData: vi.fn(async () => ok(removal)),
};

beforeEach(() => {
  listed = [early, orphan];
  removal = { removed: [], in_use: [], failed: [], items: [] };
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1: { storage: api } } });
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const boxes = (region: HTMLElement) =>
  within(region)
    .getAllByRole('checkbox')
    .map((box) => (box as HTMLInputElement).checked);

describe('设置 → 系统 → 存储 → 旧数据', () => {
  it('lists each item ticked with its size, day and folders, and removes the ticked ones once confirmed', async () => {
    const user = userEvent.setup();
    removal = { removed: [NAMESPACE], in_use: [], failed: [], items: [early] };
    render(<OldData coreReady />);
    const region = await screen.findByRole('region', { name: '旧数据' });
    expect(boxes(region)).toEqual([true, true]);
    const rows = within(region)
      .getAllByRole('listitem')
      .map((row) => row.textContent);
    expect(rows[0]).toContain('早期版本的数据< 0.1 MB · 2026-09-01');
    expect(rows[0]).toContain('C:\\Users\\me\\AppData\\Roaming\\Aura\\aura.toml');
    expect(rows[1]).toContain('旧安装的数据 961d23db875 MB · 2026-09-24');

    await user.click(within(region).getByRole('checkbox', { name: /早期版本的数据/u }));
    await user.click(within(region).getByRole('button', { name: '删除所选' }));
    const dialog = await screen.findByRole('dialog');
    expect(dialog.textContent).toContain('删除所选的旧数据？');
    expect(dialog.textContent).toContain('共 875 MB');
    expect(
      within(dialog)
        .getAllByRole('listitem')
        .map((item) => item.textContent),
    ).toEqual(['旧安装的数据 961d23db875 MB']);
    expect(api.removeOldData).not.toHaveBeenCalled();
    await user.click(within(dialog).getByRole('button', { name: '删除' }));
    await waitFor(() => expect(api.removeOldData).toHaveBeenCalledWith({ ids: [NAMESPACE] }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(within(region).getAllByRole('listitem')).toHaveLength(1);
    expect(boxes(region)).toEqual([false]);
    expect(within(region).getByRole('button', { name: '删除所选' })).toHaveProperty(
      'disabled',
      true,
    );
  });

  it('keeps an item that is in use and says so', async () => {
    const user = userEvent.setup();
    removal = { removed: [], in_use: [NAMESPACE], failed: [], items: [orphan] };
    listed = [orphan];
    render(<OldData coreReady />);
    const region = await screen.findByRole('region', { name: '旧数据' });
    await user.click(within(region).getByRole('button', { name: '删除所选' }));
    await user.click(
      within(await screen.findByRole('dialog')).getByRole('button', { name: '删除' }),
    );
    expect(await within(region).findByRole('status')).toHaveProperty(
      'textContent',
      '旧安装的数据 961d23db 正在使用，没有删',
    );
    expect(boxes(region)).toEqual([false]);
  });

  it('is absent while there is none, and is not read before Core is ready', async () => {
    const { rerender } = render(<OldData coreReady={false} />);
    expect(api.oldData).not.toHaveBeenCalled();
    listed = [];
    rerender(<OldData coreReady />);
    await waitFor(() => expect(api.oldData).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole('region', { name: '旧数据' })).toBeNull();
  });
});

describe('旧数据 that was not removed', () => {
  it('says why and that no space was freed, as an alert', async () => {
    const user = userEvent.setup();
    api.removeOldData.mockResolvedValueOnce({
      ok: false,
      error: { code: 'ipc.local_overload', message: 'too many' },
    } as never);
    render(<OldData coreReady />);
    const region = await screen.findByRole('region', { name: '旧数据' });
    await user.click(within(region).getByRole('button', { name: '删除所选' }));
    await user.click(
      within(await screen.findByRole('dialog')).getByRole('button', { name: '删除' }),
    );
    expect((await within(region).findByRole('alert')).textContent).toBe(
      '旧数据没有删除（同时进行的操作太多），空间没有释放；请稍后重试。',
    );
  });
});
