import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { NetworkProxySection } from './NetworkProxySection';

const view = {
  revision: '7',
  max_dynamic_tool_schemas: 50,
  max_concurrent_tool_calls: 4,
  default_timeout_ms: 120_000,
  max_retries: 3,
  fixed_tool_ids: ['read'],
  tools: [],
};
const systemProxy = { enabled: true, protocol: 'http', host: '127.0.0.1', port: 7890 };
const typedProxy = { enabled: true, protocol: 'socks5', host: '127.0.0.1', port: 10808 };
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

test('the section is a region named 网络代理 under its own heading', async () => {
  render(<NetworkProxySection />);
  expect(await screen.findByRole('region', { name: '网络代理' })).toBeTruthy();
  expect(screen.getByRole('heading', { level: 3, name: '网络代理' })).toBeTruthy();
});

test('the section saves only the proxy field', async () => {
  render(<NetworkProxySection />);
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
      network_proxy: typedProxy,
    }),
  );
  await screen.findByText('网络代理已保存。');
});

test('a remote address stays in the form and is not saved', async () => {
  render(<NetworkProxySection />);
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
  render(<NetworkProxySection />);
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
  render(<NetworkProxySection />);
  await screen.findByLabelText('代理地址');
  fireEvent.change(screen.getByLabelText('代理地址'), { target: { value: '127.0.0.1' } });
  fireEvent.change(screen.getByLabelText('代理端口'), { target: { value: '1' } });
  fireEvent.click(screen.getByRole('button', { name: '测试连接' }));
  await screen.findByText('拒绝连接');
});

test('proxy recovery previews its field and applies the preview revision before saving', async () => {
  readSettings.mockResolvedValue({
    ok: true,
    value: {
      ...view,
      network_proxy_diagnostic: '代理字段损坏',
      network_proxy_recovery: { expected_revision: '7', source_revision: '5', proxy: typedProxy },
    },
  });
  render(<NetworkProxySection />);
  const preview = await screen.findByRole('button', { name: '恢复上一版有效代理' });
  fireEvent.click(preview);
  expect(configure).not.toHaveBeenCalled();
  expect(screen.getByText(/只恢复网络代理/u)).toBeTruthy();
  expect(screen.queryByText(/来源修订|修订/u)).toBeNull();
  expect(screen.getByText(/127\.0\.0\.1:10808/u)).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: '应用代理恢复' }));
  await waitFor(() =>
    expect(configure).toHaveBeenCalledWith(
      expect.objectContaining({ expected_revision: '7', network_proxy: typedProxy }),
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
  render(<NetworkProxySection />);
  fireEvent.click(await screen.findByRole('button', { name: '恢复上一版有效代理' }));
  expect(screen.getByText(/关闭网络代理/u)).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: '应用代理恢复' }));
  await screen.findByText('代理设置没有恢复：配置修订冲突');
  expect(configure).toHaveBeenCalledTimes(1);
});

test('while no proxy is switched on, the system proxy is shown and there is no switch to follow it', async () => {
  readSettings.mockResolvedValue({
    ok: true,
    value: {
      ...view,
      network_proxy: { ...typedProxy, enabled: false },
      network_proxy_draft: { ...typedProxy, enabled: false },
      system_network_proxy: { proxy: systemProxy },
    },
  });
  render(<NetworkProxySection />);
  const enable = await screen.findByRole('switch', { name: '启用网络代理' });
  expect(enable.getAttribute('aria-checked')).toBe('false');
  expect(screen.queryByRole('switch', { name: '跟随系统代理' })).toBeNull();
  expect(screen.getByText('HTTP 127.0.0.1:7890')).toBeTruthy();
  // The typed-in proxy keeps its fields while it is switched off.
  expect((screen.getByLabelText('代理端口') as HTMLInputElement).value).toBe('10808');
  expect(screen.getByLabelText('代理协议')).toHaveProperty('value', 'socks5');
  // Switched on, it replaces the system's; the save carries only the proxy field.
  fireEvent.click(enable);
  expect(screen.queryByText('HTTP 127.0.0.1:7890')).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: '保存网络代理' }));
  await waitFor(() =>
    expect(configure).toHaveBeenCalledWith(
      expect.objectContaining({ expected_revision: '7', network_proxy: typedProxy }),
    ),
  );
  expect(configure.mock.calls[0]?.[0]).not.toHaveProperty('network_proxy_follows_system');
});

test('with nothing typed in, the test button tries the proxy the system names', async () => {
  readSettings.mockResolvedValue({
    ok: true,
    value: { ...view, system_network_proxy: { proxy: systemProxy } },
  });
  render(<NetworkProxySection />);
  fireEvent.click(await screen.findByRole('button', { name: '测试连接' }));
  await screen.findByText('连接成功');
  expect(probe).toHaveBeenCalledWith({ protocol: 'http', host: '127.0.0.1', port: 7890 });
});

test('a system proxy Aura cannot use says why', async () => {
  readSettings.mockResolvedValue({
    ok: true,
    value: {
      ...view,
      system_network_proxy: {
        issue:
          'Windows 用的是自动配置脚本（PAC），Aura 读不了这种设置，不经代理直连。可以启用网络代理，填写代理软件的本机端口。',
      },
    },
  });
  render(<NetworkProxySection />);
  await screen.findByText(/自动配置脚本/u);
  expect(screen.queryByRole('switch', { name: '跟随系统代理' })).toBeNull();
});

test('a proxy test Core refused or Aura could not send says the setting is unchanged', async () => {
  probe.mockResolvedValueOnce({
    ok: false,
    error: { code: 'ipc.request_timeout', message: 'Core request timed out' },
  });
  render(<NetworkProxySection />);
  await screen.findByLabelText('代理地址');
  fireEvent.change(screen.getByLabelText('代理地址'), { target: { value: '127.0.0.1' } });
  fireEvent.change(screen.getByLabelText('代理端口'), { target: { value: '7890' } });
  fireEvent.click(screen.getByRole('button', { name: '测试连接' }));
  await screen.findByText(
    '代理测试没有完成（Aura Core 没有及时回复），代理设置没有改变。请核对地址和端口后再测。',
  );
  probe.mockRejectedValueOnce(new Error('ipc failed'));
  fireEvent.click(screen.getByRole('button', { name: '测试连接' }));
  await screen.findByText(
    '代理测试没有发出去（Aura 内部通信失败），代理设置没有改变；请稍后再测。',
  );
});

test('settings that were not read say why and that the proxy cannot be changed now', async () => {
  readSettings.mockResolvedValue({
    ok: false,
    error: { code: 'ipc.request_timeout', message: 'Core request timed out' },
  });
  render(<NetworkProxySection />);
  expect((await screen.findByRole('alert')).textContent).toBe(
    '代理设置没有读到（Aura Core 没有及时回复），现在不能修改代理；请稍后点“重新读取网络代理”。',
  );
});

test('a proxy that was not saved says the old setting is still in force', async () => {
  configure.mockResolvedValue({
    ok: false,
    error: { code: 'agent.command_failed', message: 'tools.configuration_revision_conflict' },
  });
  render(<NetworkProxySection />);
  await screen.findByLabelText('代理地址');
  fireEvent.change(screen.getByLabelText('代理地址'), { target: { value: '127.0.0.1' } });
  fireEvent.change(screen.getByLabelText('代理端口'), { target: { value: '7890' } });
  fireEvent.click(screen.getByRole('button', { name: '保存网络代理' }));
  expect((await screen.findByRole('alert')).textContent).toBe(
    '代理设置没有保存（设置刚在别处被改过），仍按原来的设置联网。请重新打开这一页，核对后再改。',
  );
});
