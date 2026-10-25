import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import { ConfigRecoveryPanel } from './ConfigRecoveryPanel';

afterEach(cleanup);
test('all-domain reset requires acknowledgement bound to the actual preview', async () => {
  const modules = [
    'aura',
    'models',
    'tools',
    'hooks',
    'mcp',
    'skills',
    'commands',
    'plugins',
    'ui',
    'prompt',
    'security',
    'browser',
    'memory',
    'diagnostics',
    'update',
  ];
  const api = setup({
    getConfigRecoveryNotice: vi.fn().mockResolvedValue({
      ok: true,
      value: { ...notice, scope_known: false, affected_domains: modules },
    }),
    planConfigRecovery: vi.fn().mockResolvedValue({
      ok: true,
      value: {
        ...plan,
        modules,
        preview_id: 'whole-current',
        domains: modules.map((module) => ({ ...domain, module })),
        options: [{ action: 'reset_to_default', available: true, reason_code: null, detail: null }],
      },
    }),
  });
  await userEvent.click(await screen.findByRole('button', { name: '查看恢复选项' }));
  const confirm = await screen.findByRole('button', { name: '确认重置全部配置' });
  expect((confirm as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByText(/现有非保密设置将丢失，凭据绑定可能需要重新设置/u)).toBeTruthy();
  expect(api.applyConfigRecovery).not.toHaveBeenCalled();
  await userEvent.click(
    screen.getByRole('checkbox', { name: '我确认重置全部配置，现有非保密设置将丢失' }),
  );
  await userEvent.click(confirm);
  await waitFor(() => expect(api.applyConfigRecovery).toHaveBeenCalledTimes(1));
  expect(api.applyConfigRecovery).toHaveBeenCalledWith({
    action: 'reset_to_default',
    domains: modules,
    preview_id: 'whole-current',
  });
});
const notice = {
  affected_domains: ['tools'],
  unavailable_capabilities: ['tool_policy'],
  fault_codes: ['config.domain.unavailable'],
  suspended_calls: ['new tool execution'],
  available_actions: ['retry', 'restore_previous_valid', 'reset_to_default'],
  scope_known: true,
  recovery_in_progress: false,
  next_step: 'preview',
};
const domain = {
  module: 'tools',
  degraded: true,
  fault_code: 'config.domain.unavailable',
  current_revision: 3,
  options: [{ action: 'retry', available: true, reason_code: null, detail: null }],
  recoverable_versions: [],
  next_revision: 4,
  references_needing_rebinding: [],
  loss: [],
  scope_known: true,
  preview_id: 'domain-visible',
};
const plan = {
  modules: ['tools'],
  options: domain.options,
  domains: [domain],
  consistent_revision: null,
  inconsistency_code: null,
  consistent_set: [],
  preview_id: 'core-bundle-visible',
};
function setup(overrides = {}, coreReady = true) {
  const api = {
    getConfigRecoveryNotice: vi.fn().mockResolvedValue({ ok: true, value: notice }),
    planConfigRecovery: vi.fn().mockResolvedValue({ ok: true, value: plan }),
    applyConfigRecovery: vi.fn().mockResolvedValue({
      ok: true,
      value: {
        action: 'retry',
        outcomes: [
          {
            module: 'tools',
            action: 'retry',
            applied: true,
            revision: 4,
            source: 'file',
            reason_code: null,
          },
        ],
        restored_capabilities: ['tool_policy'],
        unavailable_capabilities: [],
        reason_code: null,
      },
    }),
    ...overrides,
  };
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1: api } });
  const view = render(<ConfigRecoveryPanel coreReady={coreReady} />);
  return { ...api, view };
}

test('opening settings during startup waits for actual readiness before reading recovery', async () => {
  const api = setup({}, false);
  expect(api.getConfigRecoveryNotice).not.toHaveBeenCalled();
  api.view.rerender(<ConfigRecoveryPanel coreReady />);
  await screen.findByRole('heading', { name: '设置需要恢复' });
  expect(api.getConfigRecoveryNotice).toHaveBeenCalledTimes(1);
});

test('preview reads only; explicit confirmation echoes the exact single-domain token', async () => {
  const api = setup();
  await screen.findByRole('heading', { name: '设置需要恢复' });
  await userEvent.click(screen.getByRole('button', { name: '查看恢复选项' }));
  await screen.findByRole('heading', { name: '确认恢复范围' });
  expect(api.applyConfigRecovery).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole('button', { name: '确认重试当前配置' }));
  await waitFor(() => expect(api.applyConfigRecovery).toHaveBeenCalledTimes(1));
  expect(api.applyConfigRecovery).toHaveBeenCalledWith({
    action: 'retry',
    domain: 'tools',
    domains: [],
    preview_id: 'domain-visible',
  });
});

test('bundle confirmation sends the Core token without hashing or per-domain apply loops', async () => {
  const api = setup({
    planConfigRecovery: vi.fn().mockResolvedValue({
      ok: true,
      value: {
        ...plan,
        modules: ['tools', 'models'],
        domains: [domain, { ...domain, module: 'models', preview_id: 'second' }],
      },
    }),
  });
  await userEvent.click(await screen.findByRole('button', { name: '查看恢复选项' }));
  await userEvent.click(await screen.findByRole('button', { name: '确认重试当前配置' }));
  await waitFor(() => expect(api.applyConfigRecovery).toHaveBeenCalledTimes(1));
  expect(api.applyConfigRecovery).toHaveBeenCalledWith({
    action: 'retry',
    domains: ['tools', 'models'],
    preview_id: 'core-bundle-visible',
  });
});

test('stale confirmation is discarded, preserves new settings and never automatically retries', async () => {
  const apply = vi.fn().mockResolvedValue({
    ok: false,
    error: { code: 'config.recovery.preview_stale', terminal: 'rejected' },
  });
  setup({ applyConfigRecovery: apply });
  await userEvent.click(await screen.findByRole('button', { name: '查看恢复选项' }));
  await userEvent.click(await screen.findByRole('button', { name: '确认重试当前配置' }));
  await screen.findByText(/本次未覆盖新设置/u);
  expect(apply).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole('button', { name: '确认重试当前配置' })).toBeNull();
});

test('unknown result cannot be submitted again, and a failed state read cannot clear that protection', async () => {
  const get = vi
    .fn()
    .mockResolvedValueOnce({ ok: true, value: notice })
    .mockResolvedValue({
      ok: false,
      error: {
        code: 'ipc.request_timeout',
        terminal: 'failed',
        details: { outcome_recovery: 'query_runtime.command_outcome_without_replay' },
      },
    });
  const apply = vi.fn().mockResolvedValue({
    ok: false,
    error: {
      code: 'config.recovery.apply_boundary_unknown',
      terminal: 'failed',
      details: { outcome_recovery: 'query_runtime.command_outcome_without_replay' },
    },
  });
  setup({ applyConfigRecovery: apply, getConfigRecoveryNotice: get });
  await userEvent.click(await screen.findByRole('button', { name: '查看恢复选项' }));
  await userEvent.click(await screen.findByRole('button', { name: '确认重试当前配置' }));
  await screen.findByText(/恢复结果尚未确认/u);
  expect(screen.queryByRole('button', { name: '查看恢复选项' })).toBeNull();
  await userEvent.click(screen.getByRole('button', { name: '检查当前状态' }));
  await screen.findByText(/暂时无法查看恢复状态/u);
  expect(screen.queryByRole('button', { name: '查看恢复选项' })).toBeNull();
  expect(apply).toHaveBeenCalledTimes(1);
});

test('configuration recovered and service still unavailable are reported separately', async () => {
  setup({
    applyConfigRecovery: vi.fn().mockResolvedValue({
      ok: true,
      value: {
        action: 'retry',
        outcomes: [{ ...domain, applied: true }],
        restored_capabilities: [],
        unavailable_capabilities: [{ capability: 'tool_policy', reason: 'dependency missing' }],
      },
    }),
  });
  await userEvent.click(await screen.findByRole('button', { name: '查看恢复选项' }));
  await userEvent.click(await screen.findByRole('button', { name: '确认重试当前配置' }));
  await screen.findByText('设置已恢复。');
  expect(screen.getByText(/相关功能尚不可用/u)).toBeTruthy();
});

test('a healthy state read while the original recovery is still active cannot unlock another submission', async () => {
  const api = setup({
    getConfigRecoveryNotice: vi
      .fn()
      .mockResolvedValue({ ok: true, value: { ...notice, recovery_in_progress: true } }),
    applyConfigRecovery: vi.fn().mockResolvedValue({
      ok: false,
      error: {
        code: 'command.outcome_unknown',
        terminal: 'failed',
        details: { outcome_recovery: 'query_runtime.command_outcome_without_replay' },
      },
    }),
  });
  await userEvent.click(await screen.findByRole('button', { name: '查看恢复选项' }));
  await userEvent.click(await screen.findByRole('button', { name: '确认重试当前配置' }));
  await screen.findByText(/恢复结果尚未确认/u);
  await userEvent.click(screen.getByRole('button', { name: '检查当前状态' }));
  await waitFor(() => expect(api.getConfigRecoveryNotice).toHaveBeenCalledTimes(2));
  expect(screen.queryByRole('button', { name: '查看恢复选项' })).toBeNull();
  expect(api.applyConfigRecovery).toHaveBeenCalledTimes(1);
});

// An option that cannot be used said why with Core's code (原因：config.recovery.…).
test('an unavailable option says why in words, not with a code', async () => {
  setup({
    planConfigRecovery: vi.fn().mockResolvedValue({
      ok: true,
      value: {
        ...plan,
        options: [
          { action: 'retry', available: true, reason_code: null, detail: null },
          {
            action: 'restore_previous_valid',
            available: false,
            reason_code: 'config.recovery.no_valid_previous_version',
            detail: null,
          },
        ],
      },
    }),
  });
  await userEvent.click(await screen.findByRole('button', { name: '查看恢复选项' }));
  expect(await screen.findByText('原因：没有可以恢复的上一版有效设置')).toBeTruthy();
  expect(screen.queryByText(/config\.recovery\./u)).toBeNull();
});
