import type {
  AgentMainCommand,
  AgentMainParams,
  AgentMainResult,
} from '../shared/generated/routes.js';
import type { ResultEnvelope } from '../shared/aura-api.js';
import type { ComputerControlApiV1 } from '../shared/computer-control-api.js';

/** Computer control's fixed calls; each goes through the validated Agent route. */
export function computerBridge(
  invoke: <C extends AgentMainCommand>(
    command: C,
    request: AgentMainParams[C],
  ) => Promise<ResultEnvelope<AgentMainResult[C]>>,
): ComputerControlApiV1 {
  return {
    settings: () => invoke('computer.settings.get', {}),
    updateSettings: (request) => invoke('computer.settings.update', request),
    catalog: (request) => invoke('computer.catalog', request),
    icons: (request) => invoke('computer.icons', request),
    controlling: (request) => invoke('computer.controlling', request),
    stop: (request) => invoke('computer.stop', request),
    giveBack: (request) => invoke('computer.give_back', request),
    alwaysAllow: (request) => invoke('computer.always_allow', request),
  };
}
