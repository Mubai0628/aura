import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test } from 'vitest';
import type { ExecutionApprovalView } from '@aura/contracts/generated';
import { NetworkApprovalFacts } from './NetworkApprovalFacts';
afterEach(cleanup);
test('approval describes independent download permissions and hides sensitive query values', () => {
  const view = {
    requested_capabilities: ['workspace_file_write'],
    operation: {
      content: {
        capabilities: ['ordinary_network', 'workspace_file_write'],
        action_kind: 'http.download',
        action: {
          url: 'https://example.com/a?token=hidden-secret',
          path: 'result.bin',
          overwrite: true,
        },
      },
    },
  } as ExecutionApprovalView;
  render(<NetworkApprovalFacts view={view} />);
  expect(screen.getByText(/查询参数已隐藏/u)).toBeTruthy();
  expect(screen.getByText(/联网（普通网络工具）/u).textContent).not.toContain('hidden-secret');
  const details = screen.getByText('查看请求内容').closest('details');
  expect(details?.open).toBe(false);
  expect(details?.textContent).toContain('token=hidden-secret');
  expect(screen.getByText(/联网批准不增加文件写入权限/u)).toBeTruthy();
  expect(screen.getByText(/result.bin/u)).toBeTruthy();
  expect(screen.getByText(/本次需要批准：覆盖工作区文件/u)).toBeTruthy();
  expect(document.body.textContent).not.toContain('全部目标');
  expect(screen.getByText(/本次只确认文件目标/u)).toBeTruthy();
});
test('a network question explains the all-destination session answer', () => {
  const view = {
    requested_capabilities: ['ordinary_network'],
    operation: {
      content: {
        capabilities: ['ordinary_network'],
        action_kind: 'http.request',
        action: { url: 'http://127.0.0.1:8080/api' },
      },
    },
  } as ExecutionApprovalView;
  render(<NetworkApprovalFacts view={view} />);
  expect(screen.getByText(/覆盖本会话普通网络工具的全部目标/u)).toBeTruthy();
  expect(document.body.textContent).not.toContain('本次只确认文件目标');
});
test('a public https approval names the local proxy it will use', () => {
  const view = {
    network_proxy_notice: '访问公网地址时将经本机代理 127.0.0.1:10808（SOCKS5）发出',
    requested_capabilities: ['ordinary_network'],
    operation: {
      content: {
        capabilities: ['ordinary_network'],
        action_kind: 'http.request',
        action: { url: 'https://example.com/a' },
      },
    },
  } as ExecutionApprovalView;
  render(<NetworkApprovalFacts view={view} />);
  expect(screen.getByText('访问公网地址时将经本机代理 127.0.0.1:10808（SOCKS5）发出')).toBeTruthy();
});
