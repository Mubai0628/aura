import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, test, vi } from 'vitest';
import type { ModelSettingsProjection, ProviderCatalogProjection } from '@aura/contracts/generated';
import { App } from './App';
import {
  installAppTestEnvironment,
  MODEL_PROVIDER_ID,
  ANTHROPIC_PROVIDER_ID,
  SUGGESTED_PROVIDER_ID,
  MODEL_ENDPOINT_ID,
  MODEL_PROFILE_ID,
  modelSettingsProjection,
  modelProviderCatalog,
} from './app-test-fixtures';

installAppTestEnvironment();

describe('P05 prerequisite desktop shell', () => {
  test('keeps incomplete Chat fields local, marks both fields, and saves after correction', async () => {
    const endpoint = modelSettingsProjection.registry.endpoints[MODEL_ENDPOINT_ID];
    if (endpoint === undefined) throw new Error('fixture Endpoint is missing');
    vi.mocked(window.aura.v1.updateModelEndpoint).mockResolvedValue({
      ok: true,
      value: {
        changed: true,
        revision: '2',
        value: {
          ...endpoint,
          api_format: 'open_ai_chat_completions',
          chat_instruction_role: 'system',
          chat_output_token_field: 'max_completion_tokens',
          revision: '2',
        },
      },
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole('button', { name: '设置' }));
    await user.click(await screen.findByRole('button', { name: '配置 OpenAI' }));
    const dialog = screen.getByRole('dialog', { name: 'OpenAI' });
    await user.click(within(dialog).getByRole('button', { name: /OpenAI 主连接/u }));
    await user.selectOptions(
      within(dialog).getByRole('combobox', { name: 'API 类型' }),
      'open_ai_chat_completions',
    );
    await user.click(within(dialog).getByRole('button', { name: '保存连接' }));

    expect(window.aura.v1.updateModelEndpoint).not.toHaveBeenCalled();
    const instructionRole = within(dialog).getByRole('combobox', {
      name: 'Chat Instruction Role',
    });
    const outputTokenField = within(dialog).getByRole('combobox', {
      name: 'Output Token Field',
    });
    expect(instructionRole.getAttribute('aria-invalid')).toBe('true');
    expect(outputTokenField.getAttribute('aria-invalid')).toBe('true');
    expect(
      within(dialog).getByText('OpenAI Chat Completions 必须选择 Chat Instruction Role'),
    ).toBeTruthy();
    expect(
      within(dialog).getByText('OpenAI Chat Completions 必须选择 Output Token Field'),
    ).toBeTruthy();
    await waitFor(() => expect(document.activeElement).toBe(instructionRole));

    await user.selectOptions(instructionRole, 'system');
    await user.selectOptions(outputTokenField, 'max_completion_tokens');
    await user.click(within(dialog).getByRole('button', { name: '保存连接' }));
    await waitFor(() => expect(window.aura.v1.updateModelEndpoint).toHaveBeenCalledTimes(1));
    expect(window.aura.v1.updateModelEndpoint).toHaveBeenCalledWith(
      expect.objectContaining({
        api_format: 'open_ai_chat_completions',
        chat_instruction_role: 'system',
        chat_output_token_field: 'max_completion_tokens',
        endpoint_id: MODEL_ENDPOINT_ID,
      }),
    );
  });

  // Timeouts and limits are folded
  // under 高级设置, and a refusal about one of them opens it to show the field.
  test('folds the timeouts under 高级设置 and opens them for a field error', async () => {
    vi.mocked(window.aura.v1.updateModelEndpoint).mockResolvedValue({
      error: {
        category: 'validation',
        code: 'models.endpoint.invalid_timeout',
        correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        details: {},
        message: 'fixture invalid timeout',
        retryable: false,
        terminal: 'failed',
      },
      ok: false,
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole('button', { name: '设置' }));
    await user.click(await screen.findByRole('button', { name: '配置 OpenAI' }));
    const dialog = screen.getByRole('dialog', { name: 'OpenAI' });
    await user.click(within(dialog).getByRole('button', { name: /OpenAI 主连接/u }));
    const advanced = within(dialog).getByRole('button', { name: '高级设置' });
    expect(advanced.getAttribute('aria-expanded')).toBe('false');
    expect(within(dialog).queryByLabelText('Connect Timeout（ms）')).toBeNull();
    await user.type(within(dialog).getByRole('textbox', { name: /连接名称/u }), ' 2');
    await user.click(within(dialog).getByRole('button', { name: '保存连接' }));

    await waitFor(() => expect(window.aura.v1.updateModelEndpoint).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(advanced.getAttribute('aria-expanded')).toBe('true'));
    const timeout = within(dialog).getByLabelText('Connect Timeout（ms）');
    expect(timeout.getAttribute('aria-invalid')).toBe('true');
    expect(within(dialog).getByText('超时必须是大于 0 的整数')).toBeTruthy();
  });

  test('maps a Core Endpoint field error back to the same inline field', async () => {
    vi.mocked(window.aura.v1.updateModelEndpoint).mockResolvedValue({
      error: {
        category: 'validation',
        code: 'models.endpoint.invalid_base_url',
        correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        details: { field: 'base_url' },
        message: 'fixture invalid base URL',
        retryable: false,
        terminal: 'failed',
      },
      ok: false,
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole('button', { name: '设置' }));
    await user.click(await screen.findByRole('button', { name: '配置 OpenAI' }));
    const dialog = screen.getByRole('dialog', { name: 'OpenAI' });
    await user.click(within(dialog).getByRole('button', { name: /OpenAI 主连接/u }));
    const baseUrl = within(dialog).getByRole('textbox', { name: /Base URL/u });
    await user.clear(baseUrl);
    await user.type(baseUrl, 'not-a-valid-url');
    await user.click(within(dialog).getByRole('button', { name: '保存连接' }));

    await waitFor(() => expect(window.aura.v1.updateModelEndpoint).toHaveBeenCalledTimes(1));
    expect(baseUrl.getAttribute('aria-invalid')).toBe('true');
    expect(
      within(dialog).getByText('Base URL 无效，请填写不含用户信息、查询参数或片段的有效地址'),
    ).toBeTruthy();
    await waitFor(() => expect(document.activeElement).toBe(baseUrl));
  });

  test('retains per-adapter drafts while serializing hidden adapter fields as null', async () => {
    const endpoint = modelSettingsProjection.registry.endpoints[MODEL_ENDPOINT_ID];
    if (endpoint === undefined) throw new Error('fixture Endpoint is missing');
    vi.mocked(window.aura.v1.updateModelEndpoint).mockResolvedValue({
      ok: true,
      value: {
        changed: true,
        revision: '2',
        value: { ...endpoint, base_url: 'https://api.openai.com/v2', revision: '2' },
      },
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole('button', { name: '设置' }));
    await user.click(await screen.findByRole('button', { name: '配置 OpenAI' }));
    const dialog = screen.getByRole('dialog', { name: 'OpenAI' });
    await user.click(within(dialog).getByRole('button', { name: /OpenAI 主连接/u }));
    const apiFormat = within(dialog).getByRole('combobox', { name: 'API 类型' });
    await user.selectOptions(apiFormat, 'open_ai_chat_completions');
    await user.selectOptions(
      within(dialog).getByRole('combobox', { name: 'Chat Instruction Role' }),
      'developer',
    );
    await user.selectOptions(
      within(dialog).getByRole('combobox', { name: 'Output Token Field' }),
      'max_tokens',
    );
    await user.selectOptions(apiFormat, 'open_ai_responses');
    await user.selectOptions(apiFormat, 'open_ai_chat_completions');
    expect(
      (
        within(dialog).getByRole('combobox', {
          name: 'Chat Instruction Role',
        }) as HTMLSelectElement
      ).value,
    ).toBe('developer');
    expect(
      (
        within(dialog).getByRole('combobox', {
          name: 'Output Token Field',
        }) as HTMLSelectElement
      ).value,
    ).toBe('max_tokens');

    await user.selectOptions(apiFormat, 'open_ai_responses');
    const baseUrl = within(dialog).getByRole('textbox', { name: 'Base URL' });
    await user.clear(baseUrl);
    await user.type(baseUrl, 'https://api.openai.com/v2');
    await user.click(within(dialog).getByRole('button', { name: '保存连接' }));

    await waitFor(() => expect(window.aura.v1.updateModelEndpoint).toHaveBeenCalledTimes(1));
    expect(window.aura.v1.updateModelEndpoint).toHaveBeenCalledWith(
      expect.objectContaining({
        api_format: 'open_ai_responses',
        chat_instruction_role: null,
        chat_output_token_field: null,
      }),
    );
  });

  test('returns focus to Add connection after an Endpoint deletion completes', async () => {
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
        providers: {
          [MODEL_PROVIDER_ID]: { ...provider, endpoint_order: [] },
        },
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
          credential_cleanup_pending: false,
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

    const addConnection = await within(providerDialog).findByRole('button', {
      name: '添加连接',
    });
    await waitFor(() => expect(document.activeElement).toBe(addConnection));
    await user.click(addConnection);
    await waitFor(() =>
      expect(document.activeElement).toBe(
        within(providerDialog).getByRole('textbox', { name: '连接名称' }),
      ),
    );
  });

  test('applies the recommended trusted Preset when creating an Endpoint draft', async () => {
    const presetSettings: ModelSettingsProjection = {
      ...modelSettingsProjection,
      registry: {
        ...modelSettingsProjection.registry,
        provider_order: [MODEL_PROVIDER_ID, ANTHROPIC_PROVIDER_ID],
        providers: {
          ...modelSettingsProjection.registry.providers,
          [ANTHROPIC_PROVIDER_ID]: {
            created_at: '2026-09-01T00:00:00Z',
            display_name: 'Anthropic',
            endpoint_order: [],
            id: ANTHROPIC_PROVIDER_ID,
            slug: 'anthropic',
            source: { catalog_provider_id: 'anthropic', type: 'models_dev' },
            updated_at: '2026-09-01T00:00:00Z',
          },
        },
      },
    };
    const presetCatalog: ProviderCatalogProjection = {
      ...modelProviderCatalog,
      cards: modelProviderCatalog.cards.map((card) =>
        card.catalog_provider_id === 'anthropic'
          ? { ...card, local_provider_id: ANTHROPIC_PROVIDER_ID }
          : card,
      ),
    };
    vi.mocked(window.aura.v1.getModelSettings).mockResolvedValue({
      ok: true,
      value: presetSettings,
    });
    vi.mocked(window.aura.v1.getModelProviderCatalog).mockResolvedValue({
      ok: true,
      value: presetCatalog,
    });
    vi.mocked(window.aura.v1.ensureModelProviderCatalogFresh).mockResolvedValue({
      ok: true,
      value: presetCatalog,
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole('button', { name: '设置' }));
    await user.click(await screen.findByRole('button', { name: '配置 Anthropic' }));
    const dialog = screen.getByRole('dialog', { name: 'Anthropic' });
    await user.click(within(dialog).getByRole('button', { name: '添加连接' }));

    expect((within(dialog).getByLabelText(/Aura 连接预设/u) as HTMLSelectElement).value).toBe(
      'anthropic/messages',
    );
    expect((within(dialog).getByLabelText(/连接名称/u) as HTMLInputElement).value).toBe(
      'Anthropic Messages',
    );
    expect((within(dialog).getByLabelText(/Base URL/u) as HTMLInputElement).value).toBe(
      'https://api.anthropic.com/v1',
    );
    expect((within(dialog).getByLabelText('API 类型') as HTMLSelectElement).value).toBe(
      'anthropic_messages',
    );
    expect((within(dialog).getByLabelText(/认证方式/u) as HTMLSelectElement).value).toBe(
      'anthropic_api_key',
    );
    expect((within(dialog).getByLabelText(/模型目录请求模式/u) as HTMLSelectElement).value).toBe(
      'automatic',
    );
  });

  test('prefills only the models.dev HTTPS Base URL suggestion for a manual Provider', async () => {
    const suggestedSettings: ModelSettingsProjection = {
      ...modelSettingsProjection,
      registry: {
        ...modelSettingsProjection.registry,
        provider_order: [MODEL_PROVIDER_ID, SUGGESTED_PROVIDER_ID],
        providers: {
          ...modelSettingsProjection.registry.providers,
          [SUGGESTED_PROVIDER_ID]: {
            created_at: '2026-09-01T00:00:00Z',
            display_name: 'Suggested API',
            endpoint_order: [],
            id: SUGGESTED_PROVIDER_ID,
            slug: 'suggested-api',
            source: { catalog_provider_id: 'suggested-api', type: 'models_dev' },
            updated_at: '2026-09-01T00:00:00Z',
          },
        },
      },
    };
    const suggestedCatalog: ProviderCatalogProjection = {
      ...modelProviderCatalog,
      cards: [
        ...modelProviderCatalog.cards,
        {
          catalog_model_count: 5,
          catalog_provider_id: 'suggested-api',
          configuration_class: 'manual_configuration',
          connection_defaults: {
            base_url: 'https://api.suggested.example/v1',
            source: 'models_dev_api_suggestion',
          },
          display_name: 'Suggested API',
          fallback_initial: 'S',
          icon_ref: null,
          local_provider_id: SUGGESTED_PROVIDER_ID,
        },
      ],
    };
    vi.mocked(window.aura.v1.getModelSettings).mockResolvedValue({
      ok: true,
      value: suggestedSettings,
    });
    vi.mocked(window.aura.v1.getModelProviderCatalog).mockResolvedValue({
      ok: true,
      value: suggestedCatalog,
    });
    vi.mocked(window.aura.v1.ensureModelProviderCatalogFresh).mockResolvedValue({
      ok: true,
      value: suggestedCatalog,
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole('button', { name: '设置' }));
    await user.click(await screen.findByRole('button', { name: '配置 Suggested API' }));
    const dialog = screen.getByRole('dialog', { name: 'Suggested API' });
    await user.click(within(dialog).getByRole('button', { name: '添加连接' }));

    expect((within(dialog).getByLabelText(/Base URL/u) as HTMLInputElement).value).toBe(
      'https://api.suggested.example/v1',
    );
    expect(within(dialog).getByText('来自 models.dev 的未保存建议')).toBeTruthy();
    expect((within(dialog).getByLabelText('API 类型') as HTMLSelectElement).value).toBe('');
    expect((within(dialog).getByLabelText(/认证方式/u) as HTMLSelectElement).value).toBe('none');
  });
});
