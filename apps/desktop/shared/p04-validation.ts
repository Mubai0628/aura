import type {
  ArtifactExportRequest,
  PrepareQuarantineReleaseInput,
  ReleaseQuarantineInput,
  SecurityProjectionChange,
} from '@aura/contracts/generated';
import type { P04MainCommand, P04MainParams } from './generated/routes.js';
import { mainRoute } from './generated/routes.js';
import { validateSessionPermissionProjection } from './generated/agent-validator.js';
export type {
  P04CoreCommand,
  P04CoreParams,
  P04CoreResult,
  P04MainCommand,
  P04MainParams,
  P04MainResult,
} from './generated/routes.js';

import { isExpectedState, isSecretNonce, isWorkspacePath } from './p03-validation.js';

export interface RendererAttachExternalReadOnlyInput {
  workspace_id: string;
  session_id: string;
}

export const parseSecurityProjection = (value: unknown): SecurityProjectionChange | undefined => {
  if (
    !exactRecord(value, ['payload', 'projection_kind', 'revision', 'runtime_instance_id']) ||
    !isAuraId(value.runtime_instance_id, 'run') ||
    !isRevisionString(value.revision) ||
    typeof value.projection_kind !== 'string' ||
    !isRecord(value.payload)
  ) {
    return undefined;
  }
  if (value.projection_kind === 'session_permission') {
    return isSessionPermissionPayload(value.payload)
      ? (value as unknown as SecurityProjectionChange)
      : undefined;
  }
  if (value.projection_kind === 'pending_approval') {
    return isPendingApprovalPayload(value.payload)
      ? (value as unknown as SecurityProjectionChange)
      : undefined;
  }
  if (value.projection_kind === 'security_runtime') {
    return isSecurityRuntimePayload(value.payload)
      ? (value as unknown as SecurityProjectionChange)
      : undefined;
  }
  return undefined;
};

const DECISIONS = new Set([
  'allow_once',
  'allow_session',
  'allow_workspace',
  'allow_for_current_turn',
  'deny',
]);
const MODES = new Set(['read_only', 'workspace_read_write', 'full_access']);
export const isP04MainCommand = (value: string): value is P04MainCommand =>
  mainRoute(value)?.owner === 'security';

export const isP04MainParams = <Command extends P04MainCommand>(
  command: Command,
  value: unknown,
): value is P04MainParams[Command] => {
  switch (command) {
    case 'permission.set_defaults':
      return (
        exactRecord(value, ['expected_revision', 'mode']) &&
        isRevision(value.expected_revision) &&
        typeof value.mode === 'string' &&
        MODES.has(value.mode)
      );
    case 'permission.get_defaults':
    case 'approval.list_pending':
    case 'security.host_preparation.status':
    case 'security.quarantine.list':
      return exactRecord(value, []);
    case 'security.host_preparation.prepare':
    case 'security.host_preparation.remove':
    case 'security.host_preparation.repair':
      return exactRecord(value, ['expected_revision']) && isRevision(value.expected_revision);
    case 'artifact.export':
      return isArtifactExportRequest(value);
    case 'security.external_grant.attach_read_only':
      return exactRecord(value, ['session_id', 'workspace_id']) && isWorkspaceSession(value);
    case 'security.external_grant.derive_workspace':
      return (
        exactRecord(value, ['permission_grant_id', 'session_id', 'workspace_id']) &&
        isWorkspaceSession(value) &&
        isAuraId(value.permission_grant_id, 'pgr')
      );
    case 'security.external_grant.list':
      return exactRecord(value, ['session_id']) && isAuraId(value.session_id, 'ses');
    case 'security.external_grant.revalidate':
    case 'security.external_grant.revoke':
      return (
        exactRecord(value, ['external_grant_id', 'session_id']) &&
        isAuraId(value.external_grant_id, 'xgr') &&
        isAuraId(value.session_id, 'ses')
      );
    case 'security.quarantine.prepare_release':
      return isPrepareQuarantineRelease(value);
    case 'security.quarantine.release':
      return isReleaseQuarantine(value);
    case 'security.quarantine.issue_discard_confirmation':
      return exactRecord(value, ['quarantine_id']) && isAuraId(value.quarantine_id, 'qnr');
    case 'security.quarantine.discard':
      return (
        exactRecord(value, ['confirmation_nonce', 'quarantine_id']) &&
        isAuraId(value.quarantine_id, 'qnr') &&
        isSecretNonce(value.confirmation_nonce)
      );
    case 'permission.get_session_state':
      return exactRecord(value, ['session_id']) && isAuraId(value.session_id, 'ses');
    case 'permission.set_session_mode':
      return (
        exactRecord(value, ['expected_revision', 'mode', 'session_id']) &&
        isAuraId(value.session_id, 'ses') &&
        isRevision(value.expected_revision) &&
        typeof value.mode === 'string' &&
        MODES.has(value.mode)
      );
    case 'permission.list_grants':
      return (
        exactRecord(value, ['session_id', 'workspace_id']) &&
        isAuraId(value.workspace_id, 'wsp') &&
        (value.session_id === null || isAuraId(value.session_id, 'ses'))
      );
    case 'permission.revoke_grant':
      return exactRecord(value, ['grant_id']) && isAuraId(value.grant_id, 'pgr');
    case 'security.file_session_consent.list':
    case 'security.memory_session_consent.list':
    case 'security.network_session_consent.list':
    case 'security.native_session_consent.list':
      // `include_inactive` is optional in the contract, so an exact record here
      // would reject the shape the Rust default accepts.
      return (
        isRecord(value) &&
        Object.keys(value).every((key) => key === 'session_id' || key === 'include_inactive') &&
        isAuraId(value.session_id, 'ses') &&
        (value.include_inactive === undefined || typeof value.include_inactive === 'boolean')
      );
    case 'security.file_session_consent.revoke':
    case 'security.memory_session_consent.revoke':
    case 'security.network_session_consent.revoke':
      return (
        exactRecord(value, ['expected_revision', 'grant_id', 'session_id']) &&
        isAuraId(value.session_id, 'ses') &&
        isAuraId(value.grant_id, 'pgr') &&
        isRevisionString(value.expected_revision)
      );
    case 'security.native_session_consent.revoke':
      return (
        exactRecord(value, ['correlation_id', 'expected_revision', 'grant_id', 'session_id']) &&
        isAuraId(value.session_id, 'ses') &&
        isAuraId(value.grant_id, 'pgr') &&
        isRevisionString(value.expected_revision) &&
        isAuraId(value.correlation_id, 'cor')
      );
    case 'approval.get_details':
      return exactRecord(value, ['approval_id']) && isAuraId(value.approval_id, 'apr');
    case 'approval.resolve':
      return (
        exactRecord(value, ['approval_id', 'decision', 'expected_request_revision']) &&
        isAuraId(value.approval_id, 'apr') &&
        isRevision(value.expected_request_revision) &&
        typeof value.decision === 'string' &&
        DECISIONS.has(value.decision)
      );
  }
};

const isWorkspaceSession = (value: Record<string, unknown>): boolean =>
  isAuraId(value.workspace_id, 'wsp') && isAuraId(value.session_id, 'ses');

const isArtifactExportRequest = (value: unknown): value is ArtifactExportRequest =>
  exactRecord(value, ['artifact_id', 'expected', 'session_id', 'target', 'workspace_id']) &&
  isAuraId(value.artifact_id, 'art') &&
  isWorkspaceSession(value) &&
  isArtifactTarget(value.target) &&
  isExpectedState(value.expected) &&
  isRecord(value.expected) &&
  value.expected.expected_kind !== 'directory';

const isArtifactTarget = (value: unknown): boolean => {
  if (!isRecord(value) || typeof value.target_kind !== 'string') return false;
  if (value.target_kind === 'workspace') {
    return exactRecord(value, ['path', 'target_kind']) && isWorkspacePath(value.path);
  }
  return (
    value.target_kind === 'external_grant' &&
    exactRecord(value, ['external_grant_id', 'relative_path', 'target_kind']) &&
    isAuraId(value.external_grant_id, 'xgr') &&
    isWorkspacePath(value.relative_path)
  );
};

const isQuarantineReleaseTarget = (value: unknown): boolean => isArtifactTarget(value);

const isPrepareQuarantineRelease = (value: unknown): value is PrepareQuarantineReleaseInput =>
  exactRecord(value, ['quarantine_id', 'session_id', 'target', 'workspace_id']) &&
  isAuraId(value.quarantine_id, 'qnr') &&
  isWorkspaceSession(value) &&
  isQuarantineReleaseTarget(value.target);

const isReleaseQuarantine = (value: unknown): value is ReleaseQuarantineInput =>
  exactRecord(value, ['approval_id', 'quarantine_id', 'session_id', 'target', 'workspace_id']) &&
  isAuraId(value.approval_id, 'apr') &&
  isAuraId(value.quarantine_id, 'qnr') &&
  isWorkspaceSession(value) &&
  isQuarantineReleaseTarget(value.target);

const exactRecord = (value: unknown, keys: readonly string[]): value is Record<string, unknown> => {
  if (!isRecord(value)) return false;
  const actual = Object.keys(value).sort();
  const expected = [...keys].sort();
  return actual.length === expected.length && actual.every((key, index) => key === expected[index]);
};

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

const isRevision = (value: unknown): value is number =>
  typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;

const isRevisionString = (value: unknown): value is string =>
  typeof value === 'string' && /^(?:0|[1-9][0-9]*)$/u.test(value);

const isAuraId = (value: unknown, prefix: string): value is string =>
  typeof value === 'string' &&
  new RegExp(`^${prefix}_[0-7][0-9A-HJKMNP-TV-Z]{25}$`, 'u').test(value);

const isSessionPermissionPayload = (value: Record<string, unknown>): boolean =>
  // Current Core must publish the actual transition. The serde default exists
  // for stored historical records, not for inferring a live mode-switch result.
  Object.hasOwn(value, 'transition') && validateSessionPermissionProjection(value);

const isPendingApprovalPayload = (value: Record<string, unknown>): boolean =>
  exactRecord(value, ['approvals', 'revision']) &&
  Array.isArray(value.approvals) &&
  value.approvals.every(isApprovalView) &&
  isRevision(value.revision);

const isApprovalView = (value: unknown): boolean =>
  exactRecord(value, [
    'actual_operation_summary',
    'allowed_decisions',
    'created_at',
    'id',
    'model_purpose',
    'request_revision',
    'release_descriptor',
    'session_id',
    'risk_flags',
    'source_summary',
    'title',
  ]) &&
  isAuraId(value.id, 'apr') &&
  typeof value.title === 'string' &&
  (value.session_id === null || isAuraId(value.session_id, 'ses')) &&
  (value.model_purpose === null || typeof value.model_purpose === 'string') &&
  typeof value.actual_operation_summary === 'string' &&
  typeof value.source_summary === 'string' &&
  Array.isArray(value.risk_flags) &&
  Array.isArray(value.allowed_decisions) &&
  value.allowed_decisions.every(
    (decision) => typeof decision === 'string' && DECISIONS.has(decision),
  ) &&
  isRevision(value.request_revision) &&
  typeof value.created_at === 'string';

const COMPONENT_STATES = new Set([
  'not_started',
  'starting',
  'ready',
  'degraded',
  'unavailable',
  'crashed',
]);

const isSecurityRuntimePayload = (value: Record<string, unknown>): boolean =>
  exactRecord(value, [
    'egress_proxy_state',
    'elevated_helper_state',
    'host_preparation',
    'quarantine_count',
    'sandbox_available',
    'sandbox_host_state',
  ]) &&
  typeof value.sandbox_available === 'boolean' &&
  isRecord(value.host_preparation) &&
  isHostPreparationSummary(value.host_preparation) &&
  typeof value.sandbox_host_state === 'string' &&
  COMPONENT_STATES.has(value.sandbox_host_state) &&
  typeof value.egress_proxy_state === 'string' &&
  COMPONENT_STATES.has(value.egress_proxy_state) &&
  typeof value.elevated_helper_state === 'string' &&
  COMPONENT_STATES.has(value.elevated_helper_state) &&
  isRevision(value.quarantine_count);

const isHostPreparationSummary = (value: Record<string, unknown>): boolean =>
  exactRecord(value, [
    'appcontainer_available',
    'last_error_code',
    'nested_jobs_available',
    'nul_compatibility_ready',
    'residual_acl_count',
    'residual_appcontainer_profile_count',
    'residual_firewall_rule_count',
    'restart_required',
    'standard_user_token_available',
    'system_drive_compatibility_ready',
  ]) &&
  typeof value.appcontainer_available === 'boolean' &&
  typeof value.nested_jobs_available === 'boolean' &&
  typeof value.standard_user_token_available === 'boolean' &&
  typeof value.system_drive_compatibility_ready === 'boolean' &&
  typeof value.nul_compatibility_ready === 'boolean' &&
  isRevision(value.residual_acl_count) &&
  isRevision(value.residual_firewall_rule_count) &&
  isRevision(value.residual_appcontainer_profile_count) &&
  typeof value.restart_required === 'boolean' &&
  (value.last_error_code === null || typeof value.last_error_code === 'string');
