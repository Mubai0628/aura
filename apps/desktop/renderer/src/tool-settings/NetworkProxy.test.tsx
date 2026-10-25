import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ToolPolicySections } from './NetworkProxy';
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
const probe = vi.fn();
const readSettings = vi.fn();
beforeEach(() => {
  configure.mockReset().mockImplementation(async (input) => ({
    ok: true,
    value: { ...view, ...input, revision: '8' },
  }));
  probe.mockReset().mockResolvedValue({ ok: true, value: { ok: true, message: '连接成功' } });
  readSettings.mockReset().mockResolvedValue({ ok: true, value: view });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        readAgentToolSettings: readSettings,
        configureAgentTools: configure,
        testAgentNetworkProxy: probe,
      },
    },
  });
});
afterEach(cleanup);

function Harness() {
  const configuration = useAgentConfiguration();
  return <ToolPolicySections configuration={configuration} strategyReady />;
}

test('the proxy section saves only the proxy field', async () => {
  render(<Harness />);
  // Switching the proxy on is a switch, not a bare checkbox; no explanations.
  const enable = await screen.findByRole('switch', { name: '启用网络代理' });
  expect(screen.queryByText(/只用于 http|适用于工具调用的全局限制/u)).toBeNull();
  expect(enable.getAttribute('aria-checked')).toBe('false');
  fireEvent.click(enable);
  expect(enable.getAttribute('aria-checked')).toBe('true');
  fireEvent.change(screen.getByLabelText('代理协议'), { target: { value: 'socks5' } });
  fireEvent.change(screen.getByLabelText('代理地址'), { target: { value: '127.0.0.1' } });
  fireEvent.change(screen.getByLabelText('代理端口'), { target: { value: '10808' } });
  fireEvent.click(screen.getByRole('button', { name: '测试连接' }));
  await screen.findByText('连接成功');
  expect(probe).toHaveBeenCalledWith({ protocol: 'socks5', host: '127.0.0.1', port: 10808 });
  fireEvent.click(screen.getByRole('button', { name: '保存网络代理' }));
  await waitFor(() =>
    expect(configure).toHaveBeenCalledWith({
      expected_revision: '7',
      fixed_tool_ids: ['read'],
      max_dynamic_tool_schemas: 50,
      max_concurrent_tool_calls: 4,
      default_timeout_ms: 120_000,
      max_retries: 3,
      use_recommended_fixed_tool_ids: false,
      network_proxy: { enabled: true, protocol: 'socks5', host: '127.0.0.1', port: 10808 },
    }),
  );
  await screen.findByText('网络代理已保存。');
});

test('a remote address stays in the form and is not saved', async () => {
  render(<Harness />);
  await screen.findByLabelText('代理地址');
  fireEvent.change(screen.getByLabelText('代理地址'), { target: { value: 'example.com' } });
  fireEvent.change(screen.getByLabelText('代理端口'), { target: { value: '10808' } });
  expect((screen.getByRole('button', { name: '保存网络代理' }) as HTMLButtonElement).disabled).toBe(
    true,
  );
  expect(screen.getByRole('alert').textContent).toContain('只允许本机回环地址');
  expect(configure).not.toHaveBeenCalled();
});

test('a localhost alias in any letter case is sent in the form the contract accepts', async () => {
  render(<Harness />);
  await screen.findByLabelText('代理地址');
  fireEvent.click(screen.getByLabelText('启用网络代理'));
  fireEvent.change(screen.getByLabelText('代理地址'), { target: { value: 'LocalHost' } });
  fireEvent.change(screen.getByLabelText('代理端口'), { target: { value: '10809' } });
  fireEvent.click(screen.getByRole('button', { name: '测试连接' }));
  await screen.findByText('连接成功');
  expect(probe).toHaveBeenCalledWith({ protocol: 'http', host: 'localhost', port: 10809 });
  fireEvent.click(screen.getByRole('button', { name: '保存网络代理' }));
  await waitFor(() =>
    expect(configure).toHaveBeenCalledWith(
      expect.objectContaining({
        network_proxy: { enabled: true, protocol: 'http', host: 'localhost', port: 10809 },
      }),
    ),
  );
});

test('a refused probe is shown in plain language', async () => {
  probe.mockResolvedValue({ ok: true, value: { ok: false, message: '拒绝连接' } });
  render(<Harness />);
  await screen.findByLabelText('代理地址');
  fireEvent.change(screen.getByLabelText('代理地址'), { target: { value: '127.0.0.1' } });
  fireEvent.change(screen.getByLabelText('代理端口'), { target: { value: '1' } });
  fireEvent.click(screen.getByRole('button', { name: '测试连接' }));
  await screen.findByText('拒绝连接');
});

test('proxy recovery previews its field and applies the preview revision before saving', async () => {
  const proxy = { enabled: true, protocol: 'socks5', host: '127.0.0.1', port: 10808 };
  readSettings.mockResolvedValue({
    ok: true,
    value: {
      ...view,
      network_proxy_diagnostic: '代理字段损坏',
      network_proxy_recovery: { expected_revision: '7', source_revision: '5', proxy },
    },
  });
  render(<Harness />);
  const preview = await screen.findByRole('button', { name: '恢复上一版有效代理' });
  fireEvent.click(preview);
  expect(configure).not.toHaveBeenCalled();
  expect(screen.getByText(/只恢复网络代理/u)).toBeTruthy();
  expect(screen.getByText(/127\.0\.0\.1:10808/u)).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: '应用代理恢复' }));
  await waitFor(() =>
    expect(configure).toHaveBeenCalledWith(
      expect.objectContaining({ expected_revision: '7', network_proxy: proxy }),
    ),
  );
  await screen.findByText('网络代理已恢复。');
});

test('a rejected recovery keeps the preview and reports the revision conflict', async () => {
  readSettings.mockResolvedValue({
    ok: true,
    value: {
      ...view,
      network_proxy_recovery: { expected_revision: '7', source_revision: '5', proxy: null },
    },
  });
  configure.mockResolvedValue({ ok: false, error: { code: 'conflict', message: '配置修订冲突' } });
  render(<Harness />);
  fireEvent.click(await screen.findByRole('button', { name: '恢复上一版有效代理' }));
  expect(screen.getByText(/关闭网络代理/u)).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: '应用代理恢复' }));
  await screen.findByText('配置修订冲突');
  expect(configure).toHaveBeenCalledTimes(1);
});
