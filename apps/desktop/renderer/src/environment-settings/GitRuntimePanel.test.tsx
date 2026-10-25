import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { GitRuntimePanel } from './GitRuntimePanel';
const state = (value: string) => ({
  ok: true,
  value: {
    state: value,
    executable: 'C:/Git/cmd/git.exe',
    version: value === 'too_old' ? 'git version 2.39.0' : 'git version 2.40.1',
    message:
      value === 'available'
        ? '本机 Git 可用'
        : value === 'missing'
          ? '未找到本机 Git'
          : '本机 Git 版本低于 2.40',
    next_step: '安装或升级后重新检测',
    installation_url: 'https://git-scm.com/download/win',
  },
});
afterEach(cleanup);
function setup(runtime = vi.fn().mockResolvedValue(state('available'))) {
  const git = {
    runtime,
    openInstallation: vi.fn().mockResolvedValue({ ok: true, value: { opened: true } }),
  };
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1: { git } } });
  render(<GitRuntimePanel />);
  return git;
}
test('missing and old versions explain the gate, open installation, and recover after redetection', async () => {
  const git = setup(
    vi
      .fn()
      .mockResolvedValueOnce(state('missing'))
      .mockResolvedValueOnce(state('too_old'))
      .mockResolvedValue(state('available')),
  );
  expect(await screen.findByText('未找到本机 Git')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: '前往 Git 官网安装或升级' }));
  expect(git.openInstallation).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole('button', { name: '重新检测' }));
  expect(await screen.findByText('本机 Git 版本低于 2.40')).toBeTruthy();
  expect(screen.getByText('git version 2.39.0')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: '重新检测' }));
  expect(await screen.findByText('本机 Git 可用')).toBeTruthy();
  expect(screen.queryByRole('button', { name: '前往 Git 官网安装或升级' })).toBeNull();
});
test('failed detection stays visible and retry clears the error', async () => {
  setup(
    vi
      .fn()
      .mockRejectedValueOnce(new Error('test transport unavailable'))
      .mockResolvedValue(state('available')),
  );
  expect(await screen.findByRole('alert')).toHaveProperty(
    'textContent',
    'Git 检测未完成，请重试。',
  );
  fireEvent.click(screen.getByRole('button', { name: '重新检测' }));
  expect(await screen.findByText('本机 Git 可用')).toBeTruthy();
  expect(screen.queryByRole('alert')).toBeNull();
});
