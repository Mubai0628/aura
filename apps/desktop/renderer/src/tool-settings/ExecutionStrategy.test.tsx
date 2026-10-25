import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ExecutionStrategy } from './ExecutionStrategy';
import { useAgentConfiguration } from './useAgentConfiguration';

const view = {
  revision: '7',
  max_dynamic_tool_schemas: 50,
  max_concurrent_tool_calls: 4,
  default_timeout_ms: 120_000,
  max_retries: 3,
  fixed_tool_ids: ['read'],
  tools: [],
};
const configure = vi.fn();
beforeEach(() => {
  configure.mockReset().mockImplementation(async (input) => ({
    ok: true,
    value: { ...view, ...input, revision: '8' },
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

/** Renders the strategy editor against the shared owner, as the tool page does. */
function Harness() {
  const configuration = useAgentConfiguration();
  return <ExecutionStrategy configuration={configuration} />;
}

test('reads all four values and saves them under the read revision', async () => {
  render(<Harness />);
  const concurrency = await screen.findByLabelText('普通工具全局并发');
  expect((concurrency as HTMLInputElement).value).toBe('4');
  // The fields carry no explanations.
  expect(
    screen.queryByText(/每个会话可同时加载|所有会话合计|单个工具从实际开始|只约束本来就允许/u),
  ).toBeNull();
  fireEvent.change(concurrency, { target: { value: '8' } });
  fireEvent.change(screen.getByLabelText('最大重试次数'), { target: { value: '1' } });
  fireEvent.click(screen.getByRole('button', { name: '保存执行策略' }));
  await waitFor(() =>
    expect(configure).toHaveBeenCalledWith({
      expected_revision: '7',
      fixed_tool_ids: ['read'],
      max_dynamic_tool_schemas: 50,
      max_concurrent_tool_calls: 8,
      default_timeout_ms: 120_000,
      max_retries: 1,
      use_recommended_fixed_tool_ids: false,
    }),
  );
  await screen.findByText('执行策略已保存。');
});

test('a value outside the Rust range blocks the save instead of sending it', async () => {
  render(<Harness />);
  const timeout = await screen.findByLabelText('默认 Timeout（毫秒）');
  fireEvent.change(timeout, { target: { value: '999' } });
  const save = screen.getByRole('button', { name: '保存执行策略' }) as HTMLButtonElement;
  expect(save.disabled).toBe(true);
  fireEvent.change(timeout, { target: { value: '1000' } });
  expect(save.disabled).toBe(false);
  expect(configure).not.toHaveBeenCalled();
});

test('a field-level rejection keeps the typed value and names the field', async () => {
  configure.mockResolvedValue({
    ok: false,
    error: { message: 'tools.invalid_setting:max_concurrent_tool_calls' },
  });
  render(<Harness />);
  const concurrency = await screen.findByLabelText('普通工具全局并发');
  fireEvent.change(concurrency, { target: { value: '12' } });
  fireEvent.click(screen.getByRole('button', { name: '保存执行策略' }));
  await screen.findByRole('alert');
  expect((concurrency as HTMLInputElement).value).toBe('12');
  expect((concurrency as HTMLInputElement).getAttribute('aria-invalid')).toBe('true');
});

test('a revision conflict keeps the typed value so the edit is not silently lost', async () => {
  configure.mockResolvedValue({
    ok: false,
    error: { message: 'tools.configuration_revision_conflict' },
  });
  render(<Harness />);
  const retries = await screen.findByLabelText('最大重试次数');
  fireEvent.change(retries, { target: { value: '0' } });
  fireEvent.click(screen.getByRole('button', { name: '保存执行策略' }));
  await screen.findByRole('alert');
  expect((retries as HTMLInputElement).value).toBe('0');
});
