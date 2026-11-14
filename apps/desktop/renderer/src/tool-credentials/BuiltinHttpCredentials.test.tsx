import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { BuiltinHttpCredentials } from './BuiltinHttpCredentials';
import { resetScopedToolProjectionStoreForTests } from '../tool-projection-store';

// The HTTP tool's credential binding says why it is not shown yet, and a clear that did not happen
// says the tool still uses the credential it had.

afterEach(() => {
  cleanup();
  resetScopedToolProjectionStoreForTests();
});

const TOOL_ID = 'tol_01AAAAAAAAAAAAAAAAAAAAAAAA';
const definition = {
  payload: {
    versions: [
      {
        instances: [
          {
            credential_slot_binding: {
              bound_credential: {
                binding_revision: '2',
                credential_id: 'cred_A',
                display_name: '搜索服务',
              },
              placement: { placement: 'http_bearer_authorization' },
              rebind_required_reason: null,
              slot_key: 'api_key',
            },
            entity_revision: '3',
            tool_instance_id: 'tin_01AAAAAAAAAAAAAAAAAAAAAAAA',
          },
        ],
      },
    ],
  },
  projection_kind: 'tool_definition',
  projection_scope: { tool_id: TOOL_ID, type: 'tool_definition' },
  revision: '1',
  runtime_instance_id: 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV',
};

const stub = (v1: Record<string, unknown>) =>
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        listToolCredentials: vi.fn().mockResolvedValue({
          ok: true,
          value: { credentials: [], pending_credential_ids: [] },
        }),
        onToolProjection: vi.fn(() => vi.fn()),
        refreshToolProjection: vi.fn().mockResolvedValue({ ok: true, value: definition }),
        subscribeToolProjection: vi.fn().mockResolvedValue({ ok: true, value: undefined }),
        unsubscribeToolProjection: vi.fn().mockResolvedValue({ ok: true, value: undefined }),
        ...v1,
      },
    },
  });

test('a binding that cannot be subscribed says why and that it is retried', async () => {
  stub({
    refreshToolProjection: vi.fn().mockResolvedValue({
      ok: false,
      error: { code: 'tool_projection.unavailable', message: 'no committed value' },
    }),
    subscribeToolProjection: vi.fn().mockResolvedValue({
      ok: false,
      error: { code: 'ipc.request_timeout', message: 'Core request timed out' },
    }),
  });
  render(<BuiltinHttpCredentials toolId={TOOL_ID as never} />);
  fireEvent.click(screen.getByRole('button', { name: '管理 HTTP 凭据与允许网站' }));
  expect(
    await screen.findByText(
      'HTTP 工具凭据绑定还没有读到（Aura Core 没有及时回复），正在重试；一直这样请重启 Aura。',
    ),
  ).toBeTruthy();
  expect(screen.queryByText('正在读取 HTTP 工具凭据绑定…')).toBeNull();
});

test('a clear that did not happen says the tool still uses its credential', async () => {
  stub({
    clearToolInstanceCredential: vi.fn().mockResolvedValue({
      ok: false,
      error: { code: 'tools.failed', message: 'failed' },
    }),
  });
  render(<BuiltinHttpCredentials toolId={TOOL_ID as never} />);
  fireEvent.click(screen.getByRole('button', { name: '管理 HTTP 凭据与允许网站' }));
  fireEvent.click(await screen.findByRole('button', { name: '清除 HTTP 凭据绑定' }));
  expect(await screen.findByText('绑定没有清除，HTTP 工具仍使用原来的凭据；请重试。')).toBeTruthy();
});
