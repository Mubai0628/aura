import type {
  ProjectionChange,
  ProjectionUnsubscribeRequest,
  ToolActivityPageRequestV1,
  ToolActivityPageV1,
  ToolCredentialCandidatesProjectionV1,
} from '@aura/contracts/generated';
import {
  isSessionToolActivity,
  isSessionToolRegistry,
  isToolCallActivityHistory,
  isToolDraftSummary,
  isToolPin,
  isWorkspaceToolRegistrySummary,
} from './tool-projection-validation/activity.js';
import {
  isAuraId,
  isOptionalNullable,
  isRevision,
  isSafeUnsigned,
  isToolActivityCursor,
} from './tool-projection-validation/common.js';
import {
  isCredentialSlotKey,
  isToolCredentialCandidate,
  isToolCredentialPlacement,
} from './tool-projection-validation/credentials.js';
import { isToolDefinitionProjection } from './tool-projection-validation/definitions.js';
import {
  isToolBatchProjection,
  isToolCallProjection,
  isToolConformanceProjection,
  isToolDraftProjection,
  isToolExecutionAttemptProjection,
  isToolLongOperationProjection,
} from './tool-projection-validation/execution.js';
import {
  isToolPackageProjection,
  isToolPackageSummary,
} from './tool-projection-validation/packages.js';
import { exactRecord, isRecord, recordWithOptional } from './tool-projection-validation/records.js';
import { isToolSettingsProjection } from './tool-projection-validation/settings.js';
export type ToolProjectionKind =
  | 'session_effective_tool_registry'
  | 'session_loaded_tools'
  | 'session_tool_activity'
  | 'session_tool_pins'
  | 'tool_batch'
  | 'tool_call'
  | 'tool_conformance_run'
  | 'tool_definition'
  | 'tool_draft'
  | 'tool_draft_list'
  | 'tool_execution_attempt'
  | 'tool_long_operation'
  | 'tool_package'
  | 'tool_package_list'
  | 'tool_settings'
  | 'workspace_tool_pins'
  | 'workspace_tool_registry_summary';

export type ToolProjectionChange = Extract<
  ProjectionChange,
  { projection_kind: ToolProjectionKind }
>;

export type ToolProjectionRouteRequest = Extract<
  ProjectionUnsubscribeRequest,
  { projection_kind: ToolProjectionKind }
>;

export const isToolProjectionKind = (value: unknown): value is ToolProjectionKind =>
  value === 'tool_settings' ||
  value === 'tool_package_list' ||
  value === 'tool_draft_list' ||
  value === 'workspace_tool_pins' ||
  value === 'workspace_tool_registry_summary' ||
  value === 'session_effective_tool_registry' ||
  value === 'session_loaded_tools' ||
  value === 'session_tool_activity' ||
  value === 'session_tool_pins' ||
  value === 'tool_package' ||
  value === 'tool_definition' ||
  value === 'tool_draft' ||
  value === 'tool_conformance_run' ||
  value === 'tool_batch' ||
  value === 'tool_call' ||
  value === 'tool_execution_attempt' ||
  value === 'tool_long_operation';

export const isToolProjectionRouteRequest = (value: unknown): value is ToolProjectionRouteRequest =>
  exactRecord(value, ['projection_kind', 'projection_scope']) &&
  isToolProjectionKind(value.projection_kind) &&
  isToolProjectionScope(value.projection_kind, value.projection_scope);

export const isToolActivityPageRequest = (value: unknown): value is ToolActivityPageRequestV1 =>
  exactRecord(value, ['cursor']) && isToolActivityCursor(value.cursor);

export const isToolActivityPage = (value: unknown): value is ToolActivityPageV1 =>
  recordWithOptional(value, ['cutoff_session_sequence', 'items', 'session_id'], ['next_cursor']) &&
  isAuraId(value.session_id, 'ses') &&
  isSafeUnsigned(value.cutoff_session_sequence) &&
  Array.isArray(value.items) &&
  value.items.every(isToolCallActivityHistory) &&
  isOptionalNullable(value.next_cursor, isToolActivityCursor);

export const isToolCredentialCandidatesProjection = (
  value: unknown,
): value is ToolCredentialCandidatesProjectionV1 =>
  exactRecord(value, [
    'candidate_set_revision',
    'candidates',
    'credential_slot_key',
    'instance_revision',
    'placement',
    'session_id',
    'tool_instance_id',
    'workspace_id',
  ]) &&
  isAuraId(value.tool_instance_id, 'tin') &&
  isRevision(value.instance_revision) &&
  isCredentialSlotKey(value.credential_slot_key) &&
  isToolCredentialPlacement(value.placement) &&
  isOptionalNullable(value.workspace_id, (candidate) => isAuraId(candidate, 'wsp')) &&
  isOptionalNullable(value.session_id, (candidate) => isAuraId(candidate, 'ses')) &&
  !(value.session_id != null && value.workspace_id == null) &&
  isRevision(value.candidate_set_revision) &&
  Array.isArray(value.candidates) &&
  value.candidates.every(isToolCredentialCandidate);

export const toolProjectionRouteKey = (route: ToolProjectionRouteRequest): string => {
  switch (route.projection_scope.type) {
    case 'global':
      return `${route.projection_kind}:global`;
    case 'workspace':
      return `${route.projection_kind}:${route.projection_scope.workspace_id}`;
    case 'session':
      return `${route.projection_kind}:${route.projection_scope.session_id}`;
    case 'tool_package':
      return `${route.projection_kind}:${route.projection_scope.tool_package_id}`;
    case 'tool_definition':
      return `${route.projection_kind}:${route.projection_scope.tool_id}`;
    case 'tool_draft':
      return `${route.projection_kind}:${route.projection_scope.tool_draft_id}`;
    case 'tool_conformance_run':
      return `${route.projection_kind}:${route.projection_scope.tool_conformance_run_id}`;
    case 'tool_batch':
      return `${route.projection_kind}:${route.projection_scope.tool_batch_id}`;
    case 'tool_call':
      return `${route.projection_kind}:${route.projection_scope.tool_call_id}`;
    case 'tool_execution_attempt':
      return `${route.projection_kind}:${route.projection_scope.tool_execution_attempt_id}`;
    case 'tool_long_operation':
      return `${route.projection_kind}:${route.projection_scope.journal_id}`;
  }
};

export const isToolProjectionScope = (kind: unknown, scope: unknown): boolean => {
  switch (kind) {
    case 'tool_settings':
    case 'tool_package_list':
    case 'tool_draft_list':
      return exactRecord(scope, ['type']) && scope.type === 'global';
    case 'workspace_tool_pins':
    case 'workspace_tool_registry_summary':
      return (
        exactRecord(scope, ['type', 'workspace_id']) &&
        scope.type === 'workspace' &&
        isAuraId(scope.workspace_id, 'wsp')
      );
    case 'session_effective_tool_registry':
    case 'session_loaded_tools':
    case 'session_tool_activity':
    case 'session_tool_pins':
      return (
        exactRecord(scope, ['session_id', 'type']) &&
        scope.type === 'session' &&
        isAuraId(scope.session_id, 'ses')
      );
    case 'tool_package':
      return (
        exactRecord(scope, ['tool_package_id', 'type']) &&
        scope.type === 'tool_package' &&
        isAuraId(scope.tool_package_id, 'tpk')
      );
    case 'tool_definition':
      return (
        exactRecord(scope, ['tool_id', 'type']) &&
        scope.type === 'tool_definition' &&
        isAuraId(scope.tool_id, 'tol')
      );
    case 'tool_draft':
      return (
        exactRecord(scope, ['tool_draft_id', 'type']) &&
        scope.type === 'tool_draft' &&
        isAuraId(scope.tool_draft_id, 'tdr')
      );
    case 'tool_conformance_run':
      return (
        exactRecord(scope, ['tool_conformance_run_id', 'type']) &&
        scope.type === 'tool_conformance_run' &&
        isAuraId(scope.tool_conformance_run_id, 'tcr')
      );
    case 'tool_batch':
      return (
        exactRecord(scope, ['tool_batch_id', 'type']) &&
        scope.type === 'tool_batch' &&
        isAuraId(scope.tool_batch_id, 'tbt')
      );
    case 'tool_call':
      return (
        exactRecord(scope, ['tool_call_id', 'type']) &&
        scope.type === 'tool_call' &&
        isAuraId(scope.tool_call_id, 'tcl')
      );
    case 'tool_execution_attempt':
      return (
        exactRecord(scope, ['tool_execution_attempt_id', 'type']) &&
        scope.type === 'tool_execution_attempt' &&
        isAuraId(scope.tool_execution_attempt_id, 'tea')
      );
    case 'tool_long_operation':
      return (
        exactRecord(scope, ['journal_id', 'type']) &&
        scope.type === 'tool_long_operation' &&
        isAuraId(scope.journal_id, 'jnl')
      );
    default:
      return false;
  }
};

export const toolProjectionPayloadMatchesScope = (
  kind: ToolProjectionKind,
  scope: unknown,
  payload: unknown,
): boolean => {
  if (!isRecord(scope) || !isRecord(payload)) return false;
  switch (kind) {
    case 'workspace_tool_pins':
    case 'workspace_tool_registry_summary':
      return payload.workspace_id === scope.workspace_id;
    case 'session_effective_tool_registry':
    case 'session_loaded_tools':
    case 'session_tool_activity':
    case 'session_tool_pins':
      return payload.session_id === scope.session_id;
    case 'tool_package':
      return payload.tool_package_id === scope.tool_package_id;
    case 'tool_definition':
      return payload.tool_id === scope.tool_id;
    case 'tool_draft':
      return payload.tool_draft_id === scope.tool_draft_id;
    case 'tool_conformance_run':
      return payload.tool_conformance_run_id === scope.tool_conformance_run_id;
    case 'tool_batch':
      return payload.tool_batch_id === scope.tool_batch_id;
    case 'tool_call':
      return payload.tool_call_id === scope.tool_call_id;
    case 'tool_execution_attempt':
      return payload.tool_execution_attempt_id === scope.tool_execution_attempt_id;
    case 'tool_long_operation':
      return isRecord(payload.operation) && payload.operation.journal_id === scope.journal_id;
    case 'tool_settings':
    case 'tool_package_list':
    case 'tool_draft_list':
      return true;
  }
};

export const isToolProjectionPayload = (kind: ToolProjectionKind, value: unknown): boolean => {
  switch (kind) {
    case 'tool_settings':
      return isToolSettingsProjection(value);
    case 'tool_package_list':
      return (
        exactRecord(value, ['packages']) &&
        Array.isArray(value.packages) &&
        value.packages.every(isToolPackageSummary)
      );
    case 'tool_draft_list':
      return (
        exactRecord(value, ['drafts']) &&
        Array.isArray(value.drafts) &&
        value.drafts.every(isToolDraftSummary)
      );
    case 'workspace_tool_pins':
      return (
        exactRecord(value, ['pin_set_revision', 'pins', 'workspace_id']) &&
        isAuraId(value.workspace_id, 'wsp') &&
        isRevision(value.pin_set_revision) &&
        Array.isArray(value.pins) &&
        value.pins.every(isToolPin)
      );
    case 'workspace_tool_registry_summary':
      return isWorkspaceToolRegistrySummary(value);
    case 'session_effective_tool_registry':
      return isSessionToolRegistry(value, false);
    case 'session_loaded_tools':
      return isSessionToolRegistry(value, true);
    case 'session_tool_activity':
      return isSessionToolActivity(value);
    case 'session_tool_pins':
      return (
        exactRecord(value, ['pin_set_revision', 'pins', 'session_id']) &&
        isAuraId(value.session_id, 'ses') &&
        isRevision(value.pin_set_revision) &&
        Array.isArray(value.pins) &&
        value.pins.every(isToolPin)
      );
    case 'tool_package':
      return isToolPackageProjection(value);
    case 'tool_definition':
      return isToolDefinitionProjection(value);
    case 'tool_draft':
      return isToolDraftProjection(value);
    case 'tool_conformance_run':
      return isToolConformanceProjection(value);
    case 'tool_batch':
      return isToolBatchProjection(value);
    case 'tool_call':
      return isToolCallProjection(value);
    case 'tool_execution_attempt':
      return isToolExecutionAttemptProjection(value);
    case 'tool_long_operation':
      return isToolLongOperationProjection(value);
  }
};
