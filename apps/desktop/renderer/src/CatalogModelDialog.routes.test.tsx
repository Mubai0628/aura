import type {
  ModelEndpointConfig,
  ModelSettingsProjection,
  ProviderCatalogModelPage,
  ProviderCatalogModelView,
  ProviderPresetVariantView,
} from '@aura/contracts/generated';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';

import { CatalogModelDialog } from './CatalogModelDialog';

// A Provider whose models each go by their own route: every model goes to the connection of its
// route, a model whose route is not connected yet or that Aura cannot reach is not added, and
// there is no one connection to pick for all of them.

const GO_OPENAI = 'https://opencode.ai/inference/go/openai/v1';
const GO_ANTHROPIC = 'https://opencode.ai/inference/go/anthropic/v1';
const CHAT_ID = 'end_01ARZ3NDEKTSV4RRFFQ69G5FC1';
const ANTHROPIC_ID = 'end_01ARZ3NDEKTSV4RRFFQ69G5FC2';

const variant = (
  variant_id: string,
  base_url: string,
  api_format: ProviderPresetVariantView['api_format'],
): ProviderPresetVariantView => ({
  api_format,
  auth_scheme: api_format === 'anthropic_messages' ? 'anthropic_api_key' : 'open_ai_bearer_api_key',
  base_url,
  chat_instruction_role: null,
  chat_output_token_field: null,
  discovery_mode: { type: 'disabled' },
  display_name: `OpenCode Go ${variant_id}`,
  recommended: false,
  variant_id,
});

const endpoint = (
  id: string,
  display_name: string,
  base_url: string,
  api_format: ModelEndpointConfig['api_format'],
) => ({ api_format, base_url, display_name, id, revision: '3' }) as unknown as ModelEndpointConfig;

const ENDPOINTS = [
  endpoint(CHAT_ID, '聊天连接', GO_OPENAI, 'open_ai_chat_completions'),
  endpoint(ANTHROPIC_ID, 'Anthropic 连接', GO_ANTHROPIC, 'anthropic_messages'),
];

const model = (id: string, route: ProviderCatalogModelView['route']): ProviderCatalogModelView =>
  ({
    catalog_object_hash: { algorithm: 'blake3', hex: id.padEnd(64, '0').slice(0, 64) },
    model: { catalog_model_id: id, display_name: id },
    route,
  }) as unknown as ProviderCatalogModelView;

const page = {
  catalog_content_hash: { algorithm: 'sha256', hex: '0'.repeat(64) },
  catalog_revision: '9',
  models: [
    model('glm-5.3', { type: 'variant', variant_id: 'chat-completions' }),
    model('minimax-m3', { type: 'variant', variant_id: 'anthropic-messages' }),
    model('gpt-6-luna', { type: 'variant', variant_id: 'responses' }),
    model('gemini-4-argon', { type: 'unsupported' }),
  ],
  next_model_id: null,
  provider: {
    catalog_model_count: 4,
    catalog_provider_id: 'opencode-go',
    configuration_class: 'trusted_preset',
    connection_defaults: {
      by_model: true,
      source: 'aura_preset',
      variants: [
        variant('chat-completions', GO_OPENAI, 'open_ai_chat_completions'),
        variant('responses', GO_OPENAI, 'open_ai_responses'),
        variant('anthropic-messages', GO_ANTHROPIC, 'anthropic_messages'),
      ],
    },
    display_name: 'OpenCode Go',
    fallback_initial: 'O',
    icon_ref: null,
    local_provider_id: null,
  },
} as unknown as ProviderCatalogModelPage;

const settings = {
  models_revision: '4',
  registry: { models: {} },
} as unknown as ModelSettingsProjection;

beforeEach(() => {
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        createModelProfileFromCatalog: vi.fn().mockResolvedValue({
          ok: true,
          value: { value: { display_name: 'added', id: 'mdl_01ARZ3NDEKTSV4RRFFQ69G5FC1' } },
        }),
        getModelCatalogProvider: vi.fn().mockResolvedValue({ ok: true, value: page }),
      },
    },
  });
});

afterEach(cleanup);

const row = async (id: string) => {
  const name = await screen.findByText(id, { selector: 'small' });
  return name.closest('article')!;
};

test('each model goes to the connection of its own route', async () => {
  const user = userEvent.setup();
  render(
    <CatalogModelDialog
      catalogProviderId="opencode-go"
      endpoints={ENDPOINTS}
      initialEndpointId={CHAT_ID}
      onClose={vi.fn()}
      onNotify={vi.fn()}
      onSaved={vi.fn(async () => undefined)}
      settings={settings}
    />,
  );

  const glm = await row('glm-5.3');
  expect(within(glm).getByText('聊天连接')).toBeTruthy();
  const minimax = await row('minimax-m3');
  expect(within(minimax).getByText('Anthropic 连接')).toBeTruthy();
  const luna = await row('gpt-6-luna');
  expect(within(luna).getByText('OpenCode Go responses')).toBeTruthy();
  expect((within(luna).getByRole('button', { name: '未连接' }) as HTMLButtonElement).disabled).toBe(
    true,
  );
  const gemini = await row('gemini-4-argon');
  expect(
    (within(gemini).getByRole('button', { name: '不支持' }) as HTMLButtonElement).disabled,
  ).toBe(true);
  expect(screen.queryByRole('combobox', { name: /目标 Endpoint/u })).toBeNull();

  await user.click(within(minimax).getByRole('button', { name: '添加' }));
  await waitFor(() =>
    expect(window.aura.v1.createModelProfileFromCatalog).toHaveBeenCalledTimes(1),
  );
  expect(window.aura.v1.createModelProfileFromCatalog).toHaveBeenLastCalledWith(
    expect.objectContaining({ catalog_model_id: 'minimax-m3', endpoint_id: ANTHROPIC_ID }),
  );
  await user.click(within(glm).getByRole('button', { name: '添加' }));
  await waitFor(() =>
    expect(window.aura.v1.createModelProfileFromCatalog).toHaveBeenCalledTimes(2),
  );
  expect(window.aura.v1.createModelProfileFromCatalog).toHaveBeenLastCalledWith(
    expect.objectContaining({ catalog_model_id: 'glm-5.3', endpoint_id: CHAT_ID }),
  );
});

test('a model already on another of its connections is shown as added there', async () => {
  const RESPONSES_ID = 'end_01ARZ3NDEKTSV4RRFFQ69G5FC3';
  const responses = endpoint(RESPONSES_ID, 'Responses 连接', GO_OPENAI, 'open_ai_responses');
  const withGlm = {
    models_revision: '4',
    registry: {
      models: {
        mdl_01ARZ3NDEKTSV4RRFFQ69G5FC9: {
          endpoint_id: RESPONSES_ID,
          provider_model_name: 'glm-5.3',
        },
      },
    },
  } as unknown as ModelSettingsProjection;
  render(
    <CatalogModelDialog
      catalogProviderId="opencode-go"
      endpoints={[responses]}
      initialEndpointId={RESPONSES_ID}
      onClose={vi.fn()}
      onNotify={vi.fn()}
      onSaved={vi.fn(async () => undefined)}
      settings={withGlm}
    />,
  );

  const glm = await row('glm-5.3');
  expect(within(glm).getByText('Responses 连接')).toBeTruthy();
  expect(within(glm).queryByText('OpenCode Go chat-completions')).toBeNull();
  expect((within(glm).getByRole('button', { name: '已添加' }) as HTMLButtonElement).disabled).toBe(
    true,
  );
  const luna = await row('gpt-6-luna');
  expect(within(luna).getByText('Responses 连接')).toBeTruthy();
  expect((within(luna).getByRole('button', { name: '添加' }) as HTMLButtonElement).disabled).toBe(
    false,
  );
  const minimax = await row('minimax-m3');
  expect(
    (within(minimax).getByRole('button', { name: '未连接' }) as HTMLButtonElement).disabled,
  ).toBe(true);
});
