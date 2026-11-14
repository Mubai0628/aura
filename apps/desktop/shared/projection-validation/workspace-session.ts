import { exactRecord, isRevision, recordWithOptional } from './common.js';

/** `worktrees` is left out when no workspace is a worktree, so older payloads stay valid. */
export const isWorkspaceSessionPayload = (value: unknown): boolean =>
  recordWithOptional(value, ['sessions', 'workspaces'], ['worktrees']) &&
  Array.isArray(value.workspaces) &&
  value.workspaces.every(isWorkspaceSummary) &&
  Array.isArray(value.sessions) &&
  value.sessions.every(isSessionSummary) &&
  (value.worktrees === undefined ||
    (Array.isArray(value.worktrees) && value.worktrees.every(isWorktreeLink)));

const isWorkspaceSummary = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [
      'authorization_epoch',
      'canonical_root',
      'catalog_freshness',
      'display_name',
      'revision',
      'selected_root',
      'state',
      'workspace_id',
    ],
    ['root_identity'],
  ) &&
  typeof value.workspace_id === 'string' &&
  typeof value.display_name === 'string' &&
  typeof value.selected_root === 'string' &&
  typeof value.canonical_root === 'string' &&
  (value.root_identity === undefined ||
    value.root_identity === null ||
    isFileIdentity(value.root_identity)) &&
  typeof value.state === 'string' &&
  isRevision(value.revision) &&
  typeof value.catalog_freshness === 'string' &&
  isRevision(value.authorization_epoch);

const isFileIdentity = (value: unknown): boolean =>
  exactRecord(value, ['file_id', 'volume_serial_number']) &&
  typeof value.file_id === 'string' &&
  typeof value.volume_serial_number === 'string';

const isSessionSummary = (value: unknown): boolean =>
  exactRecord(value, ['session_id', 'state', 'title', 'workspace_id']) &&
  typeof value.session_id === 'string' &&
  typeof value.workspace_id === 'string' &&
  typeof value.title === 'string' &&
  typeof value.state === 'string';

const isWorktreeLink = (value: unknown): boolean =>
  exactRecord(value, ['parent_workspace_id', 'workspace_id']) &&
  typeof value.workspace_id === 'string' &&
  typeof value.parent_workspace_id === 'string';
