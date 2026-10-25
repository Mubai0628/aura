import type { ProjectionChange } from '@aura/contracts/generated';
import type { SecretInputOperation } from '../protocol.js';
import type { ModelProjectionChange, P05SecretMainCommand } from '../../shared/p05-validation.js';
import { isModelProjectionKind } from '../../shared/projection-validation.js';
import type {
  CoreHealthProjectionChange,
  WorkspaceFilesProjectionChange,
  WorkspaceSessionProjectionChange,
} from './config.js';

/** Comparing projection payloads and the local projections Main serves while Core is unavailable. */
export const sameCoreHealthPayload = (
  left: CoreHealthProjectionChange,
  right: CoreHealthProjectionChange,
): boolean =>
  left.payload.state === right.payload.state &&
  left.payload.read_only === right.payload.read_only &&
  left.payload.safe_mode === right.payload.safe_mode &&
  left.payload.restart_attempt === right.payload.restart_attempt &&
  left.payload.status_code === right.payload.status_code &&
  left.payload.status_message === right.payload.status_message;

export const sameWorkspaceSessionPayload = (
  left: WorkspaceSessionProjectionChange,
  right: WorkspaceSessionProjectionChange,
): boolean =>
  left.payload.workspaces.length === right.payload.workspaces.length &&
  left.payload.sessions.length === right.payload.sessions.length &&
  left.payload.workspaces.every((workspace, index) => {
    const other = right.payload.workspaces[index];
    return (
      other !== undefined &&
      workspace.workspace_id === other.workspace_id &&
      workspace.display_name === other.display_name &&
      workspace.selected_root === other.selected_root &&
      workspace.canonical_root === other.canonical_root &&
      workspace.revision === other.revision &&
      workspace.state === other.state
    );
  }) &&
  left.payload.sessions.every((session, index) => {
    const other = right.payload.sessions[index];
    return (
      other !== undefined &&
      session.session_id === other.session_id &&
      session.workspace_id === other.workspace_id &&
      session.title === other.title &&
      session.state === other.state
    );
  });

export const sameWorkspaceFilesPayload = (
  left: WorkspaceFilesProjectionChange,
  right: WorkspaceFilesProjectionChange,
): boolean => JSON.stringify(left.payload) === JSON.stringify(right.payload);

export const isModelProjectionChange = (
  change: ProjectionChange,
): change is ModelProjectionChange => isModelProjectionKind(change.projection_kind);

export const p05SecretOperation = (command: P05SecretMainCommand): SecretInputOperation => {
  switch (command) {
    case 'models.endpoint.create_with_credential':
      return 'models_endpoint_create_with_credential';
    case 'models.endpoint.configure_credential':
      return 'models_endpoint_configure_credential';
    case 'models.endpoint.replace_credential':
      return 'models_endpoint_replace_credential';
  }
};

export const localProjection = (
  runtimeInstanceId: string,
  state: CoreHealthProjectionChange['payload']['state'],
  statusCode: string,
  statusMessage: string,
  readOnly: boolean,
  revision: number,
  safeMode: boolean,
  restartAttempt: number,
): CoreHealthProjectionChange => ({
  payload: {
    read_only: readOnly,
    restart_attempt: restartAttempt,
    safe_mode: safeMode,
    state,
    status_code: statusCode,
    status_message: statusMessage,
  },
  projection_kind: 'core_health',
  projection_scope: { type: 'global' },
  revision: String(revision),
  runtime_instance_id: runtimeInstanceId,
});

export const nextLocalProjection = (
  previous: CoreHealthProjectionChange,
  state: CoreHealthProjectionChange['payload']['state'],
  statusCode: string,
  statusMessage: string,
  readOnly: boolean,
): CoreHealthProjectionChange =>
  localProjection(
    previous.runtime_instance_id,
    state,
    statusCode,
    statusMessage,
    readOnly,
    Number(previous.revision) + 1,
    previous.payload.safe_mode,
    previous.payload.restart_attempt,
  );

export const localWorkspaceSessionProjection = (
  runtimeInstanceId: string,
  revision: number,
): WorkspaceSessionProjectionChange => ({
  payload: {
    sessions: [],
    workspaces: [],
  },
  projection_kind: 'workspace_session',
  projection_scope: { type: 'global' },
  revision: String(revision),
  runtime_instance_id: runtimeInstanceId,
});

export const localWorkspaceFilesProjection = (
  runtimeInstanceId: string,
  revision: number,
): WorkspaceFilesProjectionChange => ({
  payload: { lifecycle_operations: [], workspaces: [] },
  projection_kind: 'workspace_files',
  projection_scope: { type: 'global' },
  revision: String(revision),
  runtime_instance_id: runtimeInstanceId,
});
