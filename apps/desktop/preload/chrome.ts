import type {
  AgentMainCommand,
  AgentMainParams,
  AgentMainResult,
} from '../shared/generated/routes.js';
import type { ResultEnvelope } from '../shared/aura-api.js';
import type { ChromeControlApiV1 } from '../shared/chrome-control-api.js';

/** Chrome control's fixed calls; each goes through the validated Agent route. */
export function chromeBridge(
  invoke: <C extends AgentMainCommand>(
    command: C,
    request: AgentMainParams[C],
  ) => Promise<ResultEnvelope<AgentMainResult[C]>>,
): ChromeControlApiV1 {
  return {
    status: () => invoke('chrome.status', {}),
    setEnabled: (request) => invoke('chrome.set_enabled', request),
    beginPairing: () => invoke('chrome.pair.begin', {}),
    revoke: () => invoke('chrome.revoke', {}),
    stop: (request) => invoke('chrome.stop', request),
    openExtensionFolder: () => invoke('chrome.extension.open_folder', {}),
    shares: (request) => invoke('chrome.shares', request),
    finishAssist: (request) => invoke('chrome.assist.finish', request),
  };
}
