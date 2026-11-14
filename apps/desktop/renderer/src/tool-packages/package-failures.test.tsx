import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { ToolPackagesPanel } from './ToolPackagesPanel';
import raw from '../../../../../fixtures/contracts/package-routes.v1.json?raw';

// The package page says why a list or an action did not happen and offers to read the list again;
// a credential list that was not read says so instead of staying empty.

// eslint-disable-next-line @typescript-eslint/no-explicit-any -- a fixture document of many shapes
const fixture = JSON.parse(raw) as Record<string, any>;
afterEach(cleanup);

const TIMEOUT = { code: 'ipc.request_timeout', message: 'Core request timed out' };

function stub(invoke: unknown, listToolCredentials: unknown = vi.fn()) {
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { packages: { invoke }, listToolCredentials } },
  });
}

test('a list that was not read says why, hides the loading line and reads again on 重试', async () => {
  const invoke = vi
    .fn()
    .mockResolvedValueOnce({ ok: false, error: TIMEOUT })
    .mockResolvedValue({ ok: true, value: { packages: [] } });
  stub(invoke);
  render(<ToolPackagesPanel />);
  expect((await screen.findByRole('alert')).textContent).toBe(
    '工具包列表没有读到（Aura Core 没有及时回复），请重试。',
  );
  expect(screen.queryByText('正在读取工具包…')).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: '重试' }));
  expect(await screen.findByText('尚未导入工具包。')).toBeTruthy();
  expect(screen.queryByRole('alert')).toBeNull();
});

test('an import that failed names the action and that the list is unchanged', async () => {
  const invoke = vi.fn(async (command: string) =>
    command === 'packages.list'
      ? { ok: true, value: { packages: [] } }
      : {
          ok: false,
          error: {
            code: 'ipc.core_not_ready',
            message: 'Aura Core 尚未就绪，这次操作没有执行；请等它就绪后重试。',
          },
        },
  );
  stub(invoke);
  render(<ToolPackagesPanel />);
  fireEvent.click(await screen.findByRole('button', { name: '导入目录' }));
  expect((await screen.findByRole('alert')).textContent).toBe(
    '工具包没有导入（Aura Core 还没有就绪），列表不变；请重试。',
  );
});

test('credentials that were not read for binding say so instead of an empty list', async () => {
  const list = structuredClone(fixture.list);
  const tool = list.packages[0].installed.tools[0];
  tool.credential = {
    binding: null,
    origin: 'https://api.example.test',
    placement: { placement: 'http_bearer_authorization' },
    slot_key: 'api_key',
    target: {
      credential_slot_key: 'api_key',
      expected_instance_revision: '1',
      placement: { placement: 'http_bearer_authorization' },
      session_id: null,
      tool_instance_id: tool.tool_instance_id,
      workspace_id: null,
    },
  };
  const invoke = vi.fn().mockResolvedValue({ ok: true, value: list });
  stub(invoke, vi.fn().mockResolvedValue({ ok: false, error: TIMEOUT }));
  render(<ToolPackagesPanel />);
  fireEvent.click(await screen.findByRole('button', { name: '管理' }));
  fireEvent.click(screen.getByRole('button', { name: '读取可绑定的凭据' }));
  await waitFor(() =>
    expect(
      screen.getByText(
        '可绑定的凭据没有读到（Aura Core 没有及时回复），暂时不能绑定；请稍后重试。',
      ),
    ).toBeTruthy(),
  );
});
