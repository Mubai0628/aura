import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test } from 'vitest';
import { ApprovalPanel } from './ApprovalPanel';

afterEach(cleanup);

const PACKAGE_ID = 'tpk_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const fingerprint = {
  algorithm: 'sha256',
  canonicalization_version: 'rfc8785_jcs_v1',
  hex: 'b'.repeat(64),
};

function install(view: {
  mode: string;
  runtime: 'program' | 'http';
  capabilities: string[];
  decisions: string[];
  action: unknown;
  credentials?: unknown[];
}) {
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        getAgentApproval: async () => ({
          ok: true,
          value: {
            mode: view.mode,
            backend:
              view.runtime === 'program'
                ? {
                    id: 'agent-native-package-v1',
                    class: 'native',
                    guarantees: ['child_process_control', 'bounded_output'],
                  }
                : { id: 'controlled-web-v1', class: 'core_broker', guarantees: [] },
            operation: {
              content: {
                action_kind: 'user.fixture.run',
                action: view.action,
                resources: [
                  {
                    scope_kind: 'tool_package',
                    package_id: PACKAGE_ID,
                    package_fingerprint: fingerprint,
                    runtime: view.runtime,
                  },
                ],
                capabilities: view.capabilities,
                credentials: view.credentials ?? [],
              },
            },
            requested_capabilities: view.capabilities,
            reasons: [],
            approval: {
              expires_at: '2026-10-17T00:00:00Z',
              state: 'pending',
              allowed_decisions: view.decisions,
            },
          },
        }),
        resolveAgentApproval: async () => ({ ok: true, value: {} }),
        packages: {
          invoke: async () => ({
            ok: true,
            value: {
              packages: [
                {
                  package_id: PACKAGE_ID,
                  display_name: 'Fixture Tools',
                  package_key: 'fixture',
                  revision: '1',
                  versions: [],
                },
              ],
            },
          }),
        },
      },
    },
  });
}

test('a package program says it runs as the user and what the session answer covers', async () => {
  install({
    mode: 'workspace_read_write',
    runtime: 'program',
    capabilities: ['process_execute', 'ordinary_network'],
    decisions: ['allow_once', 'allow_session', 'deny'],
    action: { name: 'demo' },
  });
  render(<ApprovalPanel sessionId="session" approvalId="approval" onChanged={() => {}} />);
  expect(await screen.findByText(/工具包「Fixture Tools」的工具 user.fixture.run/)).toBeTruthy();
  expect(screen.getByText(/以当前 Windows 用户身份运行程序/)).toBeTruthy();
  expect(screen.getByText(/由程序自己联网，Aura 不限定它连接的地址/)).toBeTruthy();
  expect(screen.getByText(/不覆盖内置工具或其他工具包/)).toBeTruthy();
  expect(screen.getByText(/"name": "demo"/)).toBeTruthy();
  // The generic native and network wording would misstate what is approved.
  expect(screen.queryByText(/联网（普通网络工具）/)).toBeNull();
  expect(screen.queryByText(/只覆盖这一类操作/)).toBeNull();
});

test('read-only package programs ask every time and offer no session answer', async () => {
  install({
    mode: 'read_only',
    runtime: 'program',
    capabilities: ['process_execute'],
    decisions: ['allow_once', 'deny'],
    action: {},
  });
  render(<ApprovalPanel sessionId="session" approvalId="approval" onChanged={() => {}} />);
  expect(await screen.findByText(/只读模式下这个工具每次运行都会询问/)).toBeTruthy();
  expect(screen.getByText(/Aura 不在系统层面拦截它联网/)).toBeTruthy();
  expect(screen.queryByText(/「本会话允许」只覆盖/)).toBeNull();
  expect(screen.queryByRole('button', { name: '本会话允许' })).toBeNull();
});

test('a package HTTP call shows the exact request and the bound credential rule', async () => {
  install({
    mode: 'read_only',
    runtime: 'http',
    capabilities: ['ordinary_network'],
    decisions: ['allow_once', 'allow_session', 'deny'],
    action: {
      arguments: { city: 'x' },
      request: { method: 'GET', url: 'https://api.fixture.invalid/v1/weather?city=x' },
    },
    credentials: [{ credential_id: 'c' }],
  });
  render(<ApprovalPanel sessionId="session" approvalId="approval" onChanged={() => {}} />);
  expect(
    await screen.findByText(/GET https:\/\/api.fixture.invalid\/v1\/weather\?（查询参数已隐藏）/),
  ).toBeTruthy();
  expect(screen.getByText(/凭据只发往它允许的网站，模型看不到凭据/)).toBeTruthy();
  expect(screen.getByText(/不覆盖内置工具或其他工具包/)).toBeTruthy();
  expect(screen.queryByText(/城市|city=x/)).toBeNull();
});
