import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import type { ToolCredentialManagementTargetV1 } from '@aura/contracts/generated';
import { ToolCredentialManager } from './ToolCredentialManager';

afterEach(cleanup);
const target = {
  tool_instance_id: 'tin_01AAAAAAAAAAAAAAAAAAAAAAAA',
  expected_instance_revision: '1',
  credential_slot_key: 'api_key',
  placement: { placement: 'http_bearer_authorization' },
  workspace_id: null,
  session_id: null,
} as ToolCredentialManagementTargetV1;
const record = {
  credential_id: 'cred_01AAAAAAAAAAAAAAAAAAAAAAAA',
  credential_revision: 1,
  row_revision: '1',
  display_name: '测试凭据',
  state: 'configured',
};
function setup(overrides = {}) {
  const api = {
    listToolCredentials: vi.fn().mockResolvedValue({
      ok: true,
      value: { credentials: [record], pending_credential_ids: [] },
    }),
    createToolCredential: vi.fn().mockResolvedValue({
      ok: true,
      value: { credential: record, disposition: 'committed', cleanup_pending: false },
    }),
    rotateToolCredential: vi.fn(),
    clearToolCredential: vi.fn().mockResolvedValue({
      ok: true,
      value: {
        credential: { ...record, state: 'revoked' },
        disposition: 'committed',
        cleanup_pending: false,
      },
    }),
    recoverToolCredential: vi.fn(),
    readToolCredentialOutcome: vi.fn(),
    ...overrides,
  };
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1: api } });
  render(
    <ToolCredentialManager target={target} disabled={false} onChanged={async () => undefined} />,
  );
  return api;
}
test('creating a credential sends bytes once, clears the input and zeroizes the handed buffer', async () => {
  const api = setup();
  await userEvent.type(screen.getByLabelText('名称'), '新工具凭据');
  await userEvent.type(screen.getByLabelText('密钥'), 'synthetic-secret');
  await userEvent.click(screen.getByRole('button', { name: '创建凭据' }));
  await waitFor(() => expect(api.createToolCredential).toHaveBeenCalledTimes(1));
  expect(api.createToolCredential.mock.calls[0]?.[0].target).toEqual(target);
  expect([...api.createToolCredential.mock.calls[0]![1]]).toEqual(Array(16).fill(0));
  expect((screen.getByLabelText('密钥') as HTMLInputElement).value).toBe('');
  expect(screen.queryByText('synthetic-secret')).toBeNull();
});

test('the named credential selector permits rotation of the current record once', async () => {
  const api = setup({
    rotateToolCredential: vi.fn().mockResolvedValue({
      ok: true,
      value: { credential: record, disposition: 'committed', cleanup_pending: false },
    }),
  });
  await userEvent.click(screen.getByRole('button', { name: '读取凭据管理状态' }));
  await userEvent.selectOptions(screen.getByLabelText('操作'), 'rotate');
  await userEvent.selectOptions(
    screen.getByRole('combobox', { name: /^凭据$/ }),
    record.credential_id,
  );
  await userEvent.type(screen.getByLabelText('密钥'), 'replacement-secret');
  await userEvent.click(screen.getByRole('button', { name: '确认轮换密钥' }));
  await waitFor(() => expect(api.rotateToolCredential).toHaveBeenCalledTimes(1));
  expect(api.rotateToolCredential.mock.calls[0]?.[0]).toEqual({
    target,
    credential_id: record.credential_id,
    expected_row_revision: record.row_revision,
  });
  expect([...api.rotateToolCredential.mock.calls[0]![1]]).toEqual(Array(18).fill(0));
  expect((screen.getByLabelText('密钥') as HTMLInputElement).value).toBe('');
});
test('an unconfirmed write stays locked when list refresh fails and is never resubmitted', async () => {
  const api = setup({
    createToolCredential: vi.fn().mockResolvedValue({
      ok: false,
      error: {
        code: 'command.outcome_unknown',
        terminal: 'failed',
        category: 'unavailable',
        details: {},
      },
    }),
    listToolCredentials: vi.fn().mockResolvedValue({ ok: false, error: { code: 'unavailable' } }),
  });
  await userEvent.type(screen.getByLabelText('名称'), '新工具凭据');
  await userEvent.type(screen.getByLabelText('密钥'), 'synthetic-secret');
  await userEvent.click(screen.getByRole('button', { name: '创建凭据' }));
  await screen.findByRole('button', { name: '检查原操作结果' });
  await userEvent.click(screen.getByRole('button', { name: '读取凭据管理状态' }));
  expect((screen.getByRole('button', { name: '创建凭据' }) as HTMLButtonElement).disabled).toBe(
    true,
  );
  expect(api.createToolCredential).toHaveBeenCalledTimes(1);
});
test('clear requires explicit confirmation of a selected record and preserves its revision', async () => {
  const api = setup();
  await userEvent.click(screen.getByRole('button', { name: '读取凭据管理状态' }));
  await userEvent.selectOptions(screen.getByLabelText('操作'), 'clear');
  await userEvent.selectOptions(screen.getByLabelText('凭据'), record.credential_id);
  expect((screen.getByRole('button', { name: '确认清除密钥' }) as HTMLButtonElement).disabled).toBe(
    true,
  );
  await userEvent.click(screen.getByRole('checkbox'));
  await userEvent.click(screen.getByRole('button', { name: '确认清除密钥' }));
  await waitFor(() =>
    expect(api.clearToolCredential).toHaveBeenCalledWith({
      target,
      credential_id: record.credential_id,
      expected_row_revision: '1',
    }),
  );
});

test('a completed clear refreshes the selected metadata before releasing the controls', async () => {
  const api = setup({
    listToolCredentials: vi
      .fn()
      .mockResolvedValueOnce({
        ok: true,
        value: { credentials: [record], pending_credential_ids: [] },
      })
      .mockResolvedValue({
        ok: true,
        value: { credentials: [{ ...record, state: 'revoked' }], pending_credential_ids: [] },
      }),
  });
  await userEvent.click(screen.getByRole('button', { name: '读取凭据管理状态' }));
  await userEvent.selectOptions(screen.getByLabelText('操作'), 'clear');
  await userEvent.selectOptions(screen.getByLabelText('凭据'), record.credential_id);
  await userEvent.click(screen.getByRole('checkbox'));
  await userEvent.click(screen.getByRole('button', { name: '确认清除密钥' }));
  await screen.findByRole('option', { name: '测试凭据 · 已清除' });
  await waitFor(() =>
    expect(screen.getByRole('region', { name: '管理工具凭据' }).getAttribute('aria-busy')).toBe(
      'false',
    ),
  );
  expect(api.clearToolCredential).toHaveBeenCalledTimes(1);
  expect((screen.getByLabelText('凭据') as HTMLSelectElement).value).toBe(record.credential_id);
});
test('a pending credential cannot be rotated or cleared and offers recovery of the same record', async () => {
  setup({
    listToolCredentials: vi.fn().mockResolvedValue({
      ok: true,
      value: { credentials: [record], pending_credential_ids: [record.credential_id] },
    }),
  });
  await userEvent.click(screen.getByRole('button', { name: '读取凭据管理状态' }));
  await userEvent.selectOptions(screen.getByLabelText('操作'), 'clear');
  await userEvent.selectOptions(screen.getByLabelText('凭据'), record.credential_id);
  await userEvent.click(screen.getByRole('checkbox'));
  expect((screen.getByRole('button', { name: '确认清除密钥' }) as HTMLButtonElement).disabled).toBe(
    true,
  );
  await userEvent.selectOptions(screen.getByLabelText('操作'), 'recover');
  expect((screen.getByRole('button', { name: '恢复所选凭据' }) as HTMLButtonElement).disabled).toBe(
    false,
  );
});

test.each(['committed', 'unknown'])(
  'recovering another credential does not replace the original receipt (%s)',
  async (recoveryState) => {
    const command = 'civ_01AAAAAAAAAAAAAAAAAAAAAAAA';
    const api = setup({
      createToolCredential: vi.fn().mockResolvedValue({
        ok: false,
        error: {
          code: 'command.outcome_unknown',
          terminal: 'failed',
          category: 'unavailable',
          details: { command_invocation_id: command },
        },
      }),
      recoverToolCredential: vi.fn().mockResolvedValue(
        recoveryState === 'committed'
          ? {
              ok: true,
              value: { credential: record, disposition: 'committed', cleanup_pending: false },
            }
          : {
              ok: false,
              error: {
                code: 'command.outcome_unknown',
                terminal: 'failed',
                category: 'unavailable',
                details: { command_invocation_id: 'civ_01BBBBBBBBBBBBBBBBBBBBBBBB' },
              },
            },
      ),
      readToolCredentialOutcome: vi.fn().mockResolvedValue({
        ok: true,
        value: { command_invocation_id: command, state: 'not_obtained', result: null },
      }),
    });
    await userEvent.type(screen.getByLabelText('名称'), '未确认凭据');
    await userEvent.type(screen.getByLabelText('密钥'), 'synthetic-secret');
    await userEvent.click(screen.getByRole('button', { name: '创建凭据' }));
    await screen.findByRole('button', { name: '检查原操作结果' });
    await userEvent.click(screen.getByRole('button', { name: '读取凭据管理状态' }));
    await userEvent.selectOptions(screen.getByLabelText('操作'), 'recover');
    await userEvent.selectOptions(screen.getByLabelText('凭据'), record.credential_id);
    await userEvent.click(screen.getByRole('button', { name: '恢复所选凭据' }));
    await waitFor(() => expect(api.recoverToolCredential).toHaveBeenCalledTimes(1));
    await userEvent.click(screen.getByRole('button', { name: '检查原操作结果' }));
    await waitFor(() =>
      expect(api.readToolCredentialOutcome).toHaveBeenCalledWith({
        target,
        command_invocation_id: command,
      }),
    );
    await userEvent.selectOptions(screen.getByLabelText('操作'), 'create');
    expect((screen.getByRole('button', { name: '创建凭据' }) as HTMLButtonElement).disabled).toBe(
      true,
    );
    expect(api.createToolCredential).toHaveBeenCalledTimes(1);
  },
);
