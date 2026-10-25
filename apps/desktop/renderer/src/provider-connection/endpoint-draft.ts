import type {
  ModelDiscoveryMode,
  ModelEndpointConfig,
  ModelEndpointUpdateRequest,
  ProviderPresetVariantView,
} from '@aura/contracts/generated';

/** The endpoint editor's draft: what it starts from and when it differs from what is saved. */
export type EditorTarget = 'new' | string | undefined;
export type DiscoveryModeChoice = ModelDiscoveryMode['type'];
export type PendingDeletion =
  { displayName: string; kind: 'endpoint' } | { displayName: string; kind: 'provider' };

export const DEFAULT_TIMEOUTS: {
  connect_ms: number;
  first_event_ms: number;
  stream_idle_ms: number;
  total_ms: null;
} = {
  connect_ms: 15_000,
  first_event_ms: 120_000,
  stream_idle_ms: 120_000,
  total_ms: null,
} as const;

export const isEndpointDraftDirty = (
  endpoint: ModelEndpointConfig | undefined,
  fields:
    | Omit<
        ModelEndpointUpdateRequest,
        'endpoint_id' | 'expected_endpoint_revision' | 'expected_models_revision'
      >
    | undefined,
): boolean => {
  if (endpoint === undefined) return true;
  if (fields === undefined) return false;
  return (
    JSON.stringify(fields) !==
    JSON.stringify({
      anthropic_workspace_id: endpoint.anthropic_workspace_id ?? null,
      api_format: endpoint.api_format,
      auth_scheme: endpoint.auth_scheme,
      base_url: endpoint.base_url,
      chat_instruction_role: endpoint.chat_instruction_role ?? null,
      chat_output_token_field: endpoint.chat_output_token_field ?? null,
      discovery_mode: endpoint.discovery_mode,
      display_name: endpoint.display_name,
      max_parallel_model_requests: endpoint.max_parallel_model_requests,
      openai_organization_id: endpoint.openai_organization_id ?? null,
      openai_project_id: endpoint.openai_project_id ?? null,
      timeouts: {
        connect_ms: endpoint.timeouts.connect_ms,
        first_event_ms: endpoint.timeouts.first_event_ms,
        stream_idle_ms: endpoint.timeouts.stream_idle_ms,
        total_ms: endpoint.timeouts.total_ms ?? null,
      },
    })
  );
};

export const presetMatchesEndpoint = (
  preset: ProviderPresetVariantView,
  endpoint: ModelEndpointConfig,
): boolean =>
  preset.base_url === endpoint.base_url &&
  preset.api_format === endpoint.api_format &&
  preset.auth_scheme === endpoint.auth_scheme &&
  JSON.stringify(preset.discovery_mode) === JSON.stringify(endpoint.discovery_mode) &&
  (preset.chat_instruction_role ?? null) === (endpoint.chat_instruction_role ?? null) &&
  (preset.chat_output_token_field ?? null) === (endpoint.chat_output_token_field ?? null);
