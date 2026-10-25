import {
  isTimestamp,
  isAuraId,
  isContentHash,
  isRevision,
  exactRecord,
  REASONING_EFFORTS,
  recordWithOptional,
  isRecord,
} from './projection-validation/common.js';
import { isWorkspaceFilesPayload } from './projection-validation/workspace.js';
import { isCoreHealthPayload } from './projection-validation/core-health.js';
import { isPromptInspectorProjection } from './projection-validation/prompt-inspector.js';
import { projectionPayloadMatchesScope } from './projection-validation/model-scope.js';
import type { ProjectionChange, ProjectionUnsubscribeRequest } from '@aura/contracts/generated';

import {
  isToolProjectionKind,
  isToolProjectionPayload,
  isToolProjectionScope,
  toolProjectionPayloadMatchesScope,
} from './tool-projection-validation.js';

export {
  isToolProjectionKind,
  isToolProjectionRouteRequest,
  toolProjectionRouteKey,
  type ToolProjectionChange,
  type ToolProjectionKind,
  type ToolProjectionRouteRequest,
} from './tool-projection-validation.js';

export type ModelProjectionKind =
  | 'context_window_correction'
  | 'endpoint_detail'
  | 'model_availability_test'
  | 'model_discovery'
  | 'model_profile'
  | 'model_settings'
  | 'prompt_inspector'
  | 'provider_catalog'
  | 'session_model_selector';

export type ModelProjectionRouteRequest = Extract<
  ProjectionUnsubscribeRequest,
  { projection_kind: ModelProjectionKind }
>;

export const isModelProjectionRouteRequest = (
  value: unknown,
): value is ModelProjectionRouteRequest =>
  exactRecord(value, ['projection_kind', 'projection_scope']) &&
  isModelProjectionKind(value.projection_kind) &&
  isProjectionScope(value.projection_kind, value.projection_scope);

export const modelProjectionRouteKey = (route: ModelProjectionRouteRequest): string => {
  switch (route.projection_scope.type) {
    case 'global':
      return `${route.projection_kind}:global`;
    case 'endpoint':
      return `${route.projection_kind}:${route.projection_scope.endpoint_id}`;
    case 'model_profile':
      return `${route.projection_kind}:${route.projection_scope.model_profile_id}`;
    case 'session':
      return `${route.projection_kind}:${route.projection_scope.session_id}`;
    case 'model_availability_test':
      return `${route.projection_kind}:${route.projection_scope.model_availability_test_id}`;
    case 'model_discovery':
      return `${route.projection_kind}:${route.projection_scope.model_discovery_run_id}`;
  }
};

export const isModelProjectionKind = (value: unknown): value is ModelProjectionKind =>
  value === 'model_settings' ||
  value === 'provider_catalog' ||
  value === 'endpoint_detail' ||
  value === 'model_profile' ||
  value === 'prompt_inspector' ||
  value === 'session_model_selector' ||
  value === 'model_availability_test' ||
  value === 'model_discovery' ||
  value === 'context_window_correction';

export const parseProjection = (value: unknown): ProjectionChange | undefined => {
  if (
    !exactRecord(value, [
      'payload',
      'projection_kind',
      'projection_scope',
      'revision',
      'runtime_instance_id',
    ]) ||
    typeof value.runtime_instance_id !== 'string' ||
    !isRevision(value.revision) ||
    !isProjectionScope(value.projection_kind, value.projection_scope)
  ) {
    return undefined;
  }
  if (value.projection_kind === 'core_health' && isCoreHealthPayload(value.payload)) {
    return value as unknown as ProjectionChange;
  }
  if (value.projection_kind === 'workspace_session' && isWorkspaceSessionPayload(value.payload)) {
    return value as unknown as ProjectionChange;
  }
  if (value.projection_kind === 'workspace_files' && isWorkspaceFilesPayload(value.payload)) {
    return value as unknown as ProjectionChange;
  }
  if (
    isModelProjectionPayload(value.projection_kind, value.payload) &&
    projectionPayloadMatchesScope(value.projection_kind, value.projection_scope, value.payload)
  ) {
    return value as unknown as ProjectionChange;
  }
  if (
    isToolProjectionKind(value.projection_kind) &&
    isToolProjectionPayload(value.projection_kind, value.payload) &&
    toolProjectionPayloadMatchesScope(value.projection_kind, value.projection_scope, value.payload)
  ) {
    return value as unknown as ProjectionChange;
  }
  return undefined;
};

export const isProjectionScope = (kind: unknown, scope: unknown): boolean => {
  switch (kind) {
    case 'core_health':
    case 'workspace_session':
    case 'workspace_files':
    case 'model_settings':
    case 'provider_catalog':
      return exactRecord(scope, ['type']) && scope.type === 'global';
    case 'endpoint_detail':
      return (
        exactRecord(scope, ['endpoint_id', 'type']) &&
        scope.type === 'endpoint' &&
        isAuraId(scope.endpoint_id, 'end')
      );
    case 'model_profile':
      return (
        exactRecord(scope, ['model_profile_id', 'type']) &&
        scope.type === 'model_profile' &&
        isAuraId(scope.model_profile_id, 'mdl')
      );
    case 'session_model_selector':
    case 'context_window_correction':
    case 'prompt_inspector':
      return (
        exactRecord(scope, ['session_id', 'type']) &&
        scope.type === 'session' &&
        isAuraId(scope.session_id, 'ses')
      );
    case 'model_availability_test':
      return (
        exactRecord(scope, ['model_availability_test_id', 'type']) &&
        scope.type === 'model_availability_test' &&
        isAuraId(scope.model_availability_test_id, 'mav')
      );
    case 'model_discovery':
      return (
        exactRecord(scope, ['model_discovery_run_id', 'type']) &&
        scope.type === 'model_discovery' &&
        isAuraId(scope.model_discovery_run_id, 'mdr')
      );
    case 'tool_settings':
    case 'tool_package_list':
    case 'tool_draft_list':
    case 'workspace_tool_pins':
    case 'workspace_tool_registry_summary':
    case 'session_effective_tool_registry':
    case 'session_loaded_tools':
    case 'session_tool_activity':
    case 'session_tool_pins':
    case 'tool_package':
    case 'tool_definition':
    case 'tool_draft':
    case 'tool_conformance_run':
    case 'tool_batch':
    case 'tool_call':
    case 'tool_execution_attempt':
    case 'tool_long_operation':
      return isToolProjectionScope(kind, scope);
    default:
      return false;
  }
};

/** The thinking levels one model offers. */
const isModelReasoningEfforts = (value: unknown): boolean =>
  exactRecord(value, ['model_profile_id', 'reasoning_efforts']) &&
  isAuraId(value.model_profile_id, 'mdl') &&
  Array.isArray(value.reasoning_efforts) &&
  value.reasoning_efforts.every((level) => REASONING_EFFORTS.has(level));

const isModelProjectionPayload = (kind: unknown, payload: unknown): boolean => {
  switch (kind) {
    case 'model_settings':
      return (
        recordWithOptional(
          payload,
          [
            'active_credentials',
            'aura_revision',
            'dormant_credentials',
            'models_revision',
            'registry',
          ],
          ['default_model_profile_id', 'reasoning_efforts'],
        ) &&
        (payload.reasoning_efforts === undefined ||
          (Array.isArray(payload.reasoning_efforts) &&
            payload.reasoning_efforts.every(isModelReasoningEfforts))) &&
        isRevision(payload.aura_revision) &&
        isRevision(payload.models_revision) &&
        Array.isArray(payload.active_credentials) &&
        payload.active_credentials.every(isEndpointCredentialStatus) &&
        credentialsUseStableUniqueOrder(payload.active_credentials) &&
        Array.isArray(payload.dormant_credentials) &&
        payload.dormant_credentials.every(isDormantEndpointCredentialSummary) &&
        credentialsUseStableUniqueOrder(payload.dormant_credentials) &&
        credentialsAreDisjoint(payload.active_credentials, payload.dormant_credentials) &&
        isRecord(payload.registry) &&
        (payload.default_model_profile_id === undefined ||
          payload.default_model_profile_id === null ||
          isAuraId(payload.default_model_profile_id, 'mdl'))
      );
    case 'provider_catalog':
      return (
        recordWithOptional(
          payload,
          ['cards', 'projection_revision', 'state'],
          [
            'catalog_content_hash',
            'catalog_revision',
            'checked_at',
            'last_error_code',
            'last_successful_refresh_at',
          ],
        ) &&
        Array.isArray(payload.cards) &&
        payload.cards.every(isRecord) &&
        isRevision(payload.projection_revision) &&
        (payload.state === 'unavailable' ||
          payload.state === 'loading_without_cache' ||
          payload.state === 'fresh' ||
          payload.state === 'stale' ||
          payload.state === 'refreshing_with_lkg' ||
          payload.state === 'refresh_failed_with_lkg')
      );
    case 'endpoint_detail':
      return (
        recordWithOptional(
          payload,
          ['authentication', 'endpoint', 'endpoint_id', 'models', 'models_revision', 'provider'],
          ['latest_discovery_run'],
        ) &&
        isAuraId(payload.endpoint_id, 'end') &&
        isRevision(payload.models_revision) &&
        isEndpointAuthenticationProjection(payload.authentication, payload.endpoint_id) &&
        isRecord(payload.endpoint) &&
        isRecord(payload.provider) &&
        Array.isArray(payload.models) &&
        payload.models.every(isRecord)
      );
    case 'model_profile':
      return (
        recordWithOptional(
          payload,
          [
            'endpoint',
            'execution_readiness',
            'model_profile_id',
            'models_revision',
            'profile',
            'provider',
          ],
          ['active_availability_test', 'latest_availability_test'],
        ) &&
        isAuraId(payload.model_profile_id, 'mdl') &&
        isRevision(payload.models_revision) &&
        isRecord(payload.endpoint) &&
        isModelExecutionReadiness(payload.execution_readiness) &&
        isRecord(payload.profile) &&
        isRecord(payload.provider) &&
        availabilityProjectionPairIsValid(
          payload.active_availability_test,
          payload.latest_availability_test,
        )
      );
    case 'session_model_selector':
      return (
        recordWithOptional(
          payload,
          [
            'binding_revision',
            'endpoints',
            'models',
            'projection_revision',
            'providers',
            'selected_model_execution_readiness',
            'session_id',
          ],
          ['default_model_profile_id', 'selected_model_profile_id'],
        ) &&
        isAuraId(payload.session_id, 'ses') &&
        isRevision(payload.binding_revision) &&
        isRevision(payload.projection_revision) &&
        Array.isArray(payload.endpoints) &&
        isModelExecutionReadiness(payload.selected_model_execution_readiness) &&
        Array.isArray(payload.models) &&
        Array.isArray(payload.providers) &&
        (payload.default_model_profile_id === undefined ||
          payload.default_model_profile_id === null ||
          isAuraId(payload.default_model_profile_id, 'mdl')) &&
        (payload.selected_model_profile_id === undefined ||
          payload.selected_model_profile_id === null ||
          isAuraId(payload.selected_model_profile_id, 'mdl'))
      );
    case 'model_availability_test':
      return (
        exactRecord(payload, ['model_availability_test_id', 'test']) &&
        isAuraId(payload.model_availability_test_id, 'mav') &&
        isRecord(payload.test)
      );
    case 'model_discovery':
      return (
        exactRecord(payload, ['model_discovery_run_id', 'run']) &&
        isAuraId(payload.model_discovery_run_id, 'mdr') &&
        isRecord(payload.run)
      );
    case 'context_window_correction':
      return (
        exactRecord(payload, ['active_corrections', 'session_id']) &&
        isAuraId(payload.session_id, 'ses') &&
        Array.isArray(payload.active_corrections) &&
        payload.active_corrections.every(isRecord)
      );
    case 'prompt_inspector':
      return isPromptInspectorProjection(payload);
    default:
      return false;
  }
};

const isEndpointCredentialStatus = (value: unknown): boolean =>
  recordWithOptional(
    value,
    ['credential_id', 'endpoint_id', 'row_revision', 'slot_name', 'state'],
    ['active_credential_revision', 'error_code', 'last_committed_credential_revision'],
  ) &&
  isAuraId(value.endpoint_id, 'end') &&
  value.slot_name === 'api-key' &&
  isAuraId(value.credential_id, 'crd') &&
  isRevision(value.row_revision) &&
  isOptionalNullableRevision(value.active_credential_revision) &&
  isOptionalNullableRevision(value.last_committed_credential_revision) &&
  isOptionalNullableIssueCode(value.error_code) &&
  credentialRevisionMatrixIsValid(value);

const isDormantEndpointCredentialSummary = (value: unknown): boolean =>
  recordWithOptional(
    value,
    ['credential_id', 'endpoint_id', 'retained_state', 'row_revision', 'slot_name'],
    ['error_code', 'last_committed_credential_revision'],
  ) &&
  isAuraId(value.endpoint_id, 'end') &&
  value.slot_name === 'api-key' &&
  isAuraId(value.credential_id, 'crd') &&
  isRevision(value.row_revision) &&
  isOptionalNullableRevision(value.last_committed_credential_revision) &&
  isOptionalNullableIssueCode(value.error_code) &&
  (value.retained_state === 'unconfigured' ||
    value.retained_state === 'configured' ||
    value.retained_state === 'cleared' ||
    value.retained_state === 'unavailable' ||
    value.retained_state === 'reconciliation_required');

const isEndpointAuthenticationProjection = (value: unknown, endpointId: string): boolean => {
  if (
    !recordWithOptional(value, ['auth_kind'], ['active_credential', 'dormant_credential']) ||
    (value.auth_kind !== 'none' && value.auth_kind !== 'secret')
  ) {
    return false;
  }
  const active = value.active_credential;
  const dormant = value.dormant_credential;
  const validActive =
    active === undefined ||
    active === null ||
    (isRecord(active) && isEndpointCredentialStatus(active) && active.endpoint_id === endpointId);
  const validDormant =
    dormant === undefined ||
    dormant === null ||
    (isRecord(dormant) &&
      isDormantEndpointCredentialSummary(dormant) &&
      dormant.endpoint_id === endpointId);
  if (!validActive || !validDormant) return false;
  if (value.auth_kind === 'secret') {
    return active !== undefined && active !== null && (dormant === undefined || dormant === null);
  }
  return active === undefined || active === null;
};

const isModelExecutionReadiness = (value: unknown): boolean =>
  recordWithOptional(value, ['is_executable'], ['issue_code']) &&
  typeof value.is_executable === 'boolean' &&
  isOptionalNullableIssueCode(value.issue_code);

const availabilityProjectionPairIsValid = (active: unknown, latest: unknown): boolean => {
  if (!isOptionalNullableModelAvailabilityTestView(active)) return false;
  if (!isOptionalNullableModelAvailabilityTestView(latest)) return false;
  const activeRecord = isRecord(active) ? active : undefined;
  const latestRecord = isRecord(latest) ? latest : undefined;
  if (latestRecord === undefined) return activeRecord === undefined;
  const latestIsActive = latestRecord.state === 'queued' || latestRecord.state === 'running';
  if (!latestIsActive) return activeRecord === undefined;
  return (
    activeRecord !== undefined &&
    activeRecord.id === latestRecord.id &&
    activeRecord.revision === latestRecord.revision &&
    activeRecord.state === latestRecord.state
  );
};

const isOptionalNullableModelAvailabilityTestView = (value: unknown): boolean =>
  value === undefined || value === null || isModelAvailabilityTestView(value);

const isModelAvailabilityTestView = (value: unknown): boolean => {
  if (
    !recordWithOptional(
      value,
      [
        'endpoint_id',
        'endpoint_revision_at_start',
        'endpoint_security_fingerprint',
        'id',
        'model_profile_id',
        'model_profile_revision_at_start',
        'models_revision_at_start',
        'revision',
        'started_at',
        'state',
      ],
      ['cancellation_reason', 'completed_at', 'result', 'stage'],
    ) ||
    !isAuraId(value.id, 'mav') ||
    !isAuraId(value.endpoint_id, 'end') ||
    !isAuraId(value.model_profile_id, 'mdl') ||
    !isRevision(value.endpoint_revision_at_start) ||
    !isRevision(value.model_profile_revision_at_start) ||
    !isRevision(value.models_revision_at_start) ||
    !isRevision(value.revision) ||
    !isContentHash(value.endpoint_security_fingerprint) ||
    !isTimestamp(value.started_at) ||
    !isOptionalNullableTimestamp(value.completed_at) ||
    !isOptionalNullableAvailabilityStage(value.stage) ||
    !isOptionalNullableCancellationReason(value.cancellation_reason) ||
    !isOptionalNullableRecord(value.result)
  ) {
    return false;
  }
  const stage = value.stage;
  const result = value.result;
  const cancellationReason = value.cancellation_reason;
  const completedAt = value.completed_at;
  switch (value.state) {
    case 'queued':
      return (
        isAbsentOrNull(stage) &&
        isAbsentOrNull(result) &&
        isAbsentOrNull(cancellationReason) &&
        isAbsentOrNull(completedAt)
      );
    case 'running':
      return (
        typeof stage === 'string' &&
        stage !== 'completed' &&
        isAbsentOrNull(result) &&
        isAbsentOrNull(cancellationReason) &&
        isAbsentOrNull(completedAt)
      );
    case 'completed':
      return (
        stage === 'completed' &&
        isRecord(result) &&
        isAbsentOrNull(cancellationReason) &&
        isTimestamp(completedAt)
      );
    case 'cancelled':
      return (
        stage !== 'completed' &&
        isAbsentOrNull(result) &&
        typeof cancellationReason === 'string' &&
        isTimestamp(completedAt)
      );
    default:
      return false;
  }
};

const isOptionalNullableAvailabilityStage = (value: unknown): boolean =>
  value === undefined ||
  value === null ||
  value === 'config' ||
  value === 'network' ||
  value === 'tls' ||
  value === 'http' ||
  value === 'authentication' ||
  value === 'model_generation' ||
  value === 'completed';

const isOptionalNullableCancellationReason = (value: unknown): boolean =>
  value === undefined ||
  value === null ||
  value === 'user_requested' ||
  value === 'application_restarted' ||
  value === 'configuration_changed' ||
  value === 'target_deleted';

const isOptionalNullableTimestamp = (value: unknown): boolean =>
  value === undefined || value === null || isTimestamp(value);

const isOptionalNullableRecord = (value: unknown): boolean =>
  value === undefined || value === null || isRecord(value);

const isAbsentOrNull = (value: unknown): boolean => value === undefined || value === null;

const credentialRevisionMatrixIsValid = (value: Record<string, unknown>): boolean => {
  const active = value.active_credential_revision;
  const committed = value.last_committed_credential_revision;
  switch (value.state) {
    case 'unconfigured':
      return (
        (active === undefined || active === null) && (committed === undefined || committed === null)
      );
    case 'configured':
      return typeof active === 'string' && active === committed;
    case 'cleared':
      return (active === undefined || active === null) && typeof committed === 'string';
    case 'unavailable':
    case 'reconciliation_required':
      return active === undefined || active === null;
    default:
      return false;
  }
};

const credentialsUseStableUniqueOrder = (values: unknown[]): boolean => {
  let previous: string | undefined;
  for (const value of values) {
    if (!isRecord(value)) return false;
    const current = credentialIdentityKey(value);
    if (current === undefined || (previous !== undefined && previous >= current)) return false;
    previous = current;
  }
  return true;
};

const credentialsAreDisjoint = (active: unknown[], dormant: unknown[]): boolean => {
  const activeKeys = new Set(
    active.flatMap((value) => {
      if (!isRecord(value)) return [];
      const key = credentialIdentityKey(value);
      return key === undefined ? [] : [key];
    }),
  );
  return dormant.every((value) => {
    if (!isRecord(value)) return false;
    const key = credentialIdentityKey(value);
    return key !== undefined && !activeKeys.has(key);
  });
};

const credentialIdentityKey = (value: Record<string, unknown>): string | undefined =>
  typeof value.endpoint_id === 'string' &&
  typeof value.slot_name === 'string' &&
  typeof value.credential_id === 'string'
    ? `${value.endpoint_id}\u0000${value.slot_name}\u0000${value.credential_id}`
    : undefined;

const isOptionalNullableRevision = (value: unknown): boolean =>
  value === undefined || value === null || isRevision(value);

const isOptionalNullableIssueCode = (value: unknown): boolean =>
  value === undefined || value === null || isNormalizedIssueCode(value);

const isNormalizedIssueCode = (value: unknown): value is string => {
  if (typeof value !== 'string' || value.length === 0 || value.length > 128) return false;
  if (!value.split('.').every((segment) => segment.length > 0 && /^[a-z0-9_-]+$/u.test(segment))) {
    return false;
  }
  return (
    [
      'credential.',
      'configuration.',
      'network.',
      'tls.',
      'authentication.',
      'discovery.',
      'provider.',
      'transport.',
      'persistence.',
      'compatibility.',
    ].some((namespace) => value.startsWith(namespace) && value.length > namespace.length) ||
    value === 'artifact.unavailable' ||
    value === 'external.unclassified' ||
    value === 'internal.unexpected'
  );
};

const isWorkspaceSessionPayload = (value: unknown): boolean =>
  exactRecord(value, ['sessions', 'workspaces']) &&
  Array.isArray(value.workspaces) &&
  value.workspaces.every(isWorkspaceSummary) &&
  Array.isArray(value.sessions) &&
  value.sessions.every(isSessionSummary);

const isWorkspaceSummary = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [
      'authorization_epoch',
      'canonical_root',
      'catalog_freshness',
      'display_name',
      'revision',
      'selected_root',
      'state',
      'workspace_id',
    ],
    ['root_identity'],
  ) &&
  typeof value.workspace_id === 'string' &&
  typeof value.display_name === 'string' &&
  typeof value.selected_root === 'string' &&
  typeof value.canonical_root === 'string' &&
  (value.root_identity === undefined ||
    value.root_identity === null ||
    isFileIdentity(value.root_identity)) &&
  typeof value.state === 'string' &&
  isRevision(value.revision) &&
  typeof value.catalog_freshness === 'string' &&
  isRevision(value.authorization_epoch);

const isFileIdentity = (value: unknown): boolean =>
  exactRecord(value, ['file_id', 'volume_serial_number']) &&
  typeof value.file_id === 'string' &&
  typeof value.volume_serial_number === 'string';

const isSessionSummary = (value: unknown): boolean =>
  exactRecord(value, ['session_id', 'state', 'title', 'workspace_id']) &&
  typeof value.session_id === 'string' &&
  typeof value.workspace_id === 'string' &&
  typeof value.title === 'string' &&
  typeof value.state === 'string';
