import type {
  EditorBufferLifecycleState,
  EditorLifecycleTransitionAck,
  FsHashRequest,
  FsInspectRequest,
  FsListRequest,
  FsStatRequest,
  IssueDiscardConfirmationInput,
  PrepareEditorLifecycleTransition,
  ResolveWorkspaceLifecycleInput,
} from '@aura/contracts/generated';
import type { P03MainCommand, P03MainParams } from './generated/routes.js';
import { mainRoute } from './generated/routes.js';
import { validateAgentRequest } from './generated/agent-validator.js';
export type {
  P03CoreCommand,
  P03CoreParams,
  P03CoreResult,
  P03MainCommand,
  P03MainParams,
  P03MainResult,
} from './generated/routes.js';

export type RendererLifecycleAck = Omit<EditorLifecycleTransitionAck, 'renderer_instance_id'>;
export type RendererIssueDiscardConfirmation = Omit<
  IssueDiscardConfirmationInput,
  'renderer_instance_id'
>;
export type RendererResolveWorkspaceLifecycle = Omit<
  ResolveWorkspaceLifecycleInput,
  'renderer_instance_id'
>;

export const isP03MainCommand = (value: string): value is P03MainCommand =>
  mainRoute(value)?.owner === 'workspace';

export const isP03MainParams = <Command extends P03MainCommand>(
  command: Command,
  value: unknown,
): value is P03MainParams[Command] => {
  switch (command) {
    case 'session.create':
    case 'session.rename':
    case 'session.archive':
    case 'session.restore':
    case 'session.delete':
    case 'workspace.select_directory':
      return validateAgentRequest({ command, value });
    case 'workspace.delete':
      return exactRecord(value, ['workspace_id']) && isAuraId(value.workspace_id, 'wsp');
    case 'workspace.create':
      return (
        exactRecord(value, ['display_name', 'selected_root']) &&
        isNonEmptyString(value.display_name) &&
        isNonEmptyString(value.selected_root)
      );
    case 'workspace.restore':
      return exactRecord(value, ['workspace_id']) && isAuraId(value.workspace_id, 'wsp');
    case 'workspace.archive':
      return (
        exactRecord(value, ['expected_workspace_revision', 'workspace_id']) &&
        isAuraId(value.workspace_id, 'wsp') &&
        isRevision(value.expected_workspace_revision)
      );
    case 'workspace.relocate':
      return (
        exactRecord(value, [
          'confirmation',
          'expected_workspace_revision',
          'selected_root',
          'workspace_id',
        ]) &&
        isAuraId(value.workspace_id, 'wsp') &&
        isNonEmptyString(value.selected_root) &&
        isRevision(value.expected_workspace_revision) &&
        (value.confirmation === 'require_same_object' ||
          value.confirmation === 'user_confirmed_same_logical_project')
      );
    case 'workspace.buffer.open':
      return (
        exactRecord(value, ['path', 'session_id', 'workspace_id', 'writable']) &&
        isWorkspaceSessionPath(value) &&
        typeof value.writable === 'boolean'
      );
    case 'workspace.buffer.edit':
      return (
        exactRecord(value, [
          'active_editing',
          'base_core_revision',
          'buffer_revision',
          'content',
          'ime_composition_active',
          'path',
          'reservation_epoch',
          'reservation_id',
          'session_id',
          'workspace_id',
        ]) &&
        isWorkspaceSessionPath(value) &&
        isAuraId(value.reservation_id, 'ers') &&
        isRevision(value.reservation_epoch) &&
        isRevision(value.buffer_revision) &&
        isRevision(value.base_core_revision) &&
        typeof value.content === 'string' &&
        typeof value.active_editing === 'boolean' &&
        typeof value.ime_composition_active === 'boolean'
      );
    case 'workspace.buffer.save':
      return (
        exactRecord(value, [
          'expected_buffer_revision',
          'path',
          'reservation_epoch',
          'reservation_id',
          'session_id',
          'workspace_id',
        ]) &&
        isWorkspaceSessionPath(value) &&
        isAuraId(value.reservation_id, 'ers') &&
        isRevision(value.reservation_epoch) &&
        isRevision(value.expected_buffer_revision)
      );
    case 'workspace.lifecycle.ack':
      return (
        exactRecord(value, ['buffers', 'lifecycle_revision', 'operation_id', 'workspace_id']) &&
        isAuraId(value.operation_id, 'wlo') &&
        isRevision(value.lifecycle_revision) &&
        isAuraId(value.workspace_id, 'wsp') &&
        Array.isArray(value.buffers) &&
        value.buffers.every(isEditorBufferLifecycleState)
      );
    case 'workspace.lifecycle.issue_discard_confirmation':
      return (
        exactRecord(value, ['expected_lifecycle_revision', 'operation_id']) &&
        isAuraId(value.operation_id, 'wlo') &&
        isRevision(value.expected_lifecycle_revision)
      );
    case 'workspace.lifecycle.resolve':
      return (
        recordWithOptional(
          value,
          ['action', 'expected_lifecycle_revision', 'operation_id'],
          ['discard_confirmation_nonce'],
        ) &&
        isAuraId(value.operation_id, 'wlo') &&
        isRevision(value.expected_lifecycle_revision) &&
        (value.action === 'save_all_and_continue' ||
          value.action === 'return_to_editing' ||
          value.action === 'discard_dirty_buffers_and_continue') &&
        (value.discard_confirmation_nonce === undefined ||
          value.discard_confirmation_nonce === null ||
          isSecretNonce(value.discard_confirmation_nonce))
      );
    case 'fs.list':
      return (
        recordWithOptional(value, ['session_id', 'target', 'workspace_id'], ['after']) &&
        isWorkspaceSession(value) &&
        isWorkspaceTarget(value.target) &&
        (value.after === undefined || value.after === null || isWorkspacePath(value.after))
      );
    case 'fs.stat':
    case 'fs.hash':
    case 'fs.inspect':
      return isTargetReadRequest(value);
    case 'fs.find_paths':
      return (
        exactRecord(value, ['limit', 'query', 'session_id', 'workspace_id']) &&
        isWorkspaceSession(value) &&
        typeof value.query === 'string' &&
        isSearchLimit(value.limit)
      );
    case 'fs.glob':
      return (
        exactRecord(value, ['limit', 'pattern', 'session_id', 'workspace_id']) &&
        isWorkspaceSession(value) &&
        typeof value.pattern === 'string' &&
        isSearchLimit(value.limit)
      );
    case 'fs.search_text':
      return (
        recordWithOptional(
          value,
          [
            'include_generated',
            'include_ignored',
            'limit',
            'pattern',
            'root',
            'session_id',
            'workspace_id',
          ],
          ['path_glob'],
        ) &&
        isWorkspaceSession(value) &&
        isSearchPattern(value.pattern) &&
        typeof value.root === 'string' &&
        typeof value.include_ignored === 'boolean' &&
        typeof value.include_generated === 'boolean' &&
        (value.path_glob === undefined ||
          value.path_glob === null ||
          typeof value.path_glob === 'string') &&
        isSearchLimit(value.limit)
      );
    case 'fs.read_text':
      return (
        recordWithOptional(
          value,
          ['range', 'session_id', 'target', 'workspace_id'],
          ['explicit_encoding'],
        ) &&
        isWorkspaceSession(value) &&
        isWorkspaceTarget(value.target) &&
        (value.explicit_encoding === undefined ||
          value.explicit_encoding === null ||
          isTextEncoding(value.explicit_encoding)) &&
        isTextReadRange(value.range)
      );
    case 'fs.read_binary_range':
      return (
        exactRecord(value, [
          'format',
          'length',
          'offset',
          'session_id',
          'target',
          'workspace_id',
        ]) &&
        isWorkspaceSession(value) &&
        isWorkspaceTarget(value.target) &&
        isSafeUnsigned(value.offset) &&
        isSafeUnsigned(value.length) &&
        value.length <= 65_536 &&
        (value.format === 'hex' || value.format === 'base64' || value.format === 'hex_with_ascii')
      );
    case 'fs.write_file':
      return (
        recordWithOptional(
          value,
          ['content', 'expected', 'session_id', 'target', 'workspace_id'],
          ['encoding', 'line_ending'],
        ) &&
        isWorkspaceSession(value) &&
        isWorkspaceTarget(value.target) &&
        typeof value.content === 'string' &&
        isExpectedState(value.expected) &&
        isOptionalTextEncoding(value.encoding) &&
        isOptionalLineEnding(value.line_ending)
      );
    case 'fs.apply_patch':
      return (
        exactRecord(value, [
          'expected_hash',
          'session_id',
          'target',
          'unified_diff',
          'workspace_id',
        ]) &&
        isWorkspaceSession(value) &&
        isWorkspaceTarget(value.target) &&
        isContentHash(value.expected_hash) &&
        typeof value.unified_diff === 'string'
      );
    case 'fs.create_directory':
      return isTargetReadRequest(value);
    case 'fs.copy':
    case 'fs.move':
      return (
        exactRecord(value, [
          'destination',
          'destination_expected',
          'session_id',
          'source',
          'source_expected',
          'workspace_id',
        ]) &&
        isWorkspaceSession(value) &&
        isWorkspaceTarget(value.source) &&
        isWorkspaceTarget(value.destination) &&
        isExpectedState(value.source_expected) &&
        isExpectedState(value.destination_expected)
      );
    case 'fs.delete':
      return (
        exactRecord(value, ['expected', 'recursive', 'session_id', 'target', 'workspace_id']) &&
        isWorkspaceSession(value) &&
        isWorkspaceTarget(value.target) &&
        isExpectedState(value.expected) &&
        typeof value.recursive === 'boolean'
      );
    case 'fs.apply_changeset':
      return (
        exactRecord(value, ['operations', 'session_id', 'workspace_id']) &&
        isWorkspaceSession(value) &&
        Array.isArray(value.operations) &&
        value.operations.every(isChangesetOperation)
      );
  }
};

const exactRecord = <Keys extends readonly string[]>(
  value: unknown,
  keys: Keys,
): value is Record<Keys[number], unknown> => recordWithOptional(value, keys, []);

const recordWithOptional = <Required extends readonly string[], Optional extends readonly string[]>(
  value: unknown,
  required: Required,
  optional: Optional,
): value is Record<Required[number], unknown> & Partial<Record<Optional[number], unknown>> => {
  if (!isRecord(value)) return false;
  const keys = Object.keys(value);
  if (!required.every((key) => keys.includes(key))) return false;
  const allowed = new Set<string>([...required, ...optional]);
  return keys.every((key) => allowed.has(key));
};

const isTargetReadRequest = (
  value: unknown,
): value is FsHashRequest | FsInspectRequest | FsListRequest | FsStatRequest =>
  exactRecord(value, ['session_id', 'target', 'workspace_id']) &&
  isWorkspaceSession(value) &&
  isWorkspaceTarget(value.target);

const isWorkspaceSession = (value: Record<string, unknown>): boolean =>
  isAuraId(value.workspace_id, 'wsp') && isAuraId(value.session_id, 'ses');

const isWorkspaceSessionPath = (value: Record<string, unknown>): boolean =>
  isWorkspaceSession(value) && isWorkspacePath(value.path);

const isWorkspaceTarget = (value: unknown): value is { path: string } =>
  exactRecord(value, ['path']) && isWorkspacePath(value.path);

export const isExpectedState = (value: unknown): boolean => {
  if (!isRecord(value) || typeof value.expected_kind !== 'string') return false;
  if (value.expected_kind === 'absent' || value.expected_kind === 'directory') {
    return exactRecord(value, ['expected_kind']);
  }
  return (
    value.expected_kind === 'file_hash' &&
    exactRecord(value, ['expected_kind', 'hash']) &&
    isContentHash(value.hash)
  );
};

const isChangesetOperation = (value: unknown): boolean => {
  if (!isRecord(value) || typeof value.operation !== 'string') return false;
  switch (value.operation) {
    case 'write_file':
      return (
        recordWithOptional(
          value,
          ['content', 'expected', 'operation', 'target'],
          ['encoding', 'line_ending'],
        ) &&
        typeof value.content === 'string' &&
        isExpectedState(value.expected) &&
        isWorkspaceTarget(value.target) &&
        isOptionalTextEncoding(value.encoding) &&
        isOptionalLineEnding(value.line_ending)
      );
    case 'apply_patch':
      return (
        exactRecord(value, ['expected_hash', 'operation', 'target', 'unified_diff']) &&
        isContentHash(value.expected_hash) &&
        isWorkspaceTarget(value.target) &&
        typeof value.unified_diff === 'string'
      );
    case 'create_directory':
      return exactRecord(value, ['operation', 'target']) && isWorkspaceTarget(value.target);
    case 'copy':
    case 'move':
      return (
        exactRecord(value, [
          'destination',
          'destination_expected',
          'operation',
          'source',
          'source_expected',
        ]) &&
        isWorkspaceTarget(value.source) &&
        isWorkspaceTarget(value.destination) &&
        isExpectedState(value.source_expected) &&
        isExpectedState(value.destination_expected)
      );
    case 'delete':
      return (
        exactRecord(value, ['expected', 'operation', 'recursive', 'target']) &&
        isExpectedState(value.expected) &&
        isWorkspaceTarget(value.target) &&
        typeof value.recursive === 'boolean'
      );
    default:
      return false;
  }
};

export const isPrepareEditorLifecycleTransition = (
  value: unknown,
): value is PrepareEditorLifecycleTransition =>
  exactRecord(value, [
    'deadline_at',
    'lifecycle_revision',
    'operation',
    'operation_id',
    'workspace_id',
  ]) &&
  isRfc3339Timestamp(value.deadline_at) &&
  isRevision(value.lifecycle_revision) &&
  (value.operation === 'archive' ||
    value.operation === 'relocate' ||
    value.operation === 'permanent_delete') &&
  isAuraId(value.operation_id, 'wlo') &&
  isAuraId(value.workspace_id, 'wsp');

const isEditorBufferLifecycleState = (value: unknown): value is EditorBufferLifecycleState =>
  recordWithOptional(
    value,
    [
      'buffer_revision',
      'canonical_relative_path',
      'core_acknowledged_revision',
      'ime_composition_active',
      'is_active_editing',
      'is_dirty',
      'logical_hash',
      'reservation_epoch',
      'save_state',
      'session_id',
    ],
    ['disk_hash', 'reservation_id'],
  ) &&
  isRevision(value.buffer_revision) &&
  isWorkspacePath(value.canonical_relative_path) &&
  isRevision(value.core_acknowledged_revision) &&
  typeof value.ime_composition_active === 'boolean' &&
  typeof value.is_active_editing === 'boolean' &&
  typeof value.is_dirty === 'boolean' &&
  isContentHash(value.logical_hash) &&
  isRevision(value.reservation_epoch) &&
  isEditorSaveState(value.save_state) &&
  isAuraId(value.session_id, 'ses') &&
  (value.disk_hash === undefined || value.disk_hash === null || isContentHash(value.disk_hash)) &&
  (value.reservation_id === undefined ||
    value.reservation_id === null ||
    isAuraId(value.reservation_id, 'ers'));

const isEditorSaveState = (value: unknown): boolean => {
  if (!isRecord(value) || typeof value.save_state !== 'string') return false;
  if (
    value.save_state === 'idle' ||
    value.save_state === 'saving' ||
    value.save_state === 'conflict'
  ) {
    return exactRecord(value, ['save_state']);
  }
  return (
    value.save_state === 'failed' &&
    exactRecord(value, ['error_code', 'save_state']) &&
    isNonEmptyString(value.error_code)
  );
};

export const isContentHash = (value: unknown): boolean =>
  exactRecord(value, ['algorithm', 'hex']) &&
  (value.algorithm === 'blake3' || value.algorithm === 'sha256') &&
  typeof value.hex === 'string' &&
  /^[0-9a-f]{64}$/u.test(value.hex);

const isRfc3339Timestamp = (value: unknown): value is string =>
  typeof value === 'string' && !Number.isNaN(Date.parse(value));

export const isSecretNonce = (value: unknown): value is string =>
  typeof value === 'string' && /^[0-9a-f]{64}$/u.test(value);

const isTextReadRange = (value: unknown): boolean => {
  if (!isRecord(value) || typeof value.range_kind !== 'string') return false;
  if (value.range_kind === 'full') return exactRecord(value, ['range_kind']);
  if (value.range_kind === 'lines') {
    return (
      exactRecord(value, ['count', 'range_kind', 'start_line_index']) &&
      isSafeUnsigned(value.start_line_index) &&
      isSafeUnsigned(value.count)
    );
  }
  if (value.range_kind === 'characters') {
    return (
      exactRecord(value, ['count', 'range_kind', 'start_character_offset']) &&
      isSafeUnsigned(value.start_character_offset) &&
      isSafeUnsigned(value.count)
    );
  }
  return false;
};

const isTextEncoding = (value: unknown): boolean =>
  value === 'utf8' ||
  value === 'utf8_bom' ||
  value === 'utf16_le' ||
  value === 'utf16_be' ||
  value === 'windows1252';

const isOptionalTextEncoding = (value: unknown): boolean =>
  value === undefined || value === null || isTextEncoding(value);

const isOptionalLineEnding = (value: unknown): boolean =>
  value === undefined ||
  value === null ||
  value === 'lf' ||
  value === 'cr_lf' ||
  value === 'cr' ||
  value === 'mixed' ||
  value === 'none';

export const isWorkspacePath = (value: unknown): value is string => {
  if (
    typeof value !== 'string' ||
    value.length === 0 ||
    value.includes('\u0000') ||
    value.startsWith('/') ||
    value.startsWith('\\') ||
    value.includes(':')
  ) {
    return false;
  }
  return !value.split(/[\\/]/u).some((component) => component === '..');
};

const isRevision = (value: unknown): value is string =>
  typeof value === 'string' && /^(0|[1-9][0-9]*)$/u.test(value);

const isSearchLimit = (value: unknown): value is number =>
  isSafeUnsigned(value) && value >= 1 && value <= 100;

const isSearchPattern = (value: unknown): boolean =>
  typeof value === 'object' &&
  value !== null &&
  !Array.isArray(value) &&
  ((exactRecord(value, ['case_sensitive', 'kind', 'text']) &&
    (value as { kind: unknown }).kind === 'literal' &&
    typeof (value as { text: unknown }).text === 'string' &&
    typeof (value as { case_sensitive: unknown }).case_sensitive === 'boolean') ||
    (exactRecord(value, ['case_sensitive', 'kind', 'pattern']) &&
      (value as { kind: unknown }).kind === 'regex' &&
      typeof (value as { pattern: unknown }).pattern === 'string' &&
      typeof (value as { case_sensitive: unknown }).case_sensitive === 'boolean'));

const isSafeUnsigned = (value: unknown): value is number =>
  typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;

const isAuraId = (value: unknown, prefix: 'ers' | 'rnd' | 'ses' | 'wlo' | 'wsp'): value is string =>
  typeof value === 'string' &&
  new RegExp(`^${prefix}_[0-7][0-9A-HJKMNP-TV-Z]{25}$`, 'u').test(value);

const isNonEmptyString = (value: unknown): value is string =>
  typeof value === 'string' && value.trim().length > 0;

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);
