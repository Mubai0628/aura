import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, test, vi } from 'vitest';
import type {
  ModelSettingsProjection,
  ProviderCatalogConnectionDefaultsView,
  ProviderCatalogProjection,
} from '@aura/contracts/generated';
import { App } from './App';
import {
  installAppTestEnvironment,
  MODEL_PROVIDER_ID,
  SUGGESTED_PROVIDER_ID,
  MODEL_ENDPOINT_ID,
  modelSettingsProjection,
  modelProviderCatalog,
} from './app-test-fixtures';

installAppTestEnvironment();

const SUGGESTED_ENDPOINT_ID = 'end_01ARZ3NDEKTSV4RRFFQ69G5FB0';

/** Opens a new connection's editor for a catalog Provider without a preset, whose catalog entry
 *  suggests `connectionDefaults`, and which has a saved connection where `saved`. */
const openSuggestedEditor = async (
  connectionDefaults: ProviderCatalogConnectionDefaultsView,
  saved = false,
): Promise<{ dialog: HTMLElement; user: ReturnType<typeof userEvent.setup> }> => {
  const endpoint = modelSettingsProjection.registry.endpoints[MODEL_ENDPOINT_ID];
  if (endpoint === undefined) throw new Error('fixture Endpoint is missing');
  const suggestedSettings: ModelSettingsProjection = {
    ...modelSettingsProjection,
    registry: {
      ...modelSettingsProjection.registry,
      endpoints: saved
        ? {
            ...modelSettingsProjection.registry.endpoints,
            [SUGGESTED_ENDPOINT_ID]: {
              ...endpoint,
              display_name: 'Suggested 主连接',
              id: SUGGESTED_ENDPOINT_ID,
              model_order: [],
              provider_id: SUGGESTED_PROVIDER_ID,
              slug: 'suggested-main',
            },
          }
        : modelSettingsProjection.registry.endpoints,
      provider_order: [MODEL_PROVIDER_ID, SUGGESTED_PROVIDER_ID],
      providers: {
        ...modelSettingsProjection.registry.providers,
        [SUGGESTED_PROVIDER_ID]: {
          created_at: '2026-09-01T00:00:00Z',
          display_name: 'Suggested API',
          endpoint_order: saved ? [SUGGESTED_ENDPOINT_ID] : [],
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
        connection_defaults: connectionDefaults,
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
  return { dialog, user };
};

const SUGGESTION = '来自 models.dev 的未保存建议';

/** The fields the open editor marks as suggested, by their labels. */
const suggestedFields = (dialog: HTMLElement): string[] =>
  within(dialog)
    .queryAllByText(SUGGESTION)
    .map((note) => note.closest('label')?.querySelector('span')?.textContent ?? '');

describe('a new connection to a catalog Provider without a preset', () => {
  test('prefills only the models.dev HTTPS Base URL suggestion for a manual Provider', async () => {
    const { dialog } = await openSuggestedEditor({
      base_url: 'https://api.suggested.example/v1',
      source: 'models_dev_api_suggestion',
    });

    expect((within(dialog).getByLabelText(/Base URL/u) as HTMLInputElement).value).toBe(
      'https://api.suggested.example/v1',
    );
    expect(suggestedFields(dialog)).toEqual(['Base URL']);
    expect((within(dialog).getByLabelText('API 类型') as HTMLSelectElement).value).toBe('');
    expect((within(dialog).getByLabelText(/认证方式/u) as HTMLSelectElement).value).toBe('none');
  });

  test('drafts the protocol the catalog package speaks; an edited address leaves it suggested until the format changes', async () => {
    const { dialog, user } = await openSuggestedEditor({
      base_url: 'https://api.suggested.example/v1',
      protocol: {
        api_format: 'open_ai_chat_completions',
        auth_scheme: 'open_ai_bearer_api_key',
        chat_instruction_role: 'system',
        chat_output_token_field: 'max_tokens',
      },
      source: 'models_dev_api_suggestion',
    });
    const value = (label: string | RegExp) =>
      (within(dialog).getByLabelText(label) as HTMLInputElement | HTMLSelectElement).value;

    expect(value(/Base URL/u)).toBe('https://api.suggested.example/v1');
    expect(value('API 类型')).toBe('open_ai_chat_completions');
    expect(value(/认证方式/u)).toBe('open_ai_bearer_api_key');
    expect(value('Chat Instruction Role')).toBe('system');
    expect(value('Output Token Field')).toBe('max_tokens');
    expect(suggestedFields(dialog)).toEqual(['API 类型', 'Base URL']);

    await user.type(within(dialog).getByLabelText(/Base URL/u), '/edited');
    expect(suggestedFields(dialog)).toEqual(['API 类型']);
    await user.selectOptions(within(dialog).getByLabelText('API 类型'), 'open_ai_responses');
    expect(suggestedFields(dialog)).toEqual([]);
  });

  test('drafts Anthropic Messages with its own key, and a changed key leaves the address suggested', async () => {
    const { dialog, user } = await openSuggestedEditor({
      base_url: 'https://api.suggested.example/anthropic/v1',
      protocol: { api_format: 'anthropic_messages', auth_scheme: 'anthropic_api_key' },
      source: 'models_dev_api_suggestion',
    });

    expect((within(dialog).getByLabelText('API 类型') as HTMLSelectElement).value).toBe(
      'anthropic_messages',
    );
    expect((within(dialog).getByLabelText(/认证方式/u) as HTMLSelectElement).value).toBe(
      'anthropic_api_key',
    );
    expect(within(dialog).queryByLabelText('Chat Instruction Role')).toBeNull();
    expect(suggestedFields(dialog)).toEqual(['API 类型', 'Base URL']);

    await user.selectOptions(within(dialog).getByLabelText(/认证方式/u), 'none');
    expect(suggestedFields(dialog)).toEqual(['Base URL']);
  });

  test('a saved connection opened after a draft shows none of its suggestions, and the next draft shows them again', async () => {
    const { dialog, user } = await openSuggestedEditor(
      {
        base_url: 'https://api.suggested.example/anthropic/v1',
        protocol: { api_format: 'anthropic_messages', auth_scheme: 'anthropic_api_key' },
        source: 'models_dev_api_suggestion',
      },
      true,
    );
    expect(suggestedFields(dialog)).toEqual(['API 类型', 'Base URL']);

    await user.click(within(dialog).getByRole('button', { name: '返回连接列表' }));
    await user.click(within(dialog).getByRole('button', { name: /Suggested 主连接/u }));
    expect((within(dialog).getByLabelText(/连接名称/u) as HTMLInputElement).value).toBe(
      'Suggested 主连接',
    );
    expect(suggestedFields(dialog)).toEqual([]);

    await user.click(within(dialog).getByRole('button', { name: '返回连接列表' }));
    await user.click(within(dialog).getByRole('button', { name: '添加连接' }));
    expect(suggestedFields(dialog)).toEqual(['API 类型', 'Base URL']);
    await user.selectOptions(within(dialog).getByLabelText('API 类型'), 'open_ai_responses');
    expect(suggestedFields(dialog)).toEqual(['Base URL']);
  });
});
