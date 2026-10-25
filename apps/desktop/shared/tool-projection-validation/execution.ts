import { isToolCallActivity } from './activity.js';
import {
  isAuraId,
  isCanonicalHash,
  isConformanceStatus,
  isOneOf,
  isOptionalNullable,
  isOptionalNullableRecord,
  isOptionalNullableString,
  isRevision,
  isSafeUnsigned,
  isSideEffectState,
  isTimestamp,
  isToolBatchStatus,
  isToolCallStatus,
  isToolDraftStatus,
  isToolOutcome,
  isToolProjectionError,
} from './common.js';
import { isRecord, recordWithOptional } from './records.js';
import {
  isRegistrySnapshotRef,
  isToolLongOperationRef,
  isToolProgress,
  isUserReadableArtifact,
} from './snapshots.js';

/** Drafts, conformance, batches, calls, attempts and long operations. */
export const isToolDraftProjection = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [
      'can_cancel',
      'created_at',
      'current_draft_revision',
      'draft_hash',
      'entity_revision',
      'kind',
      'status',
      'tool_draft_id',
      'updated_at',
      'user_readable_artifacts',
    ],
    [
      'approval',
      'conformance_run_id',
      'package_delete_approval',
      'package_delete_snapshot',
      'preview',
      'safe_error',
    ],
  ) &&
  isAuraId(value.tool_draft_id, 'tdr') &&
  isOneOf(value.kind, ['content_mutation', 'package_deletion']) &&
  isToolDraftStatus(value.status) &&
  isRevision(value.current_draft_revision) &&
  isCanonicalHash(value.draft_hash, 'sha256') &&
  isRevision(value.entity_revision) &&
  isTimestamp(value.created_at) &&
  isTimestamp(value.updated_at) &&
  isOptionalNullable(value.conformance_run_id, (candidate) => isAuraId(candidate, 'tcr')) &&
  isOptionalNullableRecord(value.preview) &&
  isOptionalNullableRecord(value.approval) &&
  isOptionalNullableRecord(value.package_delete_snapshot) &&
  isOptionalNullableRecord(value.package_delete_approval) &&
  isOptionalNullable(value.safe_error, isToolProjectionError) &&
  Array.isArray(value.user_readable_artifacts) &&
  value.user_readable_artifacts.every(isUserReadableArtifact) &&
  typeof value.can_cancel === 'boolean';

export const isToolConformanceProjection = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [
      'can_cancel',
      'entity_revision',
      'package_fingerprint',
      'status',
      'tool_conformance_run_id',
      'user_readable_artifacts',
    ],
    ['completed_at', 'safe_error', 'started_at', 'tool_draft_id'],
  ) &&
  isAuraId(value.tool_conformance_run_id, 'tcr') &&
  isOptionalNullable(value.tool_draft_id, (candidate) => isAuraId(candidate, 'tdr')) &&
  isCanonicalHash(value.package_fingerprint, 'sha256') &&
  isConformanceStatus(value.status) &&
  isRevision(value.entity_revision) &&
  isOptionalNullable(value.started_at, isTimestamp) &&
  isOptionalNullable(value.completed_at, isTimestamp) &&
  isOptionalNullable(value.safe_error, isToolProjectionError) &&
  Array.isArray(value.user_readable_artifacts) &&
  value.user_readable_artifacts.every(isUserReadableArtifact) &&
  typeof value.can_cancel === 'boolean';

export const isToolBatchProjection = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [
      'agent_run_id',
      'batch_content_hash',
      'calls',
      'can_cancel',
      'control_state_hash',
      'control_state_revision',
      'model_step_id',
      'registry_snapshot_ref',
      'session_id',
      'status',
      'tool_batch_id',
      'turn_task_id',
    ],
    ['completed_at', 'safe_error', 'started_at'],
  ) &&
  isAuraId(value.tool_batch_id, 'tbt') &&
  isAuraId(value.session_id, 'ses') &&
  isAuraId(value.turn_task_id, 'trn') &&
  isAuraId(value.agent_run_id, 'agt') &&
  isAuraId(value.model_step_id, 'stp') &&
  isCanonicalHash(value.batch_content_hash, 'sha256') &&
  isRegistrySnapshotRef(value.registry_snapshot_ref) &&
  isRevision(value.control_state_revision) &&
  isCanonicalHash(value.control_state_hash, 'sha256') &&
  isToolBatchStatus(value.status) &&
  Array.isArray(value.calls) &&
  value.calls.every(isToolCallActivity) &&
  isOptionalNullable(value.started_at, isTimestamp) &&
  isOptionalNullable(value.completed_at, isTimestamp) &&
  isOptionalNullable(value.safe_error, isToolProjectionError) &&
  typeof value.can_cancel === 'boolean';

export const isToolCallProjection = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [
      'attempt_ids',
      'can_cancel',
      'definition_ref',
      'ordinal',
      'status',
      'tool_batch_id',
      'tool_call_id',
    ],
    [
      'completed_at',
      'invalid_input_issue',
      'model_visible_result',
      'outcome',
      'purpose',
      'safe_error',
      'side_effect_state',
      'started_at',
      'user_audit_result',
    ],
  ) &&
  isAuraId(value.tool_call_id, 'tcl') &&
  isAuraId(value.tool_batch_id, 'tbt') &&
  isSafeUnsigned(value.ordinal) &&
  isRecord(value.definition_ref) &&
  isOptionalNullableString(value.purpose) &&
  isOptionalNullableString(value.invalid_input_issue) &&
  isToolCallStatus(value.status) &&
  isOptionalNullable(value.outcome, isToolOutcome) &&
  isOptionalNullable(value.side_effect_state, isSideEffectState) &&
  Array.isArray(value.attempt_ids) &&
  value.attempt_ids.every((candidate) => isAuraId(candidate, 'tea')) &&
  isOptionalNullable(value.started_at, isTimestamp) &&
  isOptionalNullable(value.completed_at, isTimestamp) &&
  isOptionalNullable(value.safe_error, isToolProjectionError) &&
  isOptionalNullable(value.model_visible_result, isUserReadableArtifact) &&
  isOptionalNullable(value.user_audit_result, isUserReadableArtifact) &&
  typeof value.can_cancel === 'boolean';

export const isToolExecutionAttemptProjection = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [
      'attempt_number',
      'can_cancel',
      'side_effect_state',
      'started_at',
      'status',
      'tool_call_id',
      'tool_execution_attempt_id',
      'user_readable_artifacts',
    ],
    ['completed_at', 'dispatch_prepared_at', 'progress', 'safe_error'],
  ) &&
  isAuraId(value.tool_execution_attempt_id, 'tea') &&
  isAuraId(value.tool_call_id, 'tcl') &&
  isSafeUnsigned(value.attempt_number) &&
  isOneOf(value.status, [
    'started',
    'dispatch_prepared',
    'running',
    'waiting_external',
    'cancelling',
    'succeeded',
    'failed',
    'cancelled',
    'indeterminate',
  ]) &&
  isSideEffectState(value.side_effect_state) &&
  isOptionalNullable(value.progress, isToolProgress) &&
  isTimestamp(value.started_at) &&
  isOptionalNullable(value.dispatch_prepared_at, isTimestamp) &&
  isOptionalNullable(value.completed_at, isTimestamp) &&
  isOptionalNullable(value.safe_error, isToolProjectionError) &&
  Array.isArray(value.user_readable_artifacts) &&
  value.user_readable_artifacts.every(isUserReadableArtifact) &&
  typeof value.can_cancel === 'boolean';

export const isToolLongOperationProjection = (value: unknown): boolean =>
  recordWithOptional(
    value,
    ['can_cancel', 'operation', 'operational_revision', 'state'],
    ['completed_at', 'safe_error', 'started_at'],
  ) &&
  isToolLongOperationRef(value.operation) &&
  isOneOf(value.state, [
    'queued',
    'running',
    'cancelling',
    'cancelled',
    'succeeded',
    'failed',
    'recovery_required',
  ]) &&
  isRevision(value.operational_revision) &&
  typeof value.can_cancel === 'boolean' &&
  isOptionalNullable(value.safe_error, isToolProjectionError) &&
  isOptionalNullable(value.started_at, isTimestamp) &&
  isOptionalNullable(value.completed_at, isTimestamp);
