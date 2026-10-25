import {
  isAuraId,
  isCanonicalHash,
  isJsonValue,
  isOneOf,
  isOptionalNullable,
  isOptionalNullableString,
  isPositiveRevision,
  isRevision,
  isSafeUnsigned,
  isToolProjectionError,
  isToolRuntimeKey,
  isToolVersion,
} from './common.js';
import { isToolCredentialPlacement } from './credentials.js';
import { exactRecord, isRecord, recordWithOptional } from './records.js';

/** Tool definitions: versions, instances, configuration and source selection. */
export const isToolDefinitionSummary = (value: unknown): boolean =>
  exactRecord(value, [
    'audience',
    'canonical_name',
    'display_name',
    'entity_revision',
    'instances',
    'model_summary',
    'tool_id',
    'tool_version',
  ]) &&
  isAuraId(value.tool_id, 'tol') &&
  isToolVersion(value.tool_version) &&
  typeof value.canonical_name === 'string' &&
  typeof value.display_name === 'string' &&
  typeof value.model_summary === 'string' &&
  isOneOf(value.audience, [
    'child_runtime_required',
    'subagent_configurable',
    'parent_agent_only',
  ]) &&
  isRevision(value.entity_revision) &&
  Array.isArray(value.instances) &&
  value.instances.every(isToolDefinitionInstance);

export const isToolDefinitionProjection = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [
      'canonical_name',
      'current_selection',
      'display_name',
      'entity_revision',
      'tool_id',
      'versions',
    ],
    ['safe_error'],
  ) &&
  isAuraId(value.tool_id, 'tol') &&
  typeof value.canonical_name === 'string' &&
  typeof value.display_name === 'string' &&
  Array.isArray(value.versions) &&
  value.versions.every(isToolDefinitionVersion) &&
  isToolDefinitionCurrentSelection(value.current_selection) &&
  isRevision(value.entity_revision) &&
  isOptionalNullable(value.safe_error, isToolProjectionError);

export const isToolDefinitionVersion = (value: unknown): boolean =>
  exactRecord(value, [
    'audience',
    'capability_count',
    'execution_semantics',
    'instance_configuration_schema',
    'model_description',
    'model_summary',
    'instances',
    'payload_available',
    'resource_claim_count',
    'result_contract',
    'schema',
    'source',
    'tool_version',
  ]) &&
  isToolVersion(value.tool_version) &&
  typeof value.model_description === 'string' &&
  typeof value.model_summary === 'string' &&
  isRecord(value.schema) &&
  isRecord(value.result_contract) &&
  isOneOf(value.source, [
    'built_in',
    'user_created',
    'model_created',
    'imported',
    'mcp',
    'plugin',
  ]) &&
  isOneOf(value.audience, [
    'child_runtime_required',
    'subagent_configurable',
    'parent_agent_only',
  ]) &&
  isRecord(value.execution_semantics) &&
  (value.instance_configuration_schema === null ||
    isCanonicalJsonSchemaDocument(value.instance_configuration_schema)) &&
  isSafeUnsigned(value.capability_count) &&
  isSafeUnsigned(value.resource_claim_count) &&
  typeof value.payload_available === 'boolean' &&
  Array.isArray(value.instances) &&
  value.instances.every(isToolDefinitionInstance);

export const isToolDefinitionInstance = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [
      'enabled',
      'entity_revision',
      'ready',
      'runtime_health',
      'tool_instance_id',
      'tool_version',
      'trusted',
    ],
    ['configuration', 'credential_slot_binding'],
  ) &&
  isAuraId(value.tool_instance_id, 'tin') &&
  isToolVersion(value.tool_version) &&
  typeof value.enabled === 'boolean' &&
  typeof value.trusted === 'boolean' &&
  isOneOf(value.runtime_health, ['healthy', 'unavailable', 'corrupted']) &&
  typeof value.ready === 'boolean' &&
  isRevision(value.entity_revision) &&
  isOptionalNullable(value.configuration, isToolInstanceConfiguration) &&
  isOptionalNullable(value.credential_slot_binding, isToolCredentialSlotBinding);

export const isToolInstanceConfiguration = (value: unknown): boolean =>
  exactRecord(value, ['configuration_hash', 'configuration_revision', 'safe_non_secret_value']) &&
  isCanonicalHash(value.configuration_hash, 'sha256') &&
  isRevision(value.configuration_revision) &&
  isJsonValue(value.safe_non_secret_value);

export const isCanonicalJsonSchemaDocument = (value: unknown): boolean =>
  exactRecord(value, ['document', 'schema_dialect', 'schema_hash', 'schema_policy_version']) &&
  value.schema_dialect === 'json_schema_draft_2020_12' &&
  value.schema_policy_version === 1 &&
  isRecord(value.document) &&
  isJsonValue(value.document) &&
  isCanonicalHash(value.schema_hash, 'sha256');

export const isToolCredentialSlotBinding = (value: unknown): boolean =>
  recordWithOptional(
    value,
    ['bound_credential', 'placement', 'slot_key'],
    ['rebind_required_reason'],
  ) &&
  isOptionalNullableString(value.rebind_required_reason) &&
  isToolRuntimeKey(value.slot_key) &&
  isToolCredentialPlacement(value.placement) &&
  isOptionalNullable(
    value.bound_credential,
    (candidate) =>
      exactRecord(candidate, ['binding_revision', 'credential_id', 'credential_revision']) &&
      isRevision(candidate.binding_revision) &&
      isAuraId(candidate.credential_id, 'crd') &&
      isPositiveRevision(candidate.credential_revision),
  );

export const isToolDefinitionCurrentSelection = (value: unknown): boolean => {
  if (!isRecord(value) || typeof value.type !== 'string') return false;
  if (value.type === 'resolved') {
    return (
      exactRecord(value, ['selection_basis', 'source_selection_ref', 'type']) &&
      isToolDefinitionSourceSelectionRef(value.source_selection_ref) &&
      isOneOf(value.selection_basis, [
        'signed_builtin_release',
        'active_package_version',
        'external_source_authority',
        'explicit_instance_selection',
      ])
    );
  }
  if (value.type === 'unavailable') {
    return (
      exactRecord(value, ['reason', 'type']) &&
      isOneOf(value.reason, [
        'release_definition_unavailable',
        'active_package_version_unavailable',
        'external_source_authority_unavailable',
        'no_ready_instance',
        'explicit_selection_unavailable',
      ])
    );
  }
  if (value.type === 'ambiguous') {
    return (
      exactRecord(value, ['candidates', 'type']) &&
      Array.isArray(value.candidates) &&
      value.candidates.length > 1 &&
      value.candidates.every(isToolDefinitionSourceSelectionRef)
    );
  }
  return false;
};

export const isToolDefinitionSourceSelectionRef = (value: unknown): boolean => {
  if (!isRecord(value) || typeof value.source !== 'string') return false;
  if (value.source === 'signed_builtin_release') {
    return (
      exactRecord(value, [
        'definition_fingerprint',
        'signed_builtin_release_hash',
        'source',
        'source_contract_fingerprint',
        'tool_id',
        'tool_instance_id',
        'tool_instance_revision',
        'tool_version',
      ]) &&
      isSourceSelectionToolIdentity(value) &&
      isCanonicalHash(value.definition_fingerprint, 'sha256') &&
      isCanonicalHash(value.signed_builtin_release_hash, 'sha256') &&
      isCanonicalHash(value.source_contract_fingerprint, 'sha256')
    );
  }
  if (value.source === 'active_package_version') {
    return (
      exactRecord(value, [
        'definition_fingerprint',
        'package_fingerprint',
        'package_revision',
        'package_security_fingerprint',
        'package_version',
        'source',
        'tool_id',
        'tool_instance_id',
        'tool_instance_revision',
        'tool_package_id',
        'tool_version',
      ]) &&
      isSourceSelectionToolIdentity(value) &&
      isAuraId(value.tool_package_id, 'tpk') &&
      typeof value.package_version === 'string' &&
      isRevision(value.package_revision) &&
      isCanonicalHash(value.definition_fingerprint, 'sha256') &&
      isCanonicalHash(value.package_fingerprint, 'sha256') &&
      isCanonicalHash(value.package_security_fingerprint, 'blake3')
    );
  }
  if (value.source === 'external_source_authority') {
    return (
      exactRecord(value, [
        'authority',
        'definition_fingerprint',
        'source',
        'tool_id',
        'tool_instance_id',
        'tool_instance_revision',
        'tool_version',
      ]) &&
      isSourceSelectionToolIdentity(value) &&
      isCanonicalHash(value.definition_fingerprint, 'sha256') &&
      isToolDefinitionExternalSourceAuthority(value.authority)
    );
  }
  return false;
};

export const isSourceSelectionToolIdentity = (value: Record<string, unknown>): boolean =>
  isAuraId(value.tool_id, 'tol') &&
  isToolVersion(value.tool_version) &&
  isAuraId(value.tool_instance_id, 'tin') &&
  isRevision(value.tool_instance_revision);

export const isToolDefinitionExternalSourceAuthority = (value: unknown): boolean => {
  if (!isRecord(value) || typeof value.source_kind !== 'string') return false;
  if (value.source_kind === 'mcp') {
    return (
      exactRecord(value, ['authority', 'source_kind']) &&
      exactRecord(value.authority, [
        'authority_revision',
        'normalized_mcp_tool_contract_fingerprint',
        'source_contract_key',
      ]) &&
      typeof value.authority.source_contract_key === 'string' &&
      value.authority.source_contract_key.length > 0 &&
      isRevision(value.authority.authority_revision) &&
      isCanonicalHash(value.authority.normalized_mcp_tool_contract_fingerprint, 'sha256')
    );
  }
  if (value.source_kind === 'plugin') {
    return (
      exactRecord(value, ['authority', 'source_kind']) &&
      exactRecord(value.authority, [
        'authority_revision',
        'normalized_plugin_tool_contract_fingerprint',
        'source_contract_key',
      ]) &&
      typeof value.authority.source_contract_key === 'string' &&
      value.authority.source_contract_key.length > 0 &&
      isRevision(value.authority.authority_revision) &&
      isCanonicalHash(value.authority.normalized_plugin_tool_contract_fingerprint, 'sha256')
    );
  }
  return false;
};
