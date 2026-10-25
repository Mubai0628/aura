import {
  exactRecord,
  recordWithOptional,
  isAuraId,
  isContentHash,
  isRevision,
  isSafeUnsigned,
} from './common.js';
import type { AuraIdPrefix } from './common.js';
import { isCanonicalHash } from '../tool-projection-validation/common.js';

const isOptionalNullable = (value: unknown, validator: (candidate: unknown) => boolean): boolean =>
  value === undefined || value === null || validator(value);
const isOptionalNullableSafeUnsigned = (value: unknown): boolean =>
  isOptionalNullable(value, isSafeUnsigned);
const isOptionalNullableAuraId = (value: unknown, prefix: AuraIdPrefix): boolean =>
  isOptionalNullable(value, (candidate) => isAuraId(candidate, prefix));
export const isPromptInspectorProjection = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [
      'active_tool_call_ids',
      'attachments',
      'cache_usage',
      'compaction',
      'revision',
      'sections',
      'session_id',
      'skills',
      'tools',
    ],
    [
      'active_agent_run_id',
      'active_model_step_id',
      'active_task_lineage',
      'activity',
      'budget',
      'context_snapshot_id',
      'context_usage',
      'model_egress',
      'model_profile_id',
      'overflow',
      'prompt_bundle_snapshot_id',
      'reduction',
    ],
  ) &&
  isAuraId(value.session_id, 'ses') &&
  isRevision(value.revision) &&
  isOptionalNullable(value.activity, isPromptInspectorActivity) &&
  isOptionalNullableAuraId(value.active_agent_run_id, 'agt') &&
  isOptionalNullableAuraId(value.active_model_step_id, 'stp') &&
  isOptionalNullableAuraId(value.context_snapshot_id, 'cps') &&
  isOptionalNullableAuraId(value.model_profile_id, 'mdl') &&
  isOptionalNullableAuraId(value.prompt_bundle_snapshot_id, 'pbs') &&
  Array.isArray(value.active_tool_call_ids) &&
  value.active_tool_call_ids.every((id) => isAuraId(id, 'tcl')) &&
  Array.isArray(value.attachments) &&
  value.attachments.every(isPromptInspectorAttachment) &&
  Array.isArray(value.sections) &&
  value.sections.every(isPromptInspectorSection) &&
  Array.isArray(value.skills) &&
  value.skills.every(isPromptInspectorSkill) &&
  Array.isArray(value.tools) &&
  value.tools.every(isPromptInspectorTool) &&
  isPromptInspectorCacheUsage(value.cache_usage) &&
  isPromptInspectorCompaction(value.compaction) &&
  isOptionalNullable(value.active_task_lineage, isPromptInspectorTaskLineage) &&
  isOptionalNullable(value.budget, isPromptInspectorBudget) &&
  isOptionalNullable(value.context_usage, isContextUsageSummary) &&
  isOptionalNullable(value.model_egress, isPromptInspectorModelEgress) &&
  isOptionalNullable(value.overflow, isContextOverflowDiagnostic) &&
  isOptionalNullable(value.reduction, isPromptInspectorReduction);

/** What one step's preparation gave back before its request went out. */
const isPromptInspectorReduction = (value: unknown): boolean =>
  exactRecord(value, ['cleared_tool_results', 'compacted', 'input_tokens_before']) &&
  isSafeUnsigned(value.cleared_tool_results) &&
  typeof value.compacted === 'boolean' &&
  isSafeUnsigned(value.input_tokens_before);

const isPromptInspectorActivity = (value: unknown): boolean =>
  exactRecord(value, [
    'body_hash',
    'coverage',
    'fragment_present',
    'omitted',
    'revision',
    'rows_in_scope',
    'rows_shown',
    'runtime_instance_id',
    'sampled_at',
    'truncated',
  ]) &&
  isContentHash(value.body_hash) &&
  ['complete', 'partial', 'unavailable'].includes(String(value.coverage)) &&
  typeof value.fragment_present === 'boolean' &&
  isSafeUnsigned(value.omitted) &&
  isRevision(value.revision) &&
  isSafeUnsigned(value.rows_in_scope) &&
  isSafeUnsigned(value.rows_shown) &&
  typeof value.runtime_instance_id === 'string' &&
  /^run_[0-7][0-9A-HJKMNP-TV-Z]{25}$/u.test(value.runtime_instance_id) &&
  typeof value.sampled_at === 'string' &&
  !Number.isNaN(Date.parse(value.sampled_at)) &&
  typeof value.truncated === 'boolean';

const isContextUsageSummary = (value: unknown): boolean => {
  if (
    !exactRecord(value, [
      'estimated',
      'runtime_effective_window_tokens',
      'segments',
      'used_input_tokens',
    ]) ||
    typeof value.estimated !== 'boolean' ||
    !isSafeUnsigned(value.runtime_effective_window_tokens) ||
    !isSafeUnsigned(value.used_input_tokens) ||
    !Array.isArray(value.segments)
  ) {
    return false;
  }
  const expectedKinds = [
    'system_and_rules',
    'tools_and_results',
    'conversation',
    'background_and_compaction',
  ];
  if (value.segments.length !== expectedKinds.length) return false;
  let total = 0;
  for (const [index, segment] of value.segments.entries()) {
    if (
      !exactRecord(segment, ['estimated_tokens', 'kind', 'label']) ||
      segment.kind !== expectedKinds[index] ||
      typeof segment.label !== 'string' ||
      !isSafeUnsigned(segment.estimated_tokens)
    ) {
      return false;
    }
    total += segment.estimated_tokens;
    if (!Number.isSafeInteger(total)) return false;
  }
  return total === value.used_input_tokens;
};

const isPromptInspectorBudget = (value: unknown): boolean =>
  exactRecord(value, [
    'estimated_input_tokens',
    'estimator_id',
    'estimator_version',
    'exact',
    'output_reserve_tokens',
    'policy',
    'request_input_budget_tokens',
    'runtime_effective_window_tokens',
    'safety_reserve_tokens',
  ]) &&
  isSafeUnsigned(value.estimated_input_tokens) &&
  typeof value.estimator_id === 'string' &&
  isSafeUnsigned(value.estimator_version) &&
  typeof value.exact === 'boolean' &&
  isSafeUnsigned(value.output_reserve_tokens) &&
  typeof value.policy === 'string' &&
  isSafeUnsigned(value.request_input_budget_tokens) &&
  isSafeUnsigned(value.runtime_effective_window_tokens) &&
  isSafeUnsigned(value.safety_reserve_tokens);

const isPromptInspectorCacheUsage = (value: unknown): boolean =>
  recordWithOptional(
    value,
    ['stable_prefix_estimate_tokens'],
    ['actual_cached_input_tokens', 'actual_input_tokens'],
  ) &&
  isSafeUnsigned(value.stable_prefix_estimate_tokens) &&
  isOptionalNullableSafeUnsigned(value.actual_cached_input_tokens) &&
  isOptionalNullableSafeUnsigned(value.actual_input_tokens);

const isPromptInspectorCompaction = (value: unknown): boolean =>
  recordWithOptional(
    value,
    ['retained_recent_tokens', 'retention_exceptions', 'state'],
    ['source_context_snapshot_id'],
  ) &&
  isSafeUnsigned(value.retained_recent_tokens) &&
  Array.isArray(value.retention_exceptions) &&
  value.retention_exceptions.every(
    (exception) =>
      exception === 'oversized_atomic_segment' ||
      exception === 'insufficient_eligible_recent_history',
  ) &&
  isOptionalNullableAuraId(value.source_context_snapshot_id, 'cps') &&
  (value.state === 'not_required' ||
    value.state === 'required' ||
    value.state === 'running' ||
    value.state === 'completed' ||
    value.state === 'failed' ||
    value.state === 'cancelled');

const isPromptInspectorAttachment = (value: unknown): boolean =>
  recordWithOptional(
    value,
    ['byte_length', 'content_hash', 'display_name', 'media_type', 'resolved'],
    ['estimated_tokens'],
  ) &&
  isSafeUnsigned(value.byte_length) &&
  isContentHash(value.content_hash) &&
  typeof value.display_name === 'string' &&
  typeof value.media_type === 'string' &&
  typeof value.resolved === 'boolean' &&
  isOptionalNullableSafeUnsigned(value.estimated_tokens);

const isPromptInspectorSection = (value: unknown): boolean =>
  exactRecord(value, [
    'byte_count',
    'changed_from_previous_step',
    'character_count',
    'compressibility',
    'content_hash',
    'estimated_tokens',
    'is_empty_marker',
    'kind',
    'physical_ordinal',
    'placement',
    'privacy_class',
    'source_kinds',
  ]) &&
  isSafeUnsigned(value.byte_count) &&
  typeof value.changed_from_previous_step === 'boolean' &&
  isSafeUnsigned(value.character_count) &&
  isContextCompressibility(value.compressibility) &&
  isContentHash(value.content_hash) &&
  isSafeUnsigned(value.estimated_tokens) &&
  typeof value.is_empty_marker === 'boolean' &&
  isContextSectionKind(value.kind) &&
  isSafeUnsigned(value.physical_ordinal) &&
  isPromptInspectorSectionPlacement(value.placement) &&
  isContextPrivacyClass(value.privacy_class) &&
  Array.isArray(value.source_kinds) &&
  value.source_kinds.every((source) => typeof source === 'string');

const isPromptInspectorSkill = (value: unknown): boolean =>
  exactRecord(value, ['content_hash', 'estimated_tokens', 'skill_instance_id']) &&
  isContentHash(value.content_hash) &&
  isSafeUnsigned(value.estimated_tokens) &&
  isAuraId(value.skill_instance_id, 'skl');

const isPromptInspectorTool = (value: unknown): boolean =>
  exactRecord(value, ['canonical_name', 'estimated_tokens', 'schema_hash', 'tool_id']) &&
  typeof value.canonical_name === 'string' &&
  isSafeUnsigned(value.estimated_tokens) &&
  isCanonicalHash(value.schema_hash, 'sha256') &&
  isAuraId(value.tool_id, 'tol');

const isPromptInspectorTaskLineage = (value: unknown): boolean =>
  exactRecord(value, [
    'completion_gate',
    'completion_policy',
    'lineage_id',
    'preview_characters',
    'preview_estimated_tokens',
    'revision',
    'source_count',
    'source_recheck_required',
    'state',
  ]) &&
  typeof value.completion_gate === 'string' &&
  typeof value.completion_policy === 'string' &&
  isAuraId(value.lineage_id, 'lin') &&
  isSafeUnsigned(value.preview_characters) &&
  isSafeUnsigned(value.preview_estimated_tokens) &&
  isRevision(value.revision) &&
  isSafeUnsigned(value.source_count) &&
  typeof value.source_recheck_required === 'boolean' &&
  typeof value.state === 'string';

const isPromptInspectorModelEgress = (value: unknown): boolean =>
  exactRecord(value, [
    'attachments_resolved',
    'classes',
    'manifest_hash',
    'policy_version',
    'unresolved_source_kinds',
  ]) &&
  typeof value.attachments_resolved === 'boolean' &&
  Array.isArray(value.classes) &&
  value.classes.every(isPromptInspectorEgressClass) &&
  isContentHash(value.manifest_hash) &&
  isSafeUnsigned(value.policy_version) &&
  Array.isArray(value.unresolved_source_kinds) &&
  value.unresolved_source_kinds.every((source) => typeof source === 'string');

const isPromptInspectorEgressClass = (value: unknown): boolean =>
  exactRecord(value, ['byte_length', 'data_class', 'disposition', 'item_count']) &&
  isSafeUnsigned(value.byte_length) &&
  isNetworkDataClass(value.data_class) &&
  (value.disposition === 'allowed_by_model_channel' ||
    value.disposition === 'requires_source_egress_grant' ||
    value.disposition === 'always_denied') &&
  isSafeUnsigned(value.item_count);

const isContextOverflowDiagnostic = (value: unknown): boolean =>
  exactRecord(value, [
    'compressible_history_tokens',
    'error_code',
    'estimated_input_tokens',
    'non_compressible_tokens',
    'overflow_tokens',
    'remediation',
    'request_input_budget_tokens',
    'user_reducible_tokens',
  ]) &&
  isSafeUnsigned(value.compressible_history_tokens) &&
  typeof value.error_code === 'string' &&
  isSafeUnsigned(value.estimated_input_tokens) &&
  isSafeUnsigned(value.non_compressible_tokens) &&
  isSafeUnsigned(value.overflow_tokens) &&
  Array.isArray(value.remediation) &&
  value.remediation.every((item) => typeof item === 'string') &&
  isSafeUnsigned(value.request_input_budget_tokens) &&
  isSafeUnsigned(value.user_reducible_tokens);

const isContextCompressibility = (value: unknown): boolean =>
  value === 'never_compress' ||
  value === 'compressible_history' ||
  value === 'compressible_sealed_active_segment' ||
  value === 'user_reducible_only';

const isContextSectionKind = (value: unknown): boolean =>
  typeof value === 'string' &&
  [
    'core_system_prompt',
    'permission_and_security',
    'loaded_tool_schemas',
    'workspace_agents_instructions',
    'historical_snapshot_summary',
    'agent_mode_prompt',
    'hook_injections',
    'current_task_state',
    'recent_history',
    'file_references_and_pending_attachments',
    'current_turn_working_set',
    'steered_user_additions',
  ].includes(value);

const isPromptInspectorSectionPlacement = (value: unknown): boolean =>
  value === 'instruction_envelope' ||
  value === 'conversation' ||
  value === 'current_user_part' ||
  value === 'attachment';

const isContextPrivacyClass = (value: unknown): boolean =>
  value === 'model_visible' || value === 'user_only' || value === 'internal_only';

const isNetworkDataClass = (value: unknown): boolean =>
  typeof value === 'string' &&
  [
    'public_query',
    'public_content',
    'workspace_metadata',
    'workspace_source',
    'user_file_content',
    'personal_data',
    'credential_material',
    'security_input',
    'password',
    'otp',
    'cookie',
    'private_key',
    'suspected_secret',
    'diagnostic_data',
    'unknown',
  ].includes(value);
