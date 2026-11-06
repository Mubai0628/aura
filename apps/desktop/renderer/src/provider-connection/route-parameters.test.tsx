import type {
  ModelEndpointConfig,
  ModelSettingsProjection,
  ProviderPresetVariantView,
} from '@aura/contracts/generated';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';

import { ProviderConnectionDialog } from '../ProviderConnectionDialog';
import type { ProviderConfigurationView } from '../model-settings-types';

// A route whose address takes a value of the user's own: the editor asks for it, keeps it to one
// host label, and saves each route's address with it filled in; the Base URL it shows is where
// the key goes. A connection reads back as its route only at the address the route writes.

const PROVIDER_ID = 'prv_01ARZ3NDEKTSV4RRFFQ69G5FC1';
const KEY = 'bedrock-route-test-key';

const mantle = (variant_id: string, prefix: string): ProviderPresetVariantView => ({
  api_format: 'open_ai_responses',
  auth_scheme: 'open_ai_bearer_api_key',
  base_url: `https://bedrock-mantle.{region}.api.aws${prefix}`,
  base_url_parameters: [
    { display_name: '区域', example: 'us-east-1', kind: 'domain_label', name: 'region' },
  ],
  chat_instruction_role: null,
  chat_output_token_field: null,
  discovery_mode: { type: 'disabled' },
  display_name: `Amazon Bedrock Mantle ${prefix}`,
  recommended: false,
  variant_id,
});

const VARIANTS = [mantle('mantle-v1', '/v1'), mantle('mantle-openai-v1', '/openai/v1')];

const existing = (base_url: string, display_name = 'Amazon Bedrock Mantle /v1') =>
  ({
    anthropic_workspace_id: null,
    api_format: 'open_ai_responses',
    auth_scheme: 'open_ai_bearer_api_key',
    base_url,
    chat_instruction_role: null,
    chat_output_token_field: null,
    discovery_mode: { type: 'disabled' },
    display_name,
    id: 'end_01ARZ3NDEKTSV4RRFFQ69G5FC1',
    max_parallel_model_requests: 2,
    model_order: [],
    openai_organization_id: null,
    openai_project_id: null,
    provider_id: PROVIDER_ID,
    revision: '3',
    timeouts: {
      connect_ms: 15_000,
      first_event_ms: 120_000,
      stream_idle_ms: 120_000,
      total_ms: null,
    },
    updated_at: '2026-11-03T00:00:00Z',
  }) as unknown as ModelEndpointConfig;

function provider(endpoints: ModelEndpointConfig[]): ProviderConfigurationView {
  return {
    catalogModelCount: 203,
    catalogProviderId: 'amazon-bedrock',
    configurationClass: 'trusted_preset',
    configuredEndpointCount: endpoints.length,
    connectionDefaults: { by_model: true, source: 'aura_preset', variants: VARIANTS },
    endpoints,
    id: 'amazon-bedrock',
    initials: 'A',
    localProvider: {
      backup_endpoint_ids: [],
      created_at: '2026-11-03T00:00:00Z',
      default_endpoint_id: null,
      display_name: 'Amazon Bedrock',
      endpoint_order: endpoints.map((endpoint) => endpoint.id),
      id: PROVIDER_ID,
      slug: 'amazon-bedrock',
      source: { catalog_provider_id: 'amazon-bedrock', type: 'models_dev' },
      updated_at: '2026-11-03T00:00:00Z',
    },
    models: [],
    name: 'Amazon Bedrock',
    source: 'models.dev',
  } as unknown as ProviderConfigurationView;
}

const settings = {
  active_credentials: [],
  dormant_credentials: [],
  models_revision: '5',
} as unknown as ModelSettingsProjection;

beforeEach(() => {
  let revision = 5;
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        createModelEndpoint: vi.fn(() => {
          revision += 1;
          return Promise.resolve({
            ok: true,
            value: {
              display_name: 'saved',
              id: 'end_01ARZ3NDEKTSV4RRFFQ69G5FC9',
              revision: String(revision),
            },
          });
        }),
        createModelEndpointWithCredential: vi.fn(() => {
          revision += 1;
          return Promise.resolve({
            ok: true,
            value: {
              endpoint_projection: {
                endpoint: { display_name: 'saved', id: 'end_01ARZ3NDEKTSV4RRFFQ69G5FC9' },
              },
              models_revision: String(revision),
            },
          });
        }),
        getModelCompatibility: vi.fn().mockResolvedValue({ ok: false, error: {} }),
        onModelProjection: vi.fn().mockReturnValue(() => undefined),
        subscribeModelProjection: vi.fn().mockResolvedValue({ ok: true, value: undefined }),
        unsubscribeModelProjection: vi.fn().mockResolvedValue({ ok: true, value: undefined }),
        updateModelEndpoint: vi.fn(() => {
          revision += 1;
          return Promise.resolve({
            ok: true,
            value: {
              revision: String(revision),
              value: { display_name: 'saved', id: 'end_01ARZ3NDEKTSV4RRFFQ69G5FC1' },
            },
          });
        }),
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
      onNotify={vi.fn()}
      onSaved={vi.fn(async () => undefined)}
      provider={view}
      settings={settings}
      writeUnavailableReason={undefined}
    />,
  );
  return screen.getByRole('dialog', { name: 'Amazon Bedrock' });
}

const savedAddresses = () =>
  vi
    .mocked(window.aura.v1.createModelEndpointWithCredential)
    .mock.calls.map(([request]) => (request as { base_url: string }).base_url);

test('one region fills the address of each route, and one key connects them', async () => {
  const user = userEvent.setup();
  const dialog = open(provider([]));
  await user.click(within(dialog).getByRole('button', { name: /添加连接/u }));
  expect(
    (within(dialog).getByRole('combobox', { name: /Aura 连接预设/u }) as HTMLSelectElement).value,
  ).toBe('*');
  expect(within(dialog).getAllByRole('textbox', { name: '区域' })).toHaveLength(1);
  await user.type(within(dialog).getByPlaceholderText('输入后通过专用 SecretInput 通道提交'), KEY);

  await user.click(within(dialog).getByRole('button', { name: '保存连接' }));
  expect(within(dialog).getByText('请输入区域')).toBeTruthy();
  expect(window.aura.v1.createModelEndpointWithCredential).not.toHaveBeenCalled();

  await user.type(within(dialog).getByRole('textbox', { name: '区域' }), 'EU-Central-1');
  expect((within(dialog).getByRole('textbox', { name: '区域' }) as HTMLInputElement).value).toBe(
    'eu-central-1',
  );
  await user.click(within(dialog).getByRole('button', { name: '保存连接' }));
  await waitFor(() => expect(savedAddresses()).toHaveLength(2));
  expect(savedAddresses()).toEqual([
    'https://bedrock-mantle.eu-central-1.api.aws/v1',
    'https://bedrock-mantle.eu-central-1.api.aws/openai/v1',
  ]);
});

test('a region that is not one host label is not saved', async () => {
  const user = userEvent.setup();
  const dialog = open(provider([]));
  await user.click(within(dialog).getByRole('button', { name: /添加连接/u }));
  await user.type(within(dialog).getByRole('textbox', { name: '区域' }), 'evil.test/x');
  await user.click(within(dialog).getByRole('button', { name: '保存连接' }));
  expect(
    within(dialog).getByText('区域只能含小写字母、数字和中间的连字符，最长 63 个字符'),
  ).toBeTruthy();
  expect(window.aura.v1.createModelEndpointWithCredential).not.toHaveBeenCalled();
  expect(window.aura.v1.createModelEndpoint).not.toHaveBeenCalled();
});

test('one route alone shows where its key goes, and leaving it keeps that address', async () => {
  const user = userEvent.setup();
  const dialog = open(provider([]));
  await user.click(within(dialog).getByRole('button', { name: /添加连接/u }));
  await user.selectOptions(
    within(dialog).getByRole('combobox', { name: /Aura 连接预设/u }),
    'mantle-v1',
  );
  const baseUrl = within(dialog).getByRole('textbox', { name: 'Base URL' }) as HTMLInputElement;
  expect(baseUrl.readOnly).toBe(true);
  expect(baseUrl.value).toBe('https://bedrock-mantle.{region}.api.aws/v1');
  await user.type(within(dialog).getByRole('textbox', { name: '区域' }), 'us-west-2');
  expect(baseUrl.value).toBe('https://bedrock-mantle.us-west-2.api.aws/v1');

  await user.selectOptions(within(dialog).getByRole('combobox', { name: /Aura 连接预设/u }), '');
  const custom = within(dialog).getByRole('textbox', { name: 'Base URL' }) as HTMLInputElement;
  expect(custom.readOnly).toBe(false);
  expect(custom.value).toBe('https://bedrock-mantle.us-west-2.api.aws/v1');
  expect(within(dialog).queryByRole('textbox', { name: '区域' })).toBeNull();
});

test('one route alone is saved at the address its region makes', async () => {
  const user = userEvent.setup();
  const dialog = open(provider([]));
  await user.click(within(dialog).getByRole('button', { name: /添加连接/u }));
  await user.selectOptions(
    within(dialog).getByRole('combobox', { name: /Aura 连接预设/u }),
    'mantle-openai-v1',
  );
  await user.type(within(dialog).getByRole('textbox', { name: '区域' }), 'sa-east-1');
  await user.type(within(dialog).getByPlaceholderText('输入后通过专用 SecretInput 通道提交'), KEY);
  await user.click(within(dialog).getByRole('button', { name: '保存连接' }));
  await waitFor(() => expect(savedAddresses()).toHaveLength(1));
  expect(savedAddresses()).toEqual(['https://bedrock-mantle.sa-east-1.api.aws/openai/v1']);
});

test.each([
  ['/v1', 'mantle-v1', 'ap-northeast-1'],
  ['/openai/v1', 'mantle-openai-v1', 'eu-west-3'],
])(
  'a connection of the %s route reads its region back, and a new region moves it',
  async (prefix, variantId, region) => {
    const user = userEvent.setup();
    const dialog = open(
      provider([existing(`https://bedrock-mantle.${region}.api.aws${prefix}`, '已有连接')]),
    );
    await user.click(within(dialog).getByRole('button', { name: /已有连接/u }));
    expect(
      (within(dialog).getByRole('combobox', { name: /Aura 连接预设/u }) as HTMLSelectElement).value,
    ).toBe(variantId);
    const regionField = within(dialog).getByRole('textbox', { name: '区域' }) as HTMLInputElement;
    expect(regionField.value).toBe(region);
    expect(
      (within(dialog).getByRole('textbox', { name: 'Base URL' }) as HTMLInputElement).value,
    ).toBe(`https://bedrock-mantle.${region}.api.aws${prefix}`);
    const save = within(dialog).getByRole('button', { name: '保存连接' }) as HTMLButtonElement;
    expect(save.disabled).toBe(true);

    await user.clear(regionField);
    await user.type(regionField, 'us-east-2');
    expect(save.disabled).toBe(false);
    await user.click(save);
    await waitFor(() => expect(window.aura.v1.updateModelEndpoint).toHaveBeenCalledTimes(1));
    expect(vi.mocked(window.aura.v1.updateModelEndpoint).mock.calls[0]?.[0]).toMatchObject({
      base_url: `https://bedrock-mantle.us-east-2.api.aws${prefix}`,
    });
  },
);

test.each([
  'https://bedrock.example.test/v1',
  'https://bedrock-mantle.ap-northeast-1.api.aws/v1/',
  'https://BEDROCK-MANTLE.ap-northeast-1.api.aws/v1',
])(
  'a connection at %s, not as its route writes it, keeps its address as typed',
  async (address) => {
    const user = userEvent.setup();
    const dialog = open(provider([existing(address, '已有连接')]));
    await user.click(within(dialog).getByRole('button', { name: /已有连接/u }));
    expect(
      (within(dialog).getByRole('combobox', { name: /Aura 连接预设/u }) as HTMLSelectElement).value,
    ).toBe('');
    expect(within(dialog).queryByRole('textbox', { name: '区域' })).toBeNull();
    const baseUrl = within(dialog).getByRole('textbox', { name: 'Base URL' }) as HTMLInputElement;
    expect(baseUrl.readOnly).toBe(false);
    expect(baseUrl.value).toBe(address);
    expect(
      (within(dialog).getByRole('button', { name: '保存连接' }) as HTMLButtonElement).disabled,
    ).toBe(true);
  },
);

test('a new connection starts without the region of the one opened before', async () => {
  const user = userEvent.setup();
  const dialog = open(provider([existing('https://bedrock-mantle.ap-northeast-1.api.aws/v1')]));
  await user.click(within(dialog).getByRole('button', { name: /Amazon Bedrock Mantle \/v1/u }));
  expect((within(dialog).getByRole('textbox', { name: '区域' }) as HTMLInputElement).value).toBe(
    'ap-northeast-1',
  );
  await user.click(within(dialog).getByRole('button', { name: /返回连接列表/u }));
  await user.click(within(dialog).getByRole('button', { name: /添加连接/u }));
  expect((within(dialog).getByRole('textbox', { name: '区域' }) as HTMLInputElement).value).toBe(
    '',
  );
});

test('only the route no connection is yet of is still to connect, at the region given again', async () => {
  const user = userEvent.setup();
  const dialog = open(provider([existing('https://bedrock-mantle.ap-northeast-1.api.aws/v1/')]));
  await user.click(within(dialog).getByRole('button', { name: /添加连接/u }));
  expect(within(dialog).getByRole('option', { name: 'Amazon Bedrock 其余连接' })).toBeTruthy();
  expect((within(dialog).getByLabelText('将保存的连接') as HTMLInputElement).value).toBe(
    'Amazon Bedrock Mantle /openai/v1',
  );
  await user.type(within(dialog).getByRole('textbox', { name: '区域' }), 'ap-northeast-1');
  await user.type(within(dialog).getByPlaceholderText('输入后通过专用 SecretInput 通道提交'), KEY);
  await user.click(within(dialog).getByRole('button', { name: '保存连接' }));
  await waitFor(() => expect(savedAddresses()).toHaveLength(1));
  expect(savedAddresses()).toEqual(['https://bedrock-mantle.ap-northeast-1.api.aws/openai/v1']);
});
