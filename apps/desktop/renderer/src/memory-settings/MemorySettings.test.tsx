import { cleanup, render, screen, within, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import type { MemoryDesktopSnapshot, MemorySettingsSnapshot } from '@aura/contracts/generated';
import { MemorySettings } from './MemorySettings';
const configuration: MemorySettingsSnapshot = {
  revision: 1,
  settings: { automatic_maintenance: true, event_retention_days: 30 },
  issue: null,
};
const snapshot: MemoryDesktopSnapshot = {
  vaults: [
    {
      scope: 'global',
      directory: 'C:/TEMP/global',
      exists: false,
      valid: 0,
      invalid: 0,
      rebuilding: false,
      watcher_error: null,
      recent_changes: [],
    },
    {
      scope: 'project',
      directory: 'C:/TEMP/project/.memories',
      exists: true,
      valid: 2,
      invalid: 1,
      rebuilding: false,
      watcher_error: null,
      recent_changes: [],
    },
  ],
  recent_runs: [],
};
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});
test('settings shows real counts, validates, previews before repair, exports and saves with revision', async () => {
  const invoke = vi.fn(async (command: string) => {
    if (command === 'memory.vaults.get') return { ok: true, value: snapshot };
    if (command === 'memory.settings.get' || command === 'memory.settings.update')
      return { ok: true, value: configuration };
    if (command === 'memory.vault.validate')
      return {
        ok: true,
        value: {
          valid: 2,
          invalid: [{ path: 'rules/bad.md', reason: 'missing id' }],
          rebuilding: false,
        },
      };
    if (command === 'memory.vault.repair.preview')
      return {
        ok: true,
        value: {
          preview_token: 'bound',
          changes: [
            {
              path: 'rules/bad.md',
              expected_hash: 'old',
              proposed_hash: 'new',
              changes: ['补充 ID'],
            },
          ],
          manual: [],
          rebuild_index: true,
        },
      };
    if (command === 'memory.vault.repair.apply') return { ok: true, value: { applied: true } };
    return { ok: true, value: { completed: true, message: '已完成' } };
  });
  vi.stubGlobal('aura', { v1: { memory: { invoke } } });
  render(<MemorySettings coreReady workspaceId="workspace-test" />);
  const project = await screen.findByRole('region', { name: '项目记忆' });
  const global = screen.getByRole('region', { name: '全局记忆' });
  expect(within(project).getByText('2 条 · 1 个无效文件')).toBeTruthy();
  // No explanations of retention or export limits.
  expect(screen.queryByText(/超出保留天数|ZIP 包含完整 Vault/u)).toBeNull();
  expect(
    (within(global).getByRole('button', { name: '打开目录' }) as HTMLButtonElement).disabled,
  ).toBe(true);
  expect(screen.queryByRole('button', { name: '应用此预览' })).toBeNull();
  await userEvent.click(within(project).getByRole('button', { name: '校验' }));
  await screen.findByText('missing id', { exact: false });
  await userEvent.click(within(project).getByRole('button', { name: '预览修复' }));
  await screen.findByText('补充 ID', { exact: false });
  expect(invoke.mock.calls.filter((call) => call[0] === 'memory.vault.repair.apply')).toHaveLength(
    0,
  );
  await userEvent.click(screen.getByRole('button', { name: '应用此预览' }));
  await screen.findByText('修复已应用');
  await waitFor(() => expect(screen.getByRole('button', { name: '保存记忆设置' })).toBeTruthy());
  await userEvent.click(screen.getByRole('switch', { name: '任务完成后自动整理记忆' }));
  const days = screen.getByRole('spinbutton', { name: '事件保留天数' });
  await userEvent.clear(days);
  await userEvent.type(days, '12');
  await userEvent.click(screen.getByRole('button', { name: '保存记忆设置' }));
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith('memory.settings.update', {
      expected_revision: 1,
      settings: { automatic_maintenance: false, event_retention_days: 12 },
    }),
  );
  await waitFor(() => expect(screen.queryByText('正在读取记忆…')).toBeNull());
  await userEvent.click(
    within(screen.getByRole('region', { name: '项目记忆' })).getByRole('button', {
      name: '导出 ZIP',
    }),
  );
  await screen.findByText('已完成');
  expect(screen.queryByRole('textbox')).toBeNull();
});
test('late data from a previous workspace cannot replace the current Vault', async () => {
  let oldResolve!: (value: unknown) => void;
  const invoke = vi.fn((command: string, input: { workspace_id?: string }) => {
    if (command === 'memory.settings.get')
      return Promise.resolve({ ok: true, value: configuration });
    if (input.workspace_id === 'first')
      return new Promise((resolve) => {
        oldResolve = resolve;
      });
    return Promise.resolve({
      ok: true,
      value: {
        ...snapshot,
        vaults: [{ ...snapshot.vaults[1], directory: 'C:/TEMP/second/.memories' }],
      },
    });
  });
  vi.stubGlobal('aura', { v1: { memory: { invoke } } });
  const ui = render(<MemorySettings coreReady workspaceId="first" />);
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith('memory.vaults.get', { workspace_id: 'first' }),
  );
  ui.rerender(<MemorySettings coreReady workspaceId="second" />);
  await screen.findByText('C:/TEMP/second/.memories');
  oldResolve({ ok: true, value: snapshot });
  await waitFor(() => expect(screen.queryByText('C:/TEMP/project/.memories')).toBeNull());
});
test('recent changes say what happened and who did it, in local time', async () => {
  const at = '2026-10-16T02:03:04Z';
  const change = { id: null, session_id: null, turn_id: null, before_hash: null, at };
  const changed: MemoryDesktopSnapshot = {
    vaults: [
      {
        ...snapshot.vaults[1]!,
        recent_changes: [
          {
            ...change,
            path: 'rules/a.md',
            action: 'interrupted_undo',
            actor: 'recovery',
            after_hash: 'a',
            title: '构建约定',
          },
          {
            ...change,
            path: 'rules/b.md',
            action: 'external_edit',
            actor: 'external',
            after_hash: 'c',
            title: '',
          },
        ],
      },
    ],
    recent_runs: [],
  };
  const invoke = vi.fn(async (command: string) =>
    command === 'memory.vaults.get'
      ? { ok: true, value: changed }
      : { ok: true, value: configuration },
  );
  vi.stubGlobal('aura', { v1: { memory: { invoke } } });
  render(<MemorySettings coreReady workspaceId="workspace-test" />);
  const project = await screen.findByRole('region', { name: '项目记忆' });
  expect(within(project).getByText(/撤回中断的写入/)).toBeTruthy();
  expect(within(project).getByText(/Aura 恢复/)).toBeTruthy();
  expect(within(project).getByText(/在外部修改/)).toBeTruthy();
  expect(project.textContent).not.toMatch(/interrupted_undo|external_edit|recovery/);
  expect(project.textContent).toContain(new Date(at).toLocaleString());
});
