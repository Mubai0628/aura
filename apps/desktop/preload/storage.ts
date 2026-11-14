import type {
  AgentMainCommand,
  AgentMainParams,
  AgentMainResult,
} from '../shared/generated/routes.js';
import type { ResultEnvelope } from '../shared/aura-api.js';
import type { StorageApiV1 } from '../shared/storage-api.js';

/** The storage page's fixed calls; each goes through the validated Agent route. */
export function storageBridge(
  invoke: <C extends AgentMainCommand>(
    command: C,
    request: AgentMainParams[C],
  ) => Promise<ResultEnvelope<AgentMainResult[C]>>,
): StorageApiV1 {
  return {
    overview: () => invoke('storage.overview', {}),
    check: (request) => invoke('storage.check', request),
    requestMove: (request) => invoke('storage.move.request', request),
    withdrawMove: () => invoke('storage.move.withdraw', {}),
    dismissFailure: () => invoke('storage.failure.dismiss', {}),
    oldData: () => invoke('storage.old_data.list', {}),
    removeOldData: (request) => invoke('storage.old_data.remove', request),
    selectFolder: () => invoke('storage.select_folder', {}),
    openFolder: (request) => invoke('storage.open_folder', request),
    restart: () => invoke('storage.restart', {}),
    moveInstall: () => invoke('storage.move_install', {}),
  };
}
