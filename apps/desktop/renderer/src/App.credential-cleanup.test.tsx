import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, test, vi } from 'vitest';
import type { ModelSettingsProjection } from '@aura/contracts/generated';
import { App } from './App';
import {
  installAppTestEnvironment,
  MODEL_ENDPOINT_ID,
  MODEL_PROFILE_ID,
  MODEL_PROVIDER_ID,
  modelSettingsProjection,
} from './app-test-fixtures';

installAppTestEnvironment();

/** Deletes the fixture's Endpoint while Windows Credential Manager keeps its key. */
async function deleteEndpointLeavingItsKey() {
  const provider = modelSettingsProjection.registry.providers[MODEL_PROVIDER_ID];
  if (provider === undefined) throw new Error('fixture Provider is missing');
  const deletedSettings: ModelSettingsProjection = {
    ...modelSettingsProjection,
    active_credentials: [],
    default_model_profile_id: null,
    models_revision: '2',
    registry: {
      ...modelSettingsProjection.registry,
      endpoints: {},
      models: {},
      providers: { [MODEL_PROVIDER_ID]: { ...provider, endpoint_order: [] } },
    },
  };
  vi.mocked(window.aura.v1.getModelSettings)
    .mockResolvedValueOnce({ ok: true, value: modelSettingsProjection })
    .mockResolvedValue({ ok: true, value: deletedSettings });
  vi.mocked(window.aura.v1.deleteModelEndpoint).mockResolvedValue({
    ok: true,
    value: {
      changed: true,
      revision: '2',
      value: {
        credential_cleanup_pending: true,
        default_model_cleared: true,
        deleted: {
          deleted_endpoint_ids: [MODEL_ENDPOINT_ID],
          deleted_model_profile_ids: [MODEL_PROFILE_ID],
          deleted_provider_ids: [],
        },
      },
    },
  });
  const user = userEvent.setup();
  render(<App />);

  await user.click(screen.getByRole('button', { name: '设置' }));
  await user.click(await screen.findByRole('button', { name: '配置 OpenAI' }));
  const providerDialog = screen.getByRole('dialog', { name: 'OpenAI' });
  await user.click(within(providerDialog).getByRole('button', { name: /OpenAI 主连接/u }));
  await user.click(within(providerDialog).getByRole('button', { name: '删除 Endpoint' }));
  const confirmation = screen.getByRole('dialog', { name: '确认删除配置' });
  await user.click(within(confirmation).getByRole('button', { name: '删除 Endpoint' }));
  return user;
}

const notice = async (title: string) => {
  const heading = await screen.findByText(title, { selector: '.notification-copy strong' });
  const article = heading.closest('article');
  if (article === null) throw new Error(`${title} is not a notice`);
  return article;
};

describe('credentials a deletion left in Windows Credential Manager', () => {
  test('the notice names the deleted Endpoint and 重试 reports when the key is gone', async () => {
    vi.mocked(window.aura.v1.retryModelCredentialCleanup).mockResolvedValue({
      ok: true,
      value: { changed: true, pending_credentials: 0 },
    });
    const user = await deleteEndpointLeavingItsKey();

    const pending = await notice('凭据清理待完成');
    expect(pending.textContent).toContain('OpenAI 主连接 已删除');
    await user.click(within(pending).getByRole('button', { name: '重试' }));

    expect(window.aura.v1.retryModelCredentialCleanup).toHaveBeenCalledWith({});
    expect((await notice('凭据已清理')).textContent).toContain('Windows 凭据管理器');
    expect(screen.queryByText('凭据清理待完成')).toBeNull();
  });

  test('a retry that still finds the key keeps 重试 on the notice', async () => {
    vi.mocked(window.aura.v1.retryModelCredentialCleanup).mockResolvedValue({
      ok: true,
      value: { changed: false, pending_credentials: 1 },
    });
    const user = await deleteEndpointLeavingItsKey();

    await user.click(within(await notice('凭据清理待完成')).getByRole('button', { name: '重试' }));

    await waitFor(() =>
      expect(screen.getByText(/仍未删除 1 个凭据，Aura 会在后台继续重试/u)).toBeTruthy(),
    );
    const still = await notice('凭据清理待完成');
    expect(within(still).getByRole('button', { name: '重试' })).toBeTruthy();
  });

  test('a retry Core refused says why and keeps 重试', async () => {
    vi.mocked(window.aura.v1.retryModelCredentialCleanup).mockResolvedValue({
      error: {
        category: 'conflict',
        code: 'broker.facts_changed',
        correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        details: {},
        message: '相关内容正在被其他操作修改，这次请求没有执行，请稍后再试。',
        retryable: true,
        terminal: 'failed',
      },
      ok: false,
    });
    const user = await deleteEndpointLeavingItsKey();

    await user.click(within(await notice('凭据清理待完成')).getByRole('button', { name: '重试' }));

    const refused = await notice('凭据清理未完成');
    expect(refused.textContent).toContain('模型设置正在被修改，请稍后再试。');
    expect(within(refused).getByRole('button', { name: '重试' })).toBeTruthy();
  });
});
