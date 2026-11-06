import type {
  ModelEndpointConfig,
  ProviderCatalogConnectionDefaultsView,
  ProviderPresetVariantView,
} from '@aura/contracts/generated';
import { expect, test } from 'vitest';

import { byModelVariants, connectionIsVariant, missingVariants, modelTarget } from './model-routes';

// A Provider whose models each go by their own variant: a connection is a variant's route when
// it has the variant's address and protocol, and a catalog model goes to its route's connection.

const GO_OPENAI = 'https://opencode.ai/inference/go/openai/v1';
const GO_ANTHROPIC = 'https://opencode.ai/inference/go/anthropic/v1';

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
  display_name: variant_id,
  recommended: false,
  variant_id,
});

const VARIANTS = [
  variant('chat-completions', GO_OPENAI, 'open_ai_chat_completions'),
  variant('responses', GO_OPENAI, 'open_ai_responses'),
  variant('anthropic-messages', GO_ANTHROPIC, 'anthropic_messages'),
];

const endpoint = (
  id: string,
  base_url: string,
  api_format: ModelEndpointConfig['api_format'],
): ModelEndpointConfig => ({ api_format, base_url, id }) as unknown as ModelEndpointConfig;

test('only a preset whose models go by their own variants lists them', () => {
  const preset = (by_model: boolean): ProviderCatalogConnectionDefaultsView => ({
    by_model,
    source: 'aura_preset',
    variants: VARIANTS,
  });
  expect(byModelVariants(preset(true))).toEqual(VARIANTS);
  expect(byModelVariants(preset(false))).toEqual([]);
  expect(byModelVariants({ base_url: GO_OPENAI, source: 'models_dev_api_suggestion' })).toEqual([]);
  expect(byModelVariants(undefined)).toEqual([]);
});

test('a connection is a route when its address and protocol are the variant’s', () => {
  const [chat, responses, anthropic] = VARIANTS as [
    ProviderPresetVariantView,
    ProviderPresetVariantView,
    ProviderPresetVariantView,
  ];
  expect(connectionIsVariant(chat, endpoint('a', GO_OPENAI, 'open_ai_chat_completions'))).toBe(
    true,
  );
  expect(
    connectionIsVariant(chat, endpoint('a', `${GO_OPENAI}/`, 'open_ai_chat_completions')),
  ).toBe(true);
  expect(
    connectionIsVariant(
      chat,
      endpoint('a', 'https://OpenCode.AI:443/inference/go/openai/v1', 'open_ai_chat_completions'),
    ),
  ).toBe(true);
  expect(
    connectionIsVariant(
      chat,
      endpoint('a', 'https://opencode.ai./inference/go/openai/v1', 'open_ai_chat_completions'),
    ),
  ).toBe(true);
  expect(
    connectionIsVariant(
      chat,
      endpoint('a', 'https://opencode.ai:8443/inference/go/openai/v1', 'open_ai_chat_completions'),
    ),
  ).toBe(false);
  expect(connectionIsVariant(responses, endpoint('a', GO_OPENAI, 'open_ai_chat_completions'))).toBe(
    false,
  );
  expect(connectionIsVariant(anthropic, endpoint('a', GO_OPENAI, 'anthropic_messages'))).toBe(
    false,
  );
  expect(
    connectionIsVariant(
      chat,
      endpoint('a', 'http://opencode.ai/inference/go/openai/v1', 'open_ai_chat_completions'),
    ),
  ).toBe(false);
  expect(connectionIsVariant(chat, endpoint('a', 'not a url', 'open_ai_chat_completions'))).toBe(
    false,
  );
});

test('the variants still to connect are those no connection is the route of', () => {
  const endpoints = [
    endpoint('a', GO_OPENAI, 'open_ai_responses'),
    endpoint('b', 'https://opencode.ai/zen/go/v1', 'open_ai_chat_completions'),
  ];
  expect(missingVariants(VARIANTS, endpoints).map((item) => item.variant_id)).toEqual([
    'chat-completions',
    'anthropic-messages',
  ]);
  expect(missingVariants(VARIANTS, [])).toHaveLength(3);
});

test('a catalog model goes to the first connection of its variant', () => {
  const first = endpoint('first', GO_ANTHROPIC, 'anthropic_messages');
  const endpoints = [
    endpoint('chat', GO_OPENAI, 'open_ai_chat_completions'),
    first,
    endpoint('second', GO_ANTHROPIC, 'anthropic_messages'),
  ];
  expect(
    modelTarget({ type: 'variant', variant_id: 'anthropic-messages' }, VARIANTS, endpoints),
  ).toEqual({ endpoint: first, kind: 'endpoint' });
  expect(modelTarget({ type: 'variant', variant_id: 'responses' }, VARIANTS, endpoints)).toEqual({
    kind: 'unconnected',
    variant: VARIANTS[1],
  });
  expect(modelTarget({ type: 'unsupported' }, VARIANTS, endpoints)).toEqual({
    kind: 'unsupported',
  });
  expect(modelTarget({ type: 'variant', variant_id: 'gone' }, VARIANTS, endpoints)).toEqual({
    kind: 'unsupported',
  });
});

test('a variant whose address takes a value is the route of a connection at any valid value', () => {
  const mantle = {
    ...variant('mantle-v1', 'https://bedrock-mantle.{region}.api.aws/v1', 'open_ai_responses'),
    base_url_parameters: [
      { display_name: '区域', example: 'us-east-1', kind: 'domain_label' as const, name: 'region' },
    ],
  };
  const at = (base_url: string, format: ModelEndpointConfig['api_format'] = 'open_ai_responses') =>
    connectionIsVariant(mantle, endpoint('end', base_url, format));
  expect(at('https://bedrock-mantle.eu-central-1.api.aws/v1')).toBe(true);
  expect(at('https://bedrock-mantle.eu-central-1.api.aws/v1/')).toBe(true);
  expect(at('https://bedrock-mantle.eu-central-1.api.aws/openai/v1')).toBe(false);
  expect(at('https://bedrock-mantle.evil.test.api.aws/v1')).toBe(false);
  expect(at('https://bedrock-mantle.{region}.api.aws/v1')).toBe(false);
  expect(at('https://bedrock-mantle.eu-central-1.api.aws/v1', 'open_ai_chat_completions')).toBe(
    false,
  );
  expect(
    missingVariants(
      [mantle],
      [endpoint('end', 'https://bedrock-mantle.us-east-1.api.aws/v1', 'open_ai_responses')],
    ),
  ).toEqual([]);
});
