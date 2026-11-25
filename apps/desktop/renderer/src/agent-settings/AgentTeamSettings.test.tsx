import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  AgentTeamLimits,
  AgentTeamPermissions,
  AgentTeamSettingsView,
} from '@aura/contracts/generated';

import { AgentTeamSettings } from './AgentTeamSettings';

const DEFAULTS: AgentTeamLimits = {
  max_running: 3,
  token_budget: null,
  minutes_per_task: 120,
  cpu_percent: 75,
};
const PERMISSIONS: AgentTeamPermissions = {
  worktree_mode: 'workspace_read_write',
  shared_mode: 'read_only',
  allow_deletions: false,
  allow_outside_writes: false,
  allow_downloads: false,
  main_agent_approves: false,
};
let current: AgentTeamSettingsView;
const ok = <T,>(value: T) => ({ ok: true as const, value });
const refusal = (message: string) => ({
  ok: false as const,
  error: { code: 'domain_error', message, retryable: false },
});
const api = {
  readAgentTeamSettings: vi.fn(async () => ok(current)),
  setAgentTeamSettings: vi.fn(
    async (request: {
      limits: AgentTeamLimits;
      enabled?: boolean;
      permissions?: AgentTeamPermissions;
      expected_revision: number;
    }): Promise<ReturnType<typeof ok<AgentTeamSettingsView>> | ReturnType<typeof refusal>> => {
      if (request.expected_revision !== current.revision)
        return refusal(
          'agents.limits_changed: 多 Agent 的设置刚被改过，这次没有保存，请在最新的设置上再改。',
        );
      current = {
        ...current,
        limits: request.limits,
        enabled: request.enabled ?? current.enabled,
        permissions: request.permissions ?? current.permissions,
        revision: current.revision + 1,
      };
      return ok(current);
    },
  ),
};

beforeEach(() => {
  current = {
    enabled: true,
    limits: DEFAULTS,
    defaults: DEFAULTS,
    minimum: { max_running: 0, token_budget: 100_000, minutes_per_task: 5, cpu_percent: 10 },
    maximum: {
      max_running: 8,
      token_budget: 100_000_000,
      minutes_per_task: 480,
      cpu_percent: 100,
    },
    permissions: PERMISSIONS,
    permission_defaults: PERMISSIONS,
    revision: 0,
  };
  Object.values(api).forEach((mock) => mock.mockClear());
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1: api } });
});
afterEach(cleanup);

const field = (name: string) => screen.findByRole('spinbutton', { name });
const unlimited = () => screen.findByRole('checkbox', { name: '不限' });

describe('设置 → 智能能力 → 多 Agent', () => {
  it('accepts an exact integer token limit through form submission', async () => {
    const user = userEvent.setup();
    render(<AgentTeamSettings coreReady />);
    await user.click(await unlimited());
    const input = (await field('每个子 Agent 的 token 上限')) as HTMLInputElement;
    await user.type(input, '6757756');
    expect(input.validity.stepMismatch).toBe(false);
    await user.keyboard('{Enter}');
    await waitFor(() =>
      expect(api.setAgentTeamSettings).toHaveBeenCalledWith({
        limits: { ...DEFAULTS, token_budget: 6757756 },
        expected_revision: 0,
      }),
    );
  });

  it('does not repeat an unconfirmed limits write until the user rereads', async () => {
    const user = userEvent.setup();
    render(<AgentTeamSettings coreReady />);
    const running = await field('同时运行的子 Agent');
    await user.clear(running);
    await user.type(running, '5');
    api.setAgentTeamSettings.mockRejectedValueOnce(new Error('transport lost after send'));
    const save = screen.getByRole('button', { name: /^保存$/ });
    await user.click(save);
    expect((await screen.findByRole('alert')).textContent).toContain('尚未确认');
    await user.click(save);
    expect(api.setAgentTeamSettings).toHaveBeenCalledTimes(1);
    current = { ...current, limits: { ...DEFAULTS, max_running: 5 }, revision: 1 };
    await user.click(screen.getByRole('button', { name: '重新读取' }));
    await waitFor(() => expect(running).toHaveProperty('value', '5'));
    expect(api.setAgentTeamSettings).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole('switch', { name: '允许多 Agent' })).toBeNull();
  });

  it('accepts zero temporary concurrency without turning off collaboration', async () => {
    const user = userEvent.setup();
    render(<AgentTeamSettings coreReady />);
    const running = await field('同时运行的子 Agent');
    await user.clear(running);
    await user.type(running, '0');
    await user.click(screen.getByRole('button', { name: '保存' }));
    await waitFor(() =>
      expect(api.setAgentTeamSettings).toHaveBeenCalledWith({
        limits: { ...DEFAULTS, max_running: 0 },
        expected_revision: 0,
      }),
    );
    expect(current.enabled).toBe(true);
  });

  it('shows the limits and saves a changed one over the revision it read', async () => {
    const user = userEvent.setup();
    render(<AgentTeamSettings coreReady />);
    const running = await field('同时运行的子 Agent');
    expect(running).toHaveProperty('value', '3');
    // By default the tokens are not limited: no number, and 不限 ticked.
    const tokens = await field('每个子 Agent 的 token 上限');
    expect(tokens).toHaveProperty('value', '');
    expect(tokens).toHaveProperty('disabled', true);
    expect(await unlimited()).toHaveProperty('checked', true);
    expect(await field('每个任务的用时上限')).toHaveProperty('value', '120');
    expect(await field('子 Agent 命令合计的 CPU 占比')).toHaveProperty('value', '75');
    const save = screen.getByRole('button', { name: '保存' });
    expect(save).toHaveProperty('disabled', true);
    expect(screen.queryByRole('button', { name: '恢复默认' })).toBeNull();

    await user.clear(running);
    await user.type(running, '5');
    await user.click(save);
    await waitFor(() =>
      expect(api.setAgentTeamSettings).toHaveBeenCalledWith({
        limits: { ...DEFAULTS, max_running: 5 },
        expected_revision: 0,
      }),
    );
    await waitFor(() =>
      expect(screen.getByRole('button', { name: '保存' })).toHaveProperty('disabled', true),
    );
    expect(screen.getByRole('button', { name: '恢复默认' })).toBeTruthy();
  });

  it('sets a token budget once 不限 is cleared, and takes it away again', async () => {
    const user = userEvent.setup();
    render(<AgentTeamSettings coreReady />);
    const tokens = await field('每个子 Agent 的 token 上限');
    await user.click(await unlimited());
    expect(tokens).toHaveProperty('disabled', false);
    // Nothing typed yet: the range is named and nothing can be saved.
    expect(screen.getByRole('alert').textContent).toContain(
      '每个子 Agent 的 token 上限：请输入 100000 到 100000000 的整数。',
    );
    expect(screen.getByRole('button', { name: '保存' })).toHaveProperty('disabled', true);
    await user.type(tokens, '2000000');
    await user.click(screen.getByRole('button', { name: '保存' }));
    await waitFor(() =>
      expect(api.setAgentTeamSettings).toHaveBeenLastCalledWith({
        limits: { ...DEFAULTS, token_budget: 2_000_000 },
        expected_revision: 0,
      }),
    );
    await waitFor(() => expect(tokens).toHaveProperty('value', '2000000'));
    await user.click(await unlimited());
    await user.click(screen.getByRole('button', { name: '保存' }));
    await waitFor(() =>
      expect(api.setAgentTeamSettings).toHaveBeenLastCalledWith({
        limits: { ...DEFAULTS, token_budget: null },
        expected_revision: 1,
      }),
    );
    await waitFor(() => expect(screen.queryByRole('button', { name: '恢复默认' })).toBeNull());
  });

  it('keeps a number outside the range from being saved and says which', async () => {
    const user = userEvent.setup();
    render(<AgentTeamSettings coreReady />);
    const minutes = await field('每个任务的用时上限');
    await user.clear(minutes);
    await user.type(minutes, '1');
    expect(minutes.getAttribute('aria-invalid')).toBe('true');
    expect(screen.getByRole('alert').textContent).toContain(
      '每个任务的用时上限：请输入 5 到 480 的整数。',
    );
    expect(screen.getByRole('button', { name: '保存' })).toHaveProperty('disabled', true);
    expect(api.setAgentTeamSettings).not.toHaveBeenCalled();
  });

  it('goes back to the defaults in one click', async () => {
    current = { ...current, limits: { ...DEFAULTS, cpu_percent: 40 }, revision: 2 };
    const user = userEvent.setup();
    render(<AgentTeamSettings coreReady />);
    await user.click(await screen.findByRole('button', { name: '恢复默认' }));
    await waitFor(() =>
      expect(api.setAgentTeamSettings).toHaveBeenCalledWith({
        limits: DEFAULTS,
        expected_revision: 2,
      }),
    );
    expect(await field('子 Agent 命令合计的 CPU 占比')).toHaveProperty('value', '75');
    await waitFor(() => expect(screen.queryByRole('button', { name: '恢复默认' })).toBeNull());
  });

  it('changed elsewhere, it says the save did not go in and shows what is saved now', async () => {
    const user = userEvent.setup();
    render(<AgentTeamSettings coreReady />);
    const running = await field('同时运行的子 Agent');
    current = { ...current, limits: { ...DEFAULTS, max_running: 6 }, revision: 1 };
    await user.clear(running);
    await user.type(running, '4');
    await user.click(screen.getByRole('button', { name: '保存' }));
    expect((await screen.findByRole('alert')).textContent).toContain('这次没有保存');
    await waitFor(() => expect(running).toHaveProperty('value', '6'));
    expect(api.readAgentTeamSettings).toHaveBeenCalledTimes(2);
  });

  it('saves a preset and the approval switch at once, keeping limits typed and not saved', async () => {
    const user = userEvent.setup();
    render(<AgentTeamSettings coreReady />);
    const running = await field('同时运行的子 Agent');
    await user.clear(running);
    await user.type(running, '5');
    await user.click(screen.getByRole('switch', { name: '命令删除文件' }));
    await waitFor(() =>
      expect(api.setAgentTeamSettings).toHaveBeenLastCalledWith({
        limits: DEFAULTS,
        permissions: { ...PERMISSIONS, allow_deletions: true },
        expected_revision: 0,
      }),
    );
    await waitFor(() =>
      expect(
        screen.getByRole('switch', { name: '命令删除文件' }).getAttribute('aria-checked'),
      ).toBe('true'),
    );
    expect(running).toHaveProperty('value', '5');
    await user.click(screen.getByRole('switch', { name: '主 Agent 代批' }));
    await waitFor(() =>
      expect(api.setAgentTeamSettings).toHaveBeenLastCalledWith({
        limits: DEFAULTS,
        permissions: { ...PERMISSIONS, allow_deletions: true, main_agent_approves: true },
        expected_revision: 1,
      }),
    );
  });

  it('asks before a default of 完全访问 and saves it only once confirmed', async () => {
    const user = userEvent.setup();
    render(<AgentTeamSettings coreReady />);
    const worktree = await screen.findByRole('combobox', { name: '有工作副本的子 Agent' });
    await user.selectOptions(worktree, '完全访问');
    await user.click(await screen.findByRole('button', { name: '取消' }));
    expect(api.setAgentTeamSettings).not.toHaveBeenCalled();
    expect(worktree).toHaveProperty('value', 'workspace_read_write');
    await user.selectOptions(worktree, '完全访问');
    await user.click(await screen.findByRole('button', { name: '启用完全访问' }));
    await waitFor(() =>
      expect(api.setAgentTeamSettings).toHaveBeenLastCalledWith({
        limits: DEFAULTS,
        permissions: { ...PERMISSIONS, worktree_mode: 'full_access' },
        expected_revision: 0,
      }),
    );
    await waitFor(() => expect(worktree).toHaveProperty('value', 'full_access'));
    // Another mode is saved without asking; the defaults come back in one click.
    await user.selectOptions(
      screen.getByRole('combobox', { name: '共用工作区的子 Agent' }),
      '工作区读写',
    );
    await waitFor(() =>
      expect(api.setAgentTeamSettings).toHaveBeenLastCalledWith({
        limits: DEFAULTS,
        permissions: {
          ...PERMISSIONS,
          worktree_mode: 'full_access',
          shared_mode: 'workspace_read_write',
        },
        expected_revision: 1,
      }),
    );
    await user.click(await screen.findByRole('button', { name: '恢复默认权限' }));
    await waitFor(() =>
      expect(api.setAgentTeamSettings).toHaveBeenLastCalledWith({
        limits: DEFAULTS,
        permissions: PERMISSIONS,
        expected_revision: 2,
      }),
    );
    await waitFor(() => expect(screen.queryByRole('button', { name: '恢复默认权限' })).toBeNull());
  });

  it('says it could not read the settings and reads them again on 重试', async () => {
    api.readAgentTeamSettings.mockRejectedValueOnce(new Error('pipe closed'));
    const user = userEvent.setup();
    render(<AgentTeamSettings coreReady />);
    expect((await screen.findByRole('alert')).textContent).toContain('读不到多 Agent 的设置');
    await user.click(screen.getByRole('button', { name: '重试' }));
    expect(await field('同时运行的子 Agent')).toHaveProperty('value', '3');
    expect(screen.queryByRole('alert')).toBeNull();
  });
});

describe('多 Agent before Core is ready', () => {
  it('says the page waits for Core', () => {
    render(<AgentTeamSettings coreReady={false} />);
    expect(screen.getByText('Core 尚未就绪，多 Agent 设置暂不可用。')).toBeTruthy();
  });
});
