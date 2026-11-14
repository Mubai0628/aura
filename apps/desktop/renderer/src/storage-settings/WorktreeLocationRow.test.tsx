import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { WorktreeLocationView } from '@aura/contracts/generated';

import { WorktreeLocationRow } from './WorktreeLocationRow';
import { size } from './size';

const GIB = 1024 ** 3;
const DEFAULT = 'C:\\Users\\me\\AuraWorktrees';
let current: WorktreeLocationView;
const ok = <T,>(value: T) => ({ ok: true as const, value });
const api = {
  readWorktreeLocation: vi.fn(async () => ok(current)),
  selectWorktreeLocation: vi.fn(async () => ok({ path: 'D:\\Worktrees' as string | null })),
  setWorktreeLocation: vi.fn(
    async (request: { location?: string | null; expected_revision: number }) => {
      current = {
        ...current,
        location: request.location ?? DEFAULT,
        chosen: request.location != null,
        revision: current.revision + 1,
      };
      return ok(current);
    },
  ),
};

beforeEach(() => {
  current = {
    location: DEFAULT,
    chosen: false,
    default_location: DEFAULT,
    revision: 3,
    worktrees: 2,
    size_bytes: 3 * GIB,
    free_bytes: 120 * GIB,
  };
  Object.values(api).forEach((mock) => mock.mockClear());
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: api },
  });
});
afterEach(cleanup);

describe('设置 → 存储 → 工作副本', () => {
  it('shows where new worktrees go, how many there are, their size and the free space', async () => {
    render(<WorktreeLocationRow />);
    const row = await screen.findByRole('region', { name: '工作副本' });
    expect(row.textContent).toContain(DEFAULT);
    expect(row.textContent).toContain('2 个');
    expect(row.textContent).toContain(size(3 * GIB));
    expect(row.textContent).toContain(`剩余 ${size(120 * GIB)}`);
    expect(screen.queryByRole('button', { name: '恢复默认' })).toBeNull();
  });

  it('saves a folder chosen in the dialog, then goes back to the default', async () => {
    const user = userEvent.setup();
    render(<WorktreeLocationRow />);
    await user.click(await screen.findByRole('button', { name: '更改…' }));
    await waitFor(() =>
      expect(api.setWorktreeLocation).toHaveBeenCalledWith({
        expected_revision: 3,
        location: 'D:\\Worktrees',
      }),
    );
    expect(await screen.findByText('D:\\Worktrees')).toBeTruthy();
    await user.click(screen.getByRole('button', { name: '恢复默认' }));
    await waitFor(() =>
      expect(api.setWorktreeLocation).toHaveBeenLastCalledWith({ expected_revision: 4 }),
    );
    expect(await screen.findByText(DEFAULT)).toBeTruthy();
  });

  it('keeps the folder when the dialog is cancelled and says why a folder was refused', async () => {
    const user = userEvent.setup();
    api.selectWorktreeLocation.mockResolvedValueOnce(ok({ path: null }));
    render(<WorktreeLocationRow />);
    await user.click(await screen.findByRole('button', { name: '更改…' }));
    expect(api.setWorktreeLocation).not.toHaveBeenCalled();
    api.setWorktreeLocation.mockResolvedValueOnce({
      ok: false,
      error: {
        code: 'agents.location_in_aura_data',
        message: '工作副本不能放进 Aura 的数据位置。',
      },
    } as never);
    await user.click(screen.getByRole('button', { name: '更改…' }));
    expect((await screen.findByRole('alert')).textContent).toContain(
      '工作副本不能放进 Aura 的数据位置',
    );
    expect(screen.getByText(DEFAULT)).toBeTruthy();
  });
  it('a change that did not go through says new worktrees stay where they were', async () => {
    const user = userEvent.setup();
    api.setWorktreeLocation.mockResolvedValueOnce({
      ok: false,
      error: { code: 'ipc.request_not_sent', message: 'not sent' },
    } as never);
    render(<WorktreeLocationRow />);
    await user.click(await screen.findByRole('button', { name: '更改…' }));
    expect((await screen.findByRole('alert')).textContent).toBe(
      '工作副本位置没有改（请求没有发出），新的工作副本仍放在原位置；请重试。',
    );
  });
  it('a location that was not read says why and reads again from its retry button', async () => {
    const user = userEvent.setup();
    api.readWorktreeLocation.mockResolvedValueOnce({
      ok: false,
      error: { code: 'ipc.request_timeout', message: 'Core request timed out' },
    } as never);
    render(<WorktreeLocationRow />);
    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toBe(
      '工作副本的位置没有读到（Aura Core 没有及时回复），暂时不能更改；请点“重试”。重试',
    );
    await user.click(within(alert).getByRole('button', { name: '重试' }));
    expect(await screen.findByText(DEFAULT)).toBeTruthy();
    await waitFor(() => expect(screen.queryByRole('alert')).toBeNull());
  });
});
