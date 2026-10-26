import {
  isTimestamp,
  isAuraId,
  isContentHash,
  isRevision,
  isSafeUnsigned,
  exactRecord,
  recordWithOptional,
  isRecord,
} from './common.js';
export const isWorkspaceFilesPayload = (value: unknown): boolean =>
  exactRecord(value, ['lifecycle_operations', 'workspaces']) &&
  Array.isArray(value.lifecycle_operations) &&
  value.lifecycle_operations.every(isWorkspaceLifecycleSummary) &&
  Array.isArray(value.workspaces) &&
  value.workspaces.every(isWorkspaceFileStatus);

const isWorkspaceLifecycleSummary = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [
      'lifecycle_revision',
      'operation',
      'operation_id',
      'renderer_instance_id',
      'state',
      'workspace_id',
    ],
    ['ack_deadline_at', 'error_code', 'preflight', 'approval_follow_up'],
  ) &&
  isApprovalFollowUp(value.approval_follow_up) &&
  isRevision(value.lifecycle_revision) &&
  isLifecycleOperation(value.operation) &&
  isAuraId(value.operation_id, 'wlo') &&
  isAuraId(value.renderer_instance_id, 'rnd') &&
  isLifecycleState(value.state) &&
  isAuraId(value.workspace_id, 'wsp') &&
  (value.ack_deadline_at === undefined ||
    value.ack_deadline_at === null ||
    isTimestamp(value.ack_deadline_at)) &&
  (value.error_code === undefined ||
    value.error_code === null ||
    typeof value.error_code === 'string') &&
  (value.preflight === undefined ||
    value.preflight === null ||
    isLifecyclePreflight(value.preflight));

const isLifecyclePreflight = (value: unknown): boolean =>
  exactRecord(value, [
    'blockers',
    'lifecycle_revision',
    'operation',
    'operation_id',
    'releasable_editor_states',
    'workspace_id',
  ]) &&
  Array.isArray(value.blockers) &&
  value.blockers.every(isLifecycleBlocker) &&
  isApprovalFollowUp(value.approval_follow_up) &&
  isRevision(value.lifecycle_revision) &&
  isLifecycleOperation(value.operation) &&
  isAuraId(value.operation_id, 'wlo') &&
  Array.isArray(value.releasable_editor_states) &&
  value.releasable_editor_states.every(isEditorStateRef) &&
  isAuraId(value.workspace_id, 'wsp');

const isEditorStateRef = (value: unknown): boolean =>
  recordWithOptional(
    value,
    ['buffer_revision', 'canonical_relative_path', 'reservation_epoch', 'session_id'],
    ['reservation_id'],
  ) &&
  isRevision(value.buffer_revision) &&
  typeof value.canonical_relative_path === 'string' &&
  isRevision(value.reservation_epoch) &&
  isAuraId(value.session_id, 'ses') &&
  (value.reservation_id === undefined ||
    value.reservation_id === null ||
    isAuraId(value.reservation_id, 'ers'));

const isLifecycleBlocker = (value: unknown): boolean => {
  if (!isRecord(value) || typeof value.blocker_kind !== 'string') return false;
  switch (value.blocker_kind) {
    case 'dirty_buffer':
      return (
        exactRecord(value, ['blocker_kind', 'buffer_revision', 'path', 'session_id']) &&
        isRevision(value.buffer_revision) &&
        typeof value.path === 'string' &&
        isAuraId(value.session_id, 'ses')
      );
    case 'active_editing':
      return (
        exactRecord(value, ['blocker_kind', 'ime_composition_active', 'path', 'session_id']) &&
        typeof value.ime_composition_active === 'boolean' &&
        typeof value.path === 'string' &&
        isAuraId(value.session_id, 'ses')
      );
    case 'pending_buffer_synchronization':
      return (
        exactRecord(value, [
          'blocker_kind',
          'core_revision',
          'path',
          'renderer_revision',
          'session_id',
        ]) &&
        isRevision(value.core_revision) &&
        typeof value.path === 'string' &&
        isRevision(value.renderer_revision) &&
        isAuraId(value.session_id, 'ses')
      );
    case 'save_in_progress':
    case 'file_version_conflict':
    case 'deferred_mutation_exists':
      return (
        exactRecord(value, ['blocker_kind', 'path', 'session_id']) &&
        typeof value.path === 'string' &&
        isAuraId(value.session_id, 'ses')
      );
    case 'save_failed':
      return (
        exactRecord(value, ['blocker_kind', 'error_code', 'path', 'session_id']) &&
        typeof value.error_code === 'string' &&
        typeof value.path === 'string' &&
        isAuraId(value.session_id, 'ses')
      );
    case 'editor_state_unconfirmed':
      return (
        recordWithOptional(value, ['blocker_kind'], ['path', 'session_id']) &&
        (value.path === undefined || value.path === null || typeof value.path === 'string') &&
        (value.session_id === undefined ||
          value.session_id === null ||
          isAuraId(value.session_id, 'ses'))
      );
    case 'runtime_objects_active':
    case 'runtime_leases_active':
    case 'mutation_ownership_active':
    case 'unfinished_journals':
      return exactRecord(value, ['blocker_kind', 'count']) && isSafeUnsigned(value.count);
    default:
      return false;
  }
};

const isWorkspaceFileStatus = (value: unknown): boolean =>
  exactRecord(value, [
    'catalog',
    'deferred_mutations',
    'editor_reservations',
    'logical_files',
    'workspace_id',
    'workspace_revision',
  ]) &&
  typeof value.workspace_id === 'string' &&
  isRevision(value.workspace_revision) &&
  isCatalogStatus(value.catalog) &&
  Array.isArray(value.logical_files) &&
  value.logical_files.every(isLogicalFileStatus) &&
  Array.isArray(value.editor_reservations) &&
  value.editor_reservations.every(isEditorReservation) &&
  Array.isArray(value.deferred_mutations) &&
  value.deferred_mutations.every(isDeferredMutation);

const isCatalogStatus = (value: unknown): boolean =>
  recordWithOptional(
    value,
    ['catalog_revision', 'freshness', 'partitions', 'watcher_state'],
    ['indexed_at', 'indexing'],
  ) &&
  (value.indexing === undefined || isIndexingStatus(value.indexing)) &&
  typeof value.freshness === 'string' &&
  isRevision(value.catalog_revision) &&
  (value.indexed_at === undefined ||
    value.indexed_at === null ||
    typeof value.indexed_at === 'string') &&
  typeof value.watcher_state === 'string' &&
  Array.isArray(value.partitions) &&
  value.partitions.every(isCatalogPartition);

const isIndexingStatus = (value: unknown): boolean =>
  recordWithOptional(value, ['listed_entries', 'state', 'unindexed_directories'], ['failure']) &&
  ['building', 'incomplete', 'failed'].includes(value.state as string) &&
  isSafeUnsigned(value.listed_entries) &&
  isSafeUnsigned(value.unindexed_directories) &&
  (value.failure === undefined || typeof value.failure === 'string');

const isCatalogPartition = (value: unknown): boolean =>
  exactRecord(value, ['file_count', 'index_mode', 'indexable_text_bytes', 'partition_path']) &&
  typeof value.partition_path === 'string' &&
  isSafeUnsigned(value.file_count) &&
  isSafeUnsigned(value.indexable_text_bytes) &&
  typeof value.index_mode === 'string';

const isLogicalFileStatus = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [
      'active_editing',
      'buffer_revision',
      'core_acknowledged_revision',
      'dirty',
      'ime_composition_active',
      'logical_hash',
      'path',
      'reservation_epoch',
      'save_state',
      'session_id',
    ],
    ['disk_hash', 'reservation_id', 'approval_follow_up'],
  ) &&
  isAuraId(value.session_id, 'ses') &&
  typeof value.path === 'string' &&
  isApprovalFollowUp(value.approval_follow_up) &&
  isContentHash(value.logical_hash) &&
  (value.disk_hash === undefined || value.disk_hash === null || isContentHash(value.disk_hash)) &&
  isRevision(value.buffer_revision) &&
  isRevision(value.core_acknowledged_revision) &&
  typeof value.dirty === 'boolean' &&
  typeof value.active_editing === 'boolean' &&
  typeof value.ime_composition_active === 'boolean' &&
  isRevision(value.reservation_epoch) &&
  (value.reservation_id === undefined ||
    value.reservation_id === null ||
    isAuraId(value.reservation_id, 'ers')) &&
  isEditorSaveState(value.save_state);

const isEditorReservation = (value: unknown): boolean =>
  exactRecord(value, ['path', 'reservation_epoch', 'reservation_id', 'session_id']) &&
  isAuraId(value.session_id, 'ses') &&
  typeof value.path === 'string' &&
  isRevision(value.reservation_epoch) &&
  isAuraId(value.reservation_id, 'ers');

const isDeferredMutation = (value: unknown): boolean =>
  exactRecord(value, ['created_at', 'deferred_mutation_id', 'path', 'reason', 'session_id']) &&
  typeof value.deferred_mutation_id === 'string' &&
  typeof value.session_id === 'string' &&
  typeof value.path === 'string' &&
  typeof value.reason === 'string' &&
  typeof value.created_at === 'string';

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
    typeof value.error_code === 'string'
  );
};

const isLifecycleOperation = (value: unknown): boolean =>
  value === 'archive' || value === 'relocate' || value === 'permanent_delete';

const isLifecycleState = (value: unknown): boolean =>
  value === 'preparing' ||
  value === 'blocked' ||
  value === 'saving' ||
  value === 'discarding' ||
  value === 'committing' ||
  value === 'completed' ||
  value === 'cancelled' ||
  value === 'failed';

const isApprovalFollowUp = (value: unknown): boolean =>
  value === undefined ||
  (Array.isArray(value) &&
    value.every(
      (item) =>
        exactRecord(item, ['path', 'requires_reapproval', 'message']) &&
        typeof item.path === 'string' &&
        typeof item.requires_reapproval === 'boolean' &&
        typeof item.message === 'string',
    ));
