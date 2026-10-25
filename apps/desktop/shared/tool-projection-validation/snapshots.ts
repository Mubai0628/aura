import {
  isAuraId,
  isCanonicalHash,
  isOneOf,
  isOptionalNullable,
  isOptionalNullableString,
  isRevision,
  isSafeUnsigned,
  isTimestamp,
  isToolVersion,
} from './common.js';
import { exactRecord, isRecord, recordWithOptional } from './records.js';

/** Snapshot references, artifacts, progress and contract fingerprints. */
export const isUserReadableArtifact = (value: unknown): boolean =>
  exactRecord(value, ['artifact_id', 'byte_length', 'media_type']) &&
  isAuraId(value.artifact_id, 'art') &&
  isSafeUnsigned(value.byte_length) &&
  typeof value.media_type === 'string';

export const isRegistrySnapshotRef = (value: unknown): boolean =>
  exactRecord(value, ['content_hash', 'registry_revision', 'tool_registry_snapshot_id']) &&
  isCanonicalHash(value.content_hash, 'sha256') &&
  isRevision(value.registry_revision) &&
  isAuraId(value.tool_registry_snapshot_id, 'trs');

export const isToolDefinitionSnapshotRef = (value: unknown): boolean =>
  exactRecord(value, ['registry_snapshot_ref', 'tool_id', 'tool_version']) &&
  isAuraId(value.tool_id, 'tol') &&
  isToolVersion(value.tool_version) &&
  isRegistrySnapshotRef(value.registry_snapshot_ref);

export const isEffectiveToolBindingSnapshot = (value: unknown): boolean =>
  exactRecord(value, [
    'definition_fingerprint',
    'exposure',
    'source_contract_fingerprint',
    'source_kind',
    'tool_id',
    'tool_instance_id',
    'tool_version',
  ]) &&
  isAuraId(value.tool_id, 'tol') &&
  isToolVersion(value.tool_version) &&
  isAuraId(value.tool_instance_id, 'tin') &&
  isOneOf(value.source_kind, [
    'built_in',
    'user_created',
    'model_created',
    'imported',
    'mcp',
    'plugin',
  ]) &&
  isOneOf(value.exposure, ['core', 'session_pin', 'workspace_pin', 'ephemeral_load']) &&
  isCanonicalHash(value.definition_fingerprint, 'sha256') &&
  isToolSourceContractFingerprint(value.source_contract_fingerprint, value.source_kind);

export const isToolSourceContractFingerprint = (value: unknown, sourceKind: unknown): boolean => {
  if (!isRecord(value) || value.source_kind !== sourceKind) return false;
  switch (sourceKind) {
    case 'built_in':
      return (
        exactRecord(value, ['signed_builtin_tool_contract_fingerprint', 'source_kind']) &&
        isCanonicalHash(value.signed_builtin_tool_contract_fingerprint, 'sha256')
      );
    case 'user_created':
    case 'model_created':
    case 'imported':
      return (
        exactRecord(value, ['source_kind', 'tool_contract_fingerprint']) &&
        exactRecord(value.tool_contract_fingerprint, [
          'fingerprint',
          'package_runtime_closure_hash',
        ]) &&
        isCanonicalHash(value.tool_contract_fingerprint.fingerprint, 'sha256') &&
        isCanonicalHash(value.tool_contract_fingerprint.package_runtime_closure_hash, 'sha256')
      );
    case 'mcp':
      return (
        exactRecord(value, ['normalized_mcp_tool_contract_fingerprint', 'source_kind']) &&
        isCanonicalHash(value.normalized_mcp_tool_contract_fingerprint, 'sha256')
      );
    case 'plugin':
      return (
        exactRecord(value, ['normalized_plugin_tool_contract_fingerprint', 'source_kind']) &&
        isCanonicalHash(value.normalized_plugin_tool_contract_fingerprint, 'sha256')
      );
    default:
      return false;
  }
};

export const isToolProgress = (value: unknown): boolean =>
  recordWithOptional(value, ['sequence', 'updated_at'], ['message', 'percentage']) &&
  isSafeUnsigned(value.sequence) &&
  isOptionalNullable(
    value.percentage,
    (candidate) => isSafeUnsigned(candidate) && Number(candidate) <= 100,
  ) &&
  isOptionalNullableString(value.message) &&
  isTimestamp(value.updated_at);

export const isToolLongOperationRef = (value: unknown): boolean => {
  if (!isRecord(value) || typeof value.operation_kind !== 'string') return false;
  if (value.operation_kind === 'draft_apply') {
    return (
      exactRecord(value, ['journal_id', 'operation_kind', 'tool_draft_id']) &&
      isAuraId(value.journal_id, 'jnl') &&
      isAuraId(value.tool_draft_id, 'tdr')
    );
  }
  if (value.operation_kind === 'package_delete') {
    return (
      exactRecord(value, ['journal_id', 'operation_kind', 'tool_package_id']) &&
      isAuraId(value.journal_id, 'jnl') &&
      isAuraId(value.tool_package_id, 'tpk')
    );
  }
  return false;
};
