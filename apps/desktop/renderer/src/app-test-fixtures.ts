import { cleanup } from '@testing-library/react';
import { afterEach, beforeEach, vi } from 'vitest';
import type {
  ModelSettingsProjection,
  ProviderCatalogModelPage,
  ProviderCatalogProjection,
} from '@aura/contracts/generated';

import type {
  CoreHealthProjectionChange,
  WorkspaceSessionProjectionChange,
} from '../../shared/aura-api';
import { resetScopedModelProjectionStoreForTests } from './model-projection-store';
import { resetModelSettingsStoreForTests } from './model-settings-store';
import { resetComposerDraftsForTests } from './agent-session/composer-drafts';
import { emptySessionGrantRoutes } from './security/session-grant-test-routes';

/** The projections the App tests start from. */
export const readyProjection: CoreHealthProjectionChange = {
  payload: {
    read_only: false,
    restart_attempt: 0,
    safe_mode: false,
    state: 'ready',
    status_code: 'core.ready',
    status_message: 'Aura Core is ready.',
  },
  projection_kind: 'core_health',
  projection_scope: { type: 'global' },
  revision: '2',
  runtime_instance_id: 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV',
};

export const workspaceSessionProjection: WorkspaceSessionProjectionChange = {
  payload: {
    sessions: [
      {
        session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        workspace_id: 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        title: '合成桌面会话',
        state: 'idle',
      },
    ],
    workspaces: [
      {
        workspace_id: 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        display_name: '合成工作区',
        selected_root: 'C:/synthetic',
        canonical_root: 'C:/synthetic',
        authorization_epoch: '1',
        revision: '1',
        catalog_freshness: 'unavailable',
        state: 'online',
      },
    ],
  },
  projection_kind: 'workspace_session',
  projection_scope: { type: 'global' },
  revision: '1',
  runtime_instance_id: readyProjection.runtime_instance_id,
};

export const MODEL_PROVIDER_ID = 'prv_01ARZ3NDEKTSV4RRFFQ69G5FAV';
export const ANTHROPIC_PROVIDER_ID = 'prv_01ARZ3NDEKTSV4RRFFQ69G5FAW';
export const SUGGESTED_PROVIDER_ID = 'prv_01ARZ3NDEKTSV4RRFFQ69G5FAX';
export const MODEL_ENDPOINT_ID = 'end_01ARZ3NDEKTSV4RRFFQ69G5FAV';
export const SECOND_MODEL_ENDPOINT_ID = 'end_01ARZ3NDEKTSV4RRFFQ69G5FAW';
export const MODEL_PROFILE_ID = 'mdl_01ARZ3NDEKTSV4RRFFQ69G5FAV';
export const SECOND_MODEL_PROFILE_ID = 'mdl_01ARZ3NDEKTSV4RRFFQ69G5FAW';
export const MODEL_CREDENTIAL_ID = 'crd_01ARZ3NDEKTSV4RRFFQ69G5FAV';

export const modelSettingsProjection: ModelSettingsProjection = {
  active_credentials: [
    {
      active_credential_revision: '1',
      credential_id: MODEL_CREDENTIAL_ID,
      endpoint_id: MODEL_ENDPOINT_ID,
      error_code: null,
      last_committed_credential_revision: '1',
      row_revision: '1',
      slot_name: 'api-key',
      state: 'configured',
    },
  ],
  aura_revision: '1',
  default_model_profile_id: MODEL_PROFILE_ID,
  dormant_credentials: [],
  models_revision: '1',
  registry: {
    endpoints: {
      [MODEL_ENDPOINT_ID]: {
        anthropic_workspace_id: null,
        api_format: 'open_ai_responses',
        auth_scheme: 'open_ai_bearer_api_key',
        base_url: 'https://api.openai.com/v1',
        chat_instruction_role: null,
        chat_output_token_field: null,
        compatibility: null,
        created_at: '2026-08-30T00:00:00Z',
        discovery_mode: { type: 'automatic' },
        display_name: 'OpenAI 主连接',
        id: MODEL_ENDPOINT_ID,
        max_parallel_model_requests: 2,
        model_order: [MODEL_PROFILE_ID],
        openai_organization_id: null,
        openai_project_id: null,
        provider_id: MODEL_PROVIDER_ID,
        revision: '1',
        slug: 'openai-main',
        timeouts: {
          connect_ms: 15_000,
          first_event_ms: 120_000,
          stream_idle_ms: 120_000,
          total_ms: null,
        },
        updated_at: '2026-08-30T00:00:00Z',
      },
    },
    models: {
      [MODEL_PROFILE_ID]: {
        available_for_subagents: true,
        catalog: {
          catalog_content_hash: {
            algorithm: 'sha256',
            hex: '0000000000000000000000000000000000000000000000000000000000000000',
          },
          catalog_model_id: 'gpt-5.6-sol',
          catalog_object_hash: {
            algorithm: 'sha256',
            hex: '1111111111111111111111111111111111111111111111111111111111111111',
          },
          catalog_provider_id: 'openai',
          catalog_revision: '1',
          match_state: 'matched',
          matched_at: '2026-08-30T00:00:00Z',
          snapshot: {
            aliases: [],
            capabilities: {
              attachment: true,
              interleaved: false,
              reasoning: true,
              structured_output: true,
              temperature: true,
              tool_call: true,
            },
            catalog_model_id: 'gpt-5.6-sol',
            context_window_tokens: 1_047_576,
            description: 'Frontier coding model',
            display_name: 'gpt-5.6-sol',
            family: 'gpt-5.6',
            input_limit_tokens: 1_047_576,
            input_modalities: ['text', 'image'],
            knowledge_cutoff: null,
            last_updated: '2026-08-30',
            open_weights: false,
            output_limit_tokens: 32_768,
            output_modalities: ['text'],
            price: null,
            reasoning_options: [],
            release_date: '2026-08-30',
            status: 'available',
          },
        },
        context: {
          catalog_window_tokens: 1_047_576,
          historical_snapshot_max_tokens: 1_024,
          runtime_corrected_window_tokens: null,
          runtime_correction_basis: null,
          runtime_effective_window_tokens: 1_047_576,
          user_override_revision: 0,
          user_override_window_tokens: null,
        },
        created_at: '2026-08-30T00:00:00Z',
        display_name: 'gpt-5.6-sol',
        enabled: true,
        endpoint_id: MODEL_ENDPOINT_ID,
        id: MODEL_PROFILE_ID,
        origin: 'discovered',
        provider_model_name: 'gpt-5.6-sol',
        request_defaults: {
          chat_instruction_role: null,
          chat_output_token_field: null,
          max_output_tokens: null,
          temperature: null,
          top_p: null,
        },
        revision: '1',
        updated_at: '2026-08-30T00:00:00Z',
      },
    },
    provider_order: [MODEL_PROVIDER_ID],
    providers: {
      [MODEL_PROVIDER_ID]: {
        created_at: '2026-08-30T00:00:00Z',
        display_name: 'OpenAI',
        endpoint_order: [MODEL_ENDPOINT_ID],
        id: MODEL_PROVIDER_ID,
        slug: 'openai',
        source: { catalog_provider_id: 'openai', type: 'models_dev' },
        updated_at: '2026-08-30T00:00:00Z',
      },
    },
  },
};

export const modelProviderCatalog: ProviderCatalogProjection = {
  cards: [
    {
      catalog_model_count: 47,
      catalog_provider_id: 'openai',
      configuration_class: 'trusted_preset',
      display_name: 'OpenAI',
      fallback_initial: 'O',
      icon_ref: null,
      local_provider_id: MODEL_PROVIDER_ID,
    },
    {
      catalog_model_count: 13,
      catalog_provider_id: 'anthropic',
      configuration_class: 'trusted_preset',
      connection_defaults: {
        source: 'aura_preset',
        variants: [
          {
            api_format: 'anthropic_messages',
            auth_scheme: 'anthropic_api_key',
            base_url: 'https://api.anthropic.com/v1',
            chat_instruction_role: null,
            chat_output_token_field: null,
            discovery_mode: { type: 'automatic' },
            display_name: 'Anthropic Messages',
            recommended: true,
            variant_id: 'anthropic/messages',
          },
        ],
      },
      display_name: 'Anthropic',
      fallback_initial: 'A',
      icon_ref: null,
      local_provider_id: null,
    },
    {
      catalog_model_count: 3,
      catalog_provider_id: 'legacy-catalog',
      configuration_class: 'catalog_information_only',
      display_name: 'Legacy Catalog',
      fallback_initial: 'L',
      icon_ref: null,
      local_provider_id: null,
    },
  ],
  catalog_content_hash: {
    algorithm: 'sha256',
    hex: '0000000000000000000000000000000000000000000000000000000000000000',
  },
  catalog_revision: '1',
  checked_at: '2026-08-30T00:00:00Z',
  last_successful_refresh_at: '2026-08-30T00:00:00Z',
  last_error_code: null,
  projection_revision: '1',
  state: 'fresh',
};

export const modelCatalogPage: ProviderCatalogModelPage = {
  catalog_content_hash: modelProviderCatalog.catalog_content_hash!,
  catalog_revision: '1',
  models: [
    {
      catalog_object_hash: {
        algorithm: 'sha256',
        hex: '1111111111111111111111111111111111111111111111111111111111111111',
      },
      model: {
        aliases: [],
        capabilities: {
          attachment: true,
          interleaved: false,
          reasoning: true,
          structured_output: true,
          temperature: true,
          tool_call: true,
        },
        catalog_model_id: 'gpt-5.6-sol',
        context_window_tokens: 1_047_576,
        description: 'Frontier coding model',
        display_name: 'gpt-5.6-sol',
        family: 'gpt-5.6',
        input_limit_tokens: 1_047_576,
        input_modalities: ['text', 'image'],
        knowledge_cutoff: null,
        last_updated: '2026-08-30',
        open_weights: false,
        output_limit_tokens: 32_768,
        output_modalities: ['text'],
        price: null,
        reasoning_options: [],
        release_date: '2026-08-30',
        status: 'available',
      },
    },
  ],
  next_model_id: null,
  provider: {
    ...modelProviderCatalog.cards[0]!,
  },
};

export const hostPreparationStatus = {
  active_operation: null,
  revision: 1,
  summary: {
    appcontainer_available: true,
    last_error_code: null,
    nested_jobs_available: true,
    nul_compatibility_ready: false,
    residual_acl_count: 0,
    residual_appcontainer_profile_count: 0,
    residual_firewall_rule_count: 0,
    restart_required: false,
    standard_user_token_available: true,
    system_drive_compatibility_ready: false,
  },
} as const;

/** The bridge every App test starts from, and the reset after each test. */
export function installAppTestEnvironment() {
  beforeEach(() => {
    window.localStorage.clear();
    resetComposerDraftsForTests();
    vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(1_200);
    Object.defineProperty(window, 'aura', {
      configurable: true,
      value: {
        v1: {
          getPermissionDefaults: vi
            .fn()
            .mockResolvedValue({ ok: true, value: { revision: 1, mode: 'read_only' } }),
          getCoreHealth: vi.fn().mockResolvedValue({ ok: true, value: readyProjection }),
          getConfigRecoveryNotice: vi.fn().mockResolvedValue({
            ok: true,
            value: {
              affected_domains: [],
              unavailable_capabilities: [],
              fault_codes: [],
              suspended_calls: [],
              available_actions: [],
              scope_known: true,
              recovery_in_progress: false,
              next_step: '',
            },
          }),
          getHostPreparationStatus: vi
            .fn()
            .mockResolvedValue({ ok: true, value: hostPreparationStatus }),
          getModelProviderCatalog: vi
            .fn()
            .mockResolvedValue({ ok: true, value: modelProviderCatalog }),
          getModelCatalogProvider: vi.fn().mockResolvedValue({ ok: true, value: modelCatalogPage }),
          getModelCompatibility: vi.fn().mockResolvedValue({
            ok: true,
            value: {
              compatibility: null,
              endpoint_id: MODEL_ENDPOINT_ID,
              revision: '1',
            },
          }),
          ensureModelProviderCatalogFresh: vi
            .fn()
            .mockResolvedValue({ ok: true, value: modelProviderCatalog }),
          getModelSettings: vi.fn().mockResolvedValue({ ok: true, value: modelSettingsProjection }),
          deleteModelEndpoint: vi.fn(),
          updateModelEndpoint: vi.fn(),
          getWorkspaceSessions: vi
            .fn()
            .mockResolvedValue({ ok: true, value: workspaceSessionProjection }),
          listPendingApprovals: vi.fn().mockResolvedValue({ ok: true, value: [] }),
          listPermissionGrants: vi.fn().mockResolvedValue({ ok: true, value: [] }),
          getSessionPermissionState: vi.fn().mockResolvedValue({
            ok: true,
            value: {
              session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
              revision: 1,
              mode: 'workspace_read_write',
              active_grant_count: 0,
              transition: 'settled',
            },
          }),
          setSessionPermissionMode: vi.fn().mockImplementation((request) =>
            Promise.resolve({
              ok: true,
              value: {
                session_id: request.session_id,
                revision: 2,
                mode: request.mode,
                active_grant_count: 0,
                transition: 'settled',
              },
            }),
          ),
          readAgentHistory: vi.fn().mockResolvedValue({
            ok: true,
            value: {
              session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
              runtime_instance_id: readyProjection.runtime_instance_id,
              revision: '1',
              active: null,
              turns: [],
              next_before_turn_task_id: null,
              inbox: {
                session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
                revision: '1',
                paused: false,
                messages: [],
              },
            },
          }),
          readAgentToolActivity: vi.fn().mockResolvedValue({
            ok: true,
            value: {
              session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
              calls: [],
              next_before_call_id: null,
            },
          }),
          getRuntimeReadiness: vi
            .fn()
            .mockResolvedValue({ ok: true, value: { core_health: 'ready', backends: [] } }),
          listQuarantine: vi.fn().mockResolvedValue({ ok: true, value: [] }),
          ...emptySessionGrantRoutes,
          onCoreHealth: vi.fn().mockReturnValue(() => undefined),
          onModelProjection: vi.fn().mockReturnValue(() => undefined),
          onSecurityControl: vi.fn().mockReturnValue(() => undefined),
          onWorkspaceSessions: vi.fn().mockReturnValue(() => undefined),
          openBinaryStream: vi.fn(),
          prepareHostPreparation: vi
            .fn()
            .mockResolvedValue({ ok: true, value: hostPreparationStatus }),
          subscribeModelProjection: vi.fn().mockResolvedValue({ ok: true, value: undefined }),
          unsubscribeModelProjection: vi.fn().mockResolvedValue({ ok: true, value: undefined }),
        },
      },
    });
  });

  afterEach(() => {
    cleanup();
    resetModelSettingsStoreForTests();
    resetScopedModelProjectionStoreForTests();
    vi.restoreAllMocks();
  });
}
