import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import type { MemoryDesktopSnapshot, MemoryMaintenanceRun } from '@aura/contracts/generated';
import { MemorySettings } from './MemorySettings';

/**
 * The memory page: finished maintenance runs go to a log; the page shows the newest
 * three, everything else opens in a dialog, and the log file opens in the user's editor.
 */
const run = (index: number): MemoryMaintenanceRun =>
  ({
    run_id: `run-${index}`,
    runtime_instance_id: 'runtime',
    session_id: `ses_${index}`,
    turn_id: null,
    state: index % 2 ? 'skipped' : 'completed',
    reason: index % 2 ? '只读会话未整理' : `新增 ${index} 条`,
    model_profile_id: null,
    input_tokens: 100 + index,
    output_tokens: 10,
    usage_complete: true,
    started_at: new Date(Date.UTC(2026, 9, 18, 12, 59 - index)).toISOString(),
    finished_at: new Date(Date.UTC(2026, 9, 18, 12, 59 - index, 30)).toISOString(),
  }) as unknown as MemoryMaintenanceRun;

const snapshot = (log: string | null): MemoryDesktopSnapshot =>
  ({
    vaults: [
      {
        scope: 'project',
        directory: 'C:/TEMP/project/.memories',
        exists: true,
        valid: 2,
        invalid: 0,
        rebuilding: false,
        watcher_error: null,
        recent_changes: [],
        maintenance_log: log,
      },
    ],
    recent_runs: Array.from({ length: 25 }, (_, index) => run(index)),
  }) as MemoryDesktopSnapshot;

function show(log: string | null) {
  const invoke = vi.fn(async (command: string) => {
    if (command === 'memory.vaults.get') return { ok: true, value: snapshot(log) };
    if (command === 'memory.settings.get')
      return {
        ok: true,
        value: {
          revision: 1,
          settings: { automatic_maintenance: true, event_retention_days: 30 },
          issue: null,
        },
      };
    return { ok: true, value: { completed: true, message: '已打开整理日志' } };
  });
  vi.stubGlobal('aura', { v1: { memory: { invoke } } });
  render(<MemorySettings coreReady workspaceId="workspace-test" />);
  return invoke;
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test('the page previews the three newest runs; all of them open in the log dialog', async () => {
  show('C:\\Aura\\memory\\cache\\project.records\\maintenance.log');
  const card = await screen.findByRole('region', { name: '整理记录' });
  const preview = within(card).getAllByRole('listitem');
  expect(preview).toHaveLength(3);
  expect(preview[0]!.textContent).toContain('新增 0 条');
  await userEvent.click(within(card).getByRole('button', { name: '查看全部' }));
  const dialog = screen.getByRole('dialog', { name: '整理日志' });
  expect(within(dialog).getAllByRole('listitem')).toHaveLength(20);
  const first = within(dialog).getAllByRole('listitem')[0]!.textContent ?? '';
  expect(first).toContain('已完成');
  expect(first).toContain('新增 0 条');
  expect(first).toContain('ses_0');
  expect(first).toContain('输入 100');
  await userEvent.click(within(dialog).getByRole('button', { name: '显示更早的 5 条' }));
  expect(within(dialog).getAllByRole('listitem')).toHaveLength(25);
  expect(within(dialog).queryByRole('button', { name: /显示更早/u })).toBeNull();
  await userEvent.click(within(dialog).getByRole('button', { name: '关闭整理日志' }));
  expect(screen.queryByRole('dialog', { name: '整理日志' })).toBeNull();
});

test('打开日志文件 asks Main for the log Core names; without one the button is off', async () => {
  const invoke = show('C:\\Aura\\memory\\cache\\project.records\\maintenance.log');
  const card = await screen.findByRole('region', { name: '整理记录' });
  await userEvent.click(within(card).getByRole('button', { name: '打开日志文件' }));
  expect(invoke).toHaveBeenCalledWith('memory.maintenance.open_log', {
    scope: 'project',
    workspace_id: 'workspace-test',
  });
  expect(await screen.findByText('已打开整理日志')).toBeTruthy();
  cleanup();
  show(null);
  const without = await screen.findByRole('region', { name: '整理记录' });
  expect(
    (within(without).getByRole('button', { name: '打开日志文件' }) as HTMLButtonElement).disabled,
  ).toBe(true);
});
