import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import type { ToolDefinitionInstanceView } from '@aura/contracts/generated';
import { ToolCredentialBindingControl } from './ToolCredentialBindingControl';

afterEach(cleanup);
test('changed credential requirements explain the reason and the rebinding action', () => {
  const instance = {
    tool_instance_id: 'tin_01AAAAAAAAAAAAAAAAAAAAAAAA',
    entity_revision: '1',
    credential_slot_binding: {
      slot_key: 'api_key',
      placement: { placement: 'http_bearer_authorization' },
      bound_credential: null,
      rebind_required_reason: 'credential_slot_changed',
    },
  } as ToolDefinitionInstanceView;
  render(
    <ToolCredentialBindingControl
      boundCredential={null}
      busyAction={undefined}
      credential={instance.credential_slot_binding!}
      instance={instance}
      mutable
      runMutation={vi.fn()}
    />,
  );
  expect(screen.getByRole('alert').textContent).toContain('凭据槽或放置方式已变化');
  expect(screen.getByRole('alert').textContent).toContain('重新绑定凭据');
  expect(screen.getByRole('button', { name: '读取可用的凭据' })).toBeDefined();
});

const instance = {
  tool_instance_id: 'tin_01AAAAAAAAAAAAAAAAAAAAAAAA',
  entity_revision: '1',
  credential_slot_binding: {
    slot_key: 'api_key',
    placement: { placement: 'http_bearer_authorization' },
    bound_credential: null,
    rebind_required_reason: null,
  },
} as unknown as ToolDefinitionInstanceView;

const control = (getToolCredentialCandidates: unknown) => {
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        getToolCredentialCandidates,
        listToolCredentials: vi.fn().mockResolvedValue({
          ok: true,
          value: { credentials: [], pending_credential_ids: [] },
        }),
      },
    },
  });
  render(
    <ToolCredentialBindingControl
      boundCredential={null}
      busyAction={undefined}
      credential={instance.credential_slot_binding!}
      instance={instance}
      mutable
      runMutation={vi.fn()}
    />,
  );
};

test('credentials that were not read for binding say so in words', async () => {
  control(
    vi.fn().mockResolvedValue({
      ok: false,
      error: { code: 'ipc.request_timeout', message: 'Core request timed out' },
    }),
  );
  fireEvent.click(screen.getByRole('button', { name: '读取可用的凭据' }));
  expect((await screen.findByRole('alert')).textContent).toBe(
    '可用的凭据没有读到（Aura Core 没有及时回复），暂时不能绑定；请稍后重试。',
  );
});

test('the credentials to bind are named in words, not codes', async () => {
  control(
    vi.fn().mockResolvedValue({
      ok: true,
      value: {
        candidate_set_revision: '1',
        candidates: [
          {
            credential_id: 'cred_01AAAAAAAAAAAAAAAAAAAAAAAA',
            credential_kind: 'api_key',
            credential_revision: 1,
            display_name: '搜索服务',
            scope_summary: { scope: 'user' },
          },
        ],
        credential_slot_key: 'api_key',
        instance_revision: '1',
        placement: { placement: 'http_bearer_authorization' },
        tool_instance_id: instance.tool_instance_id,
      },
    }),
  );
  fireEvent.click(screen.getByRole('button', { name: '读取可用的凭据' }));
  expect(await screen.findByRole('option', { name: '搜索服务 · API Key · 仅本用户' })).toBeTruthy();
  expect(screen.getByRole('option', { name: '请选择凭据' })).toBeTruthy();
  expect(screen.getByRole('combobox', { name: '凭据' })).toBeTruthy();
  expect(screen.getByRole('button', { name: '绑定所选凭据' })).toBeTruthy();
  expect(document.body.textContent).not.toMatch(/Credential|Slot|Core 认证候选/u);
});
