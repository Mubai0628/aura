import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import {
  OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
  REQUEST_TIMEOUT_CODE,
} from '../../../shared/ipc-request-failure';
import { AgentToolList } from './AgentToolList';
import { useAgentConfiguration } from './useAgentConfiguration';

const view = {
  revision: '4',
  max_dynamic_tool_schemas: 50,
  max_concurrent_tool_calls: 4,
  default_timeout_ms: 120_000,
  max_retries: 3,
  fixed_tool_ids: ['read'],
  tools: [
    {
      tool_id: 'read',
      name: 'fs.read',
      description: '读取文件',
      source: 'builtin',
      required: false,
      available: true,
    },
    {
      tool_id: 'list',
      name: 'fs.list',
      description: '列出目录',
      source: 'builtin',
      required: false,
      available: true,
    },
    {
      tool_id: 'load',
      name: 'tools.load',
      description: '管理动态工具',
      source: 'builtin',
      required: true,
      available: true,
    },
  ],
};
const configure = vi.fn();

/** Renders the list against the shared owner, exactly as the tool page does. */
function Harness() {
  const configuration = useAgentConfiguration();
  return <AgentToolList configuration={configuration} />;
}

beforeEach(() => {
  configure.mockReset().mockImplementation(async (input) => ({
    ok: true,
    value: { ...view, ...input, revision: String(Number(view.revision) + 1) },
  }));
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        readAgentToolSettings: vi.fn().mockResolvedValue({ ok: true, value: view }),
        configureAgentTools: configure,
      },
    },
  });
});
afterEach(cleanup);

test('saves a sorted fixed selection and carries the untouched execution strategy through', async () => {
  render(<Harness />);
  const list = await screen.findByRole('switch', { name: 'fs.list 默认加载' });
  expect(
    (screen.getByRole('switch', { name: 'tools.load 默认加载' }) as HTMLButtonElement).disabled,
  ).toBe(true);
  fireEvent.click(list);
  fireEvent.click(screen.getByRole('button', { name: '保存配置' }));
  await waitFor(() =>
    expect(configure).toHaveBeenCalledWith({
      expected_revision: '4',
      fixed_tool_ids: ['list', 'read'],
      max_dynamic_tool_schemas: 50,
      max_concurrent_tool_calls: 4,
      default_timeout_ms: 120_000,
      max_retries: 3,
      use_recommended_fixed_tool_ids: false,
    }),
  );
  await screen.findByText('工具配置已保存。');
  expect((screen.getByRole('button', { name: '保存配置' }) as HTMLButtonElement).disabled).toBe(
    true,
  );
});

test('a rejected save keeps the unsaved selection selected', async () => {
  configure.mockResolvedValue({
    ok: false,
    error: { message: 'tools.configuration_revision_conflict' },
  });
  render(<Harness />);
  fireEvent.click(await screen.findByRole('switch', { name: 'fs.list 默认加载' }));
  fireEvent.click(screen.getByRole('button', { name: '保存配置' }));
  await screen.findByRole('alert');
  expect(
    screen.getByRole('switch', { name: 'fs.list 默认加载' }).getAttribute('aria-checked') ===
      'true',
  ).toBe(true);
  expect(configure).toHaveBeenCalledTimes(1);
});

test('a save whose outcome is unconfirmed asks the user to verify instead of retrying', async () => {
  configure.mockResolvedValue({
    error: {
      category: 'cancelled',
      code: REQUEST_TIMEOUT_CODE,
      correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      details: {
        command_invocation_id: 'civ_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        outcome_recovery: OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
      },
      message: 'Core request timed out: agent.tools.configure',
      retryable: false,
      terminal: 'cancelled',
    },
    ok: false,
  });
  render(<Harness />);
  fireEvent.click(await screen.findByRole('switch', { name: 'fs.list 默认加载' }));
  fireEvent.click(screen.getByRole('button', { name: '保存配置' }));
  // The stored configuration may already be the new one, so the message asks for a check
  // rather than reporting a failure the user can simply repeat.
  await screen.findByText(/保存结果未确认/);
  expect(
    screen.getByRole('switch', { name: 'fs.list 默认加载' }).getAttribute('aria-checked') ===
      'true',
  ).toBe(true);
});

test('restoring the recommended defaults asks Core for them instead of sending a local list', async () => {
  render(<Harness />);
  const list = await screen.findByRole('switch', { name: 'fs.list 默认加载' });
  fireEvent.click(list);
  // The recommendation is owned by Rust, so the request must not carry a
  // renderer-assembled selection; it only asks for the published defaults.
  fireEvent.click(screen.getByRole('button', { name: '恢复推荐默认值' }));
  await waitFor(() =>
    expect(configure).toHaveBeenCalledWith(
      expect.objectContaining({ use_recommended_fixed_tool_ids: true, fixed_tool_ids: [] }),
    ),
  );
  await screen.findByText('工具配置已保存。');
});

test('a second save uses the revision the first save returned', async () => {
  render(<Harness />);
  fireEvent.click(await screen.findByRole('switch', { name: 'fs.list 默认加载' }));
  fireEvent.click(screen.getByRole('button', { name: '保存配置' }));
  await screen.findByText('工具配置已保存。');

  // The shared owner must have adopted the new revision, so the next edit submits
  // against it rather than against the one read at mount.
  fireEvent.click(screen.getByRole('switch', { name: 'fs.read 默认加载' }));
  fireEvent.click(screen.getByRole('button', { name: '保存配置' }));
  await waitFor(() => expect(configure).toHaveBeenCalledTimes(2));
  expect(configure.mock.calls[1]?.[0]?.expected_revision).toBe('5');
});

test('a failed initial read shows a real error and a retry instead of loading forever', async () => {
  const readAgentToolSettings = vi
    .fn()
    .mockResolvedValueOnce({ ok: false, error: { message: 'tools.configuration_unavailable' } })
    .mockResolvedValue({ ok: true, value: view });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentToolSettings, configureAgentTools: configure } },
  });
  render(<Harness />);
  await screen.findByRole('alert');
  expect(screen.queryByText('正在读取工具…')).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: '重新读取工具配置' }));
  await screen.findByRole('switch', { name: 'fs.list 默认加载' });
  expect(screen.queryByRole('alert')).toBeNull();
});

test('tools are grouped by family; a family filter narrows the list and keeps every choice', async () => {
  render(<Harness />);
  await screen.findByRole('switch', { name: 'fs.list 默认加载' });
  expect(screen.getByRole('region', { name: '文件工具' })).toBeTruthy();
  expect(screen.getByRole('region', { name: '工具目录工具' })).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: /^文件/u }));
  expect(screen.getByRole('button', { name: /^文件/u }).getAttribute('aria-pressed')).toBe('true');
  expect(screen.queryByRole('switch', { name: 'tools.load 默认加载' })).toBeNull();
  expect(
    screen.getByRole('switch', { name: 'fs.read 默认加载' }).getAttribute('aria-checked') ===
      'true',
  ).toBe(true);
  fireEvent.click(screen.getByRole('button', { name: /^全部/u }));
  expect(screen.getByRole('switch', { name: 'tools.load 默认加载' })).toBeTruthy();
});
