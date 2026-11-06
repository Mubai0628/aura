import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ComputerControlledV1 } from '@aura/contracts/generated';

import { ComputerControlling } from './ComputerControlling';
import { desktopState, useComputerControlling } from './useComputerControlling';

const S = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const NOTEPAD = { app_id: 'Microsoft.WindowsNotepad_8wekyb3d8bbwe!App', name: '记事本' };
const PAINT = { app_id: 'Microsoft.Paint_8wekyb3d8bbwe!App', name: '画图' };
let current: ComputerControlledV1[];
const controlling = vi.fn(async () => ({ ok: true as const, value: { apps: current } }));
const stop = vi.fn(async () => ({
  ok: true as const,
  value: { apps: [] as ComputerControlledV1[] },
}));

function Part({ session }: { session: string }) {
  return <ComputerControlling controlling={useComputerControlling(session)} />;
}

beforeEach(() => {
  current = [];
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { computer: { controlling, stop } } },
  });
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('正在操控 in the activity card', () => {
  it('names the child desktop and shows the real login and shortcut status', async () => {
    controlling.mockResolvedValueOnce({
      ok: true,
      value: {
        apps: [{ ...NOTEPAD, foreground: true, mode: 'child_session' }],
        pip: { phase: 'waiting_for_login', notice: '停止快捷键已被占用，请修改快捷键' },
      },
    } as never);
    render(<Part session={S} />);
    expect(await screen.findByText('小窗')).toBeTruthy();
    expect(screen.getByText('等待登录小窗')).toBeTruthy();
    expect(screen.getByText('停止快捷键已被占用，请修改快捷键')).toBeTruthy();
  });
  it('says the user took a program over, with nothing left to stop', async () => {
    current = [
      { ...NOTEPAD, foreground: true, taken_over: true },
      { ...PAINT, foreground: true },
    ];
    render(<Part session={S} />);
    expect(await screen.findByText('你已接管')).toBeTruthy();
    expect(screen.queryByRole('button', { name: '停止操控 记事本' })).toBeNull();
    expect(screen.getByRole('button', { name: '停止操控 画图' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: '全部停止' })).toBeNull();
  });
  it('says nothing while the model controls no program', async () => {
    const { container } = render(<Part session={S} />);
    await vi.waitFor(() => expect(controlling).toHaveBeenCalledWith({ session_id: S }));
    expect(container.textContent).toBe('');
  });

  it('lists the program with whether it has the mouse and keyboard, and stops that one', async () => {
    current = [{ ...NOTEPAD, foreground: false }];
    render(<Part session={S} />);
    expect(await screen.findByText('记事本')).toBeTruthy();
    expect(screen.getByText('后台')).toBeTruthy();
    expect(screen.queryByRole('button', { name: '全部停止' })).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: '停止操控 记事本' }));
    expect(stop).toHaveBeenCalledWith({ session_id: S, app_id: NOTEPAD.app_id });
    await vi.waitFor(() => expect(screen.queryByText('记事本')).toBeNull());
  });

  it('stops every program at once when there are two or more', async () => {
    current = [
      { ...NOTEPAD, foreground: true },
      { ...PAINT, foreground: false },
    ];
    render(<Part session={S} />);
    expect(await screen.findByText('前台')).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: '全部停止' }));
    expect(stop).toHaveBeenCalledWith({ session_id: S });
  });

  it('says why a stop did not take effect and keeps the button', async () => {
    current = [{ ...NOTEPAD, foreground: false }];
    stop.mockResolvedValueOnce({
      ok: false,
      error: { code: 'computer.unavailable', message: 'computer.unavailable: 电脑操控不可用' },
    } as never);
    render(<Part session={S} />);
    await userEvent.click(await screen.findByRole('button', { name: '停止操控 记事本' }));
    expect((await screen.findByRole('alert')).textContent).toBe('电脑操控不可用');
    expect(screen.getByRole('button', { name: '停止操控 记事本' })).toBeTruthy();
  });

  it('counts for the collapsed card only the programs the model still controls', () => {
    const taken = { ...NOTEPAD, foreground: true, taken_over: true };
    const paint = { ...PAINT, foreground: false };
    expect(desktopState([])).toEqual({ controlled: 0, takenOver: false });
    expect(desktopState([paint])).toEqual({ controlled: 1, takenOver: false });
    expect(desktopState([taken])).toEqual({ controlled: 0, takenOver: true });
    expect(desktopState([taken, paint])).toEqual({ controlled: 1, takenOver: true });
  });

  it('shows another conversation nothing of this one', async () => {
    current = [{ ...NOTEPAD, foreground: false }];
    const { rerender } = render(<Part session={S} />);
    expect(await screen.findByText('记事本')).toBeTruthy();
    controlling.mockImplementationOnce(() => new Promise(() => undefined));
    rerender(<Part session="ses_01ARZ3NDEKTSV4RRFFQ69G5FAW" />);
    expect(screen.queryByText('记事本')).toBeNull();
  });
});
