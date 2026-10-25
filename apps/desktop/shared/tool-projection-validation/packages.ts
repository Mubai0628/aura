import {
  isAuraId,
  isCanonicalHash,
  isConformanceStatus,
  isOneOf,
  isOptionalNullable,
  isOptionalNullableString,
  isRevision,
  isSafeUnsigned,
  isTimestamp,
  isToolProjectionError,
} from './common.js';
import { isToolDefinitionSummary } from './definitions.js';
import { recordWithOptional } from './records.js';

/** Tool packages and their versions. */
export const isToolPackageSummary = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [
      'deletion_pending',
      'eligible_tool_count',
      'enabled_tool_count',
      'entity_revision',
      'lifecycle',
      'package_key',
      'tool_count',
      'tool_package_id',
      'trusted',
      'updated_at',
    ],
    ['active_version'],
  ) &&
  isAuraId(value.tool_package_id, 'tpk') &&
  typeof value.package_key === 'string' &&
  isOptionalNullableString(value.active_version) &&
  isOneOf(value.lifecycle, ['installed', 'corrupted', 'deleted']) &&
  typeof value.deletion_pending === 'boolean' &&
  typeof value.trusted === 'boolean' &&
  isSafeUnsigned(value.tool_count) &&
  isSafeUnsigned(value.enabled_tool_count) &&
  isSafeUnsigned(value.eligible_tool_count) &&
  isRevision(value.entity_revision) &&
  isTimestamp(value.updated_at);

export const isToolPackageProjection = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [
      'created_at',
      'deletion_pending',
      'entity_revision',
      'lifecycle',
      'package_key',
      'tool_package_id',
      'tools',
      'updated_at',
      'versions',
    ],
    ['active_security_fingerprint', 'active_version', 'safe_error', 'trusted_security_fingerprint'],
  ) &&
  isAuraId(value.tool_package_id, 'tpk') &&
  typeof value.package_key === 'string' &&
  isOneOf(value.lifecycle, ['installed', 'corrupted', 'deleted']) &&
  typeof value.deletion_pending === 'boolean' &&
  isOptionalNullableString(value.active_version) &&
  isOptionalNullable(value.active_security_fingerprint, (candidate) =>
    isCanonicalHash(candidate, 'blake3'),
  ) &&
  isOptionalNullable(value.trusted_security_fingerprint, (candidate) =>
    isCanonicalHash(candidate, 'blake3'),
  ) &&
  Array.isArray(value.versions) &&
  value.versions.every(isToolPackageVersionSummary) &&
  Array.isArray(value.tools) &&
  value.tools.every(isToolDefinitionSummary) &&
  isRevision(value.entity_revision) &&
  isTimestamp(value.created_at) &&
  isTimestamp(value.updated_at) &&
  isOptionalNullable(value.safe_error, isToolProjectionError);

export const isToolPackageVersionSummary = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [
      'conformance_run_ids',
      'conformance_status',
      'installed_at',
      'package_fingerprint',
      'package_security_fingerprint',
      'package_version',
      'payload_available',
    ],
    ['payload_gc_eligible_at'],
  ) &&
  typeof value.package_version === 'string' &&
  isCanonicalHash(value.package_fingerprint, 'sha256') &&
  isCanonicalHash(value.package_security_fingerprint, 'blake3') &&
  Array.isArray(value.conformance_run_ids) &&
  value.conformance_run_ids.every((candidate) => isAuraId(candidate, 'tcr')) &&
  isConformanceStatus(value.conformance_status) &&
  isTimestamp(value.installed_at) &&
  typeof value.payload_available === 'boolean' &&
  isOptionalNullable(value.payload_gc_eligible_at, isTimestamp);
