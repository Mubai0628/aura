import { ipcRenderer, type IpcRendererEvent } from 'electron';
import type {
  AuraApiV1,
  CoreHealthProjectionChange,
  WorkspaceSessionProjectionChange,
  WorkspaceFilesProjectionChange,
} from '../shared/aura-api.js';
import { parseProjection } from '../shared/projection-validation.js';

function listen<Value>(channel: string, parse: (value: unknown) => Value | undefined) {
  return (listener: (value: Value) => void): (() => void) => {
    if (typeof listener !== 'function')
      throw new TypeError('Projection listener must be a function');
    const wrapped = (_event: IpcRendererEvent, value: unknown) => {
      const parsed = parse(value);
      if (parsed !== undefined) listener(parsed);
    };
    ipcRenderer.on(channel, wrapped);
    return () => ipcRenderer.removeListener(channel, wrapped);
  };
}

export function projectionListeners(): Pick<
  AuraApiV1,
  'onCoreHealth' | 'onWorkspaceSessions' | 'onWorkspaceFiles'
> {
  return {
    onCoreHealth: listen<CoreHealthProjectionChange>('aura:v1:core-health', (value) => {
      const change = parseProjection(value);
      return change?.projection_kind === 'core_health' ? change : undefined;
    }),
    onWorkspaceSessions: listen<WorkspaceSessionProjectionChange>(
      'aura:v1:workspace-session',
      (value) => {
        const change = parseProjection(value);
        return change?.projection_kind === 'workspace_session' ? change : undefined;
      },
    ),
    onWorkspaceFiles: listen<WorkspaceFilesProjectionChange>('aura:v1:workspace-files', (value) => {
      const change = parseProjection(value);
      return change?.projection_kind === 'workspace_files' ? change : undefined;
    }),
  };
}
