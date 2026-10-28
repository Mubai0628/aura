import { describe, expect, it } from 'vitest';

import {
  isModelProjectionKind,
  isToolProjectionKind,
  isToolProjectionRouteRequest,
  parseProjection,
} from './projection-validation.js';

const RUNTIME_ID = 'rnd_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const ENDPOINT_A = 'end_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const ENDPOINT_B = 'end_01ARZ3NDEKTSV4RRFFQ69G5FAW';
const CREDENTIAL_A = 'crd_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const CREDENTIAL_B = 'crd_01ARZ3NDEKTSV4RRFFQ69G5FAW';
const MODEL_ID = 'mdl_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const AVAILABILITY_ID = 'mav_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const SESSION_ID = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';

const runningAvailability = {
  cancellation_reason: null,
  completed_at: null,
  endpoint_id: ENDPOINT_A,
  endpoint_revision_at_start: '3',
  endpoint_security_fingerprint: {
    algorithm: 'blake3',
    hex: '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
  },
  id: AVAILABILITY_ID,
  model_profile_id: MODEL_ID,
  model_profile_revision_at_start: '2',
  models_revision_at_start: '8',
  result: null,
  revision: '2',
  stage: 'network',
  started_at: '2026-09-01T00:00:00Z',
  state: 'running',
};

const activeCredential = {
  active_credential_revision: '2',
  credential_id: CREDENTIAL_A,
  endpoint_id: ENDPOINT_A,
  error_code: null,
  last_committed_credential_revision: '2',
  row_revision: '5',
  slot_name: 'api-key',
  state: 'configured',
};

const dormantCredential = {
  credential_id: CREDENTIAL_B,
  endpoint_id: ENDPOINT_B,
  error_code: null,
  last_committed_credential_revision: '4',
  retained_state: 'configured',
  row_revision: '7',
  slot_name: 'api-key',
};

const change = (projectionKind: string, projectionScope: unknown, payload: unknown) => ({
  payload,
  projection_kind: projectionKind,
  projection_scope: projectionScope,
  revision: '9',
  runtime_instance_id: RUNTIME_ID,
});

describe('P05 projection boundary', () => {
  it('accepts the atomic active/dormant settings indexes and rejects the removed field', () => {
    const payload = {
      active_credentials: [activeCredential],
      aura_revision: '3',
      default_model_profile_id: null,
      dormant_credentials: [dormantCredential],
      models_revision: '8',
      registry: {},
    };
    expect(parseProjection(change('model_settings', { type: 'global' }, payload))).toBeDefined();
    expect(
      parseProjection(
        change(
          'model_settings',
          { type: 'global' },
          {
            ...payload,
            active_credentials: undefined,
            credential_statuses: [activeCredential],
          },
        ),
      ),
    ).toBeUndefined();
  });

  it('rejects duplicate, unsorted, or cross-index Credential identities', () => {
    const base = {
      aura_revision: '3',
      default_model_profile_id: null,
      models_revision: '8',
      registry: {},
    };
    expect(
      parseProjection(
        change(
          'model_settings',
          { type: 'global' },
          {
            ...base,
            active_credentials: [activeCredential, activeCredential],
            dormant_credentials: [],
          },
        ),
      ),
    ).toBeUndefined();
    expect(
      parseProjection(
        change(
          'model_settings',
          { type: 'global' },
          {
            ...base,
            active_credentials: [
              { ...activeCredential, credential_id: CREDENTIAL_B, endpoint_id: ENDPOINT_B },
              activeCredential,
            ],
            dormant_credentials: [],
          },
        ),
      ),
    ).toBeUndefined();
    expect(
      parseProjection(
        change(
          'model_settings',
          { type: 'global' },
          {
            ...base,
            active_credentials: [activeCredential],
            dormant_credentials: [
              {
                ...dormantCredential,
                credential_id: CREDENTIAL_A,
                endpoint_id: ENDPOINT_A,
              },
            ],
          },
        ),
      ),
    ).toBeUndefined();
  });

  it('enforces Endpoint authentication exclusivity and the Credential revision matrix', () => {
    const payload = {
      authentication: {
        active_credential: activeCredential,
        auth_kind: 'secret',
        dormant_credential: null,
      },
      endpoint: {},
      endpoint_id: ENDPOINT_A,
      latest_discovery_run: null,
      models: [],
      models_revision: '8',
      provider: {},
    };
    const scoped = (value: unknown) =>
      change('endpoint_detail', { endpoint_id: ENDPOINT_A, type: 'endpoint' }, value);
    expect(parseProjection(scoped(payload))).toBeDefined();
    expect(
      parseProjection(scoped({ ...payload, latest_availability_test: runningAvailability })),
    ).toBeUndefined();
    expect(
      parseProjection(
        scoped({
          ...payload,
          authentication: {
            active_credential: activeCredential,
            auth_kind: 'secret',
            dormant_credential: dormantCredential,
          },
        }),
      ),
    ).toBeUndefined();
    expect(
      parseProjection(
        scoped({
          ...payload,
          authentication: {
            ...payload.authentication,
            active_credential: {
              ...activeCredential,
              active_credential_revision: null,
            },
          },
        }),
      ),
    ).toBeUndefined();
  });

  it('accepts only the readiness view on model profile projections', () => {
    const payload = {
      active_availability_test: null,
      endpoint: {},
      execution_readiness: {
        is_executable: false,
        issue_code: 'credential.unconfigured',
      },
      latest_availability_test: null,
      model_profile_id: MODEL_ID,
      models_revision: '8',
      profile: {},
      provider: {},
    };
    const scoped = (value: unknown) =>
      change('model_profile', { model_profile_id: MODEL_ID, type: 'model_profile' }, value);
    expect(parseProjection(scoped(payload))).toBeDefined();
    expect(
      parseProjection(
        scoped({
          ...payload,
          active_availability_test: runningAvailability,
          latest_availability_test: runningAvailability,
        }),
      ),
    ).toBeDefined();
    expect(
      parseProjection(
        scoped({
          ...payload,
          active_availability_test: runningAvailability,
          latest_availability_test: null,
        }),
      ),
    ).toBeUndefined();
    expect(
      parseProjection(
        scoped({
          ...payload,
          credential_status: activeCredential,
        }),
      ),
    ).toBeUndefined();
    expect(
      parseProjection(
        scoped({
          ...payload,
          execution_readiness: {
            is_executable: false,
            issue_code: 'RAW PROVIDER ERROR',
          },
        }),
      ),
    ).toBeUndefined();
  });
});

describe('model projection routing', () => {
  it('accepts every model projection kind owned by the shared contract', () => {
    for (const kind of [
      'context_window_correction',
      'endpoint_detail',
      'model_availability_test',
      'model_discovery',
      'model_profile',
      'model_settings',
      'prompt_inspector',
      'provider_catalog',
      'session_model_selector',
    ]) {
      expect(isModelProjectionKind(kind), kind).toBe(true);
    }
  });

  it('does not route non-model projections through the model channel', () => {
    for (const kind of ['core_health', 'workspace_files', 'workspace_session', 'unknown']) {
      expect(isModelProjectionKind(kind), kind).toBe(false);
    }
  });
});

describe('P07 Tool Projection boundary', () => {
  const TOOL_SETTINGS_HASH = {
    algorithm: 'sha256',
    canonicalization_version: 'rfc8785_jcs_v1',
    hex: '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
  } as const;

  it('recognizes every closed Tool Projection kind', () => {
    for (const kind of [
      'tool_settings',
      'tool_package_list',
      'tool_draft_list',
      'workspace_tool_pins',
      'workspace_tool_registry_summary',
      'session_effective_tool_registry',
      'session_loaded_tools',
      'session_tool_activity',
      'session_tool_pins',
      'tool_package',
      'tool_definition',
      'tool_draft',
      'tool_conformance_run',
      'tool_batch',
      'tool_call',
      'tool_execution_attempt',
      'tool_long_operation',
    ]) {
      expect(isToolProjectionKind(kind), kind).toBe(true);
    }
  });

  it('accepts renderer-safe Tool Settings and rejects unknown fields', () => {
    const payload = {
      configuration_issue: null,
      settings: {
        content_hash: TOOL_SETTINGS_HASH,
        default_timeout_ms: 120_000,
        max_concurrent_tool_calls: 4,
        max_dynamic_tool_schemas: 50,
        max_retries: 3,
        revision: '4',
      },
    };
    expect(parseProjection(change('tool_settings', { type: 'global' }, payload))).toBeDefined();
    expect(
      parseProjection(
        change('tool_settings', { type: 'global' }, { ...payload, internal_diagnostic: 'secret' }),
      ),
    ).toBeUndefined();
  });

  it('binds Session Tool Activity payloads to the exact route scope', () => {
    const payload = {
      active_batches: [],
      older_activity_cursor: null,
      recent_calls: [],
      session_id: SESSION_ID,
    };
    expect(
      parseProjection(
        change('session_tool_activity', { session_id: SESSION_ID, type: 'session' }, payload),
      ),
    ).toBeDefined();
    expect(
      parseProjection(
        change(
          'session_tool_activity',
          { session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAW', type: 'session' },
          payload,
        ),
      ),
    ).toBeUndefined();
  });

  it('accepts the error code a tool call summary carries and refuses one Core never forms', () => {
    const parse = (error_code: unknown) =>
      parseProjection(
        change(
          'session_tool_activity',
          { session_id: SESSION_ID, type: 'session' },
          {
            active_batches: [],
            older_activity_cursor: null,
            recent_calls: [
              {
                can_cancel: false,
                display_name: 'chrome.screenshot',
                error_class: 'unavailable',
                error_code,
                ordinal: 1,
                status: 'finalized',
                tool_batch_id: 'tbt_01ARZ3NDEKTSV4RRFFQ69G5FAV',
                tool_call_id: 'tcl_01ARZ3NDEKTSV4RRFFQ69G5FAV',
                tool_id: 'tol_01ARZ3NDEKTSV4RRFFQ69G5FAV',
                tool_version: { definition_revision: '1', semantic_version: null },
              },
            ],
            session_id: SESSION_ID,
          },
        ),
      );
    expect(parse('chrome.background_tab')).toBeDefined();
    expect(parse(null)).toBeDefined();
    expect(parse('Chrome background tab')).toBeUndefined();
  });

  it('keeps Session Pin intent and long-operation state on independent exact scopes', () => {
    const pins = {
      pin_set_revision: '3',
      pins: [
        {
          pin_revision: '2',
          resolved_binding: null,
          safe_issue: {
            category: 'conflict',
            code: 'tool.registry.snapshot_unavailable',
            message: 'The exact pinned Tool binding is unavailable.',
            retryable: false,
          },
          state: 'unresolved',
          tool_id: 'tol_01ARZ3NDEKTSV4RRFFQ69G5FAV',
          tool_version: { definition_revision: '1', semantic_version: '1.0.0' },
        },
      ],
      session_id: SESSION_ID,
    };
    expect(
      parseProjection(
        change('session_tool_pins', { session_id: SESSION_ID, type: 'session' }, pins),
      ),
    ).toBeDefined();
    expect(
      parseProjection(
        change(
          'session_tool_pins',
          { session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAW', type: 'session' },
          pins,
        ),
      ),
    ).toBeUndefined();

    const operation = {
      can_cancel: true,
      completed_at: null,
      operation: {
        journal_id: 'jnl_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        operation_kind: 'draft_apply',
        tool_draft_id: 'tdr_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      },
      operational_revision: '1',
      safe_error: null,
      started_at: null,
      state: 'queued',
    };
    expect(
      parseProjection(
        change(
          'tool_long_operation',
          { journal_id: operation.operation.journal_id, type: 'tool_long_operation' },
          operation,
        ),
      ),
    ).toBeDefined();
    expect(
      parseProjection(
        change(
          'tool_long_operation',
          { journal_id: 'jnl_01ARZ3NDEKTSV4RRFFQ69G5FAW', type: 'tool_long_operation' },
          operation,
        ),
      ),
    ).toBeUndefined();
  });

  it('accepts schema-driven safe instance settings and rejects credential internals', () => {
    const toolId = 'tol_01ARZ3NDEKTSV4RRFFQ69G5FAV';
    const instance = {
      configuration: {
        configuration_hash: TOOL_SETTINGS_HASH,
        configuration_revision: '2',
        safe_non_secret_value: { output_mode: 'compact' },
      },
      credential_slot_binding: {
        bound_credential: {
          binding_revision: '3',
          credential_id: 'crd_01ARZ3NDEKTSV4RRFFQ69G5FAV',
          credential_revision: '4',
        },
        placement: { placement: 'http_bearer_authorization' },
        slot_key: 'api_key',
      },
      enabled: true,
      entity_revision: '7',
      ready: true,
      runtime_health: 'healthy',
      tool_instance_id: 'tin_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      tool_version: { definition_revision: '1', semantic_version: '1.0.0' },
      trusted: true,
    };
    const payload = {
      canonical_name: 'user.example.fixture.echo',
      current_selection: { reason: 'no_ready_instance', type: 'unavailable' },
      display_name: 'Fixture Echo',
      entity_revision: '7',
      safe_error: null,
      tool_id: toolId,
      versions: [
        {
          audience: 'subagent_configurable',
          capability_count: 0,
          execution_semantics: {
            automatic_retry_allowed: false,
            may_create_persistent_effects: false,
            repeat_is_idempotent: true,
          },
          instance_configuration_schema: {
            document: {
              additionalProperties: false,
              properties: { output_mode: { type: 'string' } },
              required: ['output_mode'],
              type: 'object',
            },
            schema_dialect: 'json_schema_draft_2020_12',
            schema_hash: TOOL_SETTINGS_HASH,
            schema_policy_version: 1,
          },
          instances: [instance],
          model_description: 'Fixture description.',
          model_summary: 'Fixture summary.',
          payload_available: true,
          resource_claim_count: 0,
          result_contract: { type: 'unit' },
          schema: {},
          source: 'imported',
          tool_version: { definition_revision: '1', semantic_version: '1.0.0' },
        },
      ],
    };
    expect(
      parseProjection(
        change('tool_definition', { tool_id: toolId, type: 'tool_definition' }, payload),
      ),
    ).toBeDefined();

    const unsafeInstance = {
      ...instance,
      credential_slot_binding: {
        ...instance.credential_slot_binding,
        bound_credential: {
          ...instance.credential_slot_binding.bound_credential,
          wincred_target: 'must-never-cross-the-projection-boundary',
        },
      },
    };
    expect(
      parseProjection(
        change(
          'tool_definition',
          { tool_id: toolId, type: 'tool_definition' },
          {
            ...payload,
            versions: [{ ...payload.versions[0], instances: [unsafeInstance] }],
          },
        ),
      ),
    ).toBeUndefined();
  });

  it('rejects malformed Tool entity routes before Main subscribes', () => {
    expect(
      isToolProjectionRouteRequest({
        projection_kind: 'tool_execution_attempt',
        projection_scope: {
          tool_execution_attempt_id: 'tea_01ARZ3NDEKTSV4RRFFQ69G5FAV',
          type: 'tool_execution_attempt',
        },
      }),
    ).toBe(true);
    expect(
      isToolProjectionRouteRequest({
        projection_kind: 'tool_execution_attempt',
        projection_scope: {
          tool_execution_attempt_id: 'tea_01arz3ndektsv4rrffq69g5fav',
          type: 'tool_execution_attempt',
        },
      }),
    ).toBe(false);
  });
});

describe('P06 prompt inspector projection boundary', () => {
  const payload = {
    active_agent_run_id: null,
    active_model_step_id: null,
    active_task_lineage: null,
    active_tool_call_ids: [],
    attachments: [],
    budget: {
      estimated_input_tokens: 10_000,
      estimator_id: 'utf8-byte-upper-bound',
      estimator_version: 1,
      exact: false,
      output_reserve_tokens: 8_192,
      policy: 'context_budget_policy_v1',
      request_input_budget_tokens: 36_608,
      runtime_effective_window_tokens: 65_536,
      safety_reserve_tokens: 4_096,
    },
    cache_usage: {
      actual_cached_input_tokens: null,
      stable_prefix_estimate_tokens: 4_000,
    },
    compaction: {
      retained_recent_tokens: 0,
      retention_exceptions: [],
      source_context_snapshot_id: null,
      state: 'not_required',
    },
    context_snapshot_id: null,
    context_usage: {
      estimated: true,
      runtime_effective_window_tokens: 65_536,
      segments: [
        { estimated_tokens: 4_000, kind: 'system_and_rules', label: '系统与规则' },
        { estimated_tokens: 1_000, kind: 'tools_and_results', label: '工具与结果' },
        { estimated_tokens: 3_000, kind: 'conversation', label: '对话内容' },
        {
          estimated_tokens: 2_000,
          kind: 'background_and_compaction',
          label: '背景与压缩历史',
        },
      ],
      used_input_tokens: 10_000,
    },
    model_egress: null,
    model_profile_id: null,
    overflow: null,
    prompt_bundle_snapshot_id: null,
    revision: '2',
    sections: [],
    session_id: SESSION_ID,
    skills: [],
    tools: [],
  };

  const scoped = (candidate: unknown, sessionId = SESSION_ID) =>
    change('prompt_inspector', { session_id: sessionId, type: 'session' }, candidate);

  it('accepts four ordered Core usage categories in the matching Session scope', () => {
    expect(parseProjection(scoped(payload))).toBeDefined();
  });

  it('accepts the Rust prompt Inspector optional frozen activity facts', () => {
    expect(
      parseProjection(
        scoped({
          ...payload,
          activity: {
            body_hash: { algorithm: 'blake3', hex: '1'.repeat(64) },
            coverage: 'complete',
            fragment_present: true,
            omitted: 0,
            revision: '1',
            rows_in_scope: 1,
            rows_shown: 1,
            runtime_instance_id: 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV',
            sampled_at: '2026-10-14T00:00:00Z',
            truncated: false,
          },
        }),
      ),
    ).toBeDefined();
  });

  it('rejects inconsistent usage totals, unknown fields, and cross-Session payloads', () => {
    expect(
      parseProjection(
        scoped({
          ...payload,
          context_usage: { ...payload.context_usage, used_input_tokens: 9_999 },
        }),
      ),
    ).toBeUndefined();
    expect(parseProjection(scoped({ ...payload, raw_prompt_text: 'secret' }))).toBeUndefined();
    expect(parseProjection(scoped(payload, 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAW'))).toBeUndefined();
  });
});

// The settings carry the thinking levels each model offers.
describe('the thinking levels in the model settings', () => {
  const payload = (reasoning_efforts: unknown) => ({
    active_credentials: [],
    aura_revision: '3',
    default_model_profile_id: null,
    dormant_credentials: [],
    models_revision: '8',
    reasoning_efforts,
    registry: {},
  });
  it('accepts the levels of each model', () => {
    const levels = [{ model_profile_id: MODEL_ID, reasoning_efforts: ['low', 'medium', 'high'] }];
    expect(
      parseProjection(change('model_settings', { type: 'global' }, payload(levels))),
    ).toBeDefined();
  });
  it('refuses an unknown level or a stray field', () => {
    for (const levels of [
      [{ model_profile_id: MODEL_ID, reasoning_efforts: ['turbo'] }],
      [{ model_profile_id: MODEL_ID, reasoning_efforts: [], extra: true }],
      [{ model_profile_id: 'not-an-id', reasoning_efforts: [] }],
    ])
      expect(
        parseProjection(change('model_settings', { type: 'global' }, payload(levels))),
      ).toBeUndefined();
  });
});
