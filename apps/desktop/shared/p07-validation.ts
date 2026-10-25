import type { ToolMutationResultV1 } from '@aura/contracts/generated';
import type {
  P07MainCommand,
  P07MainParams,
  P07ReadCommand,
  P07ReadParams,
} from './generated/routes.js';
import { mainRoute } from './generated/routes.js';
export type {
  P07CoreParams,
  P07CoreResult,
  P07MainCommand,
  P07MainParams,
  P07MainResult,
  P07ReadCommand,
  P07ReadParams,
  P07ReadResult,
} from './generated/routes.js';

export const isP07MainCommand = (value: string): value is P07MainCommand =>
  mainRoute(value)?.owner === 'tool' &&
  !value.startsWith('ui.') &&
  mainRoute(value)?.idempotency !== 'read_only';

export const isP07ReadParams = <Command extends P07ReadCommand>(
  command: Command,
  value: unknown,
): value is P07ReadParams[Command] => {
  switch (command) {
    case 'tools.credential.candidates':
      return (
        recordWithOptional(
          value,
          ['credential_slot_key', 'expected_instance_revision', 'placement', 'tool_instance_id'],
          ['session_id', 'workspace_id'],
        ) &&
        isAuraId(value.tool_instance_id, 'tin') &&
        isRevision(value.expected_instance_revision) &&
        isCredentialSlotKey(value.credential_slot_key) &&
        isToolCredentialPlacement(value.placement) &&
        isOptionalNullable(value.workspace_id, (candidate) => isAuraId(candidate, 'wsp')) &&
        isOptionalNullable(value.session_id, (candidate) => isAuraId(candidate, 'ses')) &&
        !(value.session_id != null && value.workspace_id == null)
      );
  }
};

export const isP07MainParams = <Command extends P07MainCommand>(
  command: Command,
  value: unknown,
): value is P07MainParams[Command] => {
  switch (command) {
    case 'tools.tool.set_enabled':
      return (
        exactRecord(value, ['enabled', 'expected_entity_revision', 'tool_id']) &&
        isAuraId(value.tool_id, 'tol') &&
        isRevision(value.expected_entity_revision) &&
        typeof value.enabled === 'boolean'
      );
    case 'tools.instance.update_configuration':
      return (
        recordWithOptional(
          value,
          ['configuration', 'expected_entity_revision', 'expected_schema_hash', 'tool_instance_id'],
          ['expected_configuration_revision'],
        ) &&
        isAuraId(value.tool_instance_id, 'tin') &&
        isRevision(value.expected_entity_revision) &&
        isOptionalNullableRevision(value.expected_configuration_revision) &&
        isCanonicalHash(value.expected_schema_hash, 'sha256') &&
        isJsonValue(value.configuration)
      );
    case 'tools.instance.bind_credential':
      return (
        recordWithOptional(
          value,
          [
            'credential_id',
            'credential_revision',
            'credential_slot_key',
            'expected_candidate_set_revision',
            'expected_entity_revision',
            'placement',
            'tool_instance_id',
          ],
          ['expected_binding_revision', 'session_id', 'workspace_id'],
        ) &&
        isAuraId(value.tool_instance_id, 'tin') &&
        isAuraId(value.credential_id, 'crd') &&
        isPositiveRevision(value.credential_revision) &&
        isCredentialSlotKey(value.credential_slot_key) &&
        isToolCredentialPlacement(value.placement) &&
        isRevision(value.expected_candidate_set_revision) &&
        isRevision(value.expected_entity_revision) &&
        isOptionalNullableRevision(value.expected_binding_revision) &&
        isOptionalNullable(value.workspace_id, (candidate) => isAuraId(candidate, 'wsp')) &&
        isOptionalNullable(value.session_id, (candidate) => isAuraId(candidate, 'ses')) &&
        !(value.session_id != null && value.workspace_id == null)
      );
    case 'tools.instance.clear_credential':
      return (
        exactRecord(value, [
          'expected_binding_revision',
          'expected_entity_revision',
          'tool_instance_id',
        ]) &&
        isAuraId(value.tool_instance_id, 'tin') &&
        isRevision(value.expected_entity_revision) &&
        isRevision(value.expected_binding_revision)
      );
    case 'tools.pin.workspace.set':
      return isPinSetRequest(value, 'workspace_id', 'wsp');
    case 'tools.pin.workspace.clear':
      return isPinClearRequest(value, 'workspace_id', 'wsp');
    case 'tools.pin.session.set':
      return isPinSetRequest(value, 'session_id', 'ses');
    case 'tools.pin.session.clear':
      return isPinClearRequest(value, 'session_id', 'ses');
  }
};

export const isToolMutationResult = (value: unknown): value is ToolMutationResultV1 => {
  if (!isRecord(value) || typeof value.result_kind !== 'string') return false;
  switch (value.result_kind) {
    case 'draft_committed':
      return (
        exactRecord(value, [
          'draft_revision',
          'entity_revision',
          'result_kind',
          'snapshot_hash',
          'tool_draft_id',
        ]) &&
        isAuraId(value.tool_draft_id, 'tdr') &&
        isRevision(value.draft_revision) &&
        isRevision(value.entity_revision) &&
        isCanonicalHash(value.snapshot_hash, 'sha256')
      );
    case 'conformance_accepted':
      return (
        exactRecord(value, ['operational_revision', 'result_kind', 'tool_conformance_run_id']) &&
        isAuraId(value.tool_conformance_run_id, 'tcr') &&
        isRevision(value.operational_revision)
      );
    case 'long_operation_accepted':
      return (
        exactRecord(value, ['accepted', 'result_kind']) &&
        exactRecord(value.accepted, ['operation', 'operational_revision', 'state']) &&
        isToolLongOperationRef(value.accepted.operation) &&
        isRevision(value.accepted.operational_revision) &&
        isToolLongOperationState(value.accepted.state)
      );
    case 'package_committed':
      return (
        exactRecord(value, ['entity_revision', 'result_kind', 'tool_package_id']) &&
        isAuraId(value.tool_package_id, 'tpk') &&
        isRevision(value.entity_revision)
      );
    case 'tool_committed':
      return (
        exactRecord(value, ['entity_revision', 'result_kind', 'tool_id']) &&
        isAuraId(value.tool_id, 'tol') &&
        isRevision(value.entity_revision)
      );
    case 'instance_committed':
      return (
        exactRecord(value, ['entity_revision', 'result_kind', 'tool_instance_id']) &&
        isAuraId(value.tool_instance_id, 'tin') &&
        isRevision(value.entity_revision)
      );
    case 'workspace_pin_committed':
      return (
        exactRecord(value, ['entity_revision', 'result_kind', 'workspace_id']) &&
        isAuraId(value.workspace_id, 'wsp') &&
        isRevision(value.entity_revision)
      );
    case 'session_pin_committed':
      return (
        exactRecord(value, ['entity_revision', 'result_kind', 'session_id']) &&
        isAuraId(value.session_id, 'ses') &&
        isRevision(value.entity_revision)
      );
    case 'operation_cancelled':
      return (
        exactRecord(value, ['operation', 'operational_revision', 'result_kind']) &&
        isToolOperationRef(value.operation) &&
        isRevision(value.operational_revision)
      );
    default:
      return false;
  }
};

const isCredentialSlotKey = (value: unknown): value is string =>
  typeof value === 'string' && value.length <= 64 && /^[a-z0-9]+(?:[-_][a-z0-9]+)*$/u.test(value);

const isToolCredentialPlacement = (value: unknown): boolean => {
  if (!isRecord(value) || typeof value.placement !== 'string') return false;
  switch (value.placement) {
    case 'command_environment':
      return (
        exactRecord(value, ['environment_variable', 'placement']) &&
        typeof value.environment_variable === 'string' &&
        /^[A-Z_][A-Z0-9_]{0,63}$/u.test(value.environment_variable)
      );
    case 'http_bearer_authorization':
      return exactRecord(value, ['placement']);
    case 'http_fixed_header':
      return (
        exactRecord(value, ['header_name', 'placement']) &&
        typeof value.header_name === 'string' &&
        value.header_name.length <= 128 &&
        /^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/u.test(value.header_name)
      );
    default:
      return false;
  }
};

const isPinSetRequest = (
  value: unknown,
  scopeKey: 'session_id' | 'workspace_id',
  scopePrefix: 'ses' | 'wsp',
): boolean =>
  recordWithOptional(value, [scopeKey, 'tool_id', 'tool_version'], ['expected_entity_revision']) &&
  isAuraId(value[scopeKey], scopePrefix) &&
  isAuraId(value.tool_id, 'tol') &&
  isToolVersion(value.tool_version) &&
  isOptionalNullableRevision(value.expected_entity_revision);

const isPinClearRequest = (
  value: unknown,
  scopeKey: 'session_id' | 'workspace_id',
  scopePrefix: 'ses' | 'wsp',
): boolean =>
  exactRecord(value, ['expected_entity_revision', scopeKey, 'tool_id']) &&
  isAuraId(value[scopeKey], scopePrefix) &&
  isAuraId(value.tool_id, 'tol') &&
  isRevision(value.expected_entity_revision);

const isToolOperationRef = (value: unknown): boolean => {
  if (!isRecord(value) || typeof value.operation_kind !== 'string') return false;
  switch (value.operation_kind) {
    case 'conformance':
      return (
        exactRecord(value, ['operation_kind', 'tool_conformance_run_id']) &&
        isAuraId(value.tool_conformance_run_id, 'tcr')
      );
    case 'draft_apply':
      return (
        exactRecord(value, ['journal_id', 'operation_kind', 'tool_draft_id']) &&
        isAuraId(value.journal_id, 'jnl') &&
        isAuraId(value.tool_draft_id, 'tdr')
      );
    case 'package_delete':
      return (
        exactRecord(value, ['journal_id', 'operation_kind', 'tool_package_id']) &&
        isAuraId(value.journal_id, 'jnl') &&
        isAuraId(value.tool_package_id, 'tpk')
      );
    default:
      return false;
  }
};

const isToolLongOperationRef = (value: unknown): boolean => {
  if (!isRecord(value) || typeof value.operation_kind !== 'string') return false;
  switch (value.operation_kind) {
    case 'draft_apply':
      return (
        exactRecord(value, ['journal_id', 'operation_kind', 'tool_draft_id']) &&
        isAuraId(value.journal_id, 'jnl') &&
        isAuraId(value.tool_draft_id, 'tdr')
      );
    case 'package_delete':
      return (
        exactRecord(value, ['journal_id', 'operation_kind', 'tool_package_id']) &&
        isAuraId(value.journal_id, 'jnl') &&
        isAuraId(value.tool_package_id, 'tpk')
      );
    default:
      return false;
  }
};

const isToolLongOperationState = (value: unknown): boolean =>
  value === 'queued' ||
  value === 'running' ||
  value === 'cancelling' ||
  value === 'cancelled' ||
  value === 'succeeded' ||
  value === 'failed' ||
  value === 'recovery_required';

const isToolVersion = (value: unknown): boolean =>
  recordWithOptional(value, ['definition_revision'], ['semantic_version']) &&
  isPositiveRevision(value.definition_revision) &&
  (value.semantic_version === undefined ||
    value.semantic_version === null ||
    isCanonicalSemVer(value.semantic_version));

const isCanonicalSemVer = (value: unknown): boolean =>
  typeof value === 'string' &&
  value.length <= 128 &&
  /^(0|[1-9][0-9]*)[.](0|[1-9][0-9]*)[.](0|[1-9][0-9]*)(?:-(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)(?:[.](?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*))*)?(?:[+][0-9A-Za-z-]+(?:[.][0-9A-Za-z-]+)*)?$/u.test(
    value,
  );

const isCanonicalHash = (value: unknown, algorithm: 'blake3' | 'sha256'): boolean =>
  exactRecord(value, ['algorithm', 'canonicalization_version', 'hex']) &&
  value.algorithm === algorithm &&
  value.canonicalization_version === 'rfc8785_jcs_v1' &&
  typeof value.hex === 'string' &&
  /^[0-9a-f]{64}$/u.test(value.hex);

const isJsonValue = (value: unknown, depth = 0): boolean => {
  if (depth > 64) return false;
  if (value === null || typeof value === 'string' || typeof value === 'boolean') return true;
  if (typeof value === 'number') return Number.isFinite(value);
  if (Array.isArray(value)) return value.every((item) => isJsonValue(item, depth + 1));
  if (!isRecord(value)) return false;
  const prototype = Object.getPrototypeOf(value);
  return (
    (prototype === Object.prototype || prototype === null) &&
    Object.values(value).every((item) => isJsonValue(item, depth + 1))
  );
};

const isOptionalNullableRevision = (value: unknown): boolean =>
  value === undefined || value === null || isRevision(value);

const isOptionalNullable = (value: unknown, predicate: (candidate: unknown) => boolean): boolean =>
  value === undefined || value === null || predicate(value);

const isPositiveRevision = (value: unknown): value is string =>
  typeof value === 'string' && /^[1-9][0-9]*$/u.test(value);

const isRevision = (value: unknown): value is string =>
  typeof value === 'string' && /^(?:0|[1-9][0-9]*)$/u.test(value);

const isAuraId = (value: unknown, prefix: string): value is string =>
  typeof value === 'string' &&
  new RegExp(`^${prefix}_[0-7][0-9A-HJKMNP-TV-Z]{25}$`, 'u').test(value);

const recordWithOptional = (
  value: unknown,
  required: readonly string[],
  optional: readonly string[],
): value is Record<string, unknown> => {
  if (!isRecord(value)) return false;
  const allowed = new Set([...required, ...optional]);
  const actual = Object.keys(value);
  return required.every((key) => actual.includes(key)) && actual.every((key) => allowed.has(key));
};

const exactRecord = (value: unknown, keys: readonly string[]): value is Record<string, unknown> => {
  if (!isRecord(value)) return false;
  const actual = Object.keys(value).sort();
  const expected = [...keys].sort();
  return actual.length === expected.length && actual.every((key, index) => key === expected[index]);
};

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);
