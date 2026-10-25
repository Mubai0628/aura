import { describe, expect, it } from 'vitest';

import { isP05MainParams, isValidApiKeyBytes } from './p05-validation.js';

const MODEL_ID = 'mdl_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const ENDPOINT_ID = 'end_01ARZ3NDEKTSV4RRFFQ69G5FAV';

describe('P05 Main command boundary', () => {
  it('accepts the Rust manual profile including notes without inventing authority', () => {
    const request = {
      expected_revision: '1',
      input: {
        endpoint_id: ENDPOINT_ID,
        provider_model_name: 'fixture',
        display_name: 'Local',
        notes: 'Synthetic notes',
        context_window_tokens: 256000,
      },
    };
    expect(isP05MainParams('models.profile.create_manual', request)).toBe(true);
    for (const input of [
      { ...request.input, notes: 7 },
      { ...request.input, network_override: true },
      { ...request.input, endpoint_id: MODEL_ID },
      { ...request.input, context_window_tokens: Number.MAX_SAFE_INTEGER + 1 },
    ]) {
      expect(isP05MainParams('models.profile.create_manual', { ...request, input })).toBe(false);
    }
  });
  it('accepts only the frozen Model Availability start revision tuple', () => {
    expect(
      isP05MainParams('models.profile.test_availability', {
        expected_endpoint_revision: '7',
        expected_model_profile_revision: '3',
        model_profile_id: MODEL_ID,
      }),
    ).toBe(true);
    expect(
      isP05MainParams('models.profile.test_availability', {
        expected_endpoint_revision: '7',
        expected_model_profile_revision: '3',
        model_profile_id: MODEL_ID,
        overall: 'available',
      }),
    ).toBe(false);
    expect(
      isP05MainParams('models.profile.test_availability', {
        endpoint_connection_test_id: 'ect_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        expected_endpoint_revision: '7',
        expected_model_profile_revision: '3',
        model_profile_id: MODEL_ID,
      }),
    ).toBe(false);
  });

  it('keeps Endpoint create metadata out of the SecretInput payload contract', () => {
    expect(
      isP05MainParams('models.endpoint.create_with_credential', {
        anthropic_workspace_id: null,
        api_format: 'open_ai_responses',
        auth_scheme: 'open_ai_bearer_api_key',
        base_url: 'https://api.example.com/v1',
        chat_instruction_role: null,
        chat_output_token_field: null,
        discovery_mode: { type: 'automatic' },
        display_name: 'Primary',
        expected_revision: '1',
        max_parallel_model_requests: 2,
        openai_organization_id: null,
        openai_project_id: null,
        provider_id: 'prv_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        timeouts: {
          connect_ms: 15_000,
          first_event_ms: 120_000,
          stream_idle_ms: 120_000,
          total_ms: null,
        },
      }),
    ).toBe(true);
    expect(
      isP05MainParams('models.endpoint.create_with_credential', {
        api_key: 'must-never-enter-json',
      }),
    ).toBe(false);
  });

  it('validates the frozen Endpoint and config revisions on Discovery start', () => {
    expect(
      isP05MainParams('models.discovery.start', {
        endpoint_id: ENDPOINT_ID,
        expected_endpoint_revision: '7',
        expected_models_revision: '11',
      }),
    ).toBe(true);
    expect(
      isP05MainParams('models.discovery.start', {
        endpoint_id: ENDPOINT_ID.toLowerCase(),
        expected_endpoint_revision: '7',
        expected_models_revision: '11',
      }),
    ).toBe(false);
  });

  it('requires separate global and Endpoint revisions on Endpoint update', () => {
    const request = {
      anthropic_workspace_id: null,
      api_format: 'open_ai_chat_completions',
      auth_scheme: 'open_ai_bearer_api_key',
      base_url: 'https://api.example.com/v1',
      chat_instruction_role: 'developer',
      chat_output_token_field: 'max_completion_tokens',
      discovery_mode: { type: 'automatic' },
      display_name: 'Primary',
      endpoint_id: ENDPOINT_ID,
      expected_endpoint_revision: '7',
      expected_models_revision: '11',
      max_parallel_model_requests: 2,
      openai_organization_id: null,
      openai_project_id: null,
      timeouts: {
        connect_ms: 15_000,
        first_event_ms: 120_000,
        stream_idle_ms: 120_000,
        total_ms: null,
      },
    };
    expect(isP05MainParams('models.endpoint.update', request)).toBe(true);
    expect(
      isP05MainParams('models.endpoint.update', {
        ...request,
        expected_endpoint_revision: undefined,
        expected_revision: '7',
      }),
    ).toBe(false);
  });
});

describe('S3 API Key byte validation', () => {
  const encode = (value: string) => new TextEncoder().encode(value);

  it('preserves ordinary leading and trailing spaces', () => {
    expect(isValidApiKeyBytes(encode(' key '))).toBe(true);
  });

  it('accepts one through 2560 UTF-8 bytes', () => {
    expect(isValidApiKeyBytes(encode('x'))).toBe(true);
    expect(isValidApiKeyBytes(encode('x'.repeat(2_560)))).toBe(true);
    expect(isValidApiKeyBytes(encode('x'.repeat(2_561)))).toBe(false);
  });

  it('rejects forbidden controls, line separators, invalid UTF-8 and empty input', () => {
    for (const value of ['', 'a\nb', 'a\u007fb', 'a\u2028b', 'a\u2029b']) {
      expect(isValidApiKeyBytes(encode(value))).toBe(false);
    }
    expect(isValidApiKeyBytes(Uint8Array.of(0xff))).toBe(false);
  });
});

// A profile names a thinking level, or none.
describe('a profile update naming a thinking level', () => {
  const update = (reasoning_effort: unknown) => ({
    display_name: 'Model',
    enabled: true,
    expected_revision: '4',
    historical_snapshot_max_tokens: 1024,
    model_profile_id: 'mdl_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    notes: null,
    request_defaults: { max_output_tokens: null, temperature: null, top_p: null, reasoning_effort },
  });
  it('passes a known level or none', () => {
    for (const level of ['off', 'minimal', 'low', 'medium', 'high', 'xhigh', null, undefined])
      expect(isP05MainParams('models.profile.update', update(level))).toBe(true);
  });
  it('refuses any other word', () => {
    expect(isP05MainParams('models.profile.update', update('turbo'))).toBe(false);
    expect(isP05MainParams('models.profile.update', update(3))).toBe(false);
  });
  // `max` is its own level, and a profile may list the levels its model offers.
  it('passes max and a listed set of levels, and refuses a list with another word', () => {
    expect(isP05MainParams('models.profile.update', update('max'))).toBe(true);
    const listed = (reasoning_efforts: unknown) => {
      const request = update('max');
      return { ...request, request_defaults: { ...request.request_defaults, reasoning_efforts } };
    };
    expect(isP05MainParams('models.profile.update', listed(['low', 'xhigh', 'max']))).toBe(true);
    expect(isP05MainParams('models.profile.update', listed([]))).toBe(true);
    expect(isP05MainParams('models.profile.update', listed(null))).toBe(true);
    expect(isP05MainParams('models.profile.update', listed(['low', 'turbo']))).toBe(false);
    expect(isP05MainParams('models.profile.update', listed('max'))).toBe(false);
  });
  // Whether the model is sent images: yes, no, or the catalog's (none).
  it('passes an image input choice and refuses anything else', () => {
    const images = (image_input: unknown) => {
      const request = update(null);
      return { ...request, request_defaults: { ...request.request_defaults, image_input } };
    };
    for (const choice of [true, false, null, undefined])
      expect(isP05MainParams('models.profile.update', images(choice))).toBe(true);
    expect(isP05MainParams('models.profile.update', images('yes'))).toBe(false);
  });
});
