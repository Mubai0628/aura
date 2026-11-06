import type {
  ModelEndpointConfig,
  ModelSettingsProjection,
  ProviderPresetVariantView,
} from '@aura/contracts/generated';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { type Mock, afterEach, beforeEach, expect, test, vi } from 'vitest';

import { ProviderConnectionDialog } from '../ProviderConnectionDialog';
import type { ProviderConfigurationView } from '../model-settings-types';
import type { NotificationInput } from '../notification-center-store';

// A Provider whose models each go by their own route is connected once: one API key, and a
// connection for each route not yet connected, each sent its own copy of the key.

const PROVIDER_ID = 'prv_01ARZ3NDEKTSV4RRFFQ69G5FB1';
const GO_OPENAI = 'https://opencode.ai/inference/go/openai/v1';
const GO_ANTHROPIC = 'https://opencode.ai/inference/go/anthropic/v1';
const KEY = 'sk-route-test-key';

const variant = (
  variant_id: string,
  display_name: string,
  base_url: string,
  api_format: ProviderPresetVariantView['api_format'],
): ProviderPresetVariantView => ({
  api_format,
  auth_scheme: api_format === 'anthropic_messages' ? 'anthropic_api_key' : 'open_ai_bearer_api_key',
  base_url,
  chat_instruction_role: api_format === 'open_ai_chat_completions' ? 'system' : null,
  chat_output_token_field: api_format === 'open_ai_chat_completions' ? 'max_tokens' : null,
  discovery_mode: { type: 'disabled' },
  display_name,
  recommended: false,
  variant_id,
});

const VARIANTS = [
  variant(
    'chat-completions',
    'OpenCode Go Chat Completions',
    GO_OPENAI,
    'open_ai_chat_completions',
  ),
  variant('responses', 'OpenCode Go Responses', GO_OPENAI, 'open_ai_responses'),
  variant(
    'anthropic-messages',
    'OpenCode Go Anthropic Messages',
    GO_ANTHROPIC,
    'anthropic_messages',
  ),
];

const existing = (base_url: string, api_format: ModelEndpointConfig['api_format']) =>
  ({
    api_format,
    auth_scheme: 'open_ai_bearer_api_key',
    base_url,
    display_name: '已有连接',
    discovery_mode: { type: 'disabled' },
    id: 'end_01ARZ3NDEKTSV4RRFFQ69G5FB1',
    provider_id: PROVIDER_ID,
    updated_at: '2026-11-03T00:00:00Z',
  }) as unknown as ModelEndpointConfig;

function provider(
  endpoints: ModelEndpointConfig[],
  byModel = true,
  variants = VARIANTS,
): ProviderConfigurationView {
  return {
    catalogModelCount: 37,
    catalogProviderId: 'opencode-go',
    configurationClass: 'trusted_preset',
    configuredEndpointCount: endpoints.length,
    connectionDefaults: { by_model: byModel, source: 'aura_preset', variants },
    endpoints,
    id: 'opencode-go',
    initials: 'O',
    localProvider: {
      backup_endpoint_ids: [],
      created_at: '2026-11-03T00:00:00Z',
      default_endpoint_id: null,
      display_name: 'OpenCode Go',
      endpoint_order: endpoints.map((endpoint) => endpoint.id),
      id: PROVIDER_ID,
      slug: 'opencode-go',
      source: { catalog_provider_id: 'opencode-go', type: 'models_dev' },
      updated_at: '2026-11-03T00:00:00Z',
    },
    models: [],
    name: 'OpenCode Go',
    source: 'models.dev',
  } as unknown as ProviderConfigurationView;
}

const settings = {
  active_credentials: [],
  dormant_credentials: [],
  models_revision: '5',
} as unknown as ModelSettingsProjection;

let seenKeys: string[];
let onNotify: Mock<(notification: NotificationInput) => void>;
let onSaved: Mock<() => Promise<void>>;

beforeEach(() => {
  seenKeys = [];
  onNotify = vi.fn<(notification: NotificationInput) => void>();
  onSaved = vi.fn(async () => undefined);
  let revision = 5;
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        createModelEndpoint: vi.fn(),
        createModelEndpointWithCredential: vi.fn((_request: unknown, secret: Uint8Array) => {
          seenKeys.push(new TextDecoder().decode(secret));
          revision += 1;
          return Promise.resolve({
            ok: true,
            value: { endpoint_projection: {}, models_revision: String(revision) },
          });
        }),
        getModelCompatibility: vi.fn().mockResolvedValue({ ok: false, error: {} }),
      },
    },
  });
});

afterEach(cleanup);

function open(view: ProviderConfigurationView) {
  render(
    <ProviderConnectionDialog
      onBrowseCatalog={vi.fn()}
      onClose={vi.fn()}
      onNotify={onNotify}
      onSaved={onSaved}
      provider={view}
      settings={settings}
      writeUnavailableReason={undefined}
    />,
  );
  return screen.getByRole('dialog', { name: 'OpenCode Go' });
}

const createCalls = () =>
  vi.mocked(window.aura.v1.createModelEndpointWithCredential).mock.calls as unknown as [
    Record<string, unknown>,
    Uint8Array,
  ][];

test('one key connects every route, each sent its own copy at the revision the last one left', async () => {
  const user = userEvent.setup();
  const dialog = open(provider([]));
  await user.click(within(dialog).getByRole('button', { name: /添加连接/u }));

  const preset = within(dialog).getByRole('combobox', { name: /Aura 连接预设/u });
  expect((preset as HTMLSelectElement).value).toBe('*');
  expect(within(dialog).getByRole('option', { name: 'OpenCode Go 全部连接' })).toBeTruthy();
  expect((within(dialog).getByLabelText('将保存的连接') as HTMLInputElement).value).toBe(
    'OpenCode Go Chat Completions、OpenCode Go Responses、OpenCode Go Anthropic Messages',
  );
  expect(within(dialog).queryByRole('textbox', { name: 'Base URL' })).toBeNull();
  expect(within(dialog).queryByRole('combobox', { name: 'API 类型' })).toBeNull();

  await user.type(within(dialog).getByPlaceholderText('输入后通过专用 SecretInput 通道提交'), KEY);
  await user.click(within(dialog).getByRole('button', { name: '保存连接' }));

  await waitFor(() => expect(createCalls()).toHaveLength(3));
  expect(
    createCalls().map(([request]) => [
      request.display_name,
      request.base_url,
      request.api_format,
      request.auth_scheme,
      request.expected_revision,
      request.provider_id,
    ]),
  ).toEqual([
    [
      'OpenCode Go Chat Completions',
      GO_OPENAI,
      'open_ai_chat_completions',
      'open_ai_bearer_api_key',
      '5',
      PROVIDER_ID,
    ],
    [
      'OpenCode Go Responses',
      GO_OPENAI,
      'open_ai_responses',
      'open_ai_bearer_api_key',
      '6',
      PROVIDER_ID,
    ],
    [
      'OpenCode Go Anthropic Messages',
      GO_ANTHROPIC,
      'anthropic_messages',
      'anthropic_api_key',
      '7',
      PROVIDER_ID,
    ],
  ]);
  expect(createCalls()[0]![0]).toMatchObject({
    chat_instruction_role: 'system',
    chat_output_token_field: 'max_tokens',
    discovery_mode: { type: 'disabled' },
  });
  expect(seenKeys).toEqual([KEY, KEY, KEY]);
  const [first, second, third] = createCalls().map(([, secret]) => secret);
  expect(first).not.toBe(second);
  expect(second).not.toBe(third);
  expect(createCalls().every(([, secret]) => secret.every((byte) => byte === 0))).toBe(true);
  expect(window.aura.v1.createModelEndpoint).not.toHaveBeenCalled();
  await waitFor(() => expect(onSaved).toHaveBeenCalled());
  expect(onNotify).toHaveBeenLastCalledWith(
    expect.objectContaining({
      message:
        'OpenCode Go Chat Completions、OpenCode Go Responses、OpenCode Go Anthropic Messages 已写入 Core 模型配置。',
      tone: 'success',
    }),
  );
});

test('only the routes not yet connected are connected', async () => {
  const user = userEvent.setup();
  const dialog = open(provider([existing(`${GO_OPENAI}/`, 'open_ai_responses')]));
  await user.click(within(dialog).getByRole('button', { name: /添加连接/u }));

  expect(within(dialog).getByRole('option', { name: 'OpenCode Go 其余连接' })).toBeTruthy();
  expect((within(dialog).getByLabelText('将保存的连接') as HTMLInputElement).value).toBe(
    'OpenCode Go Chat Completions、OpenCode Go Anthropic Messages',
  );
  await user.type(within(dialog).getByPlaceholderText('输入后通过专用 SecretInput 通道提交'), KEY);
  await user.click(within(dialog).getByRole('button', { name: '保存连接' }));
  await waitFor(() => expect(createCalls()).toHaveLength(2));
  expect(createCalls().map(([request]) => request.api_format)).toEqual([
    'open_ai_chat_completions',
    'anthropic_messages',
  ]);
});

test('a route that is not saved stops the rest and keeps those saved before it', async () => {
  vi.mocked(window.aura.v1.createModelEndpointWithCredential)
    .mockImplementationOnce(() =>
      Promise.resolve({
        ok: true,
        value: { endpoint_projection: {}, models_revision: '6' },
      } as never),
    )
    .mockImplementationOnce(() =>
      Promise.resolve({
        error: {
          category: 'conflict',
          code: 'models.configuration.revision_conflict',
          correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FB1',
          details: {},
          message: '模型设置已在别处更新，请刷新后再试。',
          retryable: true,
          terminal: 'failed',
        },
        ok: false,
      } as never),
    );
  const user = userEvent.setup();
  const dialog = open(provider([]));
  await user.click(within(dialog).getByRole('button', { name: /添加连接/u }));
  await user.type(within(dialog).getByPlaceholderText('输入后通过专用 SecretInput 通道提交'), KEY);
  await user.click(within(dialog).getByRole('button', { name: '保存连接' }));

  await waitFor(() =>
    expect(onNotify).toHaveBeenLastCalledWith(
      expect.objectContaining({ title: 'OpenCode Go Responses 未保存', tone: 'danger' }),
    ),
  );
  expect(createCalls()).toHaveLength(2);
  expect(onSaved).toHaveBeenCalled();
  expect(within(dialog).getByRole('button', { name: '保存连接' })).toBeTruthy();
});

test('a Provider whose routes are alternatives starts from its recommended one as before', async () => {
  const user = userEvent.setup();
  const alternatives = VARIANTS.map((item, index) => ({ ...item, recommended: index === 1 }));
  const dialog = open(provider([], false, alternatives));
  await user.click(within(dialog).getByRole('button', { name: /添加连接/u }));

  const preset = within(dialog).getByRole('combobox', { name: /Aura 连接预设/u });
  expect((preset as HTMLSelectElement).value).toBe('responses');
  expect(within(dialog).queryByRole('option', { name: /全部连接|其余连接/u })).toBeNull();
  expect(within(dialog).getByRole('textbox', { name: 'Base URL' })).toBeTruthy();
});
