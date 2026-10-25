import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, test, vi } from 'vitest';
import type { ModelSettingsProjection } from '@aura/contracts/generated';
import { App } from './App';
import {
  installAppTestEnvironment,
  MODEL_PROVIDER_ID,
  MODEL_ENDPOINT_ID,
  SECOND_MODEL_ENDPOINT_ID,
  MODEL_PROFILE_ID,
  SECOND_MODEL_PROFILE_ID,
  MODEL_CREDENTIAL_ID,
  modelSettingsProjection,
} from './app-test-fixtures';

installAppTestEnvironment();

describe('P05 prerequisite desktop shell', () => {
  test('opens Provider-centric model settings as an independent top-level page', async () => {
    const user = userEvent.setup();
    const { container } = render(<App />);

    await user.click(screen.getByRole('button', { name: '设置' }));
    expect(screen.getByRole('heading', { level: 1, name: '设置' })).toBeTruthy();
    expect(screen.getByRole('heading', { level: 2, name: '模型配置' })).toBeTruthy();
    expect(container.querySelector('.workspace-shell')).toBeNull();
    expect(container.querySelector('.conversation-workspace')).toBeNull();
    expect(await screen.findByRole('button', { name: /OpenAI.*47 个目录模型/u })).toBeTruthy();
    // No explanatory notice on the page.
    expect(screen.queryByText(/配置与凭据均由 Aura Core 负责/u)).toBeNull();
    expect(screen.queryByRole('button', { name: /Responses Profile/u })).toBeNull();
    expect(screen.getByRole('button', { name: /OpenAI.*47 个目录模型/u })).toBeTruthy();
    expect(screen.getByRole('button', { name: /Anthropic.*13 个目录模型/u })).toBeTruthy();
    expect(screen.getByRole('button', { name: /Legacy Catalog.*3 个目录模型/u })).toBeTruthy();
    expect(screen.getByRole('button', { name: /添加提供商/u })).toBeTruthy();
    expect(screen.getAllByText('gpt-5.6-sol')).toHaveLength(2);
    expect(screen.getByRole('button', { name: '测试可用性' })).toBeTruthy();

    await user.click(screen.getByRole('button', { name: '配置 OpenAI' }));
    const dialog = screen.getByRole('dialog', { name: 'OpenAI' });
    await user.click(within(dialog).getByRole('button', { name: /OpenAI 主连接/u }));
    expect(within(dialog).getByRole('combobox', { name: 'API 类型' })).toBeTruthy();
    expect(within(dialog).getByRole('option', { name: 'OpenAI Responses' })).toBeTruthy();
    expect(within(dialog).getByRole('option', { name: 'OpenAI Chat Completions' })).toBeTruthy();
    expect(within(dialog).getByRole('option', { name: 'Anthropic Messages' })).toBeTruthy();
    // No explanations in the Provider dialog or the connection form.
    expect(
      within(dialog).queryByText(
        /已保存 Secret 永不回读|同一个 Provider 可以保存多个|编译期可信预设/u,
      ),
    ).toBeNull();
    expect((within(dialog).getByLabelText('API Key') as HTMLInputElement).type).toBe('password');
    expect(within(dialog).queryByRole('button', { name: '显示本次输入的 API Key' })).toBeNull();
    expect(
      (within(dialog).getByRole('button', { name: '获取模型' }) as HTMLButtonElement).disabled,
    ).toBe(false);
    expect(
      (within(dialog).getByRole('button', { name: '保存连接' }) as HTMLButtonElement).disabled,
    ).toBe(true);
    expect((screen.getByRole('button', { name: '当前默认' }) as HTMLButtonElement).disabled).toBe(
      true,
    );

    await user.click(within(dialog).getByRole('button', { name: '关闭 Provider 配置' }));
    await user.click(screen.getByRole('button', { name: '配置 Legacy Catalog' }));
    const informationOnlyDialog = screen.getByRole('dialog', { name: 'Legacy Catalog' });
    expect(
      (
        within(informationOnlyDialog).getByRole('button', {
          name: '添加此提供商',
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
    expect(within(informationOnlyDialog).getByText(/仅提供目录信息/u)).toBeTruthy();
    // A provider not added yet has no connection; its model list opens to read.
    await user.click(within(informationOnlyDialog).getByRole('button', { name: '浏览模型列表' }));
    const browse = await screen.findByRole('dialog', { name: /模型目录$/u });
    expect(within(browse).getByRole('note').textContent).toMatch(/当前只浏览目录信息/u);
    await user.click(within(browse).getByRole('button', { name: '关闭模型目录' }));
    await user.click(
      within(informationOnlyDialog).getByRole('button', { name: '关闭 Provider 配置' }),
    );
    await user.click(screen.getByRole('button', { name: '返回会话' }));
    expect(screen.getByRole('heading', { level: 2, name: '合成桌面会话' })).toBeTruthy();
  });

  test('shows Core model settings before catalog freshness settles', async () => {
    vi.mocked(window.aura.v1.ensureModelProviderCatalogFresh).mockReturnValue(
      new Promise(() => undefined),
    );
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole('button', { name: '设置' }));
    expect(await screen.findByRole('heading', { level: 2, name: '模型配置' })).toBeTruthy();
    expect(screen.queryByText('正在读取 Core 模型配置…')).toBeNull();
  });

  test('scopes model cards, counts, availability, and catalog target to the selected Endpoint', async () => {
    const firstEndpoint = modelSettingsProjection.registry.endpoints[MODEL_ENDPOINT_ID];
    const firstProfile = modelSettingsProjection.registry.models[MODEL_PROFILE_ID];
    const provider = modelSettingsProjection.registry.providers[MODEL_PROVIDER_ID];
    if (firstEndpoint === undefined || firstProfile === undefined || provider === undefined) {
      throw new Error('fixture model registry is incomplete');
    }
    const scopedSettings: ModelSettingsProjection = {
      ...modelSettingsProjection,
      registry: {
        ...modelSettingsProjection.registry,
        endpoints: {
          [MODEL_ENDPOINT_ID]: {
            ...firstEndpoint,
            display_name: 'Responses Endpoint',
            model_order: [MODEL_PROFILE_ID],
          },
          [SECOND_MODEL_ENDPOINT_ID]: {
            ...firstEndpoint,
            api_format: 'open_ai_chat_completions',
            chat_instruction_role: 'system',
            chat_output_token_field: 'max_completion_tokens',
            display_name: 'Chat Endpoint',
            id: SECOND_MODEL_ENDPOINT_ID,
            model_order: [SECOND_MODEL_PROFILE_ID],
            revision: '2',
            slug: 'openai-chat',
          },
        },
        models: {
          [MODEL_PROFILE_ID]: {
            ...firstProfile,
            display_name: 'Responses scoped profile',
          },
          [SECOND_MODEL_PROFILE_ID]: {
            ...firstProfile,
            display_name: 'Chat scoped profile',
            endpoint_id: SECOND_MODEL_ENDPOINT_ID,
            id: SECOND_MODEL_PROFILE_ID,
            revision: '2',
          },
        },
        providers: {
          [MODEL_PROVIDER_ID]: {
            ...provider,
            endpoint_order: [MODEL_ENDPOINT_ID, SECOND_MODEL_ENDPOINT_ID],
          },
        },
      },
    };
    vi.mocked(window.aura.v1.getModelSettings).mockResolvedValue({
      ok: true,
      value: scopedSettings,
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole('button', { name: '设置' }));
    const endpointSelector = await screen.findByRole('combobox', {
      name: 'Model Profile 目标 Endpoint',
    });
    expect(screen.getByText('Responses scoped profile')).toBeTruthy();
    expect(screen.queryByText('Chat scoped profile')).toBeNull();
    expect(document.querySelector('.model-section-count')?.textContent).toBe('1 个');

    await user.selectOptions(endpointSelector, SECOND_MODEL_ENDPOINT_ID);
    expect(screen.getByText('Chat scoped profile')).toBeTruthy();
    expect(screen.queryByText('Responses scoped profile')).toBeNull();
    expect(document.querySelector('.model-section-count')?.textContent).toBe('1 个');

    await user.click(screen.getByRole('button', { name: '浏览模型列表' }));
    const catalogDialog = await screen.findByRole('dialog', { name: 'OpenAI 模型目录' });
    expect(
      (within(catalogDialog).getByRole('combobox', { name: '目标 Endpoint' }) as HTMLSelectElement)
        .value,
    ).toBe(SECOND_MODEL_ENDPOINT_ID);
  });

  test('marks an exact Endpoint and provider model ID match as already added', async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole('button', { name: '设置' }));
    await user.click(await screen.findByRole('button', { name: '浏览模型列表' }));

    const catalogDialog = await screen.findByRole('dialog', { name: 'OpenAI 模型目录' });
    const added = within(catalogDialog).getByRole('button', { name: '已添加' });
    expect((added as HTMLButtonElement).disabled).toBe(true);
  });

  test('shows a retained dormant Credential only under an auth-none Endpoint', async () => {
    const endpoint = modelSettingsProjection.registry.endpoints[MODEL_ENDPOINT_ID];
    if (endpoint === undefined) throw new Error('fixture Endpoint is missing');
    const dormantSettings: ModelSettingsProjection = {
      ...modelSettingsProjection,
      active_credentials: [],
      dormant_credentials: [
        {
          credential_id: MODEL_CREDENTIAL_ID,
          endpoint_id: MODEL_ENDPOINT_ID,
          error_code: null,
          last_committed_credential_revision: '1',
          retained_state: 'configured',
          row_revision: '2',
          slot_name: 'api-key',
        },
      ],
      registry: {
        ...modelSettingsProjection.registry,
        endpoints: {
          ...modelSettingsProjection.registry.endpoints,
          [MODEL_ENDPOINT_ID]: { ...endpoint, auth_scheme: 'none' },
        },
      },
    };
    vi.mocked(window.aura.v1.getModelSettings).mockResolvedValue({
      ok: true,
      value: dormantSettings,
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole('button', { name: '设置' }));
    await user.click(await screen.findByRole('button', { name: '配置 OpenAI' }));
    const dialog = screen.getByRole('dialog', { name: 'OpenAI' });
    await user.click(within(dialog).getByRole('button', { name: /OpenAI 主连接/u }));

    expect(within(dialog).getByText('已保留凭据，当前未使用')).toBeTruthy();
    expect(within(dialog).getByRole('button', { name: '清除已保留凭据' })).toBeTruthy();
    expect(within(dialog).queryByLabelText('API Key')).toBeNull();
  });

  test('keeps the last model settings revision visible through a transient Core reconnect', async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole('button', { name: '设置' }));
    expect(await screen.findByRole('heading', { level: 2, name: '模型配置' })).toBeTruthy();
    await user.click(screen.getByRole('button', { name: '返回会话' }));

    vi.mocked(window.aura.v1.getModelSettings).mockResolvedValue({
      error: {
        category: 'unavailable',
        code: 'ipc.transport_unavailable',
        correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        details: {},
        message: 'fixture reconnect',
        retryable: true,
        terminal: 'failed',
      },
      ok: false,
    });
    await user.click(screen.getByRole('button', { name: '设置' }));

    const heading = await screen.findByRole('heading', { level: 2, name: '模型配置' });
    expect(screen.getByRole('status').textContent).toContain('现有配置仍可查看');
    expect(screen.queryByText('模型配置不可用')).toBeNull();
    // The last configuration read stays; its revision is the heading's title now.
    expect(heading.getAttribute('title')).toBe('配置修订 1');
  });
});
