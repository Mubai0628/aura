import {
  isAuraId,
  isOneOf,
  isOptionalNullable,
  isRevision,
  isSafeUnsigned,
  isSideEffectState,
  isTimestamp,
  isToolActivityCursor,
  isToolBatchStatus,
  isToolCallStatus,
  isToolDraftStatus,
  isToolOutcome,
  isToolProjectionError,
  isToolVersion,
} from './common.js';
import { exactRecord, isRecord, recordWithOptional } from './records.js';
import {
  isEffectiveToolBindingSnapshot,
  isRegistrySnapshotRef,
  isToolDefinitionSnapshotRef,
} from './snapshots.js';

/** Session tool registries and tool activity. */
export const isToolPin = (value: unknown): boolean =>
  recordWithOptional(
    value,
    ['pin_revision', 'state', 'tool_id', 'tool_version'],
    ['resolved_binding', 'safe_issue'],
  ) &&
  isAuraId(value.tool_id, 'tol') &&
  isToolVersion(value.tool_version) &&
  isOneOf(value.state, ['resolved', 'unresolved', 'degraded']) &&
  isRevision(value.pin_revision) &&
  isOptionalNullable(value.safe_issue, isToolProjectionError) &&
  isOptionalNullable(value.resolved_binding, isEffectiveToolBindingSnapshot);

export const isWorkspaceToolRegistrySummary = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [
      'capacity_remediation_required',
      'discoverable_tool_count',
      'exposed_tool_count',
      'unresolved_pin_count',
      'workspace_id',
    ],
    ['registry_snapshot_ref', 'safe_issue'],
  ) &&
  isAuraId(value.workspace_id, 'wsp') &&
  isSafeUnsigned(value.discoverable_tool_count) &&
  isSafeUnsigned(value.exposed_tool_count) &&
  isSafeUnsigned(value.unresolved_pin_count) &&
  typeof value.capacity_remediation_required === 'boolean' &&
  isOptionalNullable(value.registry_snapshot_ref, isRegistrySnapshotRef) &&
  isOptionalNullable(value.safe_issue, isToolProjectionError);

export const isSessionToolRegistry = (value: unknown, loaded: boolean): boolean => {
  if (!isRecord(value)) return false;
  const shapeIsValid = loaded
    ? exactRecord(value, ['registry_snapshot_ref', 'session_id', 'tools', 'turn_task_id'])
    : exactRecord(value, ['registry_snapshot_ref', 'session_id', 'tools']);
  return (
    shapeIsValid &&
    isAuraId(value.session_id, 'ses') &&
    (!loaded || isAuraId(value.turn_task_id, 'trn')) &&
    isRegistrySnapshotRef(value.registry_snapshot_ref) &&
    Array.isArray(value.tools) &&
    value.tools.every(isEffectiveTool)
  );
};

export const isEffectiveTool = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [
      'audience',
      'canonical_name',
      'display_name',
      'eligible',
      'enabled',
      'exposure',
      'runtime_health',
      'source',
      'summary',
      'tool_id',
      'tool_instance_id',
      'tool_version',
      'trusted',
    ],
    ['pin_state', 'safe_ineligibility_reason'],
  ) &&
  isAuraId(value.tool_id, 'tol') &&
  isAuraId(value.tool_instance_id, 'tin') &&
  isToolVersion(value.tool_version) &&
  typeof value.canonical_name === 'string' &&
  typeof value.display_name === 'string' &&
  typeof value.summary === 'string' &&
  isOneOf(value.source, [
    'built_in',
    'user_created',
    'model_created',
    'imported',
    'mcp',
    'plugin',
  ]) &&
  isOneOf(value.exposure, ['core', 'session_pin', 'workspace_pin', 'ephemeral_load']) &&
  isOneOf(value.audience, [
    'child_runtime_required',
    'subagent_configurable',
    'parent_agent_only',
  ]) &&
  typeof value.enabled === 'boolean' &&
  typeof value.trusted === 'boolean' &&
  typeof value.eligible === 'boolean' &&
  isOneOf(value.runtime_health, ['healthy', 'unavailable', 'corrupted']) &&
  isOptionalNullable(value.pin_state, (candidate) =>
    isOneOf(candidate, ['resolved', 'unresolved', 'degraded']),
  ) &&
  isOptionalNullable(value.safe_ineligibility_reason, isToolProjectionError);

export const isSessionToolActivity = (value: unknown): boolean =>
  recordWithOptional(
    value,
    ['active_batches', 'recent_calls', 'session_id'],
    ['older_activity_cursor'],
  ) &&
  isAuraId(value.session_id, 'ses') &&
  Array.isArray(value.active_batches) &&
  value.active_batches.every(isToolBatchActivity) &&
  Array.isArray(value.recent_calls) &&
  value.recent_calls.every(isToolCallActivity) &&
  isOptionalNullable(value.older_activity_cursor, isToolActivityCursor);

export const isToolCallActivityHistory = (value: unknown): boolean =>
  exactRecord(value, [
    'artifact_unavailable',
    'call',
    'definition_ref',
    'session_sequence',
    'source',
    'tool_instance_id',
  ]) &&
  isSafeUnsigned(value.session_sequence) &&
  isToolCallActivity(value.call) &&
  isToolDefinitionSnapshotRef(value.definition_ref) &&
  isAuraId(value.tool_instance_id, 'tin') &&
  isOneOf(value.source, [
    'built_in',
    'user_created',
    'model_created',
    'imported',
    'mcp',
    'plugin',
  ]) &&
  typeof value.artifact_unavailable === 'boolean';

export const isToolBatchActivity = (value: unknown): boolean =>
  recordWithOptional(
    value,
    ['call_count', 'can_cancel', 'completed_call_count', 'status', 'tool_batch_id'],
    ['completed_at', 'started_at'],
  ) &&
  isAuraId(value.tool_batch_id, 'tbt') &&
  isToolBatchStatus(value.status) &&
  isSafeUnsigned(value.call_count) &&
  isSafeUnsigned(value.completed_call_count) &&
  typeof value.can_cancel === 'boolean' &&
  isOptionalNullable(value.started_at, isTimestamp) &&
  isOptionalNullable(value.completed_at, isTimestamp);

/** An error code as Core forms one: dot-separated segments of lowercase letters, digits, `_`, `-`. */
const isErrorCode = (value: unknown): boolean =>
  typeof value === 'string' &&
  value.length <= 128 &&
  /^[a-z0-9_-]+(?:\.[a-z0-9_-]+)*$/u.test(value);

export const isToolCallActivity = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [
      'can_cancel',
      'display_name',
      'ordinal',
      'status',
      'tool_batch_id',
      'tool_call_id',
      'tool_id',
      'tool_version',
    ],
    ['completed_at', 'error_class', 'error_code', 'outcome', 'side_effect_state', 'started_at'],
  ) &&
  isAuraId(value.tool_call_id, 'tcl') &&
  isAuraId(value.tool_batch_id, 'tbt') &&
  isAuraId(value.tool_id, 'tol') &&
  isToolVersion(value.tool_version) &&
  typeof value.display_name === 'string' &&
  isSafeUnsigned(value.ordinal) &&
  isToolCallStatus(value.status) &&
  isOptionalNullable(value.error_class, (candidate) =>
    isOneOf(candidate, [
      'not_found',
      'permission',
      'conflict',
      'invalid_request',
      'unavailable',
      'other',
    ]),
  ) &&
  isOptionalNullable(value.error_code, isErrorCode) &&
  isOptionalNullable(value.outcome, isToolOutcome) &&
  isOptionalNullable(value.side_effect_state, isSideEffectState) &&
  isOptionalNullable(value.started_at, isTimestamp) &&
  isOptionalNullable(value.completed_at, isTimestamp) &&
  typeof value.can_cancel === 'boolean';

export const isToolDraftSummary = (value: unknown): boolean =>
  recordWithOptional(
    value,
    ['entity_revision', 'kind', 'status', 'tool_draft_id', 'updated_at'],
    ['safe_error'],
  ) &&
  isAuraId(value.tool_draft_id, 'tdr') &&
  isOneOf(value.kind, ['content_mutation', 'package_deletion']) &&
  isToolDraftStatus(value.status) &&
  isRevision(value.entity_revision) &&
  isTimestamp(value.updated_at) &&
  isOptionalNullable(value.safe_error, isToolProjectionError);
